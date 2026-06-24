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
use serde::Deserialize;
use serde_json::{Value, json};
use std::env;
use std::fs;
use std::io::{self, BufRead, BufReader, Write};
#[cfg(unix)]
use std::os::unix::net::UnixStream;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
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
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickField {
    Prompt,
    Branch,
    Harness,
}

#[derive(Debug)]
struct QuickStartApp {
    prompt: String,
    branch: String,
    harness: Harness,
    field: QuickField,
    error: Option<String>,
}

impl Default for QuickStartApp {
    fn default() -> Self {
        Self {
            prompt: String::new(),
            branch: String::new(),
            harness: Harness::Pi,
            field: QuickField::Prompt,
            error: None,
        }
    }
}

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("apply-layout") | None => apply_layout(),
        Some("open-quick-start") => open_quick_start(),
        Some("quick-start") => quick_start(),
        Some(other) => Err(anyhow!(
            "unknown command '{other}'. Try: scatterer apply-layout | open-quick-start | quick-start"
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
    let pi_command = pi_prompt_command(&form.prompt);

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

fn focus_workspace_later(workspace_id: &str) {
    let herdr = non_empty_env("HERDR_BIN_PATH").unwrap_or_else(|| "herdr".to_string());
    let script = "sleep 0.25; \"$1\" workspace focus \"$2\" >/dev/null 2>&1 || true";
    let _ = Command::new("sh")
        .arg("-c")
        .arg(script)
        .arg("scatterer-focus")
        .arg(herdr)
        .arg(workspace_id)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
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

fn pi_prompt_command(prompt: &str) -> String {
    let name = quick_start_name(prompt);
    format!(
        "if command -v pi >/dev/null 2>&1; then pi --name {} {}; else echo 'pi not found on PATH'; fi",
        shell_quote(&name),
        shell_quote(prompt),
    )
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
    let mut app = QuickStartApp::default();
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
        KeyCode::Left | KeyCode::Right if app.field == QuickField::Harness => Ok(None),
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
            QuickField::Branch => QuickField::Harness,
            QuickField::Harness => QuickField::Prompt,
        };
    }

    fn previous_field(&mut self) {
        self.error = None;
        self.field = match self.field {
            QuickField::Prompt => QuickField::Harness,
            QuickField::Branch => QuickField::Prompt,
            QuickField::Harness => QuickField::Branch,
        };
    }

    fn insert_char(&mut self, ch: char) {
        self.error = None;
        match self.field {
            QuickField::Prompt => self.prompt.push(ch),
            QuickField::Branch => self.branch.push(ch),
            QuickField::Harness => {}
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.error = None;
        match self.field {
            QuickField::Prompt => self.prompt.push_str(text),
            QuickField::Branch => self.branch.push_str(&text.replace(['\n', '\r', '\t'], "-")),
            QuickField::Harness => {}
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
            QuickField::Harness => {}
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
        })
    }
}

fn draw_quick_start(frame: &mut Frame<'_>, app: &QuickStartApp) {
    let area = frame.area();
    frame.render_widget(Clear, area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(5),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .split(area);

    let title = Paragraph::new(vec![
        Line::from(vec![Span::styled(
            "Scatterer quick start",
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        )]),
        Line::from("Create a worktree, apply your layout, and start Pi with an initial prompt."),
    ])
    .block(Block::default().borders(Borders::ALL));
    frame.render_widget(title, chunks[0]);

    render_text_field(
        frame,
        chunks[1],
        "Prompt",
        "Describe what Pi should do...",
        &app.prompt,
        app.field == QuickField::Prompt,
    );
    render_text_field(
        frame,
        chunks[2],
        "Branch (optional)",
        "auto: quick/<prompt-slug>-<timestamp>",
        &app.branch,
        app.field == QuickField::Branch,
    );

    let harness_text = vec![Line::from(vec![
        Span::styled("● pi", Style::default().fg(Color::Green)),
        Span::raw("  "),
        Span::styled(
            "opencode/codex/etc later",
            Style::default().fg(Color::DarkGray),
        ),
    ])];
    let harness = Paragraph::new(harness_text)
        .block(field_block("Harness", app.field == QuickField::Harness));
    frame.render_widget(harness, chunks[3]);

    let footer_text = if let Some(error) = &app.error {
        vec![Line::from(vec![Span::styled(
            error,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )])]
    } else {
        vec![Line::from(
            "Tab/↓ next · Shift-Tab/↑ previous · Enter/Ctrl-S submit · Esc cancel",
        )]
    };
    frame.render_widget(Paragraph::new(footer_text), chunks[4]);
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
