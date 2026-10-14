# Android 取证对照工具（`forensic_probe`）

**用途**：判定「Android 上出现 `F libc: FORTIFY: pthread_mutex_lock called on a
destroyed mutex` 时，进程到底是不是被 abort 终结的」。

**它不属于 cargo 工作空间**，不参与任何应用构建，只在做平台行为取证时手工编译运行。

## 背景

Android 版 GUI 退后台（Activity 被销毁）时会打印：

```
F libc: FORTIFY: pthread_mutex_lock called on a destroyed mutex
ActivityManager: Process com.cping.gui (pid xxx) has died: cch CRE
```

只看这两行很容易误判为「native 崩溃」。本工具的作用是**正触发**同样的 bionic 校验，
从而得到对照组：真正的「锁已销毁互斥量」在 bionic 上会怎样结束。

结论（详见 `docs/platform-compatibility.md` §3.5）：真触发时**必然 abort**——
紧跟 `Fatal signal 6 (SIGABRT)`、`DEBUG: Abort message`、`__fortify_fatal` 回溯，
生成 `/data/tombstones/tombstone_XX`，退出码 **134**。而 cping 那次这些痕迹一个都没有，
退出原因是 `EXIT_SELF` / `status=0`（即 `tao` 主动 `process::exit(0)`），
所以那是**退出路径与 WebView 拆卸并发的日志**，不是崩溃。

## 编译

需要 NDK（本项目实测 26.1.10909125）。目标 ABI 按被测设备选，模拟器 x86_64 用：

```bash
NDK="$ANDROID_HOME/ndk/26.1.10909125/toolchains/llvm/prebuilt/windows-x86_64"
"$NDK/bin/clang.exe" \
  --target=x86_64-linux-android34 \
  --sysroot="$(cygpath -m "$NDK/sysroot")" \
  -O1 -o forensic_probe forensic_probe.c

adb push forensic_probe /data/local/tmp/ && adb shell chmod 755 /data/local/tmp/forensic_probe
```

## 运行

```bash
# 1) 默认互斥量：init → destroy → lock（预期 abort，退出码 134）
adb logcat -b all -c
adb shell "/data/local/tmp/forensic_probe mutex; echo EXIT_CODE=\$?"
adb logcat -b all -d | grep -E "FORTIFY|Fatal signal|Abort message"

# 2) ERRORCHECK 互斥量：同上，行为一致
adb shell "/data/local/tmp/forensic_probe mutex-ec; echo EXIT_CODE=\$?"

# 3) 真 abort()：确认本机 tombstone 机制有效
adb shell "/data/local/tmp/forensic_probe abort; echo EXIT_CODE=\$?"
adb shell ls -la /data/tombstones     # 应出现 tombstone_XX
```

## 实测对照结果（API 35 模拟器，x86_64）

| 对照组（真触发） | cping 退后台那次 |
| --- | --- |
| `F libc: FORTIFY: ... destroyed mutex` | 同 |
| **紧跟** `F libc: Fatal signal 6 (SIGABRT)` | 没有这一行 |
| `F DEBUG: Abort message: 'FORTIFY: ...'` + `__fortify_fatal` 回溯 | 没有 |
| 生成 `/data/tombstones/tombstone_XX` | tombstone 目录为空（mtime 未变） |
| 退出码 `134`（128+SIGABRT） | `dumpsys activity exit-info` → `EXIT_SELF` / `status=0` |

> 只按 HOME（`onPause`/`onStop`，不 `onDestroy`）**不会**触发 cping 那条日志；
> 用 `adb shell input keyevent KEYCODE_BACK` 让 Activity 真正销毁才能 1 次命中。
