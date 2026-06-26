use super::agents::load_pr_rows;
use super::github::open_pr_in_browser;
use super::{CheckState, PrRow, PrState};
use crate::focus::focus_pane_later;
use crate::herdr::herdr_socket_path;
use crate::util::{command_exists, debug_log};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::io::{self, Write};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Debug)]
struct PrPickerApp {
    rows: Vec<PrRow>,
    selected: usize,
    status: Option<String>,
}

#[derive(Debug)]
enum PrPickerAction {
    Focus(PrRow),
}

pub(super) fn run_pr_picker_tui() -> Result<()> {
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
            PrPickerAction::Focus(row) => focus_pane_later(&row.workspace_id, &row.pane_id),
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
