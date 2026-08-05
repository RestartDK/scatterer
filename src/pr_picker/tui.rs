use super::agents::load_pr_rows;
use super::github::{OpenPrOutcome, open_pr_in_browser};
use super::{CheckState, PrRow, PrState, pr_state_icon};
use crate::focus::focus_pane;
use crate::herdr::herdr_socket_path;
use crate::terminal_session::TerminalSession;
use crate::theme::PickerPalette;
use crate::util::{command_exists, copy_to_terminal_clipboard, debug_log, is_ssh_session};
use anyhow::{Context, Result};
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph, Wrap},
};
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug)]
struct PrPickerApp {
    palette: PickerPalette,
    rows: Vec<PrRow>,
    selected: usize,
    scroll: usize,
    status: Option<String>,
}

impl PrPickerApp {
    fn selected_row(&self) -> Option<&PrRow> {
        self.rows.get(self.selected)
    }

    fn select_previous(&mut self) {
        if !self.rows.is_empty() {
            self.selected = self.selected.saturating_sub(1);
        }
    }

    fn select_next(&mut self) {
        if !self.rows.is_empty() {
            self.selected = (self.selected + 1).min(self.rows.len() - 1);
        }
    }

    fn clamp_selection(&mut self) {
        if self.rows.is_empty() {
            self.selected = 0;
            self.scroll = 0;
        } else {
            self.selected = self.selected.min(self.rows.len() - 1);
        }
    }

    fn ensure_selection_visible(&mut self, visible_rows: usize) {
        if visible_rows == 0 || self.rows.is_empty() {
            self.scroll = 0;
            return;
        }
        if self.selected < self.scroll {
            self.scroll = self.selected;
        }
        let last_visible = self.scroll.saturating_add(visible_rows).saturating_sub(1);
        if self.selected > last_visible {
            self.scroll = self.selected.saturating_add(1).saturating_sub(visible_rows);
        }
        self.scroll = self
            .scroll
            .min(self.rows.len().saturating_sub(visible_rows));
    }
}

#[derive(Debug)]
enum PrPickerAction {
    Focus(PrRow),
}

pub(super) fn run_pr_picker_tui() -> Result<()> {
    debug_log("run_pr_picker_tui start");
    let mut session = TerminalSession::enter(false)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    terminal.clear().context("failed to clear terminal")?;

    let result = run_pr_picker_loop(&mut terminal);
    terminal.show_cursor().ok();
    drop(terminal);
    let cleanup = session.finish();

    // Always act on the selected PR even if terminal cleanup reports an error
    // while the popup is closing.
    let action = result?;
    cleanup?;

    debug_log(&format!("run_pr_picker_tui action={action:?}"));
    if let Some(action) = action {
        match action {
            PrPickerAction::Focus(row) => focus_pane(&row.workspace_id, &row.pane_id)?,
        }
    }

    Ok(())
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
        palette: PickerPalette::load(),
        rows,
        selected: 0,
        scroll: 0,
        status,
    };

    loop {
        terminal
            .draw(|frame| draw_pr_picker(frame, &mut app))
            .context("failed to draw PR picker UI")?;

        if !event::poll(Duration::from_millis(250)).context("failed to poll terminal event")? {
            continue;
        }

        match event::read().context("failed to read terminal event")? {
            Event::Key(key) if key.kind == KeyEventKind::Press => {
                debug_log(&format!("pr picker key={:?}", key.code));
                match key.code {
                    KeyCode::Esc | KeyCode::Char('q') => return Ok(None),
                    KeyCode::Up | KeyCode::Char('k') => app.select_previous(),
                    KeyCode::Down | KeyCode::Char('j') => app.select_next(),
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
                        if let Some(row) = app.selected_row() {
                            let number = row.number;
                            app.status = match open_pr_in_browser(&row.url) {
                                Ok(OpenPrOutcome::Opened) => Some(format!("opened #{number}")),
                                Ok(OpenPrOutcome::CopiedToTerminalClipboard) => Some(format!(
                                    "copied #{number} URL to the local terminal clipboard"
                                )),
                                Err(err) => Some(format!("open failed for #{number}: {err}")),
                            };
                        }
                    }
                    KeyCode::Char('y') => {
                        if let Some(row) = app.selected_row() {
                            let number = row.number;
                            app.status = match copy_to_clipboard(&row.url) {
                                Ok(()) => Some(format!("copied #{number} URL")),
                                Err(err) => Some(format!("copy failed for #{number}: {err}")),
                            };
                        }
                    }
                    KeyCode::Char('r') => match load_pr_rows(&socket_path) {
                        Ok(rows) => {
                            app.rows = rows;
                            app.clamp_selection();
                            app.status = if app.rows.is_empty() {
                                Some("No PRs found for active Herdr agents".to_string())
                            } else {
                                Some(format!("refreshed {} PRs", app.rows.len()))
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

fn draw_pr_picker(frame: &mut Frame<'_>, app: &mut PrPickerApp) {
    let area = frame.area();
    frame.render_widget(
        Block::default().style(Style::default().bg(app.palette.panel_bg)),
        area,
    );
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Length(2),
            Constraint::Min(4),
            Constraint::Length(8),
            Constraint::Length(1),
        ])
        .split(area);

    render_header(frame, chunks[0], app);
    render_list(frame, chunks[1], app);
    render_details(frame, chunks[2], app);
    render_footer(frame, chunks[3], &app.palette);
}

fn render_header(frame: &mut Frame<'_>, area: Rect, app: &PrPickerApp) {
    let selected = if app.rows.is_empty() {
        "0/0".to_string()
    } else {
        format!("{}/{}", app.selected + 1, app.rows.len())
    };
    let ssh_hint = if is_ssh_session() {
        " · SSH: open copies URL locally"
    } else {
        ""
    };
    let lines = vec![
        Line::from(vec![
            Span::styled(
                "Pull requests",
                Style::default()
                    .fg(app.palette.text)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                format!("  {selected}{ssh_hint}"),
                Style::default().fg(app.palette.overlay0),
            ),
        ]),
        Line::from(Span::styled(
            "Active Herdr agents, sorted by PR state. Enter focuses the agent.",
            Style::default().fg(app.palette.overlay0),
        )),
    ];
    frame.render_widget(Paragraph::new(lines), area);
}

fn render_list(frame: &mut Frame<'_>, area: Rect, app: &mut PrPickerApp) {
    if app.rows.is_empty() {
        let message = app
            .status
            .as_deref()
            .unwrap_or("No PRs found for active Herdr agents");
        frame.render_widget(
            Paragraph::new(Line::from(Span::styled(
                message,
                Style::default().fg(app.palette.overlay0),
            )))
            .block(
                Block::default()
                    .title("PRs")
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(app.palette.overlay0))
                    .style(Style::default().bg(app.palette.panel_bg)),
            ),
            area,
        );
        return;
    }

    let visible_rows = usize::from(area.height.saturating_sub(2).max(1));
    app.ensure_selection_visible(visible_rows);
    let end = app.rows.len().min(app.scroll.saturating_add(visible_rows));
    let mut lines = Vec::new();
    for (index, row) in app.rows[app.scroll..end].iter().enumerate() {
        let absolute_index = app.scroll + index;
        lines.push(pr_row_line(
            row,
            absolute_index == app.selected,
            area.width.saturating_sub(2),
            &app.palette,
        ));
    }

    let title = if app.rows.len() > visible_rows {
        format!("PRs {}-{} of {}", app.scroll + 1, end, app.rows.len())
    } else {
        "PRs".to_string()
    };
    frame.render_widget(
        Paragraph::new(lines).block(
            Block::default()
                .title(title)
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.palette.overlay0))
                .style(Style::default().bg(app.palette.panel_bg)),
        ),
        area,
    );
}

fn render_details(frame: &mut Frame<'_>, area: Rect, app: &PrPickerApp) {
    let lines = if let Some(row) = app.selected_row() {
        selected_detail_lines(
            row,
            app.status.as_deref(),
            area.width.saturating_sub(2),
            &app.palette,
        )
    } else {
        vec![Line::from(Span::styled(
            app.status.as_deref().unwrap_or("Nothing selected"),
            Style::default().fg(app.palette.overlay0),
        ))]
    };
    frame.render_widget(
        Paragraph::new(lines).wrap(Wrap { trim: false }).block(
            Block::default()
                .title("Details")
                .borders(Borders::ALL)
                .border_style(Style::default().fg(app.palette.overlay0))
                .style(Style::default().bg(app.palette.panel_bg)),
        ),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, palette: &PickerPalette) {
    let controls =
        "↑/↓ j/k select · Enter focus · o open/copy URL · y copy URL · r refresh · q/Esc close";
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            controls,
            Style::default().fg(palette.overlay0),
        ))),
        area,
    );
}

fn pr_row_line(
    row: &PrRow,
    selected: bool,
    max_width: u16,
    palette: &PickerPalette,
) -> Line<'static> {
    let base = row_style(selected, palette);
    let state_style = base.fg(pr_state_color(row.state, palette));
    let check_style = base.fg(check_state_color(row.checks, palette));
    let review = review_label(row.review.as_deref());
    let total_changed = row.additions + row.deletions;
    let title_width = usize::from(max_width).saturating_sub(76).max(18);

    Line::from(vec![
        Span::styled(if selected { "› " } else { "  " }, base),
        Span::styled(
            format!("#{:<5}", row.number),
            base.add_modifier(Modifier::BOLD),
        ),
        Span::styled(
            format!(
                "{} {:<7}",
                pr_state_icon(row.state),
                pr_state_label(row.state)
            ),
            state_style,
        ),
        Span::styled(
            format!(
                "{} {:<8}",
                check_state_icon(row.checks),
                check_state_label(row.checks)
            ),
            check_style,
        ),
        Span::styled(format!("Δ{:<5}", total_changed), base.fg(palette.yellow)),
        Span::styled(format!("+{} ", row.additions), base.fg(palette.green)),
        Span::styled(format!("-{} ", row.deletions), base.fg(palette.red)),
        Span::styled(
            format!("{}f ", row.changed_files),
            base.fg(palette.subtext0),
        ),
        Span::styled(
            format!("{}{} ", COMMENT_ICON, row.comments),
            base.fg(palette.blue),
        ),
        Span::styled(
            format!("{:<9}", review),
            base.fg(review_color(review, palette)),
        ),
        Span::styled(truncate(&row.title, title_width), base),
    ])
}

fn selected_detail_lines(
    row: &PrRow,
    status: Option<&str>,
    max_width: u16,
    palette: &PickerPalette,
) -> Vec<Line<'static>> {
    let total_changed = row.additions + row.deletions;
    let title_width = usize::from(max_width).saturating_sub(8).max(20);
    let mut lines = vec![
        Line::from(vec![
            Span::styled(
                format!("#{} ", row.number),
                Style::default()
                    .fg(palette.text)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(
                truncate(&row.title, title_width),
                Style::default().fg(palette.text),
            ),
        ]),
        Line::from(vec![
            Span::styled(
                pr_state_label(row.state),
                Style::default().fg(pr_state_color(row.state, palette)),
            ),
            Span::raw(" · "),
            Span::styled(
                check_state_label(row.checks),
                Style::default().fg(check_state_color(row.checks, palette)),
            ),
            Span::raw(" · "),
            Span::styled(
                review_label(row.review.as_deref()),
                Style::default().fg(review_color(review_label(row.review.as_deref()), palette)),
            ),
            Span::raw(format!(" · {} comments", row.comments)),
        ]),
        Line::from(vec![
            Span::styled("changes ", Style::default().fg(palette.overlay0)),
            Span::styled(
                format!("{total_changed} lines "),
                Style::default().fg(palette.yellow),
            ),
            Span::styled(
                format!("+{} ", row.additions),
                Style::default().fg(palette.green),
            ),
            Span::styled(
                format!("-{} ", row.deletions),
                Style::default().fg(palette.red),
            ),
            Span::styled(
                format!("{} files", row.changed_files),
                Style::default().fg(palette.subtext0),
            ),
        ]),
        Line::from(Span::styled(
            format!("{} · {} · {}", row.agent, row.agent_status, row.branch),
            Style::default().fg(palette.subtext0),
        )),
        Line::from(Span::styled(
            row.url.clone(),
            Style::default().fg(palette.blue),
        )),
    ];

    if let Some(status) = status {
        lines.push(Line::from(Span::styled(
            status.to_string(),
            Style::default().fg(palette.yellow),
        )));
    }

    lines
}

fn row_style(selected: bool, palette: &PickerPalette) -> Style {
    if selected {
        Style::default()
            .bg(palette.accent)
            .fg(palette.selected_fg())
    } else {
        Style::default().fg(palette.subtext0).bg(palette.panel_bg)
    }
}

const COMMENT_ICON: &str = "\u{F442}";
const CHECK_PASS_ICON: &str = "\u{F4A4}";
const CHECK_PENDING_ICON: &str = "\u{F4AA}";
const CHECK_FAIL_ICON: &str = "\u{F530}";

fn pr_state_label(state: PrState) -> &'static str {
    match state {
        PrState::Open => "OPEN",
        PrState::Draft => "DRAFT",
        PrState::Merged => "MERGED",
        PrState::Closed => "CLOSED",
    }
}

fn pr_state_color(state: PrState, palette: &PickerPalette) -> Color {
    match state {
        PrState::Open => palette.green,
        PrState::Draft => palette.overlay0,
        PrState::Merged => palette.accent,
        PrState::Closed => palette.red,
    }
}

fn check_state_label(state: CheckState) -> &'static str {
    match state {
        CheckState::Pass => "CI OK",
        CheckState::Pending => "CI WAIT",
        CheckState::Fail => "CI FAIL",
        CheckState::None => "CI -",
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

fn check_state_color(state: CheckState, palette: &PickerPalette) -> Color {
    match state {
        CheckState::Pass => palette.green,
        CheckState::Pending => palette.yellow,
        CheckState::Fail => palette.red,
        CheckState::None => palette.overlay0,
    }
}

fn review_label(review: Option<&str>) -> &'static str {
    match review {
        Some("APPROVED") => "approved",
        Some("CHANGES_REQUESTED") => "changes",
        Some("REVIEW_REQUIRED") => "review",
        Some("REVIEWED") => "reviewed",
        _ => "review -",
    }
}

fn review_color(review: &str, palette: &PickerPalette) -> Color {
    match review {
        "approved" | "APPROVED" => palette.green,
        "changes" | "CHANGES_REQUESTED" => palette.red,
        "review" | "REVIEW_REQUIRED" => palette.yellow,
        _ => palette.overlay0,
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

fn copy_to_clipboard(text: &str) -> Result<()> {
    if is_ssh_session() {
        copy_to_terminal_clipboard(text).context("failed to copy through terminal clipboard")?;
        return Ok(());
    }

    let mut command = if cfg!(target_os = "macos") {
        Command::new("pbcopy")
    } else if command_exists("wl-copy") {
        Command::new("wl-copy")
    } else if command_exists("xclip") {
        let mut command = Command::new("xclip");
        command.args(["-selection", "clipboard"]);
        command
    } else {
        copy_to_terminal_clipboard(text).context("failed to copy through terminal clipboard")?;
        return Ok(());
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
    let status = child
        .wait()
        .context("failed to wait for clipboard command")?;
    if !status.success() {
        copy_to_terminal_clipboard(text)
            .context("clipboard command failed, terminal copy failed")?;
    }
    Ok(())
}
