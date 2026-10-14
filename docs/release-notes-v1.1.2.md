# CPing v1.1.2 —— 修复「表单显示」滚不动的问题

补丁版本，只改一处 CSS，不改任何探测/导出/快照逻辑。

## 问题

「表单显示」页签里，255 行结果**只能看到前 22 行左右**：既没有滚动条，
滚轮也滚不动，后面的主机永远看不到。这是 v1.1.0 引入 M7 表格列时就存在的问题，
v1.1.1 的布局调整让它更容易被注意到。

## 根因

naive-ui 的 `NTabs` 高度链有两环是断的：

```text
.n-tabs                flex 列容器（height: 100%，正常）
 └ .n-tabs-pane-wrapper  overflow: hidden 的可收缩 flex 项
                          → 内容被静默裁掉，但自身不提供任何滚动能力
     └ .n-tab-pane         高度是 auto
         └ <div height:100%>  → 退化成 auto
             └ .result-table__scroll  → 撑到 255 行全高
```

内层滚动容器的 `max-height: 100%` 因此失去参照物，表格撑到内容全高后被
wrapper 裁掉——看起来像「显示了一部分」，实际是「其余部分永远不可达」。
「图形显示」视图只是恰好放得下 16×16 矩阵，才没有暴露同一问题。

## 修复

在桌面布局里把断掉的两环补上（`flex: 1 1 auto; min-height: 0` 与
`height: 100%`），滚动容器恢复有界，滚动条与滚轮随之正常。

## 验证

- 预置一份 255 行的快照自动恢复，向 WebView2 输入窗口
  （`Chrome_RenderWidgetHostHWND`）注入滚轮消息：表格正常滚动
  （10 格从 192.168.0.1 滚到 .33）、滚动条滑块同步移动、表头保持吸顶
- 修复前同一注入方式下表格区域 **0 像素变化**（先复现，再修复，再验证）
- `cargo fmt --check` / `cargo clippy -D warnings`（两套）/
  `vue-tsc` 干净；workspace 152 个测试全绿

## 产物

| 文件 | 说明 |
| --- | --- |
| `cping-tui-v1.1.2-windows-x64.exe` | 终端版（未改动，随版本一起出） |
| `cping-gui-v1.1.2-windows-x64.exe` | 桌面版（本版修复对象） |
| `SHA256SUMS.txt` | 上述文件的 SHA-256 |

Android APK 未随本版重新构建。

## 已知限制

- Linux / Android 的真实 ICMP 未在真机/真系统实测；拿不到权限时按「权限不足」如实上报
- Android 真机验证未做（无设备）；release APK 未签名
- 本机地址 / MAC 枚举未接入，界面显示「未知」，不伪造
