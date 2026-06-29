use super::pi::available_pi_models;
use super::worktree::default_branch_for_prompt;
use super::{Harness, QuickStartForm, QuickStartTarget};
use anyhow::{Context, Result};
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Position, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, Paragraph, Wrap},
};
use std::io;
use std::time::Duration;

const OVERLAY_WIDTH: u16 = 92;
const OVERLAY_HEIGHT: u16 = 26;
const PROMPT_PAGE_SCROLL: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickField {
    Prompt,
    Target,
    Branch,
    Base,
    Model,
}

#[derive(Debug)]
struct QuickStartApp {
    prompt: String,
    prompt_scroll: u16,
    prompt_follow_end: bool,
    branch: String,
    base: String,
    target: QuickStartTarget,
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
            prompt_scroll: 0,
            prompt_follow_end: true,
            branch: String::new(),
            base: String::new(),
            target: QuickStartTarget::Workspace,
            harness: Harness::Pi,
            models: available_pi_models(),
            model_index: 0,
            field: QuickField::Prompt,
            error: None,
        }
    }
}

pub(super) fn run_quick_start_tui() -> Result<Option<QuickStartForm>> {
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
            .draw(|frame| draw_quick_start(frame, &mut app))
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
        return Ok(app.submit_form().map(Some));
    }

    match key.code {
        KeyCode::Esc => Ok(Some(None)),
        KeyCode::Enter
            if app.field == QuickField::Prompt && key.modifiers.contains(KeyModifiers::SHIFT) =>
        {
            app.insert_char('\n');
            Ok(None)
        }
        KeyCode::Enter => Ok(app.submit_form().map(Some)),
        KeyCode::Tab | KeyCode::Down => {
            app.next_field();
            Ok(None)
        }
        KeyCode::BackTab | KeyCode::Up => {
            app.previous_field();
            Ok(None)
        }
        KeyCode::PageDown if app.field == QuickField::Prompt => {
            app.scroll_prompt_down(PROMPT_PAGE_SCROLL);
            Ok(None)
        }
        KeyCode::PageUp if app.field == QuickField::Prompt => {
            app.scroll_prompt_up(PROMPT_PAGE_SCROLL);
            Ok(None)
        }
        KeyCode::Backspace => {
            app.backspace();
            Ok(None)
        }
        KeyCode::Left if app.field == QuickField::Target => {
            app.previous_target();
            Ok(None)
        }
        KeyCode::Right if app.field == QuickField::Target => {
            app.next_target();
            Ok(None)
        }
        KeyCode::Char(' ') if app.field == QuickField::Target => {
            app.next_target();
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
            QuickField::Prompt => QuickField::Target,
            QuickField::Target => QuickField::Branch,
            QuickField::Branch => QuickField::Base,
            QuickField::Base => QuickField::Model,
            QuickField::Model => QuickField::Prompt,
        };
    }

    fn previous_field(&mut self) {
        self.error = None;
        self.field = match self.field {
            QuickField::Prompt => QuickField::Model,
            QuickField::Target => QuickField::Prompt,
            QuickField::Branch => QuickField::Target,
            QuickField::Base => QuickField::Branch,
            QuickField::Model => QuickField::Base,
        };
    }

    fn insert_char(&mut self, ch: char) {
        self.error = None;
        match self.field {
            QuickField::Prompt => {
                self.prompt.push(ch);
                self.prompt_follow_end = true;
            }
            QuickField::Target => {}
            QuickField::Branch => self.branch.push(ch),
            QuickField::Base => self.base.push(ch),
            QuickField::Model => {}
        }
    }

    fn insert_text(&mut self, text: &str) {
        self.error = None;
        match self.field {
            QuickField::Prompt => {
                self.prompt.push_str(&normalize_multiline_text(text));
                self.prompt_follow_end = true;
            }
            QuickField::Target => {}
            QuickField::Branch => self.branch.push_str(&text.replace(['\n', '\r', '\t'], "-")),
            QuickField::Base => self.base.push_str(&text.replace(['\n', '\r', '\t'], "-")),
            QuickField::Model => {}
        }
    }

    fn backspace(&mut self) {
        self.error = None;
        match self.field {
            QuickField::Prompt => {
                self.prompt.pop();
                self.prompt_follow_end = true;
            }
            QuickField::Target => {}
            QuickField::Branch => {
                self.branch.pop();
            }
            QuickField::Base => {
                self.base.pop();
            }
            QuickField::Model => {}
        }
    }

    fn scroll_prompt_up(&mut self, lines: u16) {
        self.error = None;
        self.prompt_follow_end = false;
        self.prompt_scroll = self.prompt_scroll.saturating_sub(lines);
    }

    fn scroll_prompt_down(&mut self, lines: u16) {
        self.error = None;
        self.prompt_follow_end = false;
        self.prompt_scroll = self.prompt_scroll.saturating_add(lines);
    }

    fn clamp_prompt_scroll(&mut self, area: Rect) -> u16 {
        let max_scroll = prompt_max_scroll(&self.prompt, area);
        if self.prompt_follow_end {
            self.prompt_scroll = max_scroll;
        } else {
            self.prompt_scroll = self.prompt_scroll.min(max_scroll);
        }
        self.prompt_scroll
    }

    fn next_target(&mut self) {
        self.error = None;
        self.target = match self.target {
            QuickStartTarget::Workspace => QuickStartTarget::Worktree,
            QuickStartTarget::Worktree => QuickStartTarget::Workspace,
        };
    }

    fn previous_target(&mut self) {
        self.next_target();
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

    fn submit_form(&mut self) -> Option<QuickStartForm> {
        let prompt = self.prompt.trim().to_string();
        let branch = self.branch.trim().to_string();
        let base = self.base.trim().to_string();
        if self.target == QuickStartTarget::Worktree && prompt.is_empty() && branch.is_empty() {
            self.error = Some("Branch or prompt is required for a worktree".to_string());
            return None;
        }
        if self.target == QuickStartTarget::Workspace && branch.is_empty() && !base.is_empty() {
            self.error = Some("Base requires a branch in workspace mode".to_string());
            return None;
        }
        Some(QuickStartForm {
            prompt,
            branch,
            base,
            target: self.target,
            harness: self.harness,
            model: self.selected_model(),
        })
    }
}

fn draw_quick_start(frame: &mut Frame<'_>, app: &mut QuickStartApp) {
    let area = centered_rect(frame.area(), OVERLAY_WIDTH, OVERLAY_HEIGHT);
    frame.render_widget(Clear, area);
    frame.render_widget(Block::default().borders(Borders::ALL), area);

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .margin(1)
        .constraints([
            Constraint::Min(6),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Length(2),
        ])
        .split(area);

    let prompt_scroll = app.clamp_prompt_scroll(chunks[0]);
    let prompt_max_scroll = prompt_max_scroll(&app.prompt, chunks[0]);
    render_text_field(
        frame,
        chunks[0],
        prompt_title(prompt_scroll, prompt_max_scroll),
        "Optional Pi prompt. Shift+Enter adds a new line.",
        &app.prompt,
        app.field == QuickField::Prompt,
        prompt_scroll,
    );

    let target = Paragraph::new(vec![Line::from(vec![
        Span::styled(target_label(app.target), Style::default().fg(Color::White)),
        Span::raw("  "),
        Span::styled("←/→ or Space", Style::default().fg(Color::DarkGray)),
    ])])
    .block(field_block("Mode", app.field == QuickField::Target));
    frame.render_widget(target, chunks[1]);

    let branch_placeholder = branch_placeholder(app);
    render_text_field(
        frame,
        chunks[2],
        "Branch".to_string(),
        &branch_placeholder,
        &app.branch,
        app.field == QuickField::Branch,
        0,
    );

    let base_placeholder = base_placeholder(app);
    render_text_field(
        frame,
        chunks[3],
        "Base ref".to_string(),
        base_placeholder,
        &app.base,
        app.field == QuickField::Base,
        0,
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
        Span::styled("←/→ or Space", Style::default().fg(Color::DarkGray)),
    ])])
    .block(field_block("Model", app.field == QuickField::Model));
    frame.render_widget(model, chunks[4]);

    let footer_text = if let Some(error) = &app.error {
        Line::from(vec![Span::styled(
            error,
            Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
        )])
    } else {
        Line::from("Enter submit · Shift+Enter newline · Tab/↑/↓ fields · PgUp/PgDn prompt scroll")
    };
    frame.render_widget(
        Paragraph::new(footer_text).wrap(Wrap { trim: false }),
        chunks[5],
    );

    if let Some(position) =
        active_cursor_position(app, chunks[0], chunks[1], chunks[2], chunks[3], chunks[4])
    {
        frame.set_cursor_position(position);
    }
}

fn target_label(target: QuickStartTarget) -> &'static str {
    match target {
        QuickStartTarget::Workspace => "workspace (current checkout)",
        QuickStartTarget::Worktree => "worktree (new checkout)",
    }
}

fn branch_placeholder(app: &QuickStartApp) -> String {
    match app.target {
        QuickStartTarget::Workspace => {
            "optional; blank keeps current branch, or enter branch to switch/create".to_string()
        }
        QuickStartTarget::Worktree => {
            format!("optional; auto: {}", default_branch_for_prompt(&app.prompt))
        }
    }
}

fn base_placeholder(app: &QuickStartApp) -> &'static str {
    match app.target {
        QuickStartTarget::Workspace => "optional; used only when creating a new branch",
        QuickStartTarget::Worktree => {
            "optional; blank uses current checkout, or enter parent branch"
        }
    }
}

fn centered_rect(container: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(container.width.saturating_sub(2)).max(20);
    let height = height.min(container.height.saturating_sub(2)).max(10);
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
    title: String,
    placeholder: &str,
    value: &str,
    active: bool,
    scroll: u16,
) {
    let lines = if value.is_empty() {
        vec![Line::from(Span::styled(
            placeholder.to_string(),
            Style::default().fg(Color::DarkGray),
        ))]
    } else {
        value.split('\n').map(Line::from).collect::<Vec<_>>()
    };

    let paragraph = Paragraph::new(lines)
        .wrap(Wrap { trim: false })
        .scroll((scroll, 0))
        .block(field_block(title, active));
    frame.render_widget(paragraph, area);
}

fn field_block(title: impl Into<String>, active: bool) -> Block<'static> {
    let style = if active {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    Block::default()
        .title(title.into())
        .borders(Borders::ALL)
        .border_style(style)
}

fn active_cursor_position(
    app: &QuickStartApp,
    prompt_area: Rect,
    target_area: Rect,
    branch_area: Rect,
    base_area: Rect,
    model_area: Rect,
) -> Option<Position> {
    match app.field {
        QuickField::Prompt => {
            text_field_cursor_position(prompt_area, &app.prompt, app.prompt_scroll)
        }
        QuickField::Target => fixed_field_cursor_position(target_area),
        QuickField::Branch => text_field_cursor_position(branch_area, &app.branch, 0),
        QuickField::Base => text_field_cursor_position(base_area, &app.base, 0),
        QuickField::Model => fixed_field_cursor_position(model_area),
    }
}

fn text_field_cursor_position(area: Rect, value: &str, scroll: u16) -> Option<Position> {
    let inner = inner_area(area)?;
    let (row, column) = visual_cursor_offset(value, inner.width);
    let visible_row = row.saturating_sub(scroll);
    let y = inner.y + visible_row.min(inner.height.saturating_sub(1));
    let x = inner.x + column.min(inner.width.saturating_sub(1));
    Some(Position { x, y })
}

fn fixed_field_cursor_position(area: Rect) -> Option<Position> {
    let inner = inner_area(area)?;
    Some(Position {
        x: inner.x,
        y: inner.y,
    })
}

fn inner_area(area: Rect) -> Option<Rect> {
    let width = area.width.checked_sub(2)?;
    let height = area.height.checked_sub(2)?;
    if width == 0 || height == 0 {
        return None;
    }
    Some(Rect {
        x: area.x.saturating_add(1),
        y: area.y.saturating_add(1),
        width,
        height,
    })
}

fn visual_cursor_offset(value: &str, width: u16) -> (u16, u16) {
    let width = usize::from(width.max(1));
    let mut row = 0usize;
    let mut parts = value.split('\n').peekable();

    while let Some(part) = parts.next() {
        let chars = part.chars().count();
        if parts.peek().is_none() {
            let column = chars % width;
            let row_offset = chars / width;
            return (
                row.saturating_add(row_offset).min(usize::from(u16::MAX)) as u16,
                column.min(usize::from(u16::MAX)) as u16,
            );
        }
        row = row.saturating_add(chars.max(1).div_ceil(width));
    }

    (0, 0)
}

fn prompt_title(scroll: u16, max_scroll: u16) -> String {
    if max_scroll == 0 {
        "Prompt".to_string()
    } else {
        format!(
            "Prompt {}/{}",
            scroll.saturating_add(1),
            max_scroll.saturating_add(1)
        )
    }
}

fn prompt_max_scroll(value: &str, area: Rect) -> u16 {
    let visible_height = area.height.saturating_sub(2).max(1);
    let inner_width = area.width.saturating_sub(2).max(1);
    let cursor_line_count = visual_cursor_offset(value, inner_width).0.saturating_add(1);
    prompt_visual_line_count(value, inner_width)
        .max(cursor_line_count)
        .saturating_sub(visible_height)
}

fn prompt_visual_line_count(value: &str, width: u16) -> u16 {
    let width = usize::from(width.max(1));
    let visual_lines = if value.is_empty() {
        1
    } else {
        value
            .split('\n')
            .map(|line| {
                let chars = line.chars().count();
                chars.max(1).div_ceil(width)
            })
            .sum::<usize>()
    };
    visual_lines.min(usize::from(u16::MAX)) as u16
}

fn normalize_multiline_text(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\r', "\n")
}
