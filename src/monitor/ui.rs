use crate::monitor::config::MonitorEndpoints;
use crate::monitor::event::{
    ConnectionState, ConnectionStatus, MonitorCommand, MonitorEvent, ResponseType,
};
use crate::monitor::message::{MessageKind, MonitorMessage};
use crate::monitor::state::{InputMode, MonitorState, QueueItem, StreamLayout};
use chrono::Utc;
use crossterm::{
    event::{self, Event as CEvent, KeyCode, KeyEvent, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Corner, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, List, ListItem, Paragraph, Wrap},
    Frame, Terminal,
};
use std::{
    io,
    path::{Path, PathBuf},
    time::Duration,
};
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

/// Run the TUI loop and keep the terminal interface responsive.
pub fn run_tui(
    endpoints: MonitorEndpoints,
    workspace_root: PathBuf,
    mut event_rx: UnboundedReceiver<MonitorEvent>,
    command_tx: UnboundedSender<MonitorCommand>,
) -> crate::Result<()> {
    let mut stdout = io::stdout();
    enable_raw_mode()?;
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut state = MonitorState::new(ConnectionStatus {
        state: ConnectionState::Connecting,
        detail: Some("initializing".to_string()),
    });

    loop {
        state.tick();
        while let Ok(event) = event_rx.try_recv() {
            state.apply_event(event);
        }

        terminal.draw(|frame| draw_ui(frame, &state, &endpoints, workspace_root.as_path()))?;

        if state.exit_requested() {
            break;
        }

        if event::poll(Duration::from_millis(120))? {
            if let CEvent::Key(key) = event::read()? {
                handle_key(key, &mut state, &command_tx);
            }
        }
    }

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    Ok(())
}

fn draw_ui<B: ratatui::backend::Backend>(
    frame: &mut Frame<B>,
    state: &MonitorState,
    endpoints: &MonitorEndpoints,
    workspace_root: &Path,
) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints(
            [
                Constraint::Length(4),
                Constraint::Min(0),
                Constraint::Length(3),
            ]
            .as_ref(),
        )
        .split(frame.size());

    render_header(
        frame,
        chunks[0],
        HeaderParams {
            state,
            endpoints,
            workspace_root,
        },
    );
    render_body(frame, chunks[1], state);
    render_status(frame, chunks[2], state);
}

/// Parameters for rendering the header section
struct HeaderParams<'a> {
    state: &'a MonitorState,
    endpoints: &'a MonitorEndpoints,
    workspace_root: &'a Path,
}

fn render_header<B: ratatui::backend::Backend>(
    frame: &mut Frame<B>,
    area: Rect,
    params: HeaderParams,
) {
    let content = build_header_lines(&params);

    let header = Paragraph::new(content)
        .block(
            Block::default()
                .title("Newton Monitor")
                .borders(Borders::ALL),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(header, area);
}

fn build_header_lines<'a>(params: &'a HeaderParams<'a>) -> Vec<Line<'a>> {
    let connection = match params.state.connection_status.state {
        ConnectionState::Connected => "Connected",
        ConnectionState::Connecting => "Connecting",
        ConnectionState::Disconnected => "Disconnected",
    };

    let connection_detail = params
        .state
        .connection_status
        .detail
        .as_deref()
        .unwrap_or_default();

    let layout_name = match params.state.layout {
        StreamLayout::Tiles => "Tiles",
        StreamLayout::List => "List",
    };

    let filter_display = params.state.filter.display();
    let filter_text = if filter_display.is_empty() {
        "None".to_string()
    } else {
        filter_display
    };

    vec![
        Line::from(vec![
            Span::styled("Workspace: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(params.workspace_root.display().to_string()),
        ]),
        Line::from(vec![
            Span::styled(
                "Connection: ",
                Style::default().add_modifier(Modifier::BOLD),
            ),
            Span::styled(connection, Style::default().fg(Color::LightGreen)),
            Span::raw(format!(" {}", connection_detail)),
            Span::raw(" | "),
            Span::styled("Filter: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(filter_text),
            Span::raw(" | "),
            Span::styled("View: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(layout_name),
        ]),
        Line::from(vec![
            Span::styled("HTTP: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(truncate_str(params.endpoints.http_url.as_str(), 40)),
            Span::raw(" "),
            Span::styled("WS: ", Style::default().add_modifier(Modifier::BOLD)),
            Span::raw(truncate_str(params.endpoints.ws_url.as_str(), 40)),
        ]),
    ]
}

fn render_body<B: ratatui::backend::Backend>(
    frame: &mut Frame<B>,
    area: Rect,
    state: &MonitorState,
) {
    if state.queue_tab {
        render_queue(frame, area, state);
    } else {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
            .split(area);
        render_stream(frame, cols[0], state);
        render_queue(frame, cols[1], state);
    }

    if state.help_visible {
        render_help(frame, area);
    }
}

fn render_stream<B: ratatui::backend::Backend>(
    frame: &mut Frame<B>,
    area: Rect,
    state: &MonitorState,
) {
    let channels: Vec<_> = state
        .channels
        .values()
        .filter(|channel| state.filter.matches(&channel.project, &channel.branch))
        .collect();

    if channels.is_empty() {
        render_empty_stream(frame, area);
        return;
    }

    let items = match state.layout {
        StreamLayout::Tiles => render_tiles_view(&channels),
        StreamLayout::List => render_list_view(&channels),
    };

    let title = match state.layout {
        StreamLayout::Tiles => "Stream (Tiles)",
        StreamLayout::List => "Stream (List)",
    };
    let list = List::new(items)
        .block(Block::default().title(title).borders(Borders::ALL))
        .start_corner(Corner::TopLeft);
    frame.render_widget(list, area);
}

fn render_empty_stream<B: ratatui::backend::Backend>(frame: &mut Frame<B>, area: Rect) {
    let block = Block::default()
        .title("Stream (empty)")
        .borders(Borders::ALL);
    let empty = Paragraph::new("No channels match the current filter.")
        .block(block)
        .style(Style::default().fg(Color::DarkGray));
    frame.render_widget(empty, area);
}

fn render_tiles_view<'a>(
    channels: &'a [&'a crate::monitor::state::ChannelState],
) -> Vec<ListItem<'a>> {
    channels
        .iter()
        .map(|channel| {
            let mut lines = vec![Line::from(vec![Span::styled(
                format!("{} / {}", channel.project, channel.branch),
                Style::default()
                    .fg(Color::Cyan)
                    .add_modifier(Modifier::BOLD),
            )])];
            for message in channel.messages.iter().rev().take(4).rev() {
                lines.push(render_message_span(message));
            }
            ListItem::new(lines)
        })
        .collect()
}

fn render_list_view<'a>(
    channels: &'a [&'a crate::monitor::state::ChannelState],
) -> Vec<ListItem<'a>> {
    let mut entries = Vec::new();
    for channel in channels {
        for message in channel.messages.iter() {
            entries.push((*channel, message));
        }
    }
    entries.sort_by_key(|(_, message)| message.timestamp);

    entries
        .into_iter()
        .rev()
        .take(80)
        .map(|(channel, message)| {
            let line = Line::from(vec![
                Span::styled(
                    format!("[{} / {}] ", channel.project, channel.branch),
                    Style::default().fg(Color::LightBlue),
                ),
                Span::styled(
                    message.summary.clone(),
                    Style::default().fg(message_color(message)),
                ),
            ]);
            ListItem::new(vec![line])
        })
        .collect()
}

fn render_queue<B: ratatui::backend::Backend>(
    frame: &mut Frame<B>,
    area: Rect,
    state: &MonitorState,
) {
    let items = state
        .queue
        .iter()
        .enumerate()
        .map(|(idx, item)| {
            let mut lines = Vec::new();
            lines.push(Line::from(vec![Span::styled(
                format!("{} ", item.message.channel),
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )]));
            lines.push(Line::from(Span::raw(item.message.summary.clone())));
            if let Some(remaining) = render_remaining(item) {
                lines.push(Line::from(vec![Span::styled(
                    format!(" ({})", remaining),
                    Style::default().fg(Color::Red),
                )]));
            }
            ListItem::new(lines).style(render_queue_style(idx, state))
        })
        .collect::<Vec<_>>();

    let position = if state.queue.is_empty() {
        0
    } else {
        state.queue_index + 1
    };
    let title = format!("Queue ({}/{})", position, state.queue.len());
    let block = Block::default().title(title).borders(Borders::ALL);
    let list = List::new(items).block(block);
    frame.render_widget(list, area);
}

fn render_status<B: ratatui::backend::Backend>(
    frame: &mut Frame<B>,
    area: Rect,
    state: &MonitorState,
) {
    let mut spans = Vec::new();

    match state.input_mode {
        InputMode::Filter => {
            spans.push(Span::styled(
                format!("/ {}", state.filter_input),
                Style::default().fg(Color::Magenta),
            ));
        }
        InputMode::Answer { .. } => {
            spans.push(Span::styled(
                format!("Answer: {}", state.answer_buffer),
                Style::default().fg(Color::Green),
            ));
        }
        InputMode::Authorization { .. } => {
            spans.push(Span::raw("[a] Approve  [d] Deny  [Esc] Skip"));
        }
        InputMode::Normal => {
            spans.push(Span::raw(
                "Enter/a: act  n/Tab: next  /: filter  V: toggle view  Q: queue tab  ?: help",
            ));
        }
    }

    let footer = Paragraph::new(Line::from(spans))
        .block(Block::default().borders(Borders::ALL).title("Status"))
        .wrap(Wrap { trim: true });
    frame.render_widget(footer, area);
}

fn render_help<B: ratatui::backend::Backend>(frame: &mut Frame<B>, area: Rect) {
    let help_lines = vec![
        Line::from(Span::styled(
            "Keybindings",
            Style::default().add_modifier(Modifier::BOLD),
        )),
        Line::from("q / Ctrl+C: Quit"),
        Line::from("n / Tab: Next queue item"),
        Line::from("Enter/a: Answer (question) or Approve (authorization)"),
        Line::from("d: Deny authorization"),
        Line::from("/: Filter stream (project or project/branch)"),
        Line::from("V: Toggle tiles/list"),
        Line::from("Q: Toggle queue-only view"),
        Line::from("?: Toggle this help overlay"),
        Line::from("Esc: Cancel input / close queue tab"),
    ];
    let overlay = Paragraph::new(help_lines)
        .block(
            Block::default()
                .title("Help")
                .borders(Borders::ALL)
                .style(Style::default().bg(Color::Black)),
        )
        .wrap(Wrap { trim: true });
    frame.render_widget(overlay, area);
}

fn handle_key(
    key: KeyEvent,
    state: &mut MonitorState,
    command_tx: &UnboundedSender<MonitorCommand>,
) {
    if key.modifiers.contains(KeyModifiers::CONTROL) && matches!(key.code, KeyCode::Char('c')) {
        state.request_exit();
        return;
    }

    if matches!(key.code, KeyCode::Char('?')) {
        state.help_visible = !state.help_visible;
        return;
    }

    if state.help_visible {
        if matches!(key.code, KeyCode::Esc) {
            state.help_visible = false;
        }
        return;
    }

    match state.input_mode.clone() {
        InputMode::Filter => handle_filter_input(key, state),
        InputMode::Answer { target } => handle_answer_input(key, state, command_tx, target),
        InputMode::Authorization { target } => {
            handle_authorization_input(key, state, command_tx, target)
        }
        InputMode::Normal => handle_normal_input(key, state, command_tx),
    }
}

fn handle_filter_input(key: KeyEvent, state: &mut MonitorState) {
    match key.code {
        KeyCode::Char(c) => {
            state.filter_input.push(c);
        }
        KeyCode::Backspace => {
            state.filter_input.pop();
        }
        KeyCode::Enter => {
            let input = state.filter_input.clone();
            state.apply_filter(&input);
            state.filter_input.clear();
            state.input_mode = InputMode::Normal;
        }
        KeyCode::Esc => {
            state.filter_input.clear();
            state.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn handle_answer_input(
    key: KeyEvent,
    state: &mut MonitorState,
    command_tx: &UnboundedSender<MonitorCommand>,
    target: Uuid,
) {
    match key.code {
        KeyCode::Char(c) => {
            state.answer_buffer.push(c);
        }
        KeyCode::Backspace => {
            state.answer_buffer.pop();
        }
        KeyCode::Enter => {
            let answer = state.answer_buffer.trim().to_string();
            let _ = command_tx.send(MonitorCommand::Respond {
                message_id: target,
                answer: Some(answer),
                response_type: ResponseType::Text,
            });
            state.answer_buffer.clear();
            state.input_mode = InputMode::Normal;
            state.next_queue();
        }
        KeyCode::Esc => {
            state.answer_buffer.clear();
            state.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn handle_authorization_input(
    key: KeyEvent,
    state: &mut MonitorState,
    command_tx: &UnboundedSender<MonitorCommand>,
    target: Uuid,
) {
    match key.code {
        KeyCode::Char('a') => {
            send_response(
                command_tx,
                target,
                None,
                ResponseType::AuthorizationApproved,
            );
            state.input_mode = InputMode::Normal;
            state.next_queue();
        }
        KeyCode::Char('d') => {
            send_response(command_tx, target, None, ResponseType::AuthorizationDenied);
            state.input_mode = InputMode::Normal;
            state.next_queue();
        }
        KeyCode::Esc => {
            state.input_mode = InputMode::Normal;
        }
        _ => {}
    }
}

fn handle_normal_input(
    key: KeyEvent,
    state: &mut MonitorState,
    command_tx: &UnboundedSender<MonitorCommand>,
) {
    match key.code {
        KeyCode::Char('q') => state.request_exit(),
        KeyCode::Char('/') => handle_filter_start(state),
        KeyCode::Char('n') | KeyCode::Tab | KeyCode::Down | KeyCode::Char('j') => {
            state.next_queue();
        }
        KeyCode::Char('k') | KeyCode::Up => state.previous_queue(),
        KeyCode::Char('g') => state.queue_index = 0,
        KeyCode::Char('G') => handle_jump_to_end(state),
        KeyCode::Char('V') | KeyCode::Char('v') => state.toggle_layout(),
        KeyCode::Char('Q') => state.toggle_queue_tab(),
        KeyCode::Char('a') => handle_action_key(state, command_tx),
        KeyCode::Char('d') => handle_deny_key(state, command_tx),
        KeyCode::Enter => handle_enter_key(state),
        KeyCode::Esc => handle_escape_key(state),
        _ => {}
    }
}

fn handle_filter_start(state: &mut MonitorState) {
    state.input_mode = InputMode::Filter;
    state.filter_input = state.filter.display();
}

fn handle_jump_to_end(state: &mut MonitorState) {
    if !state.queue.is_empty() {
        state.queue_index = state.queue.len() - 1;
    }
}

fn handle_action_key(state: &mut MonitorState, command_tx: &UnboundedSender<MonitorCommand>) {
    let Some(item) = state.queue.get(state.queue_index) else {
        return;
    };

    match item.message.kind {
        MessageKind::Question => {
            state.input_mode = InputMode::Answer {
                target: item.message.id,
            };
            state.answer_buffer.clear();
        }
        MessageKind::Authorization => {
            send_response(
                command_tx,
                item.message.id,
                None,
                ResponseType::AuthorizationApproved,
            );
            state.next_queue();
        }
        _ => {}
    }
}

fn handle_deny_key(state: &mut MonitorState, command_tx: &UnboundedSender<MonitorCommand>) {
    let Some(item) = state.queue.get(state.queue_index) else {
        return;
    };

    if item.message.kind == MessageKind::Authorization {
        send_response(
            command_tx,
            item.message.id,
            None,
            ResponseType::AuthorizationDenied,
        );
        state.next_queue();
    }
}

fn handle_enter_key(state: &mut MonitorState) {
    let Some(item) = state.queue.get(state.queue_index) else {
        return;
    };

    match item.message.kind {
        MessageKind::Question => {
            state.input_mode = InputMode::Answer {
                target: item.message.id,
            };
            state.answer_buffer.clear();
        }
        MessageKind::Authorization => {
            state.input_mode = InputMode::Authorization {
                target: item.message.id,
            };
        }
        _ => {}
    }
}

fn handle_escape_key(state: &mut MonitorState) {
    if state.queue_tab {
        state.queue_tab = false;
    }
}

fn send_response(
    command_tx: &UnboundedSender<MonitorCommand>,
    message_id: uuid::Uuid,
    answer: Option<String>,
    response_type: ResponseType,
) {
    let _ = command_tx.send(MonitorCommand::Respond {
        message_id,
        answer,
        response_type,
    });
}

fn render_message_span(message: &MonitorMessage) -> Line<'_> {
    let timestamp = message.timestamp.format("%H:%M:%S").to_string();
    Line::from(vec![
        Span::styled(
            format!("[{}] ", timestamp),
            Style::default().fg(Color::DarkGray),
        ),
        Span::styled(
            message.summary.clone(),
            Style::default().fg(message_color(message)),
        ),
    ])
}

fn render_queue_style(idx: usize, state: &MonitorState) -> Style {
    let mut style = Style::default();
    if idx == state.queue_index {
        style = style
            .bg(Color::LightYellow)
            .fg(Color::Black)
            .add_modifier(Modifier::BOLD);
    }
    if let Some(item) = state.queue.get(idx) {
        if item.is_flashing() {
            style = style.add_modifier(Modifier::ITALIC);
        }
    }
    style
}

fn render_remaining(item: &QueueItem) -> Option<String> {
    if let (Some(timeout), Ok(elapsed)) = (
        item.message.timeout_seconds,
        Utc::now()
            .signed_duration_since(item.message.timestamp)
            .to_std(),
    ) {
        if elapsed >= std::time::Duration::from_secs(timeout) {
            return Some("timed out".to_string());
        }
        let remaining = timeout.saturating_sub(elapsed.as_secs());
        return Some(format!("{}s left", remaining));
    }
    None
}

fn message_color(message: &MonitorMessage) -> Color {
    match message.kind {
        MessageKind::Question | MessageKind::Authorization => Color::Yellow,
        MessageKind::Stderr => Color::Red,
        MessageKind::Stdout => Color::White,
        _ => Color::LightBlue,
    }
}

fn truncate_str(value: &str, max: usize) -> String {
    if value.len() <= max {
        value.to_string()
    } else {
        format!("{}…", &value[..max.saturating_sub(1)])
    }
}
