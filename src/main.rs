use anyhow::{Context, Result, anyhow};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use regex::Regex;
use serde::Deserialize;
use serde_json::{Value, json};
use std::collections::HashSet;
use std::env;
use std::fs;
use std::fs::OpenOptions;
use std::io::{self, BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
#[cfg(unix)]
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

#[derive(Debug, Default, Deserialize)]
struct ProjectConfig {
    #[serde(default)]
    layout: LayoutConfig,
}

#[derive(Debug, Default, Deserialize)]
struct LayoutConfig {
    /// Command for the agent pane in tab 1.
    agent: Option<String>,
    /// Command for the hunk pane in tab 1.
    #[serde(alias = "diff")]
    hunk: Option<String>,
    /// Command for the single runner pane in tab 2.
    runner: Option<String>,
    /// Command for the single git pane in tab 3.
    git: Option<String>,
}

#[derive(Debug)]
struct InvocationSource {
    cwd: PathBuf,
}

#[derive(Debug)]
struct CreatedWorkspace {
    workspace_id: String,
    initial_tab_id: String,
}

#[derive(Debug)]
struct CreatedWorktree {
    workspace_id: String,
    initial_tab_id: Option<String>,
    path: PathBuf,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Harness {
    Pi,
}

impl Harness {
    fn label(self) -> &'static str {
        match self {
            Harness::Pi => "pi",
        }
    }
}

#[derive(Debug)]
struct QuickStartForm {
    prompt: String,
    branch: String,
    harness: Harness,
    model: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickField {
    Prompt,
    Branch,
    Model,
}

#[derive(Debug)]
struct QuickStartApp {
    prompt: String,
    branch: String,
    harness: Harness,
    models: Vec<String>,
    model_index: usize,
    field: QuickField,
    error: Option<String>,
}

impl QuickStartApp {
    fn new() -> Self {
        Self {
            prompt: String::new(),
            branch: String::new(),
            harness: Harness::Pi,
            models: available_pi_models(),
            model_index: 0,
            field: QuickField::Prompt,
            error: None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrState {
    Open,
    Draft,
    Merged,
    Closed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CheckState {
    Pass,
    Pending,
    Fail,
    None,
}

#[derive(Debug, Clone)]
struct PrRow {
    url: String,
    number: u64,
    title: String,
    state: PrState,
    checks: CheckState,
    comments: usize,
    review: Option<String>,
    agent: String,
    agent_status: String,
    branch: String,
    workspace_id: String,
    pane_id: String,
}

#[derive(Debug)]
struct PrPickerApp {
    rows: Vec<PrRow>,
    selected: usize,
    status: Option<String>,
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("apply-layout") | None => apply_layout(),
        Some("open-quick-start") => open_quick_start(),
        Some("quick-start") => quick_start(),
        Some("open-pr-picker") => open_pr_picker(),
        Some("pr-picker") => pr_picker(),
        Some("focus-target") => focus_target(args),
        Some(other) => Err(anyhow!(
            "unknown command '{other}'. Try: scatterer apply-layout | open-quick-start | quick-start | open-pr-picker | pr-picker"
        )),
    }
}

fn apply_layout() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let source = resolve_invocation_source(&socket_path)?;
    let (config, config_path) = load_project_config(&source.cwd)?;
    let created = create_workspace(&socket_path, &source.cwd)?;

    apply_scatterer_layout(
        &socket_path,
        &created.workspace_id,
        Some(&created.initial_tab_id),
        &source.cwd,
        &config,
        None,
    )?;

    println!(
        "scatterer: created workspace {} and applied layout",
        created.workspace_id
    );
    println!("scatterer: cwd {}", source.cwd.display());
    if let Some(path) = config_path {
        println!("scatterer: project config {}", path.display());
    } else {
        println!("scatterer: no .scatterer.toml found; used default commands");
    }

    Ok(())
}

fn open_quick_start() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let plugin_id =
        non_empty_env("HERDR_PLUGIN_ID").unwrap_or_else(|| "daniel.scatterer".to_string());
    let source = resolve_invocation_source(&socket_path)?;
    socket_call(
        &socket_path,
        "plugin.pane.open",
        json!({
            "plugin_id": plugin_id,
            "entrypoint": "quick-start",
            "placement": "overlay",
            "focus": true,
            "env": {
                "SCATTERER_SOURCE_CWD": source.cwd.to_string_lossy(),
            },
        }),
    )
    .context("failed to open Scatterer quick-start overlay")?;
    Ok(())
}

fn quick_start() -> Result<()> {
    let Some(form) = run_quick_start_tui()? else {
        println!("scatterer: quick start cancelled");
        return Ok(());
    };

    let socket_path = herdr_socket_path()?;
    let source = resolve_invocation_source(&socket_path)?;
    let (config, config_path) = load_project_config(&source.cwd)?;
    let branch = branch_for_form(&form);
    let created = create_worktree(&socket_path, &source.cwd, &branch, &form.prompt)?;
    let pi_command = pi_prompt_command(&form);

    apply_scatterer_layout(
        &socket_path,
        &created.workspace_id,
        created.initial_tab_id.as_deref(),
        &created.path,
        &config,
        Some(&pi_command),
    )?;

    println!("scatterer: quick-started {} worktree", form.harness.label());
    println!("scatterer: branch {branch}");
    println!("scatterer: path {}", created.path.display());
    if let Some(path) = config_path {
        println!("scatterer: project config {}", path.display());
    }
    focus_workspace_later(&created.workspace_id);

    Ok(())
}

fn open_pr_picker() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let plugin_id =
        non_empty_env("HERDR_PLUGIN_ID").unwrap_or_else(|| "daniel.scatterer".to_string());
    socket_call(
        &socket_path,
        "plugin.pane.open",
        json!({
            "plugin_id": plugin_id,
            "entrypoint": "pr-picker",
            "placement": "overlay",
            "focus": true,
        }),
    )
    .context("failed to open Scatterer PR picker overlay")?;
    Ok(())
}

fn pr_picker() -> Result<()> {
    run_pr_picker_tui()
}

fn focus_workspace_later(workspace_id: &str) {
    spawn_focus_process(workspace_id, None);
}

fn focus_pr_later(row: &PrRow) {
    spawn_focus_process(&row.workspace_id, Some(&row.pane_id));
}

fn debug_log(message: &str) {
    if non_empty_env("SCATTERER_DEBUG").is_none() {
        return;
    }
    let path = env::temp_dir().join("scatterer-debug.log");
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

fn spawn_focus_process(workspace_id: &str, pane_id: Option<&str>) {
    let Ok(exe) = env::current_exe() else {
        return;
    };
    let Ok(socket_path) = herdr_socket_path() else {
        return;
    };
    debug_log(&format!(
        "spawn_focus_process exe={} socket={} workspace={} pane={:?}",
        exe.display(),
        socket_path.display(),
        workspace_id,
        pane_id
    ));

    let mut command = Command::new(exe);
    command
        .arg("focus-target")
        .arg(socket_path)
        .arg(workspace_id);
    if let Some(pane_id) = pane_id {
        command.arg(pane_id);
    }

    #[cfg(unix)]
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(io::Error::last_os_error());
            }
            Ok(())
        });
    }

    match command
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    {
        Ok(_) => debug_log("spawned focus-target helper"),
        Err(err) => debug_log(&format!("failed to spawn focus-target helper: {err}")),
    }
}

fn focus_target(mut args: impl Iterator<Item = String>) -> Result<()> {
    debug_log("focus_target start");
    let socket_path = args
        .next()
        .map(PathBuf::from)
        .ok_or_else(|| anyhow!("focus-target missing socket path"))?;
    let workspace_id = args
        .next()
        .ok_or_else(|| anyhow!("focus-target missing workspace id"))?;
    let pane_id = args.next();

    // Herdr overlay panes restore focus as they close. Retry via the socket API
    // from this detached helper so the final focus wins after restoration.
    debug_log(&format!(
        "focus_target socket={} workspace={} pane={:?}",
        socket_path.display(),
        workspace_id,
        pane_id
    ));
    for delay in [200_u64, 450, 800, 1200] {
        thread::sleep(Duration::from_millis(delay));
        match socket_call(
            &socket_path,
            "workspace.focus",
            json!({ "workspace_id": workspace_id }),
        ) {
            Ok(_) => debug_log("workspace.focus ok"),
            Err(err) => debug_log(&format!("workspace.focus err: {err}")),
        }
        if let Some(pane_id) = &pane_id {
            match socket_call(&socket_path, "agent.focus", json!({ "target": pane_id })) {
                Ok(_) => debug_log("agent.focus ok"),
                Err(err) => debug_log(&format!("agent.focus err: {err}")),
            }
        }
    }

    Ok(())
}

fn open_pr_in_browser(url: &str) {
    let _ = Command::new("gh")
        .args(["pr", "view", url, "--web"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

fn copy_to_clipboard(text: &str) -> Result<()> {
    let mut command = if cfg!(target_os = "macos") {
        Command::new("pbcopy")
    } else if command_exists("wl-copy") {
        Command::new("wl-copy")
    } else {
        let mut command = Command::new("xclip");
        command.args(["-selection", "clipboard"]);
        command
    };

    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .context("failed to start clipboard command")?;
    if let Some(stdin) = &mut child.stdin {
        stdin
            .write_all(text.as_bytes())
            .context("failed to write clipboard text")?;
    }
    let _ = child.wait();
    Ok(())
}

fn command_exists(command: &str) -> bool {
    Command::new("sh")
        .arg("-c")
        .arg("command -v \"$1\" >/dev/null 2>&1")
        .arg("scatterer-command-exists")
        .arg(command)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn apply_scatterer_layout(
    socket_path: &Path,
    workspace_id: &str,
    replace_tab_id: Option<&str>,
    cwd: &Path,
    config: &ProjectConfig,
    agent_override: Option<&str>,
) -> Result<()> {
    let cwd_string = cwd.to_string_lossy().to_string();
    let agent_default = config.layout.agent.as_deref().unwrap_or(
        "if command -v pi >/dev/null 2>&1; then pi; else echo 'pi not found on PATH'; fi",
    );
    let agent = agent_override.unwrap_or(agent_default);
    let hunk = config.layout.hunk.as_deref().unwrap_or(
        "if command -v hunk >/dev/null 2>&1; then hunk; else echo 'hunk not found on PATH'; fi",
    );
    let runner = config.layout.runner.as_deref().unwrap_or(
        "if command -v process-compose >/dev/null 2>&1 && { [ -f process-compose.yml ] || [ -f process-compose.yaml ]; }; then process-compose up; else echo 'No process-compose.yml/yaml found. Configure [layout].runner in .scatterer.toml to override this tab.'; fi",
    );
    let git = config.layout.git.as_deref().unwrap_or(
        "if command -v lazygit >/dev/null 2>&1; then lazygit; else echo 'lazygit not found; showing git status'; git status --short; fi",
    );

    let dev_root = json!({
        "type": "split",
        "direction": "right",
        "ratio": 0.58,
        "first": pane("pi", &cwd_string, agent),
        "second": pane("hunk", &cwd_string, hunk),
    });

    apply_tab(
        socket_path,
        workspace_id,
        replace_tab_id,
        "agent",
        dev_root,
        true,
    )?;
    apply_tab(
        socket_path,
        workspace_id,
        None,
        "runner",
        pane("runner", &cwd_string, runner),
        false,
    )?;
    apply_tab(
        socket_path,
        workspace_id,
        None,
        "git",
        pane("lazygit", &cwd_string, git),
        false,
    )?;

    Ok(())
}

fn pane(label: &str, cwd: &str, command: &str) -> Value {
    json!({
        "type": "pane",
        "label": label,
        "cwd": cwd,
        "command": shell_command(command),
    })
}

fn shell_command(command: &str) -> Value {
    let wrapped = format!(
        r#"{command}
status=$?
printf '\n[scatterer] command exited with status %s; starting shell...\n' "$status"
exec "${{SHELL:-sh}}"
"#
    );
    json!(["sh", "-lc", wrapped])
}

fn pi_prompt_command(form: &QuickStartForm) -> String {
    let name = quick_start_name(&form.prompt);
    let mut command = format!(
        "if command -v pi >/dev/null 2>&1; then pi --name {}",
        shell_quote(&name),
    );
    if let Some(model) = &form.model {
        command.push_str(" --model ");
        command.push_str(&shell_quote(model));
    }
    command.push(' ');
    command.push_str(&shell_quote(&form.prompt));
    command.push_str("; else echo 'pi not found on PATH'; fi");
    command
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn create_workspace(socket_path: &Path, cwd: &Path) -> Result<CreatedWorkspace> {
    let cwd_string = cwd.to_string_lossy().to_string();
    let label = workspace_label(cwd);
    let result = socket_call(
        socket_path,
        "workspace.create",
        json!({
            "cwd": cwd_string,
            "label": label,
            "focus": true,
        }),
    )
    .context("failed to create Scatterer workspace")?;

    let workspace_id = first_string(
        &result,
        &[
            &["workspace", "workspace_id"],
            &["workspace", "id"],
            &["workspace_id"],
        ],
    )
    .ok_or_else(|| anyhow!("workspace.create response did not include a workspace id: {result}"))?;

    let initial_tab_id = first_string(
        &result,
        &[
            &["tab", "tab_id"],
            &["tab", "id"],
            &["root_pane", "tab_id"],
            &["pane", "tab_id"],
            &["tab_id"],
        ],
    )
    .ok_or_else(|| {
        anyhow!("workspace.create response did not include an initial tab id: {result}")
    })?;

    Ok(CreatedWorkspace {
        workspace_id,
        initial_tab_id,
    })
}

fn create_worktree(
    socket_path: &Path,
    cwd: &Path,
    branch: &str,
    prompt: &str,
) -> Result<CreatedWorktree> {
    let cwd_string = cwd.to_string_lossy().to_string();
    let label = quick_start_name(prompt);
    let result = socket_call(
        socket_path,
        "worktree.create",
        json!({
            "cwd": cwd_string,
            "branch": branch,
            "label": label,
            "focus": true,
        }),
    )
    .context("failed to create quick-start worktree")?;

    let workspace_id = first_string(
        &result,
        &[
            &["workspace", "workspace_id"],
            &["workspace", "id"],
            &["workspace_id"],
        ],
    )
    .ok_or_else(|| anyhow!("worktree.create response did not include a workspace id: {result}"))?;

    let initial_tab_id = first_string(
        &result,
        &[
            &["tab", "tab_id"],
            &["tab", "id"],
            &["root_pane", "tab_id"],
            &["pane", "tab_id"],
            &["tab_id"],
        ],
    );

    let path = first_string(
        &result,
        &[&["worktree", "path"], &["workspace", "cwd"], &["path"]],
    )
    .map(PathBuf::from)
    .unwrap_or_else(|| cwd.to_path_buf());

    Ok(CreatedWorktree {
        workspace_id,
        initial_tab_id,
        path,
    })
}

fn workspace_label(cwd: &Path) -> String {
    let name = cwd
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("workspace");
    format!("{name} · scatterer")
}

fn quick_start_name(prompt: &str) -> String {
    let slug = slugify(prompt, 42);
    format!("quick {slug}")
}

fn branch_for_form(form: &QuickStartForm) -> String {
    let branch = form.branch.trim();
    if !branch.is_empty() {
        return branch.to_string();
    }

    let slug = slugify(&form.prompt, 48);
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    format!("quick/{slug}-{seconds}")
}

fn available_pi_models() -> Vec<String> {
    let output = Command::new("pi")
        .arg("--list-models")
        .stdin(Stdio::null())
        .output();

    let mut models = vec!["default".to_string()];
    let Ok(output) = output else {
        return models;
    };
    if !output.status.success() {
        return models;
    }

    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines().skip(1) {
        let mut parts = line.split_whitespace();
        let Some(provider) = parts.next() else {
            continue;
        };
        let Some(model) = parts.next() else {
            continue;
        };
        models.push(format!("{provider}/{model}"));
    }
    models.sort();
    models.dedup();
    models
}

fn slugify(value: &str, max_len: usize) -> String {
    let mut slug = String::new();
    let mut last_dash = false;
    for ch in value.chars() {
        let lower = ch.to_ascii_lowercase();
        if lower.is_ascii_alphanumeric() {
            slug.push(lower);
            last_dash = false;
        } else if !last_dash && !slug.is_empty() {
            slug.push('-');
            last_dash = true;
        }
        if slug.len() >= max_len {
            break;
        }
    }
    while slug.ends_with('-') {
        slug.pop();
    }
    if slug.is_empty() {
        "task".to_string()
    } else {
        slug
    }
}

fn apply_tab(
    socket_path: &Path,
    workspace_id: &str,
    replace_tab_id: Option<&str>,
    tab_label: &str,
    root: Value,
    focus: bool,
) -> Result<()> {
    let mut params = serde_json::Map::new();
    if let Some(tab_id) = replace_tab_id {
        // `layout.apply` accepts either `tab_id` for replacement or
        // `workspace_id` for a new tab, but not both.
        params.insert("tab_id".to_string(), json!(tab_id));
    } else {
        params.insert("workspace_id".to_string(), json!(workspace_id));
    }
    params.insert("tab_label".to_string(), json!(tab_label));
    params.insert("focus".to_string(), json!(focus));
    params.insert("root".to_string(), root);

    socket_call(socket_path, "layout.apply", Value::Object(params))
        .with_context(|| format!("failed to apply '{tab_label}' tab"))?;
    Ok(())
}

fn load_pr_rows(socket_path: &Path) -> Result<Vec<PrRow>> {
    let result =
        socket_call(socket_path, "agent.list", json!({})).context("failed to list Herdr agents")?;
    let agents = result
        .get("agents")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    let mut rows = Vec::new();
    let mut seen_urls = HashSet::new();
    for agent in agents {
        let pane_id = string_at(&agent, &["pane_id"]).unwrap_or_default();
        let workspace_id = string_at(&agent, &["workspace_id"]).unwrap_or_default();
        let agent_name = string_at(&agent, &["agent"])
            .or_else(|| string_at(&agent, &["display_agent"]))
            .unwrap_or_else(|| "agent".to_string());
        let agent_status =
            string_at(&agent, &["agent_status"]).unwrap_or_else(|| "unknown".to_string());
        let Some(cwd) = string_at(&agent, &["foreground_cwd"])
            .or_else(|| string_at(&agent, &["cwd"]))
            .map(PathBuf::from)
        else {
            continue;
        };
        if pane_id.is_empty() || workspace_id.is_empty() || !cwd.is_dir() {
            continue;
        }

        let branch = git_branch(&cwd).unwrap_or_else(|| "detached".to_string());
        let url =
            pr_url_from_pane(socket_path, &pane_id).or_else(|| pr_url_for_branch(&cwd, &branch));
        let Some(url) = url else {
            continue;
        };
        if !seen_urls.insert(url.clone()) {
            continue;
        }

        if let Some(row) = pr_row_from_gh(
            &cwd,
            &url,
            &agent_name,
            &agent_status,
            &branch,
            &workspace_id,
            &pane_id,
        ) {
            rows.push(row);
        }
    }

    rows.sort_by(|a, b| {
        pr_state_rank(a.state)
            .cmp(&pr_state_rank(b.state))
            .then_with(|| a.number.cmp(&b.number))
    });
    Ok(rows)
}

fn git_branch(cwd: &Path) -> Option<String> {
    let output = Command::new("git")
        .args(["-C", cwd.to_str()?, "branch", "--show-current"])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if branch.is_empty() {
        None
    } else {
        Some(branch)
    }
}

fn pr_url_from_pane(socket_path: &Path, pane_id: &str) -> Option<String> {
    let result = socket_call(
        socket_path,
        "pane.read",
        json!({
            "pane_id": pane_id,
            "source": "recent",
            "lines": 4000,
        }),
    )
    .ok()?;
    let text = string_at(&result, &["read", "text"])?;
    find_latest_pr_url(&text)
}

fn find_latest_pr_url(text: &str) -> Option<String> {
    let re = Regex::new(r"https://github\.com/[^/\s]+/[^/\s]+/pull/[0-9]+").ok()?;
    re.find_iter(text).last().map(|m| m.as_str().to_string())
}

fn pr_url_for_branch(cwd: &Path, branch: &str) -> Option<String> {
    if branch.is_empty() || branch == "detached" {
        return None;
    }
    let output = Command::new("gh")
        .current_dir(cwd)
        .args([
            "pr",
            "list",
            "--head",
            branch,
            "--state",
            "all",
            "--json",
            "url",
            "--jq",
            ".[0].url // empty",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if url.is_empty() { None } else { Some(url) }
}

fn pr_row_from_gh(
    cwd: &Path,
    url: &str,
    agent: &str,
    agent_status: &str,
    branch: &str,
    workspace_id: &str,
    pane_id: &str,
) -> Option<PrRow> {
    let output = Command::new("gh")
        .current_dir(cwd)
        .args([
            "pr",
            "view",
            url,
            "--json",
            "number,title,state,isDraft,mergedAt,url,reviewDecision,statusCheckRollup,comments,headRefName,baseRefName",
        ])
        .stdin(Stdio::null())
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let value: Value = serde_json::from_slice(&output.stdout).ok()?;
    let number = value
        .get("number")
        .and_then(Value::as_u64)
        .unwrap_or_default();
    let title = string_at(&value, &["title"]).unwrap_or_else(|| "Untitled PR".to_string());
    let state = parse_pr_state(&value);
    let checks = parse_check_state(value.get("statusCheckRollup"));
    let comments = value
        .get("comments")
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or_default();
    let review = string_at(&value, &["reviewDecision"]).filter(|value| value != "");
    let head = string_at(&value, &["headRefName"]).unwrap_or_else(|| branch.to_string());

    Some(PrRow {
        url: string_at(&value, &["url"]).unwrap_or_else(|| url.to_string()),
        number,
        title,
        state,
        checks,
        comments,
        review,
        agent: agent.to_string(),
        agent_status: agent_status.to_string(),
        branch: head,
        workspace_id: workspace_id.to_string(),
        pane_id: pane_id.to_string(),
    })
}

fn parse_pr_state(value: &Value) -> PrState {
    let state = string_at(value, &["state"]).unwrap_or_default();
    let is_draft = value
        .get("isDraft")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let merged_at = string_at(value, &["mergedAt"]);
    if state == "MERGED" || merged_at.is_some() {
        PrState::Merged
    } else if is_draft {
        PrState::Draft
    } else if state == "CLOSED" {
        PrState::Closed
    } else {
        PrState::Open
    }
}

fn parse_check_state(value: Option<&Value>) -> CheckState {
    let Some(checks) = value.and_then(Value::as_array) else {
        return CheckState::None;
    };
    if checks.is_empty() {
        return CheckState::None;
    }

    let mut pending = false;
    for check in checks {
        let conclusion = string_at(check, &["conclusion"]).unwrap_or_default();
        let state = string_at(check, &["state"]).unwrap_or_default();
        if matches!(
            conclusion.as_str(),
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED"
        ) || matches!(
            state.as_str(),
            "FAILURE" | "ERROR" | "TIMED_OUT" | "CANCELLED"
        ) {
            return CheckState::Fail;
        }
        if conclusion.is_empty()
            || matches!(
                state.as_str(),
                "PENDING" | "IN_PROGRESS" | "QUEUED" | "REQUESTED" | "WAITING" | "EXPECTED"
            )
        {
            pending = true;
        }
    }
    if pending {
        CheckState::Pending
    } else {
        CheckState::Pass
    }
}

fn pr_state_rank(state: PrState) -> u8 {
    match state {
        PrState::Open => 0,
        PrState::Draft => 1,
        PrState::Closed => 2,
        PrState::Merged => 3,
    }
}

fn run_quick_start_tui() -> Result<Option<QuickStartForm>> {
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    terminal.clear().context("failed to clear terminal")?;

    let result = run_quick_start_loop(&mut terminal);

    let leave_result = execute!(terminal.backend_mut(), LeaveAlternateScreen)
        .context("failed to leave alternate screen");
    let raw_result = disable_raw_mode().context("failed to disable raw mode");
    terminal.show_cursor().ok();

    let form = result?;
    leave_result?;
    raw_result?;
    Ok(form)
}

fn run_quick_start_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<Option<QuickStartForm>> {
    let mut app = QuickStartApp::new();
    loop {
        terminal
            .draw(|frame| draw_quick_start(frame, &app))
            .context("failed to draw quick-start UI")?;

        if !event::poll(Duration::from_millis(250)).context("failed to poll terminal event")? {
            continue;
        }

        match event::read().context("failed to read terminal event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                if let Some(form) = handle_quick_start_key(&mut app, key)? {
                    return Ok(form);
                }
            }
            Event::Paste(text) => app.insert_text(&text),
            _ => {}
        }
    }
}

fn run_pr_picker_tui() -> Result<()> {
    debug_log("run_pr_picker_tui start");
    enable_raw_mode().context("failed to enable raw mode")?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen).context("failed to enter alternate screen")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    terminal.clear().context("failed to clear terminal")?;

    let result = run_pr_picker_loop(&mut terminal);

    // Always act on the selected PR even if terminal cleanup reports an error
    // while the overlay pane is closing.
    let action = result?;
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = disable_raw_mode();
    terminal.show_cursor().ok();

    debug_log(&format!("run_pr_picker_tui action={action:?}"));
    if let Some(action) = action {
        match action {
            PrPickerAction::Focus(row) => focus_pr_later(&row),
        }
    }

    Ok(())
}

#[derive(Debug)]
enum PrPickerAction {
    Focus(PrRow),
}

fn run_pr_picker_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
) -> Result<Option<PrPickerAction>> {
    let socket_path = herdr_socket_path()?;
    let (rows, status) = match load_pr_rows(&socket_path) {
        Ok(rows) if rows.is_empty() => (
            rows,
            Some("No PRs found for active Herdr agents".to_string()),
        ),
        Ok(rows) => (rows, None),
        Err(err) => (vec![], Some(format!("failed to load PRs: {err}"))),
    };
    debug_log(&format!("pr picker loaded rows={}", rows.len()));
    let mut app = PrPickerApp {
        rows,
        selected: 0,
        status,
    };

    loop {
        terminal
            .draw(|frame| draw_pr_picker(frame, &app))
            .context("failed to draw PR picker UI")?;

        if !event::poll(Duration::from_millis(250)).context("failed to poll terminal event")? {
            continue;
        }

        match event::read().context("failed to read terminal event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                debug_log(&format!("pr picker key={:?}", key.code));
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                    KeyCode::Up | KeyCode::Char('k') => {
                        if !app.rows.is_empty() {
                            app.selected = app.selected.saturating_sub(1);
                        }
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        if !app.rows.is_empty() {
                            app.selected = (app.selected + 1).min(app.rows.len() - 1);
                        }
                    }
                    KeyCode::Enter => {
                        if let Some(row) = app.rows.get(app.selected).cloned() {
                            debug_log(&format!(
                                "pr picker enter selected={} workspace={} pane={}",
                                app.selected, row.workspace_id, row.pane_id
                            ));
                            return Ok(Some(PrPickerAction::Focus(row)));
                        } else {
                            debug_log("pr picker enter with no row");
                        }
                    }
                    KeyCode::Char('o') => {
                        if let Some(row) = app.rows.get(app.selected) {
                            open_pr_in_browser(&row.url);
                            app.status = Some(format!("opened #{}", row.number));
                        }
                    }
                    KeyCode::Char('y') => {
                        if let Some(row) = app.rows.get(app.selected) {
                            app.status = match copy_to_clipboard(&row.url) {
                                Ok(()) => Some(format!("copied #{} URL", row.number)),
                                Err(err) => Some(format!("copy failed: {err}")),
                            };
                        }
                    }
                    KeyCode::Char('r') => match load_pr_rows(&socket_path) {
                        Ok(rows) => {
                            app.rows = rows;
                            app.selected = app.selected.min(app.rows.len().saturating_sub(1));
                            app.status = if app.rows.is_empty() {
                                Some("No PRs found for active Herdr agents".to_string())
                            } else {
                                Some("refreshed".to_string())
                            };
                        }
                        Err(err) => app.status = Some(format!("refresh failed: {err}")),
                    },
                    _ => {}
                }
            }
            _ => {}
        }
    }
}

fn draw_pr_picker(frame: &mut Frame<'_>, app: &PrPickerApp) {
    let height = (app.rows.len() as u16)
        .saturating_mul(2)
        .saturating_add(4)
        .clamp(8, 22);
    let area = centered_rect(frame.area(), 112, height);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().borders(Borders::ALL), area);

    let inner = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([Constraint::Min(2), Constraint::Length(1)])
        .split(area);

    let lines = if app.rows.is_empty() {
        vec![Line::from(Span::styled(
            app.status
                .as_deref()
                .unwrap_or("No PRs found for active Herdr agents"),
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        let mut lines = Vec::new();
        for (index, row) in app.rows.iter().enumerate() {
            let selected = index == app.selected;
            lines.push(pr_row_main_line(row, selected));
            lines.push(pr_row_detail_line(row, selected));
        }
        lines
    };
    frame.render_widget(Paragraph::new(lines), inner[0]);

    let controls = "↑/↓ j/k select · Enter focus · o open · r refresh · y copy URL · q/Esc close";
    let footer = if let Some(status) = app.status.as_deref() {
        format!("{status} · {controls}")
    } else {
        controls.to_string()
    };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            footer,
            Style::default().fg(Color::DarkGray),
        ))),
        inner[1],
    );
}

fn pr_row_main_line(row: &PrRow, selected: bool) -> Line<'static> {
    let base = if selected {
        Style::default().bg(Color::DarkGray).fg(Color::White)
    } else {
        Style::default()
    };
    let state_style = base.fg(pr_state_color(row.state));
    let check_style = base.fg(check_state_color(row.checks));
    let review = row.review.as_deref().unwrap_or("-");
    Line::from(vec![
        Span::styled(format!("{} ", pr_state_icon(row.state)), state_style),
        Span::styled(
            format!("#{:<5}", row.number),
            base.add_modifier(Modifier::BOLD),
        ),
        Span::styled(format!(" {:<6} ", pr_state_label(row.state)), state_style),
        Span::styled(format!("{} ci ", check_state_icon(row.checks)), check_style),
        Span::styled(
            format!("{} {:<2} ", COMMENT_ICON, row.comments),
            base.fg(Color::Cyan),
        ),
        Span::styled(format!("{:<14}", review), base.fg(review_color(review))),
        Span::styled(truncate(&row.title, 56), base),
    ])
}

fn pr_row_detail_line(row: &PrRow, selected: bool) -> Line<'static> {
    let style = if selected {
        Style::default().bg(Color::DarkGray).fg(Color::Gray)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    Line::from(vec![Span::styled(
        format!(
            "  {} · {} · {} · {}",
            row.agent, row.agent_status, row.branch, row.url
        ),
        style,
    )])
}

const PR_OPEN_ICON: &str = "\u{F407}";
const PR_MERGED_ICON: &str = "\u{F419}";
const PR_CLOSED_ICON: &str = "\u{F4DC}";
const PR_DRAFT_ICON: &str = "\u{F4DD}";
const COMMENT_ICON: &str = "\u{F442}";
const CHECK_PASS_ICON: &str = "\u{F4A4}";
const CHECK_PENDING_ICON: &str = "\u{F4AA}";
const CHECK_FAIL_ICON: &str = "\u{F530}";

fn pr_state_icon(state: PrState) -> &'static str {
    match state {
        PrState::Open => PR_OPEN_ICON,
        PrState::Draft => PR_DRAFT_ICON,
        PrState::Merged => PR_MERGED_ICON,
        PrState::Closed => PR_CLOSED_ICON,
    }
}

fn pr_state_label(state: PrState) -> &'static str {
    match state {
        PrState::Open => "OPEN",
        PrState::Draft => "DRAFT",
        PrState::Merged => "MERGED",
        PrState::Closed => "CLOSED",
    }
}

fn pr_state_color(state: PrState) -> Color {
    match state {
        PrState::Open => Color::Green,
        PrState::Draft => Color::DarkGray,
        PrState::Merged => Color::Magenta,
        PrState::Closed => Color::Red,
    }
}

fn check_state_icon(state: CheckState) -> &'static str {
    match state {
        CheckState::Pass => CHECK_PASS_ICON,
        CheckState::Pending => CHECK_PENDING_ICON,
        CheckState::Fail => CHECK_FAIL_ICON,
        CheckState::None => "-",
    }
}

fn check_state_color(state: CheckState) -> Color {
    match state {
        CheckState::Pass => Color::Green,
        CheckState::Pending => Color::Yellow,
        CheckState::Fail => Color::Red,
        CheckState::None => Color::DarkGray,
    }
}

fn review_color(review: &str) -> Color {
    match review {
        "APPROVED" => Color::Green,
        "CHANGES_REQUESTED" => Color::Red,
        "REVIEW_REQUIRED" => Color::Yellow,
        _ => Color::DarkGray,
    }
}

fn truncate(value: &str, max_chars: usize) -> String {
    if value.chars().count() <= max_chars {
        return value.to_string();
    }
    let mut out = value
        .chars()
        .take(max_chars.saturating_sub(1))
        .collect::<String>();
    out.push('…');
    out
}

fn handle_quick_start_key(
    app: &mut QuickStartApp,
    key: KeyEvent,
) -> Result<Option<Option<QuickStartForm>>> {
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Ok(Some(None));
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('s') {
        return Ok(app.to_form().map(Some));
    }

    match key.code {
        KeyCode::Esc => Ok(Some(None)),
        KeyCode::Enter => Ok(app.to_form().map(Some)),
        KeyCode::Tab | KeyCode::Down => {
            app.next_field();
            Ok(None)
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.previous_field();
            Ok(None)
        }
        KeyCode::Backspace => {
            app.backspace();
            Ok(None)
        }
        KeyCode::Left if app.field == QuickField::Model => {
            app.previous_model();
            Ok(None)
        }
        KeyCode::Right if app.field == QuickField::Model => {
            app.next_model();
            Ok(None)
        }
        KeyCode::Char(' ') if app.field == QuickField::Model => {
            app.next_model();
            Ok(None)
        }
        KeyCode::Char(ch) => {
            app.insert_char(ch);
            Ok(None)
        }
        _ => Ok(None),
    }
}

impl QuickStartApp {
    fn next_field(&mut self) {
        self.error = None;
        self.field = match self.field {
            QuickField::Prompt => QuickField::Branch,
            QuickField::Branch => QuickField::Model,
            QuickField::Model => QuickField::Prompt,
        };
    }

    fn previous_field(&mut self) {
        self.error = None;
        self.field = match self.field {
            QuickField::Prompt => QuickField::Model,
            QuickField::Branch => QuickField::Prompt,
            QuickField::Model => QuickField::Branch,
        };
    }

    fn insert_char(&mut self, ch: char) {
        self.error = None;
        match self.field {
            QuickField::Prompt => self.prompt.push(ch),
            QuickField::Branch => self.branch.push(ch),
            QuickField::Model => {}
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.error = None;
        match self.field {
            QuickField::Prompt => self.prompt.push_str(text),
            QuickField::Branch => self.branch.push_str(&text.replace(['\n', '\r', '\t'], "-")),
            QuickField::Model => {}
        }
    }

    fn backspace(&mut self) {
        self.error = None;
        match self.field {
            QuickField::Prompt => {
                self.prompt.pop();
            }
            QuickField::Branch => {
                self.branch.pop();
            }
            QuickField::Model => {}
        }
    }

    fn next_model(&mut self) {
        self.error = None;
        if !self.models.is_empty() {
            self.model_index = (self.model_index + 1) % self.models.len();
        }
    }

    fn previous_model(&mut self) {
        self.error = None;
        if !self.models.is_empty() {
            self.model_index = if self.model_index == 0 {
                self.models.len() - 1
            } else {
                self.model_index - 1
            };
        }
    }

    fn selected_model(&self) -> Option<String> {
        let model = self.models.get(self.model_index)?.trim();
        if model.is_empty() || model == "default" {
            None
        } else {
            Some(model.to_string())
        }
    }

    fn to_form(&mut self) -> Option<QuickStartForm> {
        let prompt = self.prompt.trim().to_string();
        if prompt.is_empty() {
            self.error = Some("Prompt is required".to_string());
            return None;
        }
        Some(QuickStartForm {
            prompt,
            branch: self.branch.trim().to_string(),
            harness: self.harness,
            model: self.selected_model(),
        })
    }
}

fn draw_quick_start(frame: &mut Frame<'_>, app: &QuickStartApp) {
    let area = centered_rect(frame.area(), 76, 13);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().borders(Borders::ALL), area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(area);

    render_text_field(
        frame,
        chunks[0],
        "Prompt",
        "What should Pi do?",
        &app.prompt,
        app.field == QuickField::Prompt,
    );
    render_text_field(
        frame,
        chunks[1],
        "Branch",
        "optional; auto: quick/<prompt-slug>-<timestamp>",
        &app.branch,
        app.field == QuickField::Branch,
    );

    let selected_model = app
        .models
        .get(app.model_index)
        .map(String::as_str)
        .unwrap_or("default");
    let model = Paragraph::new(vec![Line::from(vec![
        Span::styled("pi", Style::default().fg(Color::Green)),
        Span::raw("  "),
        Span::styled(selected_model, Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("←/→", Style::default().fg(Color::DarkGray)),
    ])])
    .block(field_block("Model", app.field == QuickField::Model));
    frame.render_widget(model, chunks[2]);

    let footer_text = if let Some(error) = &app.error {
        Line::from(vec![Span::styled(
            error,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )])
    } else {
        Line::from("Tab/↓ next · Shift-Tab/↑ previous · Enter submit · Esc cancel")
    };
    frame.render_widget(Paragraph::new(footer_text), chunks[3]);
}

fn centered_rect(container: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(container.width.saturating_sub(2)).max(20);
    let height = height.min(container.height.saturating_sub(2)).max(8);
    let x = container.x + container.width.saturating_sub(width) / 2;
    let y = container.y + container.height.saturating_sub(height) / 2;
    Rect {
        x,
        y,
        width,
        height,
    }
}

fn render_text_field(
    frame: &mut Frame<'_>,
    area: Rect,
    title: &'static str,
    placeholder: &'static str,
    value: &str,
    active: bool,
) {
    let lines = if value.is_empty() {
        vec![Line::from(Span::styled(
            placeholder,
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        value.lines().map(Line::from).collect::<Vec<_>>()
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .block(field_block(title, active));
    frame.render_widget(paragraph, area);
}

fn field_block(title: &'static str, active: bool) -> Block<'static> {
    let style = if active {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    Block::default()
        .title(title)
        .borders(Borders::ALL)
        .border_style(style)
}

fn resolve_invocation_source(socket_path: &Path) -> Result<InvocationSource> {
    if let Some(cwd) = non_empty_env("SCATTERER_SOURCE_CWD") {
        return Ok(InvocationSource {
            cwd: PathBuf::from(cwd),
        });
    }

    let context = env::var("HERDR_PLUGIN_CONTEXT_JSON")
        .ok()
        .and_then(|raw| serde_json::from_str::<Value>(&raw).ok());

    let pane_id =
        non_empty_env("HERDR_PANE_ID").or_else(|| context.as_ref().and_then(pane_id_from_context));
    let mut cwd = context
        .as_ref()
        .and_then(cwd_from_context)
        .map(PathBuf::from);

    if cwd.is_none() {
        let mut params = serde_json::Map::new();
        if let Some(pane_id) = &pane_id {
            params.insert("caller_pane_id".to_string(), json!(pane_id));
        }

        if let Ok(result) = socket_call(socket_path, "pane.current", Value::Object(params)) {
            let pane = result.get("pane").unwrap_or(&result);
            cwd = string_at(pane, &["foreground_cwd"])
                .or_else(|| string_at(pane, &["cwd"]))
                .map(PathBuf::from);
        }
    }

    let cwd = cwd.unwrap_or(env::current_dir().context("failed to resolve fallback cwd")?);

    Ok(InvocationSource { cwd })
}

fn pane_id_from_context(context: &Value) -> Option<String> {
    first_string(
        context,
        &[
            &["pane_id"],
            &["focused_pane", "pane_id"],
            &["pane", "pane_id"],
        ],
    )
}

fn cwd_from_context(context: &Value) -> Option<String> {
    first_string(
        context,
        &[
            &["focused_pane", "foreground_cwd"],
            &["focused_pane", "cwd"],
            &["pane", "foreground_cwd"],
            &["pane", "cwd"],
            &["workspace", "cwd"],
            &["worktree", "path"],
        ],
    )
}

fn first_string(value: &Value, paths: &[&[&str]]) -> Option<String> {
    paths.iter().find_map(|path| string_at(value, path))
}

fn string_at(value: &Value, path: &[&str]) -> Option<String> {
    let mut current = value;
    for key in path {
        current = current.get(*key)?;
    }
    current.as_str().and_then(|s| {
        let trimmed = s.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn non_empty_env(name: &str) -> Option<String> {
    env::var(name).ok().and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(trimmed.to_string())
        }
    })
}

fn load_project_config(cwd: &Path) -> Result<(ProjectConfig, Option<PathBuf>)> {
    let Some(path) = find_project_config(cwd) else {
        return Ok((ProjectConfig::default(), None));
    };

    let raw =
        fs::read_to_string(&path).with_context(|| format!("failed to read {}", path.display()))?;
    let config = toml::from_str::<ProjectConfig>(&raw)
        .with_context(|| format!("failed to parse {}", path.display()))?;
    Ok((config, Some(path)))
}

fn find_project_config(cwd: &Path) -> Option<PathBuf> {
    for dir in cwd.ancestors() {
        for name in [".scatterer.toml", "scatterer.toml"] {
            let candidate = dir.join(name);
            if candidate.is_file() {
                return Some(candidate);
            }
        }
    }
    None
}

fn herdr_socket_path() -> Result<PathBuf> {
    if let Some(path) = non_empty_env("HERDR_SOCKET_PATH") {
        return Ok(PathBuf::from(path));
    }

    let config_home = non_empty_env("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .or_else(|| non_empty_env("HOME").map(|home| PathBuf::from(home).join(".config")))
        .ok_or_else(|| anyhow!("HERDR_SOCKET_PATH is not set and HOME is unavailable"))?;

    let herdr_dir = config_home.join("herdr");
    let socket = match non_empty_env("HERDR_SESSION").as_deref() {
        Some(session) if session != "default" => {
            herdr_dir.join("sessions").join(session).join("herdr.sock")
        }
        _ => herdr_dir.join("herdr.sock"),
    };
    Ok(socket)
}

fn socket_call(socket_path: &Path, method: &str, params: Value) -> Result<Value> {
    #[cfg(not(unix))]
    {
        let _ = (socket_path, method, params);
        return Err(anyhow!(
            "scatterer currently supports Herdr's Unix socket only"
        ));
    }

    #[cfg(unix)]
    {
        let mut stream = UnixStream::connect(socket_path).with_context(|| {
            format!(
                "failed to connect to Herdr socket {}",
                socket_path.display()
            )
        })?;
        let request = json!({
            "id": request_id(method),
            "method": method,
            "params": params,
        });
        writeln!(stream, "{request}").context("failed to write Herdr socket request")?;
        stream
            .flush()
            .context("failed to flush Herdr socket request")?;

        let mut reader = BufReader::new(stream);
        let mut line = String::new();
        reader
            .read_line(&mut line)
            .context("failed to read Herdr socket response")?;
        if line.trim().is_empty() {
            return Err(anyhow!("Herdr socket returned an empty response"));
        }

        let response: Value = serde_json::from_str(&line)
            .with_context(|| format!("failed to parse Herdr socket response: {line}"))?;
        if let Some(error) = response.get("error") {
            let code = string_at(error, &["code"]).unwrap_or_else(|| "error".to_string());
            let message = string_at(error, &["message"]).unwrap_or_else(|| error.to_string());
            return Err(anyhow!("Herdr {method} failed: {code}: {message}"));
        }

        response
            .get("result")
            .cloned()
            .ok_or_else(|| anyhow!("Herdr {method} response did not include result: {response}"))
    }
}

fn request_id(method: &str) -> String {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    format!("scatterer-{method}-{millis}")
}
