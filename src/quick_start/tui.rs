use super::pi::available_pi_models;
use super::worktree::default_branch_for_prompt;
use super::{Harness, QuickStartForm};
use anyhow::{Context, Result};
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
use std::io;
use std::time::Duration;

const OVERLAY_WIDTH: u16 = 92;
const OVERLAY_HEIGHT: u16 = 20;
const PROMPT_PAGE_SCROLL: u16 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum QuickField {
    Prompt,
    Branch,
    Model,
}

#[derive(Debug)]
struct QuickStartApp {
    prompt: String,
    prompt_scroll: u16,
    prompt_follow_end: bool,
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
            prompt_scroll: 0,
            prompt_follow_end: true,
            branch: String::new(),
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
        return Ok(app.to_form().map(Some));
    }

    match key.code {
        KeyCode::Esc => Ok(Some(None)),
        KeyCode::Enter if app.field == QuickField::Prompt => {
            app.insert_char('\n');
            Ok(None)
        }
        KeyCode::Enter => Ok(app.to_form().map(Some)),
        KeyCode::Tab => {
            app.next_field();
            Ok(None)
        }
        KeyCode::BackTab => {
            app.previous_field();
            Ok(None)
        }
        KeyCode::Down if app.field == QuickField::Prompt => {
            app.scroll_prompt_down(1);
            Ok(None)
        }
        KeyCode::Up if app.field == QuickField::Prompt => {
            app.scroll_prompt_up(1);
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
        KeyCode::Down => {
            app.next_field();
            Ok(None)
        }
        KeyCode::Up => {
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
            QuickField::Prompt => {
                self.prompt.push(ch);
                self.prompt_follow_end = true;
            }
            QuickField::Branch => self.branch.push(ch),
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
            QuickField::Branch => self.branch.push_str(&text.replace(['\n', '\r', '\t'], "-")),
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
            QuickField::Branch => {
                self.branch.pop();
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
            Constraint::Length(2),
        ])
        .split(area);

    let prompt_scroll = app.clamp_prompt_scroll(chunks[0]);
    let prompt_max_scroll = prompt_max_scroll(&app.prompt, chunks[0]);
    render_text_field(
        frame,
        chunks[0],
        prompt_title(prompt_scroll, prompt_max_scroll),
        "What should Pi do? Enter adds a new line.",
        &app.prompt,
        app.field == QuickField::Prompt,
        prompt_scroll,
    );

    let branch_placeholder = format!("optional; auto: {}", default_branch_for_prompt(&app.prompt));
    render_text_field(
        frame,
        chunks[1],
        "Branch".to_string(),
        &branch_placeholder,
        &app.branch,
        app.field == QuickField::Branch,
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
        Line::from(
            "Ctrl+S submit · Enter newline in prompt / submit elsewhere · Tab fields · ↑/↓ scroll",
        )
    };
    frame.render_widget(
        Paragraph::new(footer_text).wrap(Wrap { trim: false }),
        chunks[3],
    );
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
    prompt_visual_line_count(value, inner_width).saturating_sub(visible_height)
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
