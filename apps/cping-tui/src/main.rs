//! cping-tui —— 交互式终端界面。
//!
//! 真正的 TUI（不是 CLI 包装）：常驻备用屏幕、键盘驱动、矩阵/表格双视图、
//! 实时进度、离线过滤、导出。脚本化参数只作为可选的初始值，不能替代交互界面。

mod app;
mod events;
mod input;
mod ui;

use std::io;
use std::panic;

use ratatui::backend::CrosstermBackend;
use ratatui::crossterm::event::{DisableMouseCapture, EnableMouseCapture};
use ratatui::crossterm::execute;
use ratatui::crossterm::terminal::{
    disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen,
};
use ratatui::Terminal;

fn main() -> io::Result<()> {
    let options = app::Options::from_args(std::env::args().skip(1));

    if options.help {
        print_help();
        return Ok(());
    }

    // panic 也要恢复终端，否则用户的 shell 会卡在 raw mode
    let hook = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        let _ = restore_terminal();
        hook(info);
    }));

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen, EnableMouseCapture)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let result = app::App::new(options).run(&mut terminal);

    let _ = execute!(
        terminal.backend_mut(),
        LeaveAlternateScreen,
        DisableMouseCapture
    );
    restore_terminal()?;
    result
}

fn restore_terminal() -> io::Result<()> {
    disable_raw_mode()?;
    execute!(io::stdout(), LeaveAlternateScreen, DisableMouseCapture)?;
    Ok(())
}

fn print_help() {
    println!(
        "cping-tui —— C网群PING工具（终端版）

用法:
  cping-tui [选项]

选项:
  --prefix <前三段>   初始网段前缀，例如 192.168.1
  --no-color          禁用颜色（无颜色终端下自动降级为符号标识）
  --hide-offline      启动时即隐藏不在线目标
  --view <matrix|table>  初始视图
  --mock              使用模拟探测数据（演示 / UI 调试，不发真实报文）
  --help              显示本帮助

探测后端:
  默认使用当前平台的原生 ICMP。若平台拿不到权限（Android 普通应用常见），
  状态栏会显示相应错误，而不会被当成「探测成功」。

仅对你有权探测的网络使用本工具。并发与目标数量都有限制。

交互快捷键:
  Enter   开始探测（编辑输入框时表示确认）
  i       进入/退出前缀编辑
  Esc     取消当前探测 / 关闭弹窗
  g       矩阵视图
  t       表格视图
  f       过滤不在线目标
  s       导出 CSV
  S       导出 TXT
  ↑/↓ PgUp/PgDn  表格滚动
  ?或F1   帮助
  q       退出（运行中需再按一次确认）
  Ctrl+C  退出（退出前恢复终端）"
    );
}
