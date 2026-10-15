/**
 * 中文词条（基准语言）。
 *
 * 设计取舍说明：
 * - 不引入 vue-i18n 等依赖：本项目只需要「查表 + 顺序占位符」两件事，
 *   一个 `Record<string,string>` 加一个 replace 就够了，省掉一整个运行时。
 * - 词条值统一用单引号包裹、每条一行、值内不出现单引号（英文同理用 "do not"
 *   之类的写法避开撇号）。这样 `scripts/check-i18n.mjs` 能用简单正则稳定解析，
 *   不需要引入 TypeScript 编译器或任何包。
 * - 占位符与 Rust 核心层保持一致：`{}` 按出现顺序替换；参数不足时原样保留
 *   `{}`，参数多余则忽略（见 `index.ts` 的 `t()`）。
 * - 值里出现的 `<b>` / `<code>` 是我们自己的静态文案（不含任何用户输入），
 *   用 v-html 渲染以保留原有的强调样式，因此这里带上标记。
 */
export const zh = {
  // ---- 应用名与通用 ----
  'app.title': 'C网群PING工具',
  'common.unknown': '未知',
  'common.available': '可用',
  'common.unavailable': '不可用',
  'common.onlineOnly': '仅显示在线',
  'common.elapsed': '用时 {}',
  'common.localIp': '本机 {}',
  'unit.seconds': '{} 秒',
  'round.n': '第 {} 轮',
  'lang.title': '界面语言',

  // ---- 运行状态（StatusBar） ----
  'state.running': '探测中',
  'state.cancelled': '已取消',
  'state.done': '已完成',
  'state.idle': '空闲',
  'state.monitoring': '监控中 · 每 {}s',
  'state.monitoringRescan': '监控中 · {}s 后重扫',
  'status.label': '状态：',
  'status.mode': '方式：',
  'status.online': '在线主机数：',
  'status.changes': '变化：',
  'status.changeTotal': '/ 累计 {}',
  'status.mac': '已识别 MAC：',
  'status.vendor': '厂商：',
  'status.hostname': '主机名：',
  'status.elapsed': '用时：',
  'status.backend': '后端：',
  'status.notStarted': '待启动',
  'status.localIp': '本机 IP：',
  'status.ready': '就绪',

  // ---- 状态栏 / 提示条消息（stores/ping.ts） ----
  'status.noResults': '还没有已完成的结果',
  'status.starting': '正在启动…',
  'status.scanStarted': '开始探测 {}（{}）',
  'status.startFailed': '无法启动：{}',
  'status.cancelRequested': '已请求取消',
  'status.cancelFailed': '取消失败：{}',
  'status.modeChanged': '探测方式：{}（下次探测生效）',
  'status.scanDone': '扫描完成：{} 个目标，{} 在线',
  'status.cancelledDone': '已取消：完成 {}/{}，已完成结果保留',
  'status.scanDoneInfo': '{}；识别 MAC {}、厂商 {}{}',
  'status.diffSuffix': '；{}',
  'status.resyncFailed': '结果校准失败：{}',
  'status.fpBusy': '已有一次指纹探测在进行中',
  'status.fpDone': '端口指纹：{} 台主机 · {} 个开放端口（{} 台有开放端口）',
  'status.fpFailed': '端口指纹失败：{}',
  'status.fpCleared': '已清空端口指纹',
  'status.trendFailed': '趋势读取失败：{}',
  'status.trendCleared': '已清空趋势历史（{} 个时间点）',
  'status.trendClearFailed': '清空趋势失败：{}',
  'status.trendExportFailed': '导出可用率报表失败：{}',
  'status.alertsFailed': '告警状态读取失败：{}',
  'status.alertsOn': '告警已启用（默认规则：新增主机 / 离线 / 延迟>200ms / 可用率<90%）',
  'status.alertsOff': '告警已关闭（规则不再求值；已积累的冷却记录与基线保留）',
  'status.mutedOn': '告警已静音：规则照常求值，只是不通知',
  'status.mutedOff': '告警已解除静音',
  'status.alertRulesUpdated': '告警规则已更新',
  'status.alertsCleared': '已清空告警记录（基线与冷却记录保留）',
  'status.alertsClearFailed': '清空告警失败：{}',
  'status.alertToggleFailed': '切换告警失败：{}',
  'status.muteToggleFailed': '切换静音失败：{}',
  'status.monitorOn': '已开启持续监控，每 {} 秒重扫一次',
  'status.monitorOff': '已关闭持续监控',
  'status.monitorIntervalChanged': '持续监控间隔已改为 {} 秒',
  'status.configSaved': '设置已保存',
  'status.configSaveFailed': '保存设置失败：{}',
  'status.exportFailed': '导出失败：{}',
  'status.exportDiffFailed': '导出差异失败：{}',
  'status.savedTo': '已保存：{}',
  'status.enrichFailed': '补全失败：{}',
  'status.noReverseDns': '{} 没有反向解析记录',
  'status.hostnameLookupFailed': '主机名反查失败：{}',
  'status.checkPendingFailed': '检查历史结果失败：{}',
  'status.restored': '已恢复 {} 条结果（{}，共 {} 个目标）',
  'status.restoreFailed': '恢复失败：{}',
  'status.dismissFailed': '忽略历史结果失败：{}',
  'status.snapshotSaved': '已保存快照：{} 条结果（共 {} 个目标）→ {}',
  'status.snapshotSavedApp': '已保存快照：{} 条结果 → {}',
  'status.snapshotSaveFailed': '保存快照失败：{}',
  'status.capDetail': '{}（{}）：{} · {}',
  'status.specUnparsable': '目标无法解析',

  // ---- 按钮 ----
  'btn.start': '开始探测',
  'btn.stop': '停止',
  'btn.export': '导出',
  'btn.settings': '设置',
  'btn.refresh': '刷新',
  'btn.clear': '清空',
  'btn.clearRecords': '清空记录',
  'btn.apply': '应用',
  'btn.saveResults': '保存结果（CSV / TXT / JSON / HTML）',
  'btn.exportDiff': '导出本轮差异（CSV）',
  'btn.enrich': '补全 MAC / 厂商',
  'btn.saveSnapshot': '保存快照',
  'btn.saveSnapshotShort': '存快照',
  'btn.fingerprint': '端口指纹（不影响在线判定）',
  'btn.trend': '可用率趋势',
  'btn.alertsWithCount': '告警（{} 条）',
  'btn.alertsOff': '告警（默认关闭）',
  'btn.exportCsv': '导出 CSV',
  'btn.exportHtml': '导出 HTML',
  'btn.lookup': '查主机名',
  'btn.lookupBusy': '反查中…',
  'btn.lookupShort': '反查',
  'btn.discard': '丢弃',
  'btn.notNow': '暂不恢复',
  'btn.restore': '恢复结果',
  'btn.saveConfig': '保存设置',
  'btn.findLastResult': '查找上次结果',

  // ---- 表单标签与占位符 ----
  'label.target': '目标（网段 / 单主机 / CIDR，可多个用逗号分隔）',
  'label.targetShort': '目标（网段 / 单主机 / CIDR）',
  'label.specRange': '本次范围：{}',
  'label.segments': '（{} 段）',
  'label.mode': '探测方式',
  'label.timeout': '超时 (ms)',
  'label.concurrency': '并发上限',
  'label.port': '端口',
  'label.exportFormat': '导出格式',
  'placeholder.target': '192.168.1 或 192.168.1.0/24, 10.0.0.1',
  'placeholder.targetShort': '192.168.1 或 192.168.1.10',

  // ---- 边界说明（提示文字） ----
  'hint.tcp': 'TCP 连接成功只说明端口开放，不代表 ICMP 可达；状态列会标成 TCP-OPEN。',
  'hint.icmp': 'ICMP 需要管理员/root 权限；普通用户下会被记成「权限不足」而不是「离线」。',
  'hint.tcpMobile': 'TCP 只反映端口是否开放，状态显示为 TCP-OPEN，不等价于 ICMP 可达。',
  'hint.icmpMobile': 'ICMP 在 Android 上通常被系统拒绝，会记成「权限不足」。',
  'hint.ipv6Icmp': '当前目标含 IPv6 地址，而 ICMPv6 尚未实装；请切到 TCP 方式。',
  'hint.async': '本构建已启用异步调度（async feature）',

  // ---- 进度 ----
  'progress.completed': '已完成 {} / {}',
  'progress.completedShort': '已完成 {}/{}',
  'progress.online': '在线 {}',
  'progress.detail': '超时 {} · 不可达 {} · 错误 {}',

  // ---- 识别统计与最近文件 ----
  'recognized.summary': '已识别 MAC {} · 厂商 {} · 主机名 {}',
  'last.export': '上次导出：{}',
  'last.snapshot': '上次快照：{}',

  // ---- 页签 ----
  'tab.matrix': '图形显示',
  'tab.table': '表单显示',
  'tab.changes': '变化 ({})',
  'tab.assets': '资产 ({})',
  'tab.trend': '趋势 ({} 轮)',
  'tab.alerts': '告警 ({})',
  'tab.list': '列表',
  'tab.matrixShort': '矩阵',
  'tab.trendShort': '趋势 ({})',
  'tab.onlineOnly': '仅在线',

  // ---- 矩阵视图上限 ----
  'matrix.overflow': '当前目标 {} 个，超过矩阵视图上限 256 个（一格对应一个 IP 末段），无法用图形显示完整覆盖。请切到「表单显示」查看全部结果。',
  'matrix.overflowMobile': '目标 {} 个，超过矩阵上限 256 个，请用列表查看。',

  // ---- 持续监控 ----
  'monitor.title': '持续监控',
  'monitor.interval': '间隔',
  'monitor.status': '第 {} 轮 · 变化 {} 条 · 累计 {} 条',
  'monitor.rescanIn': '· {}s 后重扫',
  'monitor.hint': '开启后按间隔自动重扫；首轮只建立基线，不报变化。',
  'monitor.mobileStatus': '第 {} 轮 · 变化 {} 条',

  // ---- 变化面板 ----
  'change.fallback': '变化',
  'change.appeared': '上线',
  'change.disappeared': '离线',
  'change.rttClassChanged': '延迟变化',
  'change.statusChanged': '状态变化',
  'change.head': '累计 {} 条变化 · 已完成 {} 轮',
  'change.empty': '还没有变化记录',
  'change.emptyHint': '首轮扫描只建立基线，不会产生变化记录。开启「持续监控」完成第二轮后，上线 / 离线 / 延迟档位变化 / 状态变化都会记在这里。',

  // ---- 告警面板 ----
  'alert.titleShort': '告警',
  'alert.enabled': '已启用',
  'alert.disabled': '未启用',
  'alert.activeRules': '活跃规则 {}',
  'alert.total': '累计告警 {} 条',
  'alert.suppressedCooldown': '冷却挡下 {} 条',
  'alert.suppressedMute': '静音抑制 {} 条',
  'alert.on': '告警开启',
  'alert.off': '告警关闭',
  'alert.muted': '已静音',
  'alert.unmuted': '声音开启',
  // 状态栏用的「一句话告警摘要」（与面板里的长说明 alert.note 区分开）
  'alert.summary': '告警 {} 条',
  'alert.summaryMuted': '告警 {} 条（已静音）',
  'alert.note': '规则匹配是<b>纯函数</b>，判定只看本轮与上一轮的事实。第一轮只建立基线，<b>不发「新增主机」</b>——否则扫个 /24 会一次刷出 255 条。阈值用<b>严格不等号</b>，恰好等于阈值不告警。<b>静音不改判定</b>：规则照常求值、冷却照常推进，所以解除静音不会补报一堆积压告警。',
  'alert.thresholdTip': '当前阈值（在下面的表单里统一修改）',
  'alert.noThreshold': '无阈值',
  'alert.cooldown': '冷却 {}s',
  'alert.latencyThreshold': '延迟阈值 (ms)',
  'alert.uptimeThreshold': '可用率阈值 (%)',
  'alert.cooldownLabel': '冷却 (秒)',
  'alert.dirty': '有未应用的修改',
  'alert.empty': '还没有告警记录',
  'alert.emptyHint': '打开上面的「告警开启」开关，然后跑一轮扫描。开着「持续监控」连续跑几轮，才能看出延迟是否在变差、某台主机是不是开始掉线。',
  'alert.valueUptime': '{}%（阈值 {}%）',
  'alert.valueLatency': '{} ms（阈值 {} ms）',

  // ---- 资产（端口指纹） ----
  'fp.head': '已探测 {} 台 · 开放端口 {} 个（{} 台有开放端口）',
  'fp.perScan': '· 每次探测 {} 个端口',
  'fp.banner': '抓取服务横幅',
  'fp.scanAll': '探测全部在线主机',
  'fp.note': '端口指纹是**独立于在线判定**的一趟探测：端口全关不会把主机改成离线，端口开放也不会把离线主机变在线。横幅只读对方主动送出的字节，不发送任何协议请求。设备类型是「厂商 + 端口」的<b>推断</b>结论，识别不出就留空。',
  'fp.empty': '还没有端口指纹',
  'fp.emptyHint': '先扫描出在线主机，再点上面的「探测全部在线主机」（或按 F）。端口扫描只在**你已获授权**的网络上做。',
  // 下面三条是「指纹面板纯文本行」版本（stores/ping.ts 的 fingerprintLines 用），
  // 与上面的富文本说明同源但去掉标记，便于纯文本场景复用。
  'fp.notePlain': '端口指纹（附加信息，与在线判定相互独立）',
  'fp.emptyLine': '还没有指纹结果：点「端口指纹」或按 F 开始',
  'fp.noOpenPortsLine': '{}  无开放端口',
  'fp.noOpenPorts': '无开放端口（不影响其在线状态）',
  'fp.inference': '推断',

  // ---- 结果表格 ----
  'table.ip': 'IP 地址',
  'table.status': '状态',
  'table.mac': 'MAC 地址',
  'table.vendor': '厂商',
  'table.hostname': '主机名',
  'table.class': '类别',

  // ---- 时延配色 ----
  'color.title': '时延配色',
  'color.note': '颜色不是唯一标识，每类另有独立符号。',
  'class.low': '低延迟',
  'class.mid': '中延迟',
  'class.high': '高延迟',
  'class.offline': '无回应',
  'class.error': '错误',
  'class.waiting': '等待',

  // ---- 趋势 ----
  'trend.title': '趋势历史',
  'trend.head': '时间跨度 {} 小时 · {} 个时间点 · 累计 {} 轮 · 保留 {} 天',
  'trend.note': '可用率 = 在线轮数 ÷ 观测轮数。<b>从未在线的主机，RTT 显示为「-」而不是 0</b>——0 会被读成「极快」。曲线断开的那些点表示该轮<b>没测到</b>这台主机，不是「离线」。历史由本进程累积；需要跨会话长期保留，请用 <code>cping-cli --history</code>（两端共用同一套时序核心）。',
  'trend.empty': '还没有可用率数据',
  'trend.emptyHint': '先做一次扫描；打开右上角的「持续监控」连续跑几轮，这里就会出现每台主机的可用率、延迟与抖动，以及一条在线率曲线。',
  'stat.onlineSamples': '在线/观测',
  'stat.avgRtt': '平均 RTT (ms)',
  'stat.worstRtt': '最差 RTT (ms)',
  'stat.jitter': '抖动 (ms)',

  // ---- 结果列表（移动端） ----
  'list.empty': '暂无数据（可关闭「仅在线」过滤后查看）',

  // ---- 矩阵单元格 ----
  'grid.untested': '{}：未探测',
  'grid.tested': '{}：{}',
  'grid.changedNote': ' · 本轮{}',
  'aria.matrix': '探测结果矩阵',

  // ---- 恢复快照对话框 ----
  'restore.title': '发现上次未处理完的扫描结果',
  'restore.scanTime': '扫描时间：{}',
  'restore.prefix': '网段：{}',
  'restore.rows': '结果：{} 条 / 共 {} 个目标',
  'restore.elapsed': '用时：{}',
  'restore.unreadable': '这份快照无法读取，不能恢复。',
  'restore.file': '文件：{}',

  // ---- 文件对话框过滤器 ----
  'filter.csv': 'CSV 文件',
  'filter.txt': '文本文件',
  'filter.json': 'JSON 文件',
  'filter.html': 'HTML 报告',
  'filter.snapshot': 'CPing 快照',

  // ---- 各类提示消息 ----
  'msg.exportExt': '请用 .csv / .txt / .json / .html 扩展名',
  'msg.noDiffBaseline': '还没有可对比的上一轮结果（至少完成两轮扫描）',
  'msg.noOnlineForFp': '没有在线主机可做端口指纹',
  'msg.noScanResults': '还没有扫描结果',
  'msg.taskRunning': '已有任务在运行，请先停止',
  'msg.noRestore': '没有可恢复的历史结果',
  'msg.discarded': '已丢弃上次的结果',
  'msg.ignored': '已忽略，下次启动不再提示',
} as const

/**
 * 类别 / 变化种类的「标签 → 字典键」映射放在 `shared/types.ts`，
 * 那里是 DTO 定义所在地；这里只存文案本身（`class.*`）。
 */
