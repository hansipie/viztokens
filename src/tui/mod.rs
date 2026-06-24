pub mod events;
pub mod widgets;

use std::collections::HashMap;
use std::io::Stdout;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context;
use crossterm::event::{self, Event};
use crossterm::execute;
use crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::backend::CrosstermBackend;
use ratatui::layout::{Constraint, Direction, Layout};
use ratatui::Terminal;
use tokio::sync::mpsc;

use crate::model::{Message, MessageFilter, MessageType};
use crate::store::Store;
use crate::watcher::session::DiscoveredSession;
use crate::watcher::WatcherEvent;
use events::{handle_key, AppAction};
use widgets::message_list::render_message_list;
use widgets::status_bar::{render_footer, render_header, StatusBarState};

pub struct App {
    pub messages: Vec<Message>,
    pub session_projects: HashMap<String, String>,
    pub session_count: usize,
    pub watching_count: usize,
    pub parse_errors: u32,
    pub scroll_offset: usize,
    pub follow_mode: bool,
    pub total_lines: usize,
    pub filter: MessageFilter,
    layout_width: u16,
    message_heights: Vec<u16>,
    rx: mpsc::Receiver<WatcherEvent>,
    #[allow(dead_code)]
    store: Arc<Store>,
}

impl App {
    pub fn new(
        rx: mpsc::Receiver<WatcherEvent>,
        store: Arc<Store>,
        sessions: Vec<DiscoveredSession>,
        history: Vec<Message>,
    ) -> Self {
        let session_projects: HashMap<String, String> = sessions
            .iter()
            .map(|s| (s.session_id.clone(), s.project_name.clone()))
            .collect();
        let session_count = sessions.len();
        App {
            messages: history,
            session_projects,
            session_count,
            watching_count: session_count,
            parse_errors: 0,
            scroll_offset: 0,
            follow_mode: true,
            total_lines: 0,
            filter: MessageFilter::default(),
            layout_width: 0,
            message_heights: Vec::new(),
            rx,
            store,
        }
    }

    pub fn visible_messages(&self) -> Vec<&Message> {
        self.messages
            .iter()
            .filter(|m| self.filter.allows(&m.message_type))
            .collect()
    }

    fn update_layout_cache(&mut self, width: u16) {
        if self.layout_width != width {
            self.layout_width = width;
            self.message_heights.clear();
        }

        self.message_heights.extend(
            self.messages[self.message_heights.len()..]
                .iter()
                .map(|message| widgets::message_list::message_height(message, width)),
        );
    }

    pub fn cycle_filter(&mut self) {
        use MessageType::*;
        let current = self.filter.allowed_types.clone();
        if current.is_empty() {
            self.filter.allowed_types = std::iter::once(User).collect();
        } else if current.len() == 1 && current.contains(&User) {
            self.filter.allowed_types = std::iter::once(Assistant).collect();
        } else if current.len() == 1 && current.contains(&Assistant) {
            self.filter.allowed_types = [ToolCall, ToolResult].iter().cloned().collect();
        } else if current.contains(&ToolCall) && current.contains(&ToolResult) {
            self.filter.allowed_types = std::iter::once(System).collect();
        } else {
            self.filter.clear();
        }
    }

    pub fn scroll_up(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_sub(n);
        self.follow_mode = false;
    }

    pub fn scroll_down(&mut self, n: usize) {
        self.scroll_offset = self.scroll_offset.saturating_add(n);
        self.follow_mode = false;
    }

    pub fn scroll_to_top(&mut self) {
        self.scroll_offset = 0;
        self.follow_mode = false;
    }

    pub fn scroll_to_bottom(&mut self) {
        self.scroll_offset = self.total_lines.saturating_sub(1);
        self.follow_mode = true;
    }

    fn apply_action(&mut self, action: AppAction, page_size: usize) {
        match action {
            AppAction::ScrollUp => self.scroll_up(1),
            AppAction::ScrollDown => self.scroll_down(1),
            AppAction::PageUp => self.scroll_up(page_size),
            AppAction::PageDown => self.scroll_down(page_size),
            AppAction::ScrollTop => self.scroll_to_top(),
            AppAction::ScrollBottom => self.scroll_to_bottom(),
            AppAction::CycleFilter => self.cycle_filter(),
            AppAction::ClearFilter => self.filter.clear(),
            AppAction::ToggleType(t) => {
                if t == MessageType::ToolCall {
                    self.filter.toggle(MessageType::ToolCall);
                    self.filter.toggle(MessageType::ToolResult);
                } else {
                    self.filter.toggle(t);
                }
            }
            AppAction::Quit | AppAction::Noop => {}
        }
    }
}

pub fn init_terminal() -> anyhow::Result<Terminal<CrosstermBackend<Stdout>>> {
    enable_raw_mode().context("enable raw mode")?;
    let mut stdout = std::io::stdout();
    execute!(stdout, EnterAlternateScreen).context("enter alternate screen")?;
    execute!(stdout, crossterm::cursor::Hide).context("hide cursor")?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend).context("create terminal")?;
    // Clear the physical terminal, not only Ratatui's in-memory frame. On
    // startup both diff buffers are blank, so rendering `Clear` alone may not
    // emit updates for stale content retained by some terminal emulators.
    terminal.clear().context("clear terminal")?;
    Ok(terminal)
}

pub fn restore_terminal(terminal: &mut Terminal<CrosstermBackend<Stdout>>) {
    let _ = disable_raw_mode();
    let _ = execute!(terminal.backend_mut(), LeaveAlternateScreen);
    let _ = execute!(terminal.backend_mut(), crossterm::cursor::Show);
    let _ = terminal.show_cursor();
}

pub fn run(mut app: App, mut terminal: Terminal<CrosstermBackend<Stdout>>) -> anyhow::Result<()> {
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        run_loop(&mut app, &mut terminal)
    }));
    restore_terminal(&mut terminal);
    match result {
        Ok(r) => r,
        Err(e) => std::panic::resume_unwind(e),
    }
}

fn run_loop(
    app: &mut App,
    terminal: &mut Terminal<CrosstermBackend<Stdout>>,
) -> anyhow::Result<()> {
    let tick = Duration::from_millis(100);
    loop {
        // Drain incoming watcher events
        while let Ok(evt) = app.rx.try_recv() {
            match evt {
                WatcherEvent::Message(msg) => {
                    app.messages.push(msg);
                    if app.follow_mode {
                        app.scroll_to_bottom();
                    }
                }
                WatcherEvent::ParseError(_) => {
                    app.parse_errors += 1;
                }
                WatcherEvent::SessionEnded(_) => {
                    app.watching_count = app.watching_count.saturating_sub(1);
                }
            }
        }

        let term_width = terminal.size().map(|s| s.width).unwrap_or(80);
        let content_height = terminal
            .size()
            .map(|s| s.height.saturating_sub(2))
            .unwrap_or(20);

        app.update_layout_cache(term_width);
        app.total_lines = app
            .messages
            .iter()
            .zip(&app.message_heights)
            .filter(|(message, _)| app.filter.allows(&message.message_type))
            .map(|(_, height)| *height as usize)
            .sum();

        // Auto-follow: keep scroll at bottom
        if app.follow_mode {
            app.scroll_offset = app.total_lines.saturating_sub(content_height as usize);
        }

        let visible: Vec<(&Message, u16)> = app
            .messages
            .iter()
            .zip(&app.message_heights)
            .filter(|(message, _)| app.filter.allows(&message.message_type))
            .map(|(message, height)| (message, *height))
            .collect();

        // Render
        terminal.draw(|frame| {
            frame.render_widget(ratatui::widgets::Clear, frame.area());
            let chunks = Layout::default()
                .direction(Direction::Vertical)
                .constraints([
                    Constraint::Length(1), // header
                    Constraint::Min(0),    // message list
                    Constraint::Length(1), // footer
                ])
                .split(frame.area());

            let (total_input_tokens, total_output_tokens) = app
                .messages
                .iter()
                .filter(|m| app.filter.allows(&m.message_type))
                .fold((0u64, 0u64), |(i, o), m| {
                    (
                        i + m.input_tokens.unwrap_or(0) as u64,
                        o + m.output_tokens.unwrap_or(0) as u64,
                    )
                });

            let state = StatusBarState {
                watching_count: app.watching_count,
                session_count: app.session_count,
                filter_label: app.filter.label(),
                parse_errors: app.parse_errors,
                follow_mode: app.follow_mode,
                total_input_tokens,
                total_output_tokens,
            };
            render_header(&state, chunks[0], frame);
            render_message_list(
                &visible,
                chunks[1],
                frame,
                app.scroll_offset,
                &app.session_projects,
            );
            render_footer(&state, chunks[2], frame);
        })?;

        // Process the complete input queue. Reading only one event per frame
        // makes key repeats accumulate and continue scrolling after key-up.
        if event::poll(tick)? {
            loop {
                if let Event::Key(key) = event::read()? {
                    let action = handle_key(key);
                    if action == AppAction::Quit {
                        return Ok(());
                    }
                    app.apply_action(action, content_height as usize);
                }

                if !event::poll(Duration::ZERO)? {
                    break;
                }
            }
        }
    }
}
