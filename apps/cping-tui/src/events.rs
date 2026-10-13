//! 终端事件轮询封装。
//!
//! 用「轮询 + 固定 tick」而不是阻塞读，这样后台探测事件能被及时拉取，
//! 同时终端 resize 由 crossterm 的 Resize 事件驱动重绘。

use std::io;
use std::time::Duration;

use ratatui::crossterm::event::{self, Event, KeyEvent};

/// 一次 tick 的最长等待时间（同时决定界面刷新频率）。
pub const TICK: Duration = Duration::from_millis(60);

pub enum UiEvent {
    Key(KeyEvent),
    Resize,
    Tick,
}

/// 等待最多 `TICK`，返回本轮要处理的事件。
pub fn next_event() -> io::Result<UiEvent> {
    if event::poll(TICK)? {
        match event::read()? {
            Event::Key(key) => Ok(UiEvent::Key(key)),
            Event::Resize(_, _) => Ok(UiEvent::Resize),
            _ => Ok(UiEvent::Tick),
        }
    } else {
        Ok(UiEvent::Tick)
    }
}
