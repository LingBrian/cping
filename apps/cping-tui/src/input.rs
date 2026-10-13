//! 按键 → 动作映射。

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyEventKind, KeyModifiers};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    None,
    Quit,
    ForceQuit,
    Start,
    Cancel,
    ToggleEdit,
    ViewMatrix,
    ViewTable,
    ToggleFilter,
    SaveCsv,
    SaveTxt,
    ToggleHelp,
    ScrollUp,
    ScrollDown,
    PageUp,
    PageDown,
    Top,
    Bottom,
    Char(char),
    Backspace,
}

/// 仅处理「按下」，忽略 Windows 上的 Release/Repeat 重复触发。
pub fn map_key(key: KeyEvent) -> Action {
    if key.kind != KeyEventKind::Press && key.kind != KeyEventKind::Repeat {
        return Action::None;
    }

    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') => Action::ForceQuit,
            _ => Action::None,
        };
    }

    match key.code {
        KeyCode::Enter => Action::Start,
        KeyCode::Esc => Action::Cancel,
        KeyCode::Char('i') => Action::ToggleEdit,
        KeyCode::Char('g') => Action::ViewMatrix,
        KeyCode::Char('t') => Action::ViewTable,
        KeyCode::Char('f') => Action::ToggleFilter,
        KeyCode::Char('s') => Action::SaveCsv,
        KeyCode::Char('S') => Action::SaveTxt,
        KeyCode::Char('?') | KeyCode::F(1) => Action::ToggleHelp,
        KeyCode::Char('q') => Action::Quit,
        KeyCode::Up => Action::ScrollUp,
        KeyCode::Down => Action::ScrollDown,
        KeyCode::PageUp => Action::PageUp,
        KeyCode::PageDown => Action::PageDown,
        KeyCode::Home => Action::Top,
        KeyCode::End => Action::Bottom,
        KeyCode::Backspace => Action::Backspace,
        KeyCode::Char(c) => Action::Char(c),
        _ => Action::None,
    }
}
