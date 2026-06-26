use super::pi::available_pi_models;
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
