//! cping-tui —— 交互式终端界面。
//!
//! 真正的 TUI（不是 CLI 包装）：常驻备用屏幕、键盘驱动、矩阵/表格双视图、
//! 实时进度、离线过滤、导出。脚本化参数只作为可选的初始值，不能替代交互界面。

mod app;
mod events;
mod i18n;
mod input;
mod ui;

use std::io;
use std::panic;

use cping_core::i18n::Lang;

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
        print_help(options.lang);
        return Ok(());
    }

    // M17：首次运行的授权提示。必须打印在进入备用屏幕**之前**——
    // 进了全屏就看不见了。TUI 里不做确认交互（同 CLI 的理由）。
    //
    // M18：语言从**命令行选项**里取。这里还没有 App 实例（App 要等进入备用
    // 屏幕之后才建），所以不能从 `self.lang` 拿——语言状态必须由 Options 一路带过来，
    // 否则这句提示会永远停在中文。
    app::print_authorization_notice(options.lang);

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

/// 打印 `--help`。整份文本按语言从 TUI 字典里取（见 `i18n::Key::CliHelp*`）。
///
/// 为什么不把帮助文本写死在 `main`：它是最常被用户复制到 issue 里的一段，
/// 中英混排会让人分不清「程序说了什么」和「用户看到什么」。
fn print_help(lang: Lang) {
    let t = |key: i18n::Key| i18n::text(lang, key);
    println!("{}", t(i18n::Key::CliHelpTop));
    println!();
    println!("{}", t(i18n::Key::CliHelpOptions));
    println!();
    println!("{}", t(i18n::Key::CliHelpBackend));
    println!();
    println!("{}", t(i18n::Key::CliHelpTargets));
    println!();
    println!(
        "{}",
        i18n::render(lang, i18n::Key::CliHelpSnapshot, &[app::AUTOSAVE_FILE])
    );
    println!();
    println!("{}", t(i18n::Key::CliHelpWarning));
    println!();
    println!("{}", t(i18n::Key::CliHelpKeys));
}
