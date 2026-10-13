#!/usr/bin/env python3
# -*- coding: utf-8 -*-
"""为 CPing 与 Android 工具链创建桌面快捷方式。

为什么要用这个脚本而不是 PowerShell：
本机的安全策略会拦截 `New-Object -ComObject WScript.Shell`（COM 实例化被认为可执行任意代码），
因此这里改用纯 Python 的 `pylnk3` 直接写 `.lnk` 二进制，不依赖任何 COM。

用法：
    python scripts/create-desktop-shortcuts.py            # 创建
    python scripts/create-desktop-shortcuts.py --dry-run  # 只看会创建什么
    python scripts/create-desktop-shortcuts.py --remove   # 删除本脚本创建的快捷方式

依赖：pylnk3（pip install pylnk3）
"""

from __future__ import annotations

import argparse
import os
import sys
from pathlib import Path

try:
    import pylnk3
except ImportError:
    print("缺少依赖：pylnk3\n请先执行： pip install pylnk3", file=sys.stderr)
    raise SystemExit(2)


REPO = Path(__file__).resolve().parent.parent
ANDROID_HOME = Path(r"D:\Program Files\Android")

# (快捷方式名, 目标程序, 参数, 工作目录, 图标, 说明)
SHORTCUTS = [
    (
        "CPing 桌面版",
        REPO / "apps/cping-gui/src-tauri/target/release/cping-gui.exe",
        None,
        None,
        None,
        "CPing 图形界面（Tauri 2 + Vue 3）",
    ),
    (
        "CPing 终端版",
        Path(os.environ.get("COMSPEC", r"C:\Windows\System32\cmd.exe")),
        f'/c "{REPO / "target/release/cping-tui.exe"}"',
        str(REPO),
        None,
        "CPing 交互式终端界面（ratatui）",
    ),
    (
        "Android SDK Manager",
        Path(os.environ.get("COMSPEC", r"C:\Windows\System32\cmd.exe")),
        f'/k "{ANDROID_HOME / "cmdline-tools/latest/bin/sdkmanager.bat"}" --list_installed',
        str(ANDROID_HOME),
        None,
        "Android SDK 命令行管理器（已装到 D:\\Program Files\\Android）",
    ),
    (
        "Android adb",
        Path(os.environ.get("COMSPEC", r"C:\Windows\System32\cmd.exe")),
        f'/k "{ANDROID_HOME / "platform-tools/adb.exe"}" devices',
        str(ANDROID_HOME / "platform-tools"),
        None,
        "Android 调试桥（adb）",
    ),
]


def desktop_dir() -> Path:
    # 优先 OneDrive 桌面，回退到标准 Desktop
    profile = Path(os.environ.get("USERPROFILE", str(Path.home())))
    for candidate in (
        profile / "OneDrive" / "Desktop",
        profile / "Desktop",
    ):
        if candidate.is_dir():
            return candidate
    return profile / "Desktop"


def create(target_dir: Path, dry_run: bool, only_existing: bool = True) -> int:
    made = 0
    for name, target, args, workdir, icon, desc in SHORTCUTS:
        lnk_path = target_dir / f"{name}.lnk"
        exists = Path(target).exists()
        if only_existing and not exists:
            print(f"  跳过 {name}：目标不存在 {target}")
            continue
        if dry_run:
            print(f"  [dry-run] {lnk_path}  ->  {target} {args or ''}")
            made += 1
            continue
        try:
            lnk = pylnk3.for_file(
                str(target),
                lnk_name=str(lnk_path),
                arguments=args,
                description=desc,
                work_dir=workdir,
            )
            lnk.save()
            print(f"  ✓ {lnk_path}")
            made += 1
        except Exception as exc:  # noqa: BLE001
            print(f"  ✗ {name}: {exc}")
    return made


def remove(target_dir: Path) -> int:
    removed = 0
    for name, *_ in SHORTCUTS:
        lnk_path = target_dir / f"{name}.lnk"
        if lnk_path.exists():
            lnk_path.unlink()
            print(f"  - 已删除 {lnk_path}")
            removed += 1
    return removed


def main() -> int:
    ap = argparse.ArgumentParser(description="为 CPing 创建桌面快捷方式")
    ap.add_argument("--dry-run", action="store_true", help="只显示将创建的内容")
    ap.add_argument("--remove", action="store_true", help="删除已创建的快捷方式")
    ap.add_argument(
        "--include-missing",
        action="store_true",
        help="即使目标程序尚未构建也创建快捷方式",
    )
    args = ap.parse_args()

    desk = desktop_dir()
    print(f"桌面目录：{desk}")

    if args.remove:
        n = remove(desk)
        print(f"共删除 {n} 个")
        return 0

    n = create(desk, args.dry_run, only_existing=not args.include_missing)
    print(f"共处理 {n} 个")
    if n == 0 and not args.dry_run:
        print(
            "\n提示：目标程序还未构建时会被跳过。\n"
            "先构建：\n"
            "  cargo build --release --workspace                 # TUI\n"
            "  cd apps/cping-gui/src-tauri && cargo build --release   # 桌面 GUI\n"
            "或加 --include-missing 强制创建。"
        )
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
