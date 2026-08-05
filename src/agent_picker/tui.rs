use super::{AgentRow, AgentStatus, WorkspaceGroup, load_agent_groups, read_visible_ansi};
use crate::focus::focus_pane;
use crate::herdr::herdr_socket_path;
use crate::terminal_session::TerminalSession;
use ansi_to_tui::IntoText;
use anyhow::{Context, Result};
use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span, Text},
    widgets::{Block, Borders, Clear, Paragraph},
};
use std::io;
use std::path::PathBuf;
use std::time::{Duration, Instant};
use unicode_width::{UnicodeWidthChar, UnicodeWidthStr};

const SIDE_BY_SIDE_MIN_WIDTH: u16 = 96;
const INPUT_POLL_INTERVAL: Duration = Duration::from_millis(50);
const AGENT_REFRESH_INTERVAL: Duration = Duration::from_millis(1_000);

// Herdr's public subscriptions do not currently expose pane output changes or
// terminal frames. Keep this selected-pane-only poll small and local; if Herdr
// adds a coalesced pane.output_changed subscription, replace this timer with it.
const PREVIEW_REFRESH_INTERVAL: Duration = Duration::from_millis(150);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StateFilter {
    Blocked,
    Working,
    Idle,
    Done,
}

impl StateFilter {
    fn matches(self, status: AgentStatus) -> bool {
        matches!(
            (self, status),
            (Self::Blocked, AgentStatus::Blocked)
                | (Self::Working, AgentStatus::Working)
                | (Self::Idle, AgentStatus::Idle)
                | (Self::Done, AgentStatus::Done)
        )
    }
}

#[derive(Debug, Clone)]
enum DisplayLine {
    Spacer,
    Workspace {
        label: String,
        count: usize,
        status: AgentStatus,
    },
    Agent {
        agent_index: usize,
        last: bool,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PickerOrientation {
    SideBySide,
    Stacked,
}

#[derive(Debug, Clone, Copy)]
struct PickerLayout {
    search: Rect,
    search_separator: Rect,
    list: Rect,
    content_separator: Rect,
    preview: Rect,
    detail: Rect,
    footer: Rect,
    orientation: PickerOrientation,
}

#[derive(Debug)]
struct AgentPickerApp {
    socket_path: PathBuf,
    groups: Vec<WorkspaceGroup>,
    selected: usize,
    list_scroll: usize,
    query: String,
    search_focused: bool,
    state_filter: Option<StateFilter>,
    preview_pane_id: Option<String>,
    preview_ansi: String,
    preview_scroll_from_bottom: usize,
    status: Option<String>,
    last_agent_refresh: Instant,
    last_preview_refresh: Instant,
}

impl AgentPickerApp {
    fn new(socket_path: PathBuf) -> Self {
        let now = Instant::now();
        Self {
            socket_path,
            groups: Vec::new(),
            selected: 0,
            list_scroll: 0,
            query: String::new(),
            search_focused: false,
            state_filter: None,
            preview_pane_id: None,
            preview_ansi: String::new(),
            preview_scroll_from_bottom: 0,
            status: None,
            last_agent_refresh: now.checked_sub(AGENT_REFRESH_INTERVAL).unwrap_or(now),
            last_preview_refresh: now.checked_sub(PREVIEW_REFRESH_INTERVAL).unwrap_or(now),
        }
    }

    fn filtered_agents(&self) -> Vec<AgentRow> {
        let query = self.query.trim().to_lowercase();
        self.groups
            .iter()
            .flat_map(|group| group.agents.iter())
            .filter(|agent| {
                self.state_filter
                    .is_none_or(|filter| filter.matches(agent.status))
                    && (query.is_empty()
                        || format!(
                            "{} {} {} {} {} {}",
                            agent.workspace_label,
                            agent.label,
                            agent.agent,
                            agent.pane_id,
                            agent.tab_id,
                            agent.status.label()
                        )
                        .to_lowercase()
                        .contains(&query))
            })
            .cloned()
            .collect()
    }

    fn display_lines(&self, agents: &[AgentRow]) -> Vec<DisplayLine> {
        let mut lines = Vec::new();
        for group in &self.groups {
            let indexes = agents
                .iter()
                .enumerate()
                .filter_map(|(index, agent)| {
                    (agent.workspace_id == group.workspace_id).then_some(index)
                })
                .collect::<Vec<_>>();
            if indexes.is_empty() {
                continue;
            }
            if !lines.is_empty() {
                lines.push(DisplayLine::Spacer);
            }
            lines.push(DisplayLine::Workspace {
                label: group.label.clone(),
                count: indexes.len(),
                status: aggregate_status(indexes.iter().map(|index| agents[*index].status)),
            });
            let last_index = indexes.len().saturating_sub(1);
            for (position, agent_index) in indexes.into_iter().enumerate() {
                lines.push(DisplayLine::Agent {
                    agent_index,
                    last: position == last_index,
                });
            }
        }
        lines
    }

    fn selected_agent(&self) -> Option<AgentRow> {
        self.filtered_agents().get(self.selected).cloned()
    }

    fn move_selection(&mut self, delta: isize, viewport: usize) {
        let count = self.filtered_agents().len();
        if count == 0 {
            self.selected = 0;
            self.list_scroll = 0;
            return;
        }
        self.selected =
            (self.selected.min(count - 1) as isize + delta).clamp(0, count as isize - 1) as usize;
        self.on_selection_changed();
        self.ensure_selection_visible(viewport);
    }

    fn select(&mut self, index: usize, viewport: usize) {
        let count = self.filtered_agents().len();
        if index >= count {
            return;
        }
        if self.selected != index {
            self.selected = index;
            self.on_selection_changed();
        }
        self.ensure_selection_visible(viewport);
    }

    fn on_selection_changed(&mut self) {
        self.preview_scroll_from_bottom = 0;
        self.last_preview_refresh = Instant::now()
            .checked_sub(PREVIEW_REFRESH_INTERVAL)
            .unwrap_or_else(Instant::now);
    }

    fn clamp_selection(&mut self) {
        let count = self.filtered_agents().len();
        self.selected = self.selected.min(count.saturating_sub(1));
        if count == 0 {
            self.list_scroll = 0;
            self.preview_pane_id = None;
            self.preview_ansi.clear();
        }
    }

    fn ensure_selection_visible(&mut self, viewport: usize) {
        if viewport == 0 {
            self.list_scroll = 0;
            return;
        }
        let agents = self.filtered_agents();
        let lines = self.display_lines(&agents);
        let selected_line = lines.iter().position(|line| {
            matches!(line, DisplayLine::Agent { agent_index, .. } if *agent_index == self.selected)
        });
        let Some(selected_line) = selected_line else {
            self.list_scroll = 0;
            return;
        };
        if selected_line < self.list_scroll {
            self.list_scroll = selected_line;
        } else if selected_line >= self.list_scroll.saturating_add(viewport) {
            self.list_scroll = selected_line.saturating_add(1).saturating_sub(viewport);
        }
        self.list_scroll = self.list_scroll.min(lines.len().saturating_sub(viewport));
    }

    fn select_first_match(&mut self) {
        self.selected = 0;
        self.list_scroll = 0;
        self.on_selection_changed();
    }

    fn refresh_agents(&mut self, force: bool) {
        if !force && self.last_agent_refresh.elapsed() < AGENT_REFRESH_INTERVAL {
            return;
        }
        self.last_agent_refresh = Instant::now();
        let previous_pane = self.selected_agent().map(|agent| agent.pane_id);
        match load_agent_groups(&self.socket_path) {
            Ok(groups) => {
                self.groups = groups;
                self.clamp_selection();
                if let Some(previous_pane) = previous_pane
                    && let Some(index) = self
                        .filtered_agents()
                        .iter()
                        .position(|agent| agent.pane_id == previous_pane)
                {
                    self.selected = index;
                }
                self.status = self
                    .filtered_agents()
                    .is_empty()
                    .then(|| "No active Herdr agents".to_string());
            }
            Err(error) => self.status = Some(format!("agent refresh failed: {error}")),
        }
    }

    fn refresh_preview(&mut self, force: bool) {
        if !force && self.last_preview_refresh.elapsed() < PREVIEW_REFRESH_INTERVAL {
            return;
        }
        self.last_preview_refresh = Instant::now();
        let Some(agent) = self.selected_agent() else {
            self.preview_pane_id = None;
            self.preview_ansi.clear();
            return;
        };
        let changed_selection = self.preview_pane_id.as_deref() != Some(&agent.pane_id);
        if changed_selection {
            self.preview_pane_id = Some(agent.pane_id.clone());
            self.preview_ansi.clear();
            self.preview_scroll_from_bottom = 0;
        }
        match read_visible_ansi(&self.socket_path, &agent.pane_id) {
            Ok(ansi) => {
                let ansi = sanitize_preview_ansi(&ansi);
                if ansi != self.preview_ansi {
                    self.preview_ansi = ansi;
                }
                if self
                    .status
                    .as_deref()
                    .is_some_and(|message| message.starts_with("preview read failed"))
                {
                    self.status = None;
                }
            }
            Err(error) => self.status = Some(format!("preview read failed: {error}")),
        }
    }

    fn scroll_preview_up(&mut self, amount: usize, viewport: usize) {
        let line_count = ansi_line_count(&self.preview_ansi);
        let max = line_count.saturating_sub(viewport);
        self.preview_scroll_from_bottom = self
            .preview_scroll_from_bottom
            .saturating_add(amount)
            .min(max);
    }

    fn scroll_preview_down(&mut self, amount: usize) {
        self.preview_scroll_from_bottom = self.preview_scroll_from_bottom.saturating_sub(amount);
    }
}

pub(super) fn run_agent_picker_tui() -> Result<()> {
    let socket_path = herdr_socket_path()?;
    let mut session = TerminalSession::enter_with_mouse(false, true)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend).context("failed to create terminal")?;
    terminal.clear().context("failed to clear terminal")?;

    let result = run_agent_picker_loop(&mut terminal, socket_path);
    terminal.show_cursor().ok();
    drop(terminal);
    let cleanup = session.finish();

    let selected = result?;
    cleanup?;
    if let Some(agent) = selected {
        focus_pane(&agent.workspace_id, &agent.pane_id)?;
    }
    Ok(())
}

fn run_agent_picker_loop(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    socket_path: PathBuf,
) -> Result<Option<AgentRow>> {
    let mut app = AgentPickerApp::new(socket_path);
    app.refresh_agents(true);
    app.refresh_preview(true);

    loop {
        terminal
            .draw(|frame| draw_agent_picker(frame, &mut app))
            .context("failed to draw agent picker")?;

        if event::poll(INPUT_POLL_INTERVAL).context("failed to poll terminal event")? {
            let size = terminal.size()?;
            let area = Rect::new(0, 0, size.width, size.height);
            match event::read().context("failed to read terminal event")? {
                Event::Key(key) if key.kind == KeyEventKind::Press => {
                    if let Some(action) = handle_key(&mut app, key, area) {
                        return Ok(action);
                    }
                }
                Event::Mouse(mouse) => {
                    if let Some(agent) = handle_mouse(&mut app, mouse, area) {
                        return Ok(Some(agent));
                    }
                }
                Event::Resize(_, _) => app.ensure_selection_visible(1),
                _ => {}
            }
        }
        app.refresh_agents(false);
        app.refresh_preview(false);
    }
}

fn handle_key(app: &mut AgentPickerApp, key: KeyEvent, area: Rect) -> Option<Option<AgentRow>> {
    let layout = picker_layout(area);
    let list_viewport = layout.list.height as usize;
    let preview_viewport = preview_inner(layout.preview).height as usize;

    if key.modifiers.contains(KeyModifiers::CONTROL) && key.code == KeyCode::Char('c') {
        return Some(None);
    }

    if app.search_focused {
        match key.code {
            KeyCode::Esc => app.search_focused = false,
            KeyCode::Enter => return Some(app.selected_agent()),
            KeyCode::Backspace => {
                app.state_filter = None;
                app.query.pop();
                app.select_first_match();
            }
            KeyCode::Up => app.move_selection(-1, list_viewport),
            KeyCode::Down => app.move_selection(1, list_viewport),
            KeyCode::Char('n') if key.modifiers == KeyModifiers::CONTROL => {
                app.move_selection(1, list_viewport)
            }
            KeyCode::Char('p') if key.modifiers == KeyModifiers::CONTROL => {
                app.move_selection(-1, list_viewport)
            }
            KeyCode::Char('u') if key.modifiers == KeyModifiers::CONTROL => {
                app.query.clear();
                app.state_filter = None;
                app.select_first_match();
            }
            KeyCode::Char(character)
                if key.modifiers.is_empty() || key.modifiers == KeyModifiers::SHIFT =>
            {
                app.state_filter = None;
                app.query.push(character);
                app.select_first_match();
            }
            _ => {}
        }
        return None;
    }

    match key.code {
        KeyCode::Esc | KeyCode::Char('q') => return Some(None),
        KeyCode::Enter => return Some(app.selected_agent()),
        KeyCode::Char('/') => {
            app.state_filter = None;
            app.search_focused = true;
        }
        KeyCode::Char('a') if key.modifiers.is_empty() => {
            app.query.clear();
            app.state_filter = None;
            app.select_first_match();
        }
        KeyCode::Char('b') if key.modifiers.is_empty() => {
            set_filter(app, StateFilter::Blocked);
        }
        KeyCode::Char('w') if key.modifiers.is_empty() => {
            set_filter(app, StateFilter::Working);
        }
        KeyCode::Char('i') if key.modifiers.is_empty() => {
            set_filter(app, StateFilter::Idle);
        }
        KeyCode::Char('d') if key.modifiers.is_empty() => {
            set_filter(app, StateFilter::Done);
        }
        KeyCode::Up | KeyCode::Char('k') => app.move_selection(-1, list_viewport),
        KeyCode::Down | KeyCode::Char('j') => app.move_selection(1, list_viewport),
        KeyCode::Home => app.select(0, list_viewport),
        KeyCode::End | KeyCode::Char('G') => {
            let last = app.filtered_agents().len().saturating_sub(1);
            app.select(last, list_viewport);
        }
        KeyCode::PageUp => app.scroll_preview_up((preview_viewport / 2).max(1), preview_viewport),
        KeyCode::PageDown => app.scroll_preview_down((preview_viewport / 2).max(1)),
        KeyCode::Char('r') => {
            app.refresh_agents(true);
            app.refresh_preview(true);
        }
        _ => {}
    }
    None
}

fn set_filter(app: &mut AgentPickerApp, filter: StateFilter) {
    app.query.clear();
    app.state_filter = Some(filter);
    app.select_first_match();
}

fn handle_mouse(app: &mut AgentPickerApp, mouse: MouseEvent, area: Rect) -> Option<AgentRow> {
    let layout = picker_layout(area);
    let agents = app.filtered_agents();
    match mouse.kind {
        MouseEventKind::Moved => {
            if let Some(index) = agent_index_at(app, &agents, layout.list, mouse.column, mouse.row)
            {
                app.select(index, layout.list.height as usize);
            }
        }
        MouseEventKind::Down(MouseButton::Left) => {
            if rect_contains(layout.search, mouse.column, mouse.row) {
                app.search_focused = true;
            } else if let Some(index) =
                agent_index_at(app, &agents, layout.list, mouse.column, mouse.row)
            {
                app.select(index, layout.list.height as usize);
                return app.selected_agent();
            }
        }
        MouseEventKind::ScrollUp if rect_contains(layout.preview, mouse.column, mouse.row) => {
            let viewport = preview_inner(layout.preview).height as usize;
            app.scroll_preview_up(3, viewport);
        }
        MouseEventKind::ScrollDown if rect_contains(layout.preview, mouse.column, mouse.row) => {
            app.scroll_preview_down(3);
        }
        MouseEventKind::ScrollUp if rect_contains(layout.list, mouse.column, mouse.row) => {
            app.move_selection(-3, layout.list.height as usize);
        }
        MouseEventKind::ScrollDown if rect_contains(layout.list, mouse.column, mouse.row) => {
            app.move_selection(3, layout.list.height as usize);
        }
        _ => {}
    }
    None
}

fn agent_index_at(
    app: &AgentPickerApp,
    agents: &[AgentRow],
    area: Rect,
    column: u16,
    row: u16,
) -> Option<usize> {
    if !rect_contains(area, column, row) {
        return None;
    }
    let line_index = app
        .list_scroll
        .saturating_add(row.saturating_sub(area.y) as usize);
    match app.display_lines(agents).get(line_index) {
        Some(DisplayLine::Agent { agent_index, .. }) => Some(*agent_index),
        _ => None,
    }
}

fn draw_agent_picker(frame: &mut Frame<'_>, app: &mut AgentPickerApp) {
    let area = frame.area();
    frame.render_widget(Clear, area);
    if area.width == 0 || area.height == 0 {
        return;
    }

    // Herdr already supplies the popup border and "Agents" title. Drawing a
    // second shell here creates a misleading nested "Sessions" container.
    let layout = picker_layout(area);
    let agents = app.filtered_agents();
    app.clamp_selection();
    app.ensure_selection_visible(layout.list.height as usize);

    render_search(frame, layout.search, app, agents.len());
    render_separator(frame, layout.search_separator, layout.orientation);
    render_agent_list(frame, layout.list, app, &agents);
    render_separator(frame, layout.content_separator, layout.orientation);
    render_preview(frame, layout.preview, app);
    render_detail(frame, layout.detail, app);
    render_footer(frame, layout.footer, app);
}

fn picker_layout(area: Rect) -> PickerLayout {
    let vertical = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(4),
            Constraint::Length(1),
            Constraint::Length(1),
        ])
        .split(area);
    let content = vertical[2];
    if area.width >= SIDE_BY_SIDE_MIN_WIDTH {
        let horizontal = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([
                Constraint::Percentage(42),
                Constraint::Length(1),
                Constraint::Min(36),
            ])
            .split(content);
        PickerLayout {
            search: vertical[0],
            search_separator: vertical[1],
            list: horizontal[0],
            content_separator: horizontal[1],
            preview: horizontal[2],
            detail: vertical[3],
            footer: vertical[4],
            orientation: PickerOrientation::SideBySide,
        }
    } else {
        let list_height = (content.height / 3)
            .max(5)
            .min(content.height.saturating_sub(4));
        let stacked = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(list_height),
                Constraint::Length(1),
                Constraint::Min(3),
            ])
            .split(content);
        PickerLayout {
            search: vertical[0],
            search_separator: vertical[1],
            list: stacked[0],
            content_separator: stacked[1],
            preview: stacked[2],
            detail: vertical[3],
            footer: vertical[4],
            orientation: PickerOrientation::Stacked,
        }
    }
}

fn render_search(frame: &mut Frame<'_>, area: Rect, app: &AgentPickerApp, count: usize) {
    let focus_style = if app.search_focused {
        Style::default()
            .fg(Color::Cyan)
            .add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::DarkGray)
    };
    let mut spans = vec![Span::styled(" / ", focus_style)];
    if let Some(filter) = app.state_filter {
        let status = match filter {
            StateFilter::Blocked => AgentStatus::Blocked,
            StateFilter::Working => AgentStatus::Working,
            StateFilter::Idle => AgentStatus::Idle,
            StateFilter::Done => AgentStatus::Done,
        };
        spans.push(Span::styled(
            format!("{} {}", status_icon(status), status.label()),
            status_style(status).add_modifier(Modifier::BOLD),
        ));
    } else if app.query.is_empty() {
        spans.push(Span::styled(
            "search agents",
            Style::default().fg(Color::DarkGray),
        ));
    } else {
        spans.push(Span::raw(app.query.clone()));
    }
    let used = spans
        .iter()
        .map(|span| UnicodeWidthStr::width(span.content.as_ref()))
        .sum::<usize>();
    let count_label = format!("{count} agents");
    let padding = area
        .width
        .saturating_sub((used + UnicodeWidthStr::width(count_label.as_str())) as u16)
        as usize;
    spans.push(Span::styled(
        format!("{}{count_label}", " ".repeat(padding)),
        Style::default().fg(Color::DarkGray),
    ));
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn render_separator(frame: &mut Frame<'_>, area: Rect, orientation: PickerOrientation) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let symbol = match orientation {
        PickerOrientation::SideBySide if area.width == 1 => "│".repeat(area.height as usize),
        _ => "─".repeat(area.width as usize),
    };
    if area.width == 1 {
        for offset in 0..area.height {
            frame.render_widget(
                Paragraph::new("│").style(Style::default().fg(Color::DarkGray)),
                Rect::new(area.x, area.y + offset, 1, 1),
            );
        }
    } else {
        frame.render_widget(
            Paragraph::new(symbol).style(Style::default().fg(Color::DarkGray)),
            area,
        );
    }
}

fn render_agent_list(frame: &mut Frame<'_>, area: Rect, app: &AgentPickerApp, agents: &[AgentRow]) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    if agents.is_empty() {
        frame.render_widget(
            Paragraph::new(
                app.status
                    .as_deref()
                    .unwrap_or("No agents match the current filter"),
            )
            .style(Style::default().fg(Color::DarkGray)),
            area,
        );
        return;
    }

    let lines = app.display_lines(agents);
    let start = app.list_scroll.min(lines.len());
    let end = lines.len().min(start.saturating_add(area.height as usize));
    for (visible_index, line) in lines[start..end].iter().enumerate() {
        let rect = Rect::new(area.x, area.y + visible_index as u16, area.width, 1);
        match line {
            DisplayLine::Spacer => {}
            DisplayLine::Workspace {
                label,
                count,
                status,
            } => {
                let text = format!(" ▾ {} {} ({count})", status_icon(*status), label);
                frame.render_widget(
                    Paragraph::new(truncate(&text, area.width as usize)).style(
                        Style::default()
                            .fg(Color::Cyan)
                            .add_modifier(Modifier::BOLD),
                    ),
                    rect,
                );
            }
            DisplayLine::Agent { agent_index, last } => {
                render_agent_row(
                    frame,
                    rect,
                    &agents[*agent_index],
                    *agent_index == app.selected,
                    *last,
                );
            }
        }
    }
}

fn render_agent_row(
    frame: &mut Frame<'_>,
    area: Rect,
    agent: &AgentRow,
    selected: bool,
    last: bool,
) {
    let base = if selected {
        Style::default().bg(Color::Cyan).fg(Color::Black)
    } else {
        Style::default()
    };
    frame.render_widget(Clear, area);
    frame.render_widget(
        Paragraph::new(" ".repeat(area.width as usize)).style(base),
        area,
    );

    let current = if agent.focused { "◆" } else { " " };
    let branch = if last { "└──" } else { "├──" };
    let prefix = format!(" {current} {branch} {} ", status_icon(agent.status));
    let meta = format!(
        "{} · {}",
        short_pane_id(&agent.pane_id),
        agent.status.label()
    );
    let available = area
        .width
        .saturating_sub(UnicodeWidthStr::width(prefix.as_str()) as u16)
        .saturating_sub(UnicodeWidthStr::width(meta.as_str()) as u16)
        .saturating_sub(2) as usize;
    let label = truncate(&agent.label, available);
    let left = format!("{prefix}{label}");
    let gap = area
        .width
        .saturating_sub(UnicodeWidthStr::width(left.as_str()) as u16)
        .saturating_sub(UnicodeWidthStr::width(meta.as_str()) as u16) as usize;

    let label_style = if selected {
        base.add_modifier(Modifier::BOLD)
    } else if agent.focused {
        Style::default().add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::Gray)
    };
    let status_color = if selected {
        base
    } else {
        status_style(agent.status)
    };
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled(
                format!(" {current} {branch} "),
                if selected {
                    base
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
            Span::styled(status_icon(agent.status), status_color),
            Span::styled(format!(" {label}"), label_style),
            Span::styled(" ".repeat(gap), base),
            Span::styled(
                meta,
                if selected {
                    base
                } else {
                    Style::default().fg(Color::DarkGray)
                },
            ),
        ]))
        .style(base),
        area,
    );
}

fn render_preview(frame: &mut Frame<'_>, area: Rect, app: &AgentPickerApp) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let agent = app.selected_agent();
    let title = agent
        .as_ref()
        .map(|agent| {
            format!(
                " {} · {} · {} ",
                agent.label,
                agent.agent,
                agent.status.label()
            )
        })
        .unwrap_or_else(|| " Preview ".to_string());
    let block = Block::default()
        .title(truncate(&title, area.width.saturating_sub(2) as usize))
        .title_style(
            Style::default().fg(agent
                .as_ref()
                .map(|agent| status_color(agent.status))
                .unwrap_or(Color::DarkGray)),
        )
        .borders(Borders::ALL)
        .border_style(Style::default().fg(Color::DarkGray));
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    if agent.is_none() {
        frame.render_widget(
            Paragraph::new("Select an agent to preview its terminal")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }
    if app.preview_ansi.is_empty() {
        frame.render_widget(
            Paragraph::new("Waiting for terminal output…")
                .style(Style::default().fg(Color::DarkGray)),
            inner,
        );
        return;
    }

    let text = app
        .preview_ansi
        .as_bytes()
        .into_text()
        .unwrap_or_else(|_| Text::raw(strip_ansi_fallback(&app.preview_ansi)));
    let max_scroll = text.lines.len().saturating_sub(inner.height as usize);
    let top = max_scroll.saturating_sub(app.preview_scroll_from_bottom);
    frame.render_widget(Paragraph::new(text).scroll((top as u16, 0)), inner);
}

fn render_detail(frame: &mut Frame<'_>, area: Rect, app: &AgentPickerApp) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let detail = app.selected_agent().map(|agent| {
        format!(
            " {} · tab {} · pane {} · {} · {}",
            agent.workspace_label,
            short_tab_id(&agent.tab_id),
            short_pane_id(&agent.pane_id),
            agent.agent,
            agent.status.label()
        )
    });
    let value = app.status.as_deref().or(detail.as_deref()).unwrap_or("");
    frame.render_widget(
        Paragraph::new(truncate(value, area.width as usize))
            .style(Style::default().fg(Color::DarkGray)),
        area,
    );
}

fn render_footer(frame: &mut Frame<'_>, area: Rect, app: &AgentPickerApp) {
    if area.width == 0 || area.height == 0 {
        return;
    }
    let key = Style::default()
        .fg(Color::Cyan)
        .add_modifier(Modifier::BOLD);
    let dim = Style::default().fg(Color::DarkGray);
    let spans = if app.search_focused {
        vec![
            Span::styled(" enter", key),
            Span::styled(" switch  ", dim),
            Span::styled("↑↓", key),
            Span::styled(" move  ", dim),
            Span::styled("ctrl+u", key),
            Span::styled(" clear  ", dim),
            Span::styled("esc", key),
            Span::styled(" back", dim),
        ]
    } else {
        vec![
            Span::styled(" enter", key),
            Span::styled(" switch  ", dim),
            Span::styled("/", key),
            Span::styled(" search  ", dim),
            Span::styled("b/w/i/d/a", key),
            Span::styled(" states  ", dim),
            Span::styled("j/k", key),
            Span::styled(" move  ", dim),
            Span::styled("pgup/dn", key),
            Span::styled(" preview  ", dim),
            Span::styled("esc", key),
            Span::styled(" close", dim),
        ]
    };
    frame.render_widget(Paragraph::new(Line::from(spans)), area);
}

fn preview_inner(area: Rect) -> Rect {
    Block::default().borders(Borders::ALL).inner(area)
}

fn aggregate_status(statuses: impl Iterator<Item = AgentStatus>) -> AgentStatus {
    statuses
        .min_by_key(|status| match status {
            AgentStatus::Blocked => 0,
            AgentStatus::Done => 1,
            AgentStatus::Working => 2,
            AgentStatus::Idle => 3,
            AgentStatus::Unknown => 4,
        })
        .unwrap_or(AgentStatus::Unknown)
}

fn status_icon(status: AgentStatus) -> &'static str {
    match status {
        AgentStatus::Blocked | AgentStatus::Working | AgentStatus::Done => "●",
        AgentStatus::Idle => "○",
        AgentStatus::Unknown => "·",
    }
}

fn status_color(status: AgentStatus) -> Color {
    match status {
        AgentStatus::Blocked => Color::Red,
        AgentStatus::Working => Color::Yellow,
        AgentStatus::Done => Color::Cyan,
        AgentStatus::Idle => Color::Green,
        AgentStatus::Unknown => Color::DarkGray,
    }
}

fn status_style(status: AgentStatus) -> Style {
    Style::default().fg(status_color(status))
}

fn short_pane_id(value: &str) -> &str {
    value.rsplit_once(':').map_or(value, |(_, suffix)| suffix)
}

fn short_tab_id(value: &str) -> &str {
    value.rsplit_once(':').map_or(value, |(_, suffix)| suffix)
}

fn truncate(value: &str, max_width: usize) -> String {
    if max_width == 0 {
        return String::new();
    }
    if UnicodeWidthStr::width(value) <= max_width {
        return value.to_string();
    }
    let ellipsis = "…";
    let target = max_width.saturating_sub(UnicodeWidthStr::width(ellipsis));
    let mut output = String::new();
    for character in value.chars() {
        let next_width = UnicodeWidthStr::width(output.as_str()) + character.width().unwrap_or(0);
        if next_width > target {
            break;
        }
        output.push(character);
    }
    output.push_str(ellipsis);
    output
}

fn ansi_line_count(value: &str) -> usize {
    value.lines().count().max(1)
}

/// `pane.read --format ansi` serializes terminal rows with CRLF endings. A
/// carriage return must never reach Ratatui as a cell symbol: when Herdr emits
/// that cell to the outer terminal it moves the cursor back to column zero and
/// lets preview text overwrite the picker. Terminal snapshots already contain
/// expanded grid cells, so discard C0 controls other than line feed and ESC
/// (which is required for SGR styling) before parsing.
fn sanitize_preview_ansi(value: &str) -> String {
    value
        .chars()
        .filter(|character| *character == '\n' || *character == '\u{1b}' || !character.is_control())
        .collect()
}

fn strip_ansi_fallback(value: &str) -> String {
    let mut output = String::with_capacity(value.len());
    let mut chars = value.chars().peekable();
    while let Some(character) = chars.next() {
        if character == '\u{1b}' && chars.peek() == Some(&'[') {
            chars.next();
            for code in chars.by_ref() {
                if ('@'..='~').contains(&code) {
                    break;
                }
            }
        } else {
            output.push(character);
        }
    }
    output
}

fn rect_contains(rect: Rect, column: u16, row: u16) -> bool {
    column >= rect.x
        && column < rect.x.saturating_add(rect.width)
        && row >= rect.y
        && row < rect.y.saturating_add(rect.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_picker_uses_side_by_side_preview() {
        let layout = picker_layout(Rect::new(0, 0, 120, 40));
        assert_eq!(layout.orientation, PickerOrientation::SideBySide);
        assert!(layout.list.x < layout.preview.x);
        assert_eq!(layout.list.y, layout.preview.y);
    }

    #[test]
    fn narrow_picker_stacks_list_above_preview() {
        let layout = picker_layout(Rect::new(0, 0, 80, 40));
        assert_eq!(layout.orientation, PickerOrientation::Stacked);
        assert!(layout.list.y < layout.preview.y);
        assert_eq!(layout.list.x, layout.preview.x);
    }

    #[test]
    fn status_aggregation_prioritizes_attention() {
        assert_eq!(
            aggregate_status([AgentStatus::Idle, AgentStatus::Blocked].into_iter()),
            AgentStatus::Blocked
        );
        assert_eq!(
            aggregate_status([AgentStatus::Idle, AgentStatus::Working].into_iter()),
            AgentStatus::Working
        );
    }

    #[test]
    fn truncation_honors_display_width() {
        assert_eq!(truncate("abcdef", 4), "abc…");
        assert_eq!(truncate("重构任务", 5), "重构…");
    }

    #[test]
    fn preview_ansi_sanitization_keeps_sgr_but_removes_cursor_controls() {
        assert_eq!(
            sanitize_preview_ansi("\u{1b}[31mred\r\nnext\tcell\u{1b}[0m"),
            "\u{1b}[31mred\nnextcell\u{1b}[0m"
        );
    }

    #[test]
    fn fallback_strips_sgr_sequences() {
        assert_eq!(strip_ansi_fallback("\u{1b}[31mred\u{1b}[0m"), "red");
    }
}
