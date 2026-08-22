use crate::util::{non_empty_env, string_at};
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;

#[derive(Clone, Copy, Debug)]
enum Direction {
    Left,
    Down,
    Up,
    Right,
}

#[derive(Debug, Default)]
struct FocusedProcessInfo {
    pane_id: Option<String>,
    passthrough_process: Option<PassthroughProcess>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum PassthroughProcess {
    Fzf,
    Ssh,
    Vim,
}

pub(crate) fn run(mut args: impl Iterator<Item = String>) -> Result<()> {
    let direction = args
        .next()
        .ok_or_else(|| anyhow!("nav missing direction: expected left, down, up, or right"))?;
    if let Some(extra) = args.next() {
        return Err(anyhow!("unexpected nav argument '{extra}'"));
    }
    run_direction(&direction)
}

pub(crate) fn run_direction(direction: &str) -> Result<()> {
    let direction = Direction::parse(direction)?;
    let herdr = non_empty_env("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".to_string());
    let env_pane_id = non_empty_env("HERDR_PANE_ID");
    let process_info = focused_process_info(&herdr, env_pane_id.as_deref()).unwrap_or_default();
    let pane_id = env_pane_id.or(process_info.pane_id);

    if let (Some(process), Some(pane_id)) = (process_info.passthrough_process, pane_id) {
        return run_herdr(
            &herdr,
            &["pane", "send-keys", pane_id.as_str(), direction.key()],
        )
        .with_context(|| {
            format!(
                "failed to send {} to focused {} pane",
                direction.key(),
                process.label()
            )
        });
    }

    run_herdr(
        &herdr,
        &[
            "pane",
            "focus",
            "--direction",
            direction.as_str(),
            "--current",
        ],
    )
    .with_context(|| format!("failed to focus Herdr pane {}", direction.as_str()))
}

impl Direction {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "left" => Ok(Self::Left),
            "down" => Ok(Self::Down),
            "up" => Ok(Self::Up),
            "right" => Ok(Self::Right),
            _ => Err(anyhow!(
                "unknown nav direction '{value}': expected left, down, up, or right"
            )),
        }
    }

    fn as_str(self) -> &'static str {
        match self {
            Self::Left => "left",
            Self::Down => "down",
            Self::Up => "up",
            Self::Right => "right",
        }
    }

    fn key(self) -> &'static str {
        match self {
            Self::Left => "ctrl+h",
            Self::Down => "ctrl+j",
            Self::Up => "ctrl+k",
            Self::Right => "ctrl+l",
        }
    }
}

fn focused_process_info(herdr: &str, pane_id: Option<&str>) -> Result<FocusedProcessInfo> {
    let mut command = Command::new(herdr);
    command.args(["pane", "process-info"]);
    if let Some(pane_id) = pane_id {
        command.args(["--pane", pane_id]);
    } else {
        command.arg("--current");
    }

    let output = command
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("failed to run {herdr} pane process-info"))?;

    if !output.status.success() {
        return Err(anyhow!(
            "herdr pane process-info --current failed with {}",
            output.status
        ));
    }

    let value: Value = serde_json::from_slice(&output.stdout)
        .context("failed to parse herdr pane process-info output")?;
    let process_info = value
        .get("result")
        .and_then(|result| result.get("process_info"))
        .unwrap_or(&value);
    let pane_id = string_at(process_info, &["pane_id"]);
    let foreground_processes = process_info
        .get("foreground_processes")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
    let passthrough_process = foreground_processes
        .iter()
        .find_map(process_should_receive_key)
        .or_else(|| descendant_process_should_receive_key(&foreground_processes));

    Ok(FocusedProcessInfo {
        pane_id,
        passthrough_process,
    })
}

fn process_should_receive_key(process: &Value) -> Option<PassthroughProcess> {
    ["name", "argv0"]
        .into_iter()
        .filter_map(|field| string_at(process, &[field]))
        .map(|name| process_basename(&name).to_ascii_lowercase())
        .find_map(|name| PassthroughProcess::try_from(name.as_str()).ok())
}

fn descendant_process_should_receive_key(processes: &[Value]) -> Option<PassthroughProcess> {
    let roots = processes.iter().filter_map(process_pid).collect::<Vec<_>>();
    if roots.is_empty() {
        return None;
    }

    let Ok(table) = ProcessTable::load() else {
        return None;
    };

    let mut stack = roots;
    let mut visited = HashSet::new();
    while let Some(parent_pid) = stack.pop() {
        if !visited.insert(parent_pid) {
            continue;
        }
        if let Some(children) = table.children.get(&parent_pid) {
            for child in children {
                let name = process_basename(&child.command).to_ascii_lowercase();
                if let Ok(process) = PassthroughProcess::try_from(name.as_str()) {
                    return Some(process);
                }
                stack.push(child.pid);
            }
        }
    }

    None
}

impl TryFrom<&str> for PassthroughProcess {
    type Error = ();

    fn try_from(name: &str) -> Result<Self, Self::Error> {
        if fzf_process_regex().is_match(name) {
            Ok(Self::Fzf)
        } else if ssh_process_regex().is_match(name) {
            Ok(Self::Ssh)
        } else if vim_process_regex().is_match(name) {
            Ok(Self::Vim)
        } else {
            Err(())
        }
    }
}

impl PassthroughProcess {
    fn label(self) -> &'static str {
        match self {
            Self::Fzf => "fzf",
            Self::Ssh => "SSH",
            Self::Vim => "Vim/Neovim",
        }
    }
}

fn process_pid(process: &Value) -> Option<u32> {
    process
        .get("pid")
        .and_then(Value::as_u64)
        .and_then(|pid| u32::try_from(pid).ok())
}

#[derive(Debug)]
struct ProcessEntry {
    pid: u32,
    command: String,
}

#[derive(Debug)]
struct ProcessTable {
    children: HashMap<u32, Vec<ProcessEntry>>,
}

impl ProcessTable {
    fn load() -> Result<Self> {
        let output = Command::new("ps")
            .args(["-axo", "pid=,ppid=,comm="])
            .stdin(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .context("failed to run ps for process tree inspection")?;
        if !output.status.success() {
            return Err(anyhow!("ps process tree inspection failed"));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut children: HashMap<u32, Vec<ProcessEntry>> = HashMap::new();
        for line in stdout.lines() {
            let mut parts = line.split_whitespace();
            let Some(pid) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
                continue;
            };
            let Some(ppid) = parts.next().and_then(|value| value.parse::<u32>().ok()) else {
                continue;
            };
            let Some(command) = parts.next() else {
                continue;
            };
            children.entry(ppid).or_default().push(ProcessEntry {
                pid,
                command: command.to_string(),
            });
        }

        Ok(Self { children })
    }
}

fn process_basename(name: &str) -> String {
    Path::new(name)
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .unwrap_or(name)
        .to_string()
}

fn vim_process_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^g?(view|l?n?vim?x?)(diff)?$").expect("valid Vim regex"))
}

fn ssh_process_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(ssh|mosh-client)$").expect("valid SSH regex"))
}

fn fzf_process_regex() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(fzf|fzf-tmux)$").expect("valid fzf regex"))
}

fn run_herdr(herdr: &str, args: &[&str]) -> Result<()> {
    let status = Command::new(herdr)
        .args(args)
        .stdin(Stdio::null())
        .status()
        .with_context(|| format!("failed to run {herdr} {}", args.join(" ")))?;

    if !status.success() {
        return Err(anyhow!("herdr {} failed with {status}", args.join(" ")));
    }

    Ok(())
}
