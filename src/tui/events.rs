use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use crate::model::MessageType;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AppAction {
    Quit,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    ScrollTop,
    ScrollBottom,
    CycleFilter,
    ClearFilter,
    ToggleType(MessageType),
    Noop,
}

pub fn handle_key(key: KeyEvent) -> AppAction {
    match key.code {
        KeyCode::Char('q') => AppAction::Quit,
        KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => AppAction::Quit,
        KeyCode::Up | KeyCode::Char('k') => AppAction::ScrollUp,
        KeyCode::Down | KeyCode::Char('j') => AppAction::ScrollDown,
        KeyCode::PageUp => AppAction::PageUp,
        KeyCode::PageDown => AppAction::PageDown,
        KeyCode::Home => AppAction::ScrollTop,
        KeyCode::End | KeyCode::Char('e') => AppAction::ScrollBottom,
        KeyCode::Char('f') => AppAction::CycleFilter,
        KeyCode::Char('F') => AppAction::ClearFilter,
        KeyCode::Char('1') => AppAction::ToggleType(MessageType::User),
        KeyCode::Char('2') => AppAction::ToggleType(MessageType::Assistant),
        KeyCode::Char('3') => AppAction::ToggleType(MessageType::ToolCall),
        KeyCode::Char('4') => AppAction::ToggleType(MessageType::System),
        _ => AppAction::Noop,
    }
}
