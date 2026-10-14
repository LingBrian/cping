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
    /// M7：读邻居表回填 MAC / 厂商
    Enrich,
    /// M7：反查当前选中行的主机名
    LookupHostname,
    /// M7：把当前结果存成快照文件
    SaveSnapshot,
    /// M7：从快照文件恢复
    LoadSnapshot,
    /// 快照恢复确认框：恢复
    PromptYes,
    /// 快照恢复确认框：不恢复
    PromptNo,
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
        // M7 快捷键：m 补厂商、n 查主机名、w 存快照、l 读快照
        KeyCode::Char('m') => Action::Enrich,
        KeyCode::Char('n') => Action::LookupHostname,
        KeyCode::Char('w') => Action::SaveSnapshot,
        KeyCode::Char('l') => Action::LoadSnapshot,
        // 确认框用：y 恢复、N 不恢复（小写 n 在确认框里也当「不恢复」，
        // 由 app 层按当前状态解释，见 App::on_key）
        KeyCode::Char('y') | KeyCode::Char('Y') => Action::PromptYes,
        KeyCode::Char('N') => Action::PromptNo,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn press(c: char) -> Action {
        map_key(KeyEvent::new(KeyCode::Char(c), KeyModifiers::NONE))
    }

    #[test]
    fn m7_shortcuts_are_mapped() {
        assert_eq!(press('m'), Action::Enrich);
        assert_eq!(press('n'), Action::LookupHostname);
        assert_eq!(press('w'), Action::SaveSnapshot);
        assert_eq!(press('l'), Action::LoadSnapshot);
        assert_eq!(press('N'), Action::PromptNo);
        assert_eq!(press('y'), Action::PromptYes);
    }

    #[test]
    fn existing_shortcuts_are_unchanged() {
        // 回归：新增快捷键不得抢占既有按键
        assert_eq!(press('s'), Action::SaveCsv);
        assert_eq!(press('S'), Action::SaveTxt);
        assert_eq!(press('i'), Action::ToggleEdit);
        assert_eq!(press('t'), Action::ViewTable);
        assert_eq!(press('g'), Action::ViewMatrix);
        assert_eq!(press('f'), Action::ToggleFilter);
        assert_eq!(press('q'), Action::Quit);
    }
}
