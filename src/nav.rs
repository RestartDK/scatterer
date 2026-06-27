use crate::util::{non_empty_env, string_at};
use anyhow::{Context, Result, anyhow};
use regex::Regex;
use serde_json::Value;
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
    is_passthrough: bool,
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
    let process_info = focused_process_info(&herdr).unwrap_or_default();
    let pane_id = non_empty_env("HERDR_PANE_ID").or(process_info.pane_id);

    if process_info.is_passthrough {
        if let Some(pane_id) = pane_id {
            return run_herdr(
                &herdr,
                &["pane", "send-keys", pane_id.as_str(), direction.key()],
            )
            .with_context(|| {
                format!("failed to send {} to focused Vim/SSH pane", direction.key())
            });
        }
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

fn focused_process_info(herdr: &str) -> Result<FocusedProcessInfo> {
    let output = Command::new(herdr)
        .args(["pane", "process-info", "--current"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .with_context(|| format!("failed to run {herdr} pane process-info --current"))?;

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
    let is_passthrough = process_info
        .get("foreground_processes")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .any(process_should_receive_key);

    Ok(FocusedProcessInfo {
        pane_id,
        is_passthrough,
    })
}

fn process_should_receive_key(process: &Value) -> bool {
    ["name", "argv0"]
        .into_iter()
        .filter_map(|field| string_at(process, &[field]))
        .map(|name| process_basename(&name).to_ascii_lowercase())
        .any(|name| vim_process_regex().is_match(&name) || ssh_process_regex().is_match(&name))
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
