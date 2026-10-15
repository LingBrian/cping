//! M16：配置文件与命名扫描模板。
//!
//! 与 [`crate::config`] 的分工：
//! - `config.rs` 管**应用配置**（探测参数、配色、界面偏好、监控）的受限子集 TOML 读写；
//! - 本模块在它之上加一层「配置文件」：**schema 版本号 + 命名模板 + 未知键保留 + 迁移**。
//!
//! 三条设计约束，都是踩过的坑的形状：
//!
//! 1. **旧版本必须能读**：读取时按 `schema_version` 决定是否迁移，**迁移结果要回报**
//!    （返回一串人话说明），绝不静默改用户的文件。
//!    但**新版本要明确拒绝**——读到 `schema_version = 5` 而你只会 2，猜着读等于把用户
//!    不认识的配置项吃掉。这和时序文件、告警状态文件是同一条纪律。
//! 2. **未知键必须原样保留**：老程序读到新配置，如果把不认识的键丢掉再写回，
//!    就等于把用户的配置吃了。所以解析时不认识的键会被记下来（连所在小节一起），
//!    写回时原样输出。
//! 3. **注释不保留**。这个子集解析器不建 AST，保留注释和格式需要完整 TOML 语法树，
//!    代价远大于收益。这一点写在生成的文件头里，而不是让用户自己发现。
//!
//! 关于「为什么只解析一次应用配置」：`[probe]` / `[colors]` / `[ui]` / `[monitor]`
//! 的**写入**交给 [`AppConfig::to_toml`]，**读取**交给 [`AppConfig::from_toml`]，
//! 本模块只负责把它们从整份文件里挑出来、再把不认识的键摘走。这样应用配置的
//! 读写实现仍然只有一处，不会两边漂移。`app_keys_cover_everything_app_config_writes`
//! 这条测试就是钉住这一点的。
//!
//! 已知的取舍：本节覆盖的是 TOML 的一个**受限子集**——顶层键值、`[section]`、
//! `[[array-of-tables]]`、`#` 注释、字符串/整数/布尔。本项目零第三方运行时依赖，
//! 手写这个子集比拉一个 TOML 库更符合约束；将来若引入 serde，接口可保持不变。

use crate::config::AppConfig;
use crate::error::{CoreError, CoreResult};
use crate::spec::{parse_spec, ProbeMode};
use crate::target::TargetRange;

/// 当前程序能读写的配置 schema 版本。
pub const CONFIG_SCHEMA_VERSION: u32 = 2;
/// schema 版本号所在的顶层键名。
pub const SCHEMA_VERSION_KEY: &str = "schema_version";
/// 默认模板的引用键。
pub const DEFAULT_PROFILE_KEY: &str = "default_profile";
/// 模板数组表的段名。
pub const PROFILES_SECTION: &str = "profiles";

/// 属于 [`AppConfig`] 的已知小节（顺序即写出顺序）。
const APP_SECTIONS: [&str; 4] = ["probe", "colors", "ui", "monitor"];

/// **`AppConfig` 认得的全部键**。
///
/// 这份清单只有一个用途：把「本程序不认识的键」摘出来原样保留。
/// 它必须与 `AppConfig::to_toml()` 写出的键完全一致，否则会出现
/// 「自己写的配置自己不认识」这种荒唐情况——所以有专门的漂移测试钉着。
const APP_KEYS: &[(&str, &str)] = &[
    ("probe", "prefix"),
    ("probe", "range"),
    ("probe", "timeout_ms"),
    ("probe", "concurrency"),
    ("probe", "mode"),
    ("probe", "tcp_port"),
    ("probe", "low_max_ms"),
    ("probe", "mid_max_ms"),
    ("colors", "low"),
    ("colors", "mid"),
    ("colors", "high"),
    ("colors", "offline"),
    ("colors", "error"),
    ("ui", "hide_offline"),
    ("ui", "view"),
    ("monitor", "enabled"),
    ("monitor", "interval_ms"),
    ("monitor", "notify_appeared"),
    ("monitor", "notify_disappeared"),
    ("monitor", "capacity"),
];

/// 模板对「探测参数」的影响，供三端按同一份规则合并。
///
/// 刻意只包含**探测参数**：模板负责「扫什么、怎么扫」，
/// 导出路径、通知通道这类「这次跑要干什么」的参数不该藏在模板里，
/// 否则 CI 里换个模板会连通知一起换掉，排查起来是场噩梦。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProbeOverrides {
    pub range: TargetRange,
    pub timeout_ms: u64,
    pub concurrency: usize,
    pub mode: ProbeMode,
}

/// 一份命名扫描模板。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanProfile {
    /// 模板名。非空、不区分大小写唯一。
    pub name: String,
    /// 目标规格原文：网段 / 单主机 / CIDR / 多网段，与 `--target` 同一套语法。
    pub spec: String,
    pub range: TargetRange,
    pub timeout_ms: u64,
    pub concurrency: usize,
    pub mode: ProbeMode,
    /// 给人看的一句话说明（可选）。
    pub note: String,
    /// 本模板里**不认识**的键，原样保留以便写回。
    pub extra: Vec<(String, String)>,
}

impl ScanProfile {
    /// 造一份模板（其余字段走默认值）。
    pub fn new(name: impl Into<String>, spec: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            spec: spec.into(),
            range: TargetRange::default(),
            timeout_ms: crate::config::DEFAULT_TIMEOUT_MS,
            concurrency: crate::config::DEFAULT_CONCURRENCY,
            mode: crate::platform_default_mode(),
            note: String::new(),
            extra: Vec::new(),
        }
    }

    /// 校验。
    ///
    /// 名字与目标规格都**实打实地解析一遍**：模板最大的价值就是「一键调用」，
    /// 存进去一个写错的规格，等于把这个错误固化下来、每次调用都踩一次。
    pub fn validate(&self) -> CoreResult<()> {
        if self.name.trim().is_empty() {
            return Err(CoreError::InvalidConfig("模板名不能为空".into()));
        }
        // 这些字符会破坏 TOML 结构：存进去写出来的文件就再也读不回来了
        if self.name.contains(['[', ']', '=']) || self.name.contains('"') {
            return Err(CoreError::InvalidConfig(format!(
                "模板名 {:?} 含非法字符（不允许 [ ] = \"）",
                self.name
            )));
        }
        if self.spec.trim().is_empty() {
            return Err(CoreError::InvalidConfig(format!(
                "模板 {:?} 没有目标规格",
                self.name
            )));
        }
        self.probe().validate()?;
        parse_spec(&self.spec, self.range)?;
        Ok(())
    }

    /// 模板落在探测参数上的那一部分（**不含目标规格**）。
    ///
    /// 目标规格原样留在 `spec` 字段里：`ProbeConfig::prefix` 只能表达单个 C 类
    /// 前三段，装不下 CIDR 与多网段。硬塞进去只会丢信息。
    pub fn probe(&self) -> crate::config::ProbeConfig {
        crate::config::ProbeConfig {
            prefix: None,
            range: self.range,
            timeout_ms: self.timeout_ms,
            concurrency: self.concurrency,
            thresholds: crate::classify::Thresholds::default(),
            mode: self.mode,
        }
    }

    /// 供三端合并用的一份覆盖集合。
    pub fn overrides(&self) -> ProbeOverrides {
        ProbeOverrides {
            range: self.range,
            timeout_ms: self.timeout_ms,
            concurrency: self.concurrency,
            mode: self.mode,
        }
    }

    /// 一行摘要，供 `--list-profiles` 与界面展示。
    pub fn describe(&self) -> String {
        let note = if self.note.trim().is_empty() {
            String::new()
        } else {
            format!("  —— {}", self.note.trim())
        };
        let count = match self.target_count() {
            Some(n) => format!("{n} 个目标"),
            None => "目标规格无法解析".to_string(),
        };
        format!(
            "{:<14} {:<24} {} · {} · {} ms · 并发 {} · {}{}",
            self.name,
            self.spec,
            count,
            self.range.key(),
            self.timeout_ms,
            self.concurrency,
            self.mode.describe(),
            note
        )
    }

    /// 目标数量（解析失败返回 `None`，不猜）。
    pub fn target_count(&self) -> Option<usize> {
        parse_spec(&self.spec, self.range).ok().map(|s| s.len())
    }
}

/// 一个「本程序不认识」的键。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UnknownEntry {
    /// 所在小节；空串表示顶层。
    pub section: String,
    pub key: String,
    /// 原始值文本（去掉行内注释后的原样），写回时原封不动。
    pub raw: String,
}

impl UnknownEntry {
    /// 供界面展示的 `小节.键` 形式。
    pub fn path(&self) -> String {
        if self.section.is_empty() {
            self.key.clone()
        } else {
            format!("{}.{}", self.section, self.key)
        }
    }
}

/// 一份配置文件。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfigDocument {
    pub schema_version: u32,
    /// `default_profile = "name"`；`None` 表示没写。
    pub default_profile: Option<String>,
    pub profiles: Vec<ScanProfile>,
    /// 应用配置（`[probe]` / `[colors]` / `[ui]` / `[monitor]`）。
    pub app: AppConfig,
    /// 不认识的键（含完全陌生的小节）。
    unknown: Vec<UnknownEntry>,
}

impl Default for ConfigDocument {
    fn default() -> Self {
        Self::new()
    }
}

impl ConfigDocument {
    pub fn new() -> Self {
        Self {
            schema_version: CONFIG_SCHEMA_VERSION,
            default_profile: None,
            profiles: Vec::new(),
            app: AppConfig::default(),
            unknown: Vec::new(),
        }
    }

    /// 以应用配置为底、不带模板的文档（界面「另存为配置」用）。
    pub fn from_app(app: AppConfig) -> Self {
        Self { app, ..Self::new() }
    }

    pub fn unknown_entries(&self) -> &[UnknownEntry] {
        &self.unknown
    }

    /// 按名取模板（名字比较**不区分大小写**：命令行上没人愿意纠结大小写）。
    pub fn profile(&self, name: &str) -> Option<&ScanProfile> {
        let want = name.trim();
        self.profiles
            .iter()
            .find(|p| p.name.eq_ignore_ascii_case(want))
    }

    /// 新增或替换同名模板（同名替换，不会留下两个）。
    pub fn upsert_profile(&mut self, profile: ScanProfile) {
        match self
            .profiles
            .iter_mut()
            .find(|p| p.name.eq_ignore_ascii_case(&profile.name))
        {
            Some(slot) => *slot = profile,
            None => self.profiles.push(profile),
        }
    }

    pub fn remove_profile(&mut self, name: &str) -> bool {
        let before = self.profiles.len();
        self.profiles
            .retain(|p| !p.name.eq_ignore_ascii_case(name.trim()));
        self.profiles.len() != before
    }

    /// 模板名列表（按文件中出现顺序）。
    pub fn profile_names(&self) -> Vec<&str> {
        self.profiles.iter().map(|p| p.name.as_str()).collect()
    }

    /// 选出这次要用的模板。
    ///
    /// `explicit` 来自 `--profile`；没给就用 `default_profile`。
    /// **点名的模板不存在时报错**，不悄悄退回默认——「我明明选了 prod 怎么扫的是
    /// 开发网段」比直接报错危险得多。
    pub fn select_profile(&self, explicit: Option<&str>) -> CoreResult<Option<&ScanProfile>> {
        fn missing(what: &str, doc: &ConfigDocument) -> CoreError {
            CoreError::InvalidConfig(format!(
                "{what}（现有模板：{}）",
                if doc.profiles.is_empty() {
                    "无".to_string()
                } else {
                    doc.profile_names().join(", ")
                }
            ))
        }
        if let Some(name) = explicit {
            return self
                .profile(name)
                .map(Some)
                .ok_or_else(|| missing(&format!("配置里没有名为 {name:?} 的模板"), self));
        }
        match &self.default_profile {
            Some(name) => self
                .profile(name)
                .map(Some)
                .ok_or_else(|| missing(&format!("default_profile 指向的 {name:?} 不存在"), self)),
            None => Ok(None),
        }
    }

    /// 整体校验。
    pub fn validate(&self) -> CoreResult<()> {
        self.app.validate()?;
        let mut seen: Vec<String> = Vec::new();
        for p in &self.profiles {
            p.validate()?;
            let lower = p.name.to_ascii_lowercase();
            if seen.contains(&lower) {
                return Err(CoreError::InvalidConfig(format!(
                    "模板名 {:?} 重复（名字不区分大小写）",
                    p.name
                )));
            }
            seen.push(lower);
        }
        if let Some(name) = &self.default_profile {
            if self.profile(name).is_none() {
                return Err(CoreError::InvalidConfig(format!(
                    "default_profile = {name:?}，但配置里没有这个模板"
                )));
            }
        }
        Ok(())
    }

    /// 序列化为 TOML 文本。
    ///
    /// 已知键在前，未知键原样跟在各自小节末尾；完全陌生的小节整体排在最后。
    pub fn to_toml(&self) -> String {
        let mut out = String::new();
        out.push_str("# CPing 配置文件\n");
        out.push_str(&format!(
            "# {SCHEMA_VERSION_KEY} 决定读取时的迁移路径；本程序当前支持 v{CONFIG_SCHEMA_VERSION}。\n"
        ));
        out.push_str("# 注意：**注释不会被保留**（子集解析器不建 AST）；不认识的键会原样保留。\n");
        out.push_str(&format!("{SCHEMA_VERSION_KEY} = {}\n", self.schema_version));
        match &self.default_profile {
            Some(name) => out.push_str(&format!("{DEFAULT_PROFILE_KEY} = \"{}\"\n", escape(name))),
            None => out.push_str(&format!("# {DEFAULT_PROFILE_KEY} = \"\"\n")),
        }
        for e in self.unknown.iter().filter(|e| e.section.is_empty()) {
            out.push_str(&format!("{} = {}\n", e.key, e.raw));
        }

        // 应用配置借 `AppConfig::to_toml` 输出，再把该小节的未知键补回去。
        for section in APP_SECTIONS {
            out.push('\n');
            out.push_str(&render_app_section(&self.app, section));
            for e in self
                .unknown
                .iter()
                .filter(|e| e.section.eq_ignore_ascii_case(section))
            {
                out.push_str(&format!("{} = {}\n", e.key, e.raw));
            }
        }

        for p in &self.profiles {
            out.push('\n');
            out.push_str(&format!(
                "[[{PROFILES_SECTION}]]\nname = \"{}\"\nspec = \"{}\"\nrange = \"{}\"\n\
                 timeout_ms = {}\nconcurrency = {}\nmode = \"{}\"\ntcp_port = {}\nnote = \"{}\"\n",
                escape(&p.name),
                escape(&p.spec),
                p.range.key(),
                p.timeout_ms,
                p.concurrency,
                p.mode.key(),
                p.mode.tcp_port().unwrap_or(80),
                escape(&p.note)
            ));
            for (k, v) in &p.extra {
                out.push_str(&format!("{k} = {v}\n"));
            }
        }

        // 完全陌生的小节：每节只在第一次遇到时输出一次
        let mut emitted: Vec<String> = Vec::new();
        for e in self.unknown.iter().filter(|e| !e.section.is_empty()) {
            if is_known_section(&e.section)
                || emitted.iter().any(|s| s.eq_ignore_ascii_case(&e.section))
            {
                continue;
            }
            emitted.push(e.section.clone());
            out.push('\n');
            out.push_str(&format!("[{}]\n", e.section));
            for e2 in self
                .unknown
                .iter()
                .filter(|x| x.section.eq_ignore_ascii_case(&e.section))
            {
                out.push_str(&format!("{} = {}\n", e2.key, e2.raw));
            }
        }
        out
    }

    /// 从 TOML 文本解析（**含迁移**）。返回文档与一串迁移说明。
    pub fn from_toml(text: &str) -> CoreResult<(Self, Vec<String>)> {
        let mut doc = Self::new();
        // 没写 `schema_version` 的老文件按 v1 处理
        doc.schema_version = 1;

        /// 解析游标。`section` 显式携带当前小节名——比「回头去找」稳得多。
        enum Cursor {
            Top,
            App(String),
            Profile(usize),
            Stranger(String),
        }
        let mut cursor = Cursor::Top;
        // 应用配置的各小节原文，最后一次性交给 `AppConfig::from_toml`
        let mut app_text = String::new();

        for (lineno, raw) in text.lines().enumerate() {
            let at = lineno + 1;
            let line = strip_comment(raw).trim().to_string();
            if line.is_empty() {
                continue;
            }
            if line.starts_with('[') {
                // 先认数组表 `[[name]]`：`section_header` 只剥一层方括号，
                // 拿它去处理 `[[profiles]]` 会剩下个尾巴 `profiles]`
                if line.starts_with("[[") {
                    let inner = line.strip_suffix("]]").ok_or_else(|| {
                        CoreError::InvalidConfig(format!(
                            "第 {at} 行：数组表标题缺少右双括号：{line:?}"
                        ))
                    })?;
                    let name = inner[2..].trim();
                    if name.contains(['.', '[', ']']) || name.is_empty() {
                        return Err(CoreError::InvalidConfig(format!(
                            "第 {at} 行：不支持嵌套数组表 [[{name}]]"
                        )));
                    }
                    if !name.eq_ignore_ascii_case(PROFILES_SECTION) {
                        return Err(CoreError::InvalidConfig(format!(
                            "第 {at} 行：不认识的数组表 [[{name}]]（只支持 [[{PROFILES_SECTION}]]）"
                        )));
                    }
                    doc.profiles.push(ScanProfile::new("", ""));
                    cursor = Cursor::Profile(doc.profiles.len() - 1);
                    continue;
                }
                let inner = section_header(&line).ok_or_else(|| {
                    CoreError::InvalidConfig(format!("第 {at} 行：小节标题缺少右方括号：{line:?}"))
                })?;
                if inner.eq_ignore_ascii_case(PROFILES_SECTION) {
                    return Err(CoreError::InvalidConfig(format!(
                        "第 {at} 行：模板必须写成数组表 [[{PROFILES_SECTION}]]"
                    )));
                }
                let lower = inner.to_ascii_lowercase();
                cursor = if APP_SECTIONS.contains(&lower.as_str()) {
                    Cursor::App(lower)
                } else {
                    Cursor::Stranger(inner.to_string())
                };
                continue;
            }

            let (key, value) = line.split_once('=').ok_or_else(|| {
                CoreError::InvalidConfig(format!("第 {at} 行不是 key = value：{line:?}"))
            })?;
            let key = key.trim().to_string();
            let value = value.trim().to_string();

            match &cursor {
                Cursor::Top => match key.as_str() {
                    SCHEMA_VERSION_KEY => {
                        let v = parse_u64(&value, at)?;
                        doc.schema_version = u32::try_from(v).map_err(|_| {
                            CoreError::InvalidConfig(format!(
                                "第 {at} 行：{SCHEMA_VERSION_KEY} 超出取值范围内"
                            ))
                        })?;
                    }
                    DEFAULT_PROFILE_KEY => doc.default_profile = Some(parse_string(&value)?),
                    _ => doc.unknown.push(UnknownEntry {
                        section: String::new(),
                        key,
                        raw: value,
                    }),
                },
                Cursor::App(section) => {
                    if is_app_key(section, &key) {
                        app_text.push_str(&format!("[{section}]\n{key} = {value}\n"));
                    } else {
                        doc.unknown.push(UnknownEntry {
                            section: section.clone(),
                            key,
                            raw: value,
                        });
                    }
                }
                Cursor::Profile(idx) => {
                    let idx = *idx;
                    let p = doc.profiles.get_mut(idx).expect("游标必指向已推入的模板");
                    match key.as_str() {
                        "name" => p.name = parse_string(&value)?,
                        "spec" => p.spec = parse_string(&value)?,
                        "range" => p.range = parse_range(&value, at)?,
                        "timeout_ms" => p.timeout_ms = parse_u64(&value, at)?,
                        "concurrency" => p.concurrency = parse_u64(&value, at)? as usize,
                        "mode" => {
                            p.mode = match parse_string(&value)?.as_str() {
                                "tcp" => ProbeMode::Tcp { port: 80 },
                                "icmp" => ProbeMode::Icmp,
                                other => {
                                    return Err(CoreError::InvalidConfig(format!(
                                        "第 {at} 行：模板的 mode 只支持 icmp / tcp，得到 {other:?}"
                                    )))
                                }
                            }
                        }
                        "tcp_port" => {
                            let port = parse_u64(&value, at)?;
                            if port > u16::MAX as u64 {
                                return Err(CoreError::InvalidConfig(format!(
                                    "第 {at} 行：tcp_port {port} 超出 0-65535"
                                )));
                            }
                            // 端口只对 TCP 有意义。ICMP 模板里的 tcp_port **不**静默
                            // 把探测方式改成 TCP——静默改「怎么扫」是这里最坏的行为。
                            if let ProbeMode::Tcp { .. } = p.mode {
                                p.mode = ProbeMode::Tcp { port: port as u16 };
                            }
                        }
                        "note" => p.note = parse_string(&value)?,
                        _ => p.extra.push((key, value)),
                    }
                }
                Cursor::Stranger(section) => doc.unknown.push(UnknownEntry {
                    section: section.clone(),
                    key,
                    raw: value,
                }),
            }
        }

        // 应用配置交给唯一的那一处实现读
        doc.app = AppConfig::from_toml(&app_text)?;

        let notes = migrate(&mut doc)?;
        Ok((doc, notes))
    }

    /// 从文件读取（含迁移 + 校验）。返回文档与一串迁移说明。
    pub fn load_from_path(path: &str) -> CoreResult<(Self, Vec<String>)> {
        let text = std::fs::read_to_string(path)
            .map_err(|e| CoreError::InvalidConfig(format!("读取配置 {path} 失败：{e}")))?;
        let (doc, notes) = Self::from_toml(&text)?;
        doc.validate()
            .map_err(|e| CoreError::InvalidConfig(format!("配置 {path} 无效：{e}")))?;
        Ok((doc, notes))
    }

    /// 写回文件（先校验，避免把写坏的配置落盘）。
    pub fn save_to_path(&self, path: &str) -> CoreResult<()> {
        self.validate()?;
        std::fs::write(path, self.to_toml())
            .map_err(|e| CoreError::InvalidConfig(format!("写入配置 {path} 失败：{e}")))
    }
}

/// 按 `schema_version` 迁移。返回一串人话说明（可能为空）。
///
/// **只做向上迁移，绝不向下**。未来的 v3 落地时在这里追加一段即可，
/// 并且必须保持「v1 文件能一路迁移到 v3」。
fn migrate(doc: &mut ConfigDocument) -> CoreResult<Vec<String>> {
    if doc.schema_version > CONFIG_SCHEMA_VERSION {
        return Err(CoreError::InvalidConfig(format!(
            "配置 schema 版本 {} 高于本程序支持的 v{}：拒绝读取（升级 CPing，或把 {SCHEMA_VERSION_KEY} 改回 {}）。\
             猜着读会把你配置里我不认识的项吃掉",
            doc.schema_version, CONFIG_SCHEMA_VERSION, CONFIG_SCHEMA_VERSION
        )));
    }
    if doc.schema_version == 0 {
        return Err(CoreError::InvalidConfig(format!(
            "{SCHEMA_VERSION_KEY} 不能是 0"
        )));
    }

    let mut notes = Vec::new();

    // ---- v1 → v2 ----
    // v1 的「配置文件」其实就是 AppConfig 那一份：探测参数散在 `[probe]` 里，
    // 没有命名模板的概念。所以迁移的动作是：**把 `[probe]` 里的目标与探测参数
    // 提升为一份名为 `default` 的模板**，让老配置在新版本里也能被 `--profile`
    // 见到。`[probe]` 本身保留不动——老字段仍然生效，迁移不改老语义。
    if doc.schema_version == 1 {
        doc.schema_version = CONFIG_SCHEMA_VERSION;
        notes.push(format!(
            "配置未标 {SCHEMA_VERSION_KEY}（或为 1）：已按 v1 读入并升级到 v{}",
            CONFIG_SCHEMA_VERSION
        ));
        if doc.profiles.is_empty() {
            if let Some(prefix) = &doc.app.probe.prefix {
                let mut p = ScanProfile::new("default", prefix.to_string());
                p.range = doc.app.probe.range;
                p.timeout_ms = doc.app.probe.timeout_ms;
                p.concurrency = doc.app.probe.concurrency;
                p.mode = doc.app.probe.mode;
                p.note = "由 v1 的 [probe] 自动迁移而来".into();
                doc.profiles.push(p);
                notes.push(
                    "已把 [probe] 的目标与探测参数提升为名为 default 的模板（老字段仍照旧生效）"
                        .into(),
                );
                if doc.default_profile.is_none() {
                    doc.default_profile = Some("default".into());
                }
            } else {
                notes.push("v1 配置里没有 [probe].prefix，没有可迁移的目标；未生成任何模板".into());
            }
        }
    }
    Ok(notes)
}

// ---------------------------------------------------------------------------
// 渲染与判定
// ---------------------------------------------------------------------------

/// 把 `AppConfig` 的某一个小节渲染出来（借 `AppConfig::to_toml`，写法只有一处）。
fn render_app_section(app: &AppConfig, section: &str) -> String {
    let mut out = String::new();
    let mut in_block = false;
    for line in app.to_toml().lines() {
        if let Some(inner) = section_header(line.trim()) {
            in_block = inner.eq_ignore_ascii_case(section);
            if in_block {
                out.push_str(&format!("[{inner}]\n"));
            }
            continue;
        }
        if in_block && !line.trim().is_empty() {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn is_known_section(section: &str) -> bool {
    APP_SECTIONS.iter().any(|s| s.eq_ignore_ascii_case(section))
        || section.eq_ignore_ascii_case(PROFILES_SECTION)
}

fn is_app_key(section: &str, key: &str) -> bool {
    APP_KEYS
        .iter()
        .any(|(s, k)| s.eq_ignore_ascii_case(section) && *k == key)
}

fn section_header(line: &str) -> Option<&str> {
    let t = line.trim();
    if t.starts_with('[') && t.ends_with(']') {
        Some(t[1..t.len() - 1].trim())
    } else {
        None
    }
}

// ---------------------------------------------------------------------------
// 受限子集 TOML 的词法小工具
// ---------------------------------------------------------------------------

/// 去掉行内注释（**字符串里的 `#` 不算注释**）。
fn strip_comment(line: &str) -> &str {
    let mut in_string = false;
    let mut escaped = false;
    for (i, ch) in line.char_indices() {
        match ch {
            '\\' if in_string => escaped = !escaped,
            '"' if !escaped => in_string = !in_string,
            '#' if !in_string => return &line[..i],
            _ => escaped = false,
        }
    }
    line
}

fn parse_string(value: &str) -> CoreResult<String> {
    let v = value.trim();
    if v.len() >= 2 && v.starts_with('"') && v.ends_with('"') {
        Ok(v[1..v.len() - 1]
            .replace("\\\"", "\"")
            .replace("\\\\", "\\"))
    } else {
        Err(CoreError::InvalidConfig(format!(
            "期望字符串，得到 {value:?}"
        )))
    }
}

fn parse_u64(value: &str, lineno: usize) -> CoreResult<u64> {
    value.trim().parse::<u64>().map_err(|_| {
        CoreError::InvalidConfig(format!("第 {lineno} 行期望非负整数，得到 {value:?}"))
    })
}

fn parse_range(value: &str, lineno: usize) -> CoreResult<TargetRange> {
    let text = parse_string(value)?;
    TargetRange::parse_key(&text).ok_or_else(|| {
        CoreError::InvalidConfig(format!(
            "第 {lineno} 行：range 只支持 standard / full / hosts_only，得到 {text:?}"
        ))
    })
}

/// 转义成可以放进双引号字符串的样子。
fn escape(text: &str) -> String {
    text.replace('\\', "\\\\").replace('"', "\\\"")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::AppConfig;

    fn doc_with_two_profiles() -> ConfigDocument {
        let mut doc = ConfigDocument::new();
        let mut prod = ScanProfile::new("prod", "10.0.0.0/24");
        prod.range = TargetRange::HostsOnly;
        prod.timeout_ms = 800;
        prod.concurrency = 64;
        prod.mode = ProbeMode::Tcp { port: 443 };
        prod.note = "生产网段".into();
        doc.upsert_profile(prod);
        doc.upsert_profile(ScanProfile::new("lab", "192.168.1"));
        doc.default_profile = Some("prod".into());
        doc
    }

    // ------------------------------------------------------------------
    // 往返与未知键保留
    // ------------------------------------------------------------------

    #[test]
    fn round_trip_is_byte_stable() {
        let doc = doc_with_two_profiles();
        let text = doc.to_toml();
        let (back, notes) = ConfigDocument::from_toml(&text).unwrap();
        assert!(notes.is_empty(), "v2 文件不应产生迁移说明：{notes:?}");
        assert_eq!(back, doc, "读回来的文档必须与写出去的一致");
        assert_eq!(back.to_toml(), text, "写出 → 读回 → 再写出必须逐字节一致");
    }

    #[test]
    fn unknown_keys_are_preserved_verbatim() {
        // 模拟「新版本写的配置被老版本读取再写回」：不认识的键一个都不能丢
        let text = r#"
schema_version = 2
future_top = "keep me"

[probe]
timeout_ms = 1500
future_probe_key = [1, 2, 3]

[unknown_section]
some = "value"
nested = 42

[[profiles]]
name = "p1"
spec = "10.0.0.0/30"
future_profile_key = "keep"
"#;
        let (doc, _) = ConfigDocument::from_toml(text).unwrap();
        assert_eq!(doc.app.probe.timeout_ms, 1500, "已知键照常生效");
        let paths: Vec<String> = doc.unknown_entries().iter().map(|e| e.path()).collect();
        assert!(paths.contains(&"future_top".to_string()), "{paths:?}");
        assert!(
            paths.contains(&"probe.future_probe_key".to_string()),
            "{paths:?}"
        );
        assert!(
            paths.contains(&"unknown_section.some".to_string()),
            "{paths:?}"
        );
        assert!(
            paths.contains(&"unknown_section.nested".to_string()),
            "{paths:?}"
        );
        assert_eq!(
            doc.profiles[0].extra,
            vec![("future_profile_key".to_string(), "\"keep\"".to_string())]
        );

        let out = doc.to_toml();
        assert!(out.contains("future_top = \"keep me\""), "{out}");
        assert!(out.contains("future_probe_key = [1, 2, 3]"), "{out}");
        assert!(out.contains("[unknown_section]"), "{out}");
        assert!(out.contains("nested = 42"), "{out}");
        assert!(out.contains("future_profile_key = \"keep\""), "{out}");

        // 再读一遍仍是同一份文档：未知键不会越写越多
        let (again, _) = ConfigDocument::from_toml(&out).unwrap();
        assert_eq!(again, doc, "二次往返必须稳定：{out}");
    }

    #[test]
    fn app_keys_cover_everything_app_config_writes() {
        // 漂移测试：`AppConfig::to_toml` 写出的每一个键都必须在本模块的已知清单里，
        // 否则会出现「自己写的配置自己不认识」——那条注释就会被当成未知键保留下来。
        let app = AppConfig::default();
        let mut section = String::new();
        for line in app.to_toml().lines() {
            if let Some(inner) = section_header(line.trim()) {
                section = inner.to_ascii_lowercase();
                assert!(
                    APP_SECTIONS.contains(&section.as_str()),
                    "AppConfig 写出了未知小节 {section:?}"
                );
                continue;
            }
            let Some((key, _)) = line.split_once('=') else {
                continue;
            };
            let key = key.trim();
            if key.is_empty() {
                continue;
            }
            assert!(
                is_app_key(&section, key),
                "{section}.{key} 未登记在 APP_KEYS 里——两份实现会漂移"
            );
        }
    }

    #[test]
    fn comments_are_accepted_and_documented_as_not_preserved() {
        let text = r#"
# 这是注释
schema_version = 2   # 行内注释
default_profile = "a#b"   # 字符串里的 # 不是注释

[probe]
timeout_ms = 2000  # 又一个
"#;
        let (doc, _) = ConfigDocument::from_toml(text).unwrap();
        assert_eq!(doc.app.probe.timeout_ms, 2000);
        assert_eq!(doc.default_profile.as_deref(), Some("a#b"));
        // 文件头必须写明这一取舍，而不是让用户自己发现
        assert!(doc.to_toml().contains("注释不会被保留"));
    }

    // ------------------------------------------------------------------
    // 迁移与版本
    // ------------------------------------------------------------------

    #[test]
    fn v1_config_without_schema_version_is_migrated_and_reported() {
        let text = r#"
[probe]
prefix = "192.168.7"
range = "full"
timeout_ms = 700
concurrency = 16
mode = "tcp"
tcp_port = 8080
"#;
        let (doc, notes) = ConfigDocument::from_toml(text).unwrap();
        assert_eq!(doc.schema_version, CONFIG_SCHEMA_VERSION);
        assert!(!notes.is_empty(), "迁移必须回报，不许静默改用户文件");
        assert!(
            notes.iter().any(|n| n.contains("升级")),
            "说明里要写清升到了哪个版本：{notes:?}"
        );
        // 老字段仍然生效
        assert_eq!(doc.app.probe.timeout_ms, 700);
        assert_eq!(doc.app.probe.range, TargetRange::Full);
        // 目标与探测参数被提升成名为 default 的模板
        let p = doc.profile("default").expect("应生成 default 模板");
        assert_eq!(p.spec, "192.168.7");
        assert_eq!(p.range, TargetRange::Full);
        assert_eq!(p.timeout_ms, 700);
        assert_eq!(p.concurrency, 16);
        assert_eq!(p.mode, ProbeMode::Tcp { port: 8080 });
        assert_eq!(doc.default_profile.as_deref(), Some("default"));
        // 迁移后的文档必须可再次读写（写回去的就是 v2 了）
        let (again, notes2) = ConfigDocument::from_toml(&doc.to_toml()).unwrap();
        assert_eq!(again, doc);
        assert!(notes2.is_empty(), "v2 文件不该再触发迁移：{notes2:?}");
    }

    #[test]
    fn v1_config_without_a_target_migrates_without_inventing_one() {
        let text = "[probe]\ntimeout_ms = 500\n";
        let (doc, notes) = ConfigDocument::from_toml(text).unwrap();
        assert!(doc.profiles.is_empty(), "没有目标就不该编一个出来");
        assert!(
            notes.iter().any(|n| n.contains("没有可迁移的目标")),
            "这件事要让用户知道：{notes:?}"
        );
    }

    #[test]
    fn a_newer_schema_version_is_refused_not_guessed() {
        let err = ConfigDocument::from_toml("schema_version = 9\n").unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("9"), "{msg}");
        assert!(msg.contains("拒绝"), "{msg}");
        assert!(ConfigDocument::from_toml("schema_version = 0\n").is_err());
    }

    /// 迁移是幂等的：同一份 v1 文件读两次得到的文档完全一样。
    #[test]
    fn migration_is_deterministic() {
        let text = "[probe]\nprefix = \"10.1.2\"\n";
        let (a, _) = ConfigDocument::from_toml(text).unwrap();
        let (b, _) = ConfigDocument::from_toml(text).unwrap();
        assert_eq!(a, b);
    }

    // ------------------------------------------------------------------
    // 模板校验与选择
    // ------------------------------------------------------------------

    #[test]
    fn profile_validation_rejects_empty_names_and_bad_specs() {
        let mut p = ScanProfile::new("", "10.0.0.0/24");
        assert!(p.validate().is_err(), "空名字必须拒绝");

        p.name = "ok".into();
        p.spec = "10.0.0.0/33".into();
        assert!(p.validate().is_err(), "非法 CIDR 必须拒绝");

        p.spec = "   ".into();
        assert!(p.validate().is_err(), "空规格必须拒绝");

        p.spec = "10.0.0.0/24".into();
        p.name = "has[bracket".into();
        assert!(p.validate().is_err(), "会破坏 TOML 结构的名字必须拒绝");

        p.name = "prod".into();
        p.timeout_ms = 0;
        assert!(p.validate().is_err(), "零超时必须拒绝（配置层的既有约束）");

        p.timeout_ms = 1000;
        p.concurrency = crate::config::MAX_CONCURRENCY + 1;
        assert!(p.validate().is_err(), "并发超上限必须拒绝");

        p.concurrency = 32;
        assert!(p.validate().is_ok(), "改回合法值就该通过：{p:?}");
    }

    #[test]
    fn named_profile_selection_is_strict_about_missing_names() {
        let doc = doc_with_two_profiles();
        assert_eq!(
            doc.select_profile(Some("prod")).unwrap().unwrap().name,
            "prod"
        );
        // 大小写不敏感
        assert_eq!(
            doc.select_profile(Some("PROD")).unwrap().unwrap().name,
            "prod"
        );

        let err = doc.select_profile(Some("nope")).unwrap_err().to_string();
        assert!(err.contains("nope"), "{err}");
        assert!(err.contains("prod"), "报错要把现有模板列出来：{err}");

        // 没给 --profile 时用 default_profile
        assert_eq!(doc.select_profile(None).unwrap().unwrap().name, "prod");
    }

    #[test]
    fn dangling_default_profile_is_rejected() {
        let mut doc = doc_with_two_profiles();
        doc.default_profile = Some("gone".into());
        assert!(doc.validate().is_err());
        let err = doc.select_profile(None).unwrap_err().to_string();
        assert!(err.contains("gone"), "{err}");
    }

    #[test]
    fn upsert_and_remove_replace_by_name_ignoring_case() {
        let mut doc = doc_with_two_profiles();
        let mut updated = ScanProfile::new("PROD", "172.16.0.0/28");
        updated.timeout_ms = 1234;
        doc.upsert_profile(updated);
        assert_eq!(doc.profiles.len(), 2, "同名替换，不该留下两个");
        assert_eq!(doc.profile("prod").unwrap().spec, "172.16.0.0/28");
        assert_eq!(doc.profile("prod").unwrap().timeout_ms, 1234);

        assert!(doc.remove_profile("Lab"));
        assert!(doc.profile("lab").is_none());
        assert!(!doc.remove_profile("lab"), "删不存在的要返回 false");
    }

    #[test]
    fn duplicate_names_in_a_file_are_rejected() {
        let text = r#"
schema_version = 2

[[profiles]]
name = "a"
spec = "10.0.0.0/30"

[[profiles]]
name = "A"
spec = "10.0.0.4/30"
"#;
        let (doc, _) = ConfigDocument::from_toml(text).unwrap();
        let err = doc.validate().unwrap_err().to_string();
        assert!(err.contains("重复"), "{err}");
    }

    #[test]
    fn profiles_must_be_written_as_an_array_of_tables() {
        let err = ConfigDocument::from_toml("[profiles]\nname = \"a\"\n").unwrap_err();
        assert!(err.to_string().contains("[[profiles]]"), "{err}");
        let err = ConfigDocument::from_toml("[[nested.profiles]]\n").unwrap_err();
        assert!(err.to_string().contains("嵌套"), "{err}");
    }

    #[test]
    fn tcp_port_never_silently_switches_icmp_to_tcp() {
        let text = r#"
schema_version = 2

[[profiles]]
name = "p"
spec = "10.0.0.0/30"
mode = "icmp"
tcp_port = 443
"#;
        let (doc, _) = ConfigDocument::from_toml(text).unwrap();
        assert_eq!(
            doc.profiles[0].mode,
            ProbeMode::Icmp,
            "端口不能把探测方式偷偷改掉——「怎么扫」被静默换掉是最坏的一类行为"
        );
    }

    #[test]
    fn invalid_enum_values_are_reported_with_line_numbers() {
        let err = ConfigDocument::from_toml(
            "schema_version = 2\n\n[[profiles]]\nname = \"p\"\nspec = \"10.0.0.0/30\"\nrange = \"wild\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("第 6 行"), "要指出是哪一行：{err}");
        assert!(err.contains("wild"), "{err}");

        let err = ConfigDocument::from_toml(
            "schema_version = 2\n\n[[profiles]]\nname = \"p\"\nspec = \"10.0.0.0/30\"\nmode = \"udp\"\n",
        )
        .unwrap_err()
        .to_string();
        assert!(err.contains("udp"), "{err}");
    }

    #[test]
    fn describe_is_informative_enough_for_a_ci_log() {
        let doc = doc_with_two_profiles();
        let line = doc.profile("prod").unwrap().describe();
        for needle in [
            "prod",
            "10.0.0.0/24",
            "hosts_only",
            "800",
            "64",
            "TCP:443",
            "生产网段",
        ] {
            assert!(line.contains(needle), "摘要缺少 {needle}：{line}");
        }
        assert!(
            line.contains("254 个目标"),
            "hosts_only 的 CIDR 应算出 254 个目标：{line}"
        );
    }

    #[test]
    fn target_count_returns_none_instead_of_guessing() {
        let mut p = ScanProfile::new("x", "10.0.0.0/24");
        assert_eq!(p.target_count(), Some(254));
        p.spec = "not-an-ip".into();
        assert_eq!(p.target_count(), None);
    }

    #[test]
    fn default_document_writes_and_reads_back_cleanly() {
        let doc = ConfigDocument::new();
        let (back, notes) = ConfigDocument::from_toml(&doc.to_toml()).unwrap();
        assert!(notes.is_empty());
        assert_eq!(back, doc);
        assert!(doc.profiles.is_empty());
        assert!(doc.unknown_entries().is_empty());
    }

    #[test]
    fn app_section_unknown_keys_land_in_the_right_section() {
        // 用 r##"..."## 是因为内容里有 `"#`（`"#00FF00"`），
        // r#"..."# 会在这里提前收尾
        let text = r##"
schema_version = 2

[colors]
low = "#00FF00"
future_color = "#123456"

[monitor]
interval_ms = 5000
future_monitor = true
"##;
        let (doc, _) = ConfigDocument::from_toml(text).unwrap();
        assert_eq!(doc.app.colors.low, "#00FF00");
        assert_eq!(doc.app.monitor.interval_ms, 5000);
        let out = doc.to_toml();
        let colors_at = out.find("[colors]").unwrap();
        let ui_at = out.find("[ui]").unwrap();
        let future_at = out.find("future_color").unwrap();
        assert!(
            colors_at < future_at && future_at < ui_at,
            "未知键要跟在自己的小节里，而不是堆到文件末尾：\n{out}"
        );
        assert!(out.contains("future_monitor = true"), "{out}");
    }
}
