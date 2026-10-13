@echo off
REM ===========================================================================
REM CPing 一键构建脚本（Windows）
REM
REM 依赖工具链（全部装在 D:\Program Files\Android）：
REM   JDK 17 / cmdline-tools / platform-tools / platforms(36) / build-tools(36) / NDK 26.1
REM
REM 用法：
REM   scripts\build-all.cmd            构建全部可构建目标
REM   scripts\build-all.cmd tui        只构建 TUI
REM   scripts\build-all.cmd gui        只构建桌面 GUI
REM   scripts\build-all.cmd android    只构建 Android APK
REM   scripts\build-all.cmd shortcuts  只创建桌面快捷方式
REM ===========================================================================
setlocal

set "REPO=%~dp0.."
pushd "%REPO%"

set "ANDROID_HOME=D:\Program Files\Android"
set "NDK_HOME=D:\Program Files\Android\ndk\26.1.10909125"
set "JAVA_HOME=D:\Program Files\Android\jdk-17"

if not exist "%JAVA_HOME%\bin\java.exe" (
  echo [警告] 未找到 Android 工具链的 JDK17: %JAVA_HOME%
  echo        桌面与 TUI 仍可构建；Android 目标会失败。
)

set "WHAT=%~1"
if "%WHAT%"=="" set "WHAT=all"

if "%WHAT%"=="all"      goto :all
if "%WHAT%"=="tui"      goto :tui
if "%WHAT%"=="gui"      goto :gui
if "%WHAT%"=="android"  goto :android
if "%WHAT%"=="shortcuts" goto :shortcuts
echo 未知目标: %WHAT%
goto :end

:all
call :do_tui
call :do_gui
call :do_android
call :do_shortcuts
goto :end

:tui
call :do_tui
goto :end

:gui
call :do_gui
goto :end

:android
call :do_android
goto :end

:shortcuts
call :do_shortcuts
goto :end

REM ---------------------------------------------------------------------------
:do_tui
echo.
echo ==== [1/4] 构建 TUI (Windows) ====
cargo build --release -p cping-tui || echo [失败] TUI
exit /b 0

:do_gui
echo.
echo ==== [2/4] 构建桌面 GUI (Windows) ====
pushd apps\cping-gui
call npm run build || echo [失败] 前端构建
pushd src-tauri
cargo build --release || echo [失败] GUI
popd
popd
exit /b 0

:do_android
echo.
echo ==== [3/4] 构建 Android APK ====
pushd apps\cping-gui
if not exist node_modules (
  echo 首次需要安装前端依赖...
  call npm install --ignore-scripts --registry=https://registry.npmmirror.com --no-audit --no-fund
)
call npx tauri android build --apk --target aarch64 || echo [失败] Android
popd
exit /b 0

:do_shortcuts
echo.
echo ==== [4/4] 创建桌面快捷方式 ====
python scripts\create-desktop-shortcuts.py || echo [失败] 快捷方式（可 pip install pylnk3）
exit /b 0

:end
echo.
echo ==== 产物位置 ====
echo   TUI        : %REPO%\target\release\cping-tui.exe
echo   桌面 GUI   : %REPO%\apps\cping-gui\src-tauri\target\release\cping-gui.exe
echo   Android APK: %REPO%\apps\cping-gui\src-tauri\gen\android\app\build\outputs\apk\
echo.
echo 注意：Linux 目标需要 Linux 环境或 CI，本机未包含。
popd
endlocal
