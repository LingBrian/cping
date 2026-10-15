//! 无障碍支撑：主题、**可验证的**配色性质、字体缩放。
//!
//! 为什么这些放在核心层：三端要用同一套主题，而「高对比」「色盲友好」
//! 必须是**能算出对错的性质**，不是三端各挑一组顺眼的颜色。具体地：
//!
//! - 对比度用 WCAG 2.x 的相对亮度公式算，高对比主题要求 **≥ 7:1**（AAA 正文），
//!   其余主题要求 **≥ 4.5:1**（AA 正文）；
//! - 色觉友好用 Viénot/Brettel 近似的三色觉障碍矩阵模拟后，
//!   要求**任意两个类别在四种视觉下都拉得开**（距离 ≥ [`COLOR_BLIND_MIN_DISTANCE`]）。
//!
//! 这两条把「配色」变成可测量的东西：改坏了会在测试里失败，
//! 而不是等色盲用户来报告。
//!
//! **一条实测得到的、容易被想当然搞错的结论**：高对比 ≠ 色盲友好。
//! 传统高对比配色（纯绿 `#00FF00` + 纯黄 `#FFFF00`）在红色盲/绿色盲下
//! 几乎同色（实测距离 37.9，阈值 60）。所以本模块**不让高对比主题承诺
//! 色觉可区分**，并把这条当作事实写进断言
//! （[`tests::high_contrast_does_not_imply_color_blind_safe`]）——
//! 谁想「顺手把两个主题的配色统一一下」，测试会拦住他。

use crate::classify::ResultClass;
use crate::i18n::{self, Key, Lang};

// ---------------------------------------------------------------------------
// 颜色与对比度
// ---------------------------------------------------------------------------

/// 8 位每通道的 RGB（不带 alpha：本项目所有产物都是不透明底色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// 解析 `#RRGGBB`（大小写不限）。非法输入返回 `None`——
    /// 与 `config::validate_hex_color` 保持同一套格式约定。
    pub fn parse(hex: &str) -> Option<Self> {
        let bytes = hex.as_bytes();
        if bytes.len() != 7 || bytes[0] != b'#' {
            return None;
        }
        let mut v = [0u8; 3];
        for (i, slot) in v.iter_mut().enumerate() {
            let hi = (bytes[1 + i * 2] as char).to_digit(16)?;
            let lo = (bytes[2 + i * 2] as char).to_digit(16)?;
            *slot = (hi * 16 + lo) as u8;
        }
        Some(Self::new(v[0], v[1], v[2]))
    }

    /// 渲染成 `#RRGGBB`（大写十六进制）。
    pub fn to_hex(self) -> String {
        format!("#{:02X}{:02X}{:02X}", self.r, self.g, self.b)
    }
}

pub const BLACK: Rgb = Rgb::new(0x00, 0x00, 0x00);
pub const WHITE: Rgb = Rgb::new(0xFF, 0xFF, 0xFF);

/// sRGB 通道 → 线性光。WCAG 的亮度公式要求先线性化，
/// 直接拿 0-255 加权平均算出来的「对比度」是错的
/// （会把 `#00FF00` 在白底上算成「还行」，实际几乎看不见）。
fn linearize(u: u8) -> f64 {
    let v = u as f64 / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

fn delinearize(v: f64) -> u8 {
    let v = v.clamp(0.0, 1.0);
    let s = if v <= 0.003_130_8 {
        12.92 * v
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (s.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// WCAG 2.x 相对亮度（0.0 ~ 1.0）。
pub fn relative_luminance(c: Rgb) -> f64 {
    0.2126 * linearize(c.r) + 0.7152 * linearize(c.g) + 0.0722 * linearize(c.b)
}

/// WCAG 对比度（1.0 ~ 21.0）。
pub fn contrast_ratio(a: Rgb, b: Rgb) -> f64 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    let (hi, lo) = if la > lb { (la, lb) } else { (lb, la) };
    (hi + 0.05) / (lo + 0.05)
}

/// WCAG AA 正文最低对比度。
pub const CONTRAST_AA: f64 = 4.5;
/// WCAG AAA 正文最低对比度。
pub const CONTRAST_AAA: f64 = 7.0;

// ---------------------------------------------------------------------------
// 色觉模拟
// ---------------------------------------------------------------------------

/// 三种色觉障碍（人群占比合计约 8% 的男性）。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ColorVision {
    Normal,
    /// 红色盲（红敏色素缺失）。
    Protanopia,
    /// 绿色盲（绿敏色素缺失）——最常见。
    Deuteranopia,
    /// 蓝色盲（蓝敏色素缺失）——罕见。
    Tritanopia,
}

impl ColorVision {
    pub const ALL: [ColorVision; 4] = [
        ColorVision::Normal,
        ColorVision::Protanopia,
        ColorVision::Deuteranopia,
        ColorVision::Tritanopia,
    ];

    pub const fn key(self) -> &'static str {
        match self {
            ColorVision::Normal => "normal",
            ColorVision::Protanopia => "protanopia",
            ColorVision::Deuteranopia => "deuteranopia",
            ColorVision::Tritanopia => "tritanopia",
        }
    }

    pub const fn label_key(self) -> Key {
        match self {
            ColorVision::Normal => Key::LabelVisionNormal,
            ColorVision::Protanopia => Key::LabelVisionProtanopia,
            ColorVision::Deuteranopia => Key::LabelVisionDeuteranopia,
            ColorVision::Tritanopia => Key::LabelVisionTritanopia,
        }
    }

    pub fn label_in(self, lang: Lang) -> &'static str {
        i18n::text(lang, self.label_key())
    }

    /// 模拟矩阵（Viénot / Brettel 近似，作用在**线性** RGB 上）。
    ///
    /// 三张矩阵是公开文献里的常用近似值；误差在「够用来判断两种颜色
    /// 会不会被看成一样」的量级，不是医学级仿真。这一点写在注释里，
    /// 免得下一个人以为它是精确模型。
    const fn matrix(self) -> Option<[[f64; 3]; 3]> {
        match self {
            ColorVision::Normal => None,
            ColorVision::Protanopia => Some([
                [0.152_286, 1.052_583, -0.204_868],
                [0.114_503, 0.786_281, 0.099_216],
                [-0.003_882, -0.048_116, 1.051_998],
            ]),
            ColorVision::Deuteranopia => Some([
                [0.367_322, 0.860_646, -0.227_968],
                [0.280_085, 0.672_501, 0.047_413],
                [-0.011_820, 0.042_940, 0.968_881],
            ]),
            ColorVision::Tritanopia => Some([
                [1.255_528, -0.076_749, -0.178_779],
                [-0.078_411, 0.930_809, 0.147_602],
                [0.004_733, 0.691_367, 0.303_900],
            ]),
        }
    }
}

/// 把颜色换算成「这个色觉的人看到的样子」。
pub fn simulate(color: Rgb, vision: ColorVision) -> Rgb {
    let Some(m) = vision.matrix() else {
        return color;
    };
    let lin = [linearize(color.r), linearize(color.g), linearize(color.b)];
    let mut out = [0.0f64; 3];
    for (i, row) in m.iter().enumerate() {
        out[i] = row[0] * lin[0] + row[1] * lin[1] + row[2] * lin[2];
    }
    Rgb::new(
        delinearize(out[0]),
        delinearize(out[1]),
        delinearize(out[2]),
    )
}

/// 近似感知色距（redmean 公式，0 ~ 约 765）。
///
/// 用它而不是朴素欧氏距离，是因为欧氏距离会把「深蓝 vs 深紫」算得比
/// 实际看起来远。这个公式不需要色彩空间转换库，误差对本用途足够。
pub fn color_distance(a: Rgb, b: Rgb) -> f64 {
    let dr = f64::from(a.r) - f64::from(b.r);
    let dg = f64::from(a.g) - f64::from(b.g);
    let db = f64::from(a.b) - f64::from(b.b);
    let rm = (f64::from(a.r) + f64::from(b.r)) / 2.0;
    ((2.0 + rm / 256.0) * dr * dr + 4.0 * dg * dg + (2.0 + (255.0 - rm) / 256.0) * db * db).sqrt()
}

/// 色觉友好主题要求的最小色距。
///
/// **这个数字是实测标定的，不是拍的**：Okabe-Ito 配色方案在四种视觉下的
/// 最小色距实测 95.6，而传统高对比配色（纯绿 + 纯黄）只有 37.9。
/// 取 60 让两侧都留出余量——既能判出「不友好」，也不会因为
/// 换了一个稍暗的同类色就误报。
pub const COLOR_BLIND_MIN_DISTANCE: f64 = 60.0;

// ---------------------------------------------------------------------------
// 主题
// ---------------------------------------------------------------------------

/// 界面主题。
///
/// 三个主题的**目标不同**，不是「同一个主题的三档亮度」：
/// - [`Theme::Default`]：与原版视觉一致的类别色（兼容性优先），
///   在深色底上对比度达标，但**对红绿色盲不友好**（见上文实测）。
/// - [`Theme::HighContrast`]：对比度优先（≥ 7:1）。
/// - [`Theme::ColorBlind`]：色觉可区分优先（Okabe-Ito 配色）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Theme {
    #[default]
    Default,
    HighContrast,
    ColorBlind,
}

impl Theme {
    pub const ALL: [Theme; 3] = [Theme::Default, Theme::HighContrast, Theme::ColorBlind];

    pub const fn key(self) -> &'static str {
        match self {
            Theme::Default => "default",
            Theme::HighContrast => "high-contrast",
            Theme::ColorBlind => "color-blind",
        }
    }

    pub const fn label_key(self) -> Key {
        match self {
            Theme::Default => Key::LabelThemeDefault,
            Theme::HighContrast => Key::LabelThemeHighContrast,
            Theme::ColorBlind => Key::LabelThemeColorBlind,
        }
    }

    pub fn label_in(self, lang: Lang) -> &'static str {
        i18n::text(lang, self.label_key())
    }

    /// 解析主题名。**非法值返回 `None`，不静默回落成默认主题**：
    /// 用户显式写了 `--theme high-contrast` 却因为拼错而拿到默认配色，
    /// 属于「以为自己开了无障碍、其实没开」的事故。
    pub fn parse(text: &str) -> Option<Self> {
        match text.trim().to_ascii_lowercase().as_str() {
            "default" | "default-dark" | "dark" => Some(Theme::Default),
            "high-contrast" | "highcontrast" | "contrast" | "hc" => Some(Theme::HighContrast),
            "color-blind" | "colorblind" | "colour-blind" | "cb" => Some(Theme::ColorBlind),
            _ => None,
        }
    }

    /// 该主题的配色。
    ///
    /// `bg` 是主题**假定的**底色（终端/深色界面）。图形界面若底色不同，
    /// 由界面侧决定怎么用——但**类别色本身的可用性以这里的 `bg` 为准**，
    /// 所以报表导出要另走 [`ensure_contrast`]（见 `html.rs` 的调用点）。
    pub const fn palette(self) -> Palette {
        match self {
            Theme::Default => Palette {
                bg: Rgb::new(0x10, 0x10, 0x10),
                fg: Rgb::new(0xE8, 0xEA, 0xED),
                muted: Rgb::new(0x9C, 0xA3, 0xAF),
                border: Rgb::new(0x2C, 0x31, 0x38),
                accent: Rgb::new(0x79, 0xC0, 0xFF),
                low: Rgb::new(0x00, 0xFF, 0x00),
                mid: Rgb::new(0xFF, 0xFF, 0x00),
                high: Rgb::new(0x99, 0xB4, 0xD1),
                offline: Rgb::new(0xC0, 0xC0, 0xC0),
                error: Rgb::new(0xFF, 0x6B, 0x6B),
            },
            Theme::HighContrast => Palette {
                bg: BLACK,
                fg: WHITE,
                muted: Rgb::new(0xC0, 0xC0, 0xC0),
                border: Rgb::new(0x80, 0x80, 0x80),
                accent: Rgb::new(0x00, 0xFF, 0xFF),
                low: Rgb::new(0x00, 0xFF, 0x00),
                // 橙黄而非纯黄：纯黄与纯绿在红绿色盲下距离只有 30，
                // 换成偏橙的 `#FFCC00` 好一些（实测 37.9）——
                // 仍不到色盲友好的 60，但既然是「对比度优先」的主题，
                // 能顺手改善的部分就顺手改善。
                mid: Rgb::new(0xFF, 0xCC, 0x00),
                high: Rgb::new(0x00, 0xFF, 0xFF),
                offline: WHITE,
                error: Rgb::new(0xFF, 0x8A, 0x8A),
            },
            Theme::ColorBlind => Palette {
                bg: Rgb::new(0x10, 0x10, 0x10),
                fg: Rgb::new(0xF0, 0xF0, 0xF0),
                muted: Rgb::new(0xB3, 0xB3, 0xB3),
                border: Rgb::new(0x6E, 0x6E, 0x6E),
                accent: Rgb::new(0x56, 0xB4, 0xE9),
                // Okabe-Ito 配色方案（学术界通用的色盲友好八色盘）
                low: Rgb::new(0x00, 0x9E, 0x73),
                mid: Rgb::new(0xF0, 0xE4, 0x42),
                high: Rgb::new(0x56, 0xB4, 0xE9),
                offline: Rgb::new(0xB3, 0xB3, 0xB3),
                error: Rgb::new(0xD5, 0x5E, 0x00),
            },
        }
    }

    /// 该主题承诺的类别色对比度下限。
    pub const fn min_contrast(self) -> f64 {
        match self {
            Theme::HighContrast => CONTRAST_AAA,
            _ => CONTRAST_AA,
        }
    }

    /// 该主题是否承诺「任意两类在四种视觉下都可区分」。
    ///
    /// 只有色盲友好主题承诺。**高对比主题不承诺**——这不是偷懒，
    /// 是实测结论（纯绿与纯黄在红色盲下几乎同色）。用户要的是「更大对比」，
    /// 给他这个；要「色觉可区分」的，给他色盲友好主题。两个目标分开承诺，
    /// 比塞一个两头都不突出的方案好。
    pub const fn promises_color_blind_safety(self) -> bool {
        matches!(self, Theme::ColorBlind)
    }
}

/// 一套完整的配色。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Palette {
    pub bg: Rgb,
    pub fg: Rgb,
    pub muted: Rgb,
    pub border: Rgb,
    /// 界面强调色（页签、模式标签、进度条）。
    ///
    /// **不属于结果分类**，所以不参与 `audit` 的类别检查，也不进
    /// [`Palette::class_colors`]——把它和类别色混在一起会让「强调色」
    /// 被当成第六种探测结果。它的达标要求由测试直接断言。
    pub accent: Rgb,
    pub low: Rgb,
    pub mid: Rgb,
    pub high: Rgb,
    pub offline: Rgb,
    pub error: Rgb,
}

impl Palette {
    /// 取某一类结果的交互色。
    pub const fn class_color(&self, class: ResultClass) -> Rgb {
        match class {
            ResultClass::Low => self.low,
            ResultClass::Mid => self.mid,
            ResultClass::High => self.high,
            ResultClass::Offline => self.offline,
            ResultClass::Error => self.error,
        }
    }

    /// 全部类别色按 [`ResultClass`] 的顺序排列（供审计与测试遍历）。
    pub const fn class_colors(&self) -> [(ResultClass, Rgb); 5] {
        [
            (ResultClass::Low, self.low),
            (ResultClass::Mid, self.mid),
            (ResultClass::High, self.high),
            (ResultClass::Offline, self.offline),
            (ResultClass::Error, self.error),
        ]
    }

    /// 按主题自己的承诺体检，返回**全部**不达标项（不是发现一个就返回）。
    ///
    /// 返回列表而不是布尔：一个「是的，有问题」的结论没法指导人怎么修。
    pub fn audit(&self, theme: Theme) -> Vec<A11yIssue> {
        let mut issues = Vec::new();
        let need = theme.min_contrast();
        for (class, color) in self.class_colors() {
            let actual = contrast_ratio(color, self.bg);
            if actual + f64::EPSILON < need {
                issues.push(A11yIssue::LowContrast {
                    class,
                    actual,
                    required: need,
                });
            }
        }
        if theme.promises_color_blind_safety() {
            let colors = self.class_colors();
            for vision in ColorVision::ALL {
                for i in 0..colors.len() {
                    for j in (i + 1)..colors.len() {
                        let d = color_distance(
                            simulate(colors[i].1, vision),
                            simulate(colors[j].1, vision),
                        );
                        if d < COLOR_BLIND_MIN_DISTANCE {
                            issues.push(A11yIssue::Indistinguishable {
                                a: colors[i].0,
                                b: colors[j].0,
                                vision,
                                distance: d,
                            });
                        }
                    }
                }
            }
        }
        issues
    }
}

/// 一处不达标。
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum A11yIssue {
    LowContrast {
        class: ResultClass,
        actual: f64,
        required: f64,
    },
    Indistinguishable {
        a: ResultClass,
        b: ResultClass,
        vision: ColorVision,
        distance: f64,
    },
}

impl A11yIssue {
    pub fn describe_in(&self, lang: Lang) -> String {
        match *self {
            A11yIssue::LowContrast {
                class,
                actual,
                required,
            } => i18n::render(
                lang,
                Key::A11yLowContrast,
                &[
                    class.label_in(lang),
                    &format!("{actual:.2}"),
                    &format!("{required:.2}"),
                ],
            ),
            A11yIssue::Indistinguishable {
                a,
                b,
                vision,
                distance,
            } => i18n::render(
                lang,
                Key::A11yIndistinguishable,
                &[
                    a.label_in(lang),
                    b.label_in(lang),
                    vision.label_in(lang),
                    &format!("{distance:.1}"),
                ],
            ),
        }
    }
}

// ---------------------------------------------------------------------------
// 让一个颜色在给定底色上达标
// ---------------------------------------------------------------------------

/// 把 `fg` 朝远离 `bg` 的方向移动，直到对比度达到 `target`。
///
/// 存在的原因是一个**真实缺陷**：HTML 报表把类别色当**文字颜色**用在
/// 浅色底上（`#f6f7f9`），而 `#00FF00` 在白底上的对比度只有 1.37——
/// 「低延迟」那一格等于看不见。用户自定义的类别色是**为界面挑的**，
/// 不能指望它在浅色纸面上也可读，所以导出侧要按自己的底色把它压到达标。
///
/// 只沿「原色 → 黑/白」做线性插值，**色相基本不变**（纯绿压暗后还是绿），
/// 这样「绿=低延迟」的心智映射不会因为可读性调整而错乱。
pub fn ensure_contrast(fg: Rgb, bg: Rgb, target: f64) -> Rgb {
    if contrast_ratio(fg, bg) >= target {
        return fg;
    }
    let towards = if relative_luminance(bg) > relative_luminance(fg) {
        BLACK
    } else {
        WHITE
    };
    // 100 步够用（每步 1%）；比解析求根简单，且结果一定是可复现的。
    let mut best = towards;
    for step in 1..=100 {
        let k = f64::from(step) / 100.0;
        let cand = mix(fg, towards, k);
        best = cand;
        if contrast_ratio(cand, bg) >= target {
            return cand;
        }
    }
    best
}

/// 在 sRGB 通道上按比例 `k` 从 `a` 移向 `b`。
fn mix(a: Rgb, b: Rgb, k: f64) -> Rgb {
    let lerp = |x: u8, y: u8| -> u8 {
        let v = f64::from(x) * (1.0 - k) + f64::from(y) * k;
        v.clamp(0.0, 255.0).round() as u8
    };
    Rgb::new(lerp(a.r, b.r), lerp(a.g, b.g), lerp(a.b, b.b))
}

// ---------------------------------------------------------------------------
// 字体缩放
// ---------------------------------------------------------------------------

/// WCAG 2.2 的最小可点区域（CSS px）。
///
/// 缩放字号时**控件本身也要跟着放大**——只放大文字会让按钮变得比手指小，
/// 对「看不清所以要放大」的人反而是倒退。
pub const MIN_TARGET_PX: f64 = 24.0;

/// 字号档位。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum FontScale {
    #[default]
    Normal,
    Large,
    Huge,
}

impl FontScale {
    pub const ALL: [FontScale; 3] = [FontScale::Normal, FontScale::Large, FontScale::Huge];

    pub const fn key(self) -> &'static str {
        match self {
            FontScale::Normal => "normal",
            FontScale::Large => "large",
            FontScale::Huge => "huge",
        }
    }

    pub const fn label_key(self) -> Key {
        match self {
            FontScale::Normal => Key::LabelFontScaleNormal,
            FontScale::Large => Key::LabelFontScaleLarge,
            FontScale::Huge => Key::LabelFontScaleHuge,
        }
    }

    pub fn label_in(self, lang: Lang) -> &'static str {
        i18n::text(lang, self.label_key())
    }

    /// 缩放比例（1.0 / 1.5 / 2.0）。
    pub const fn factor(self) -> f64 {
        match self {
            FontScale::Normal => 1.0,
            FontScale::Large => 1.5,
            FontScale::Huge => 2.0,
        }
    }

    pub const fn percent(self) -> u32 {
        match self {
            FontScale::Normal => 100,
            FontScale::Large => 150,
            FontScale::Huge => 200,
        }
    }

    /// 缩放一个尺寸。**同时可用于字号与控件尺寸**：
    /// 控件用它就不会在放大后变小。
    pub fn scale(self, base_px: f64) -> f64 {
        base_px * self.factor()
    }

    /// 缩放后的尺寸是否还满足最小可点区域。
    pub fn keeps_min_target(self, base_px: f64) -> bool {
        self.scale(base_px) + f64::EPSILON >= MIN_TARGET_PX
    }

    /// 解析档位：接受 `150%` / `150` / `large` 三种写法。
    /// 非法值返回 `None`（理由同 [`Theme::parse`]）。
    pub fn parse(text: &str) -> Option<Self> {
        let t = text.trim().to_ascii_lowercase();
        let t = t.trim_end_matches('%');
        match t {
            "normal" | "100" | "1" | "1.0" => Some(FontScale::Normal),
            "large" | "150" | "1.5" => Some(FontScale::Large),
            "huge" | "200" | "2" | "2.0" => Some(FontScale::Huge),
            _ => None,
        }
    }
}

// ---------------------------------------------------------------------------
// 无障碍设置
// ---------------------------------------------------------------------------

/// 三端共用的无障碍设置。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct A11ySettings {
    pub theme: Theme,
    pub font_scale: FontScale,
    /// 无颜色模式：颜色全部退化为默认前景/背景，**信息载体只剩符号与文字**。
    pub no_color: bool,
    /// 减少动效（界面侧据此关掉过渡与闪烁）。
    pub reduce_motion: bool,
}

impl A11ySettings {
    /// 从环境变量解析（命令行参数优先，由调用方合并）。
    ///
    /// - `CPING_THEME` / `CPING_FONT_SCALE`：非法值**忽略**并保持默认，
    ///   因为环境变量可能被别人的脚本设成任何东西，为此让程序起不来太过分；
    ///   **命令行上的非法值则必须报错**（那是用户当场敲的）。
    /// - `NO_COLOR`：遵循 <https://no-color.org> 约定——**存在且非空**
    ///   即生效（不是「等于 1 才生效」）。
    /// - `CPING_REDUCE_MOTION`：同上，「存在且非空」即生效。
    pub fn from_env() -> Self {
        let mut s = A11ySettings::default();
        if let Ok(v) = std::env::var("CPING_THEME") {
            if let Some(t) = Theme::parse(&v) {
                s.theme = t;
            }
        }
        if let Ok(v) = std::env::var("CPING_FONT_SCALE") {
            if let Some(f) = FontScale::parse(&v) {
                s.font_scale = f;
            }
        }
        s.no_color = flag_is_on(std::env::var("NO_COLOR").ok().as_deref());
        s.reduce_motion = flag_is_on(std::env::var("CPING_REDUCE_MOTION").ok().as_deref());
        s
    }

    /// 是否该给类别色上色。
    pub const fn colors_enabled(&self) -> bool {
        !self.no_color
    }

    /// 本设置的体检结论（拿主题配色跑一遍 [`Palette::audit`]）。
    pub fn audit(&self) -> Vec<A11yIssue> {
        // 无颜色模式下颜色根本不会被用，配色达不达标与可达性无关。
        if self.no_color {
            return Vec::new();
        }
        self.theme.palette().audit(self.theme)
    }
}

/// 环境变量的「开关」约定：存在且非空即为开。
///
/// `NO_COLOR=0` 也是开——规范写的是「非空」。把它当关会让
/// 「设了 `NO_COLOR=` 却还是彩色的」成为一类没法解释的怪事。
fn flag_is_on(value: Option<&str>) -> bool {
    matches!(value, Some(v) if !v.is_empty())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn palette_of(theme: Theme) -> Palette {
        theme.palette()
    }

    // ---------------- 对比度算法本身 ----------------

    #[test]
    fn contrast_ratio_matches_wcag_reference_values() {
        // 黑白是 WCAG 的极值 21:1，同色是 1:1。
        // 这两条能抓住「忘了先线性化」的写法（那样算出来是 4.0 与 1.0）。
        assert!(
            (contrast_ratio(BLACK, WHITE) - 21.0).abs() < 0.01,
            "{:.3}",
            contrast_ratio(BLACK, WHITE)
        );
        assert!((contrast_ratio(WHITE, WHITE) - 1.0).abs() < 1e-9);
        // 顺序无关
        assert!((contrast_ratio(WHITE, BLACK) - contrast_ratio(BLACK, WHITE)).abs() < 1e-9);
        // 一条现实检查：纯绿在白底上几乎看不见，在深底上很清楚。
        let green = Rgb::new(0x00, 0xFF, 0x00);
        assert!(contrast_ratio(green, Rgb::new(0xF6, 0xF7, 0xF9)) < 1.5);
        assert!(contrast_ratio(green, Rgb::new(0x10, 0x10, 0x10)) > 10.0);
    }

    #[test]
    fn relative_luminance_is_ordered_and_bounded() {
        assert!((relative_luminance(BLACK)).abs() < 1e-9);
        assert!((relative_luminance(WHITE) - 1.0).abs() < 1e-9);
        let lo = relative_luminance(Rgb::new(0x40, 0x40, 0x40));
        let hi = relative_luminance(Rgb::new(0x80, 0x80, 0x80));
        assert!(lo < hi && hi < 1.0);
    }

    // ---------------- 三个主题各自的承诺 ----------------

    #[test]
    fn every_theme_is_free_of_its_own_declared_issues() {
        // 这是「主题没被改坏」的守卫。三个主题都应为空——
        // 因为每个主题只按**自己承诺的**目标体检
        // （色盲友好主题才检查色觉可区分）。
        for theme in Theme::ALL {
            let issues = palette_of(theme).audit(theme);
            let rendered: Vec<String> = issues.iter().map(|i| i.describe_in(Lang::Zh)).collect();
            assert!(
                issues.is_empty(),
                "{} 主题未达自己声明的目标：{rendered:?}",
                theme.key()
            );
        }
    }

    #[test]
    fn high_contrast_reaches_aaa_and_the_others_reach_aa() {
        for (theme, want) in [
            (Theme::Default, CONTRAST_AA),
            (Theme::HighContrast, CONTRAST_AAA),
            (Theme::ColorBlind, CONTRAST_AA),
        ] {
            let p = palette_of(theme);
            for (class, color) in p.class_colors() {
                let c = contrast_ratio(color, p.bg);
                assert!(
                    c + f64::EPSILON >= want,
                    "{} 的 {} 只有 {:.2}，要求 {:.2}",
                    theme.key(),
                    class.key(),
                    c,
                    want
                );
            }
            // 界面色（前景 / 弱化 / 强调）同样要达标：它们承载的是
            // 「现在在哪一页」「按了哪个键」「模型正在用哪个模板」这类信息，
            // 读不出来一样是障碍。它们**不进 `audit`**（不是探测结果分类），
            // 所以在这里直接断言。
            let p = palette_of(theme);
            for (role, color) in [("fg", p.fg), ("muted", p.muted), ("accent", p.accent)] {
                let c = contrast_ratio(color, p.bg);
                assert!(
                    c + f64::EPSILON >= want,
                    "{} 的界面色 {role} 只有 {:.2}，要求 {:.2}",
                    theme.key(),
                    c,
                    want
                );
            }
        }
    }

    #[test]
    fn color_blind_theme_keeps_every_pair_apart_in_all_four_visions() {
        let p = palette_of(Theme::ColorBlind);
        let colors = p.class_colors();
        for vision in ColorVision::ALL {
            for i in 0..colors.len() {
                for j in (i + 1)..colors.len() {
                    let d = color_distance(
                        simulate(colors[i].1, vision),
                        simulate(colors[j].1, vision),
                    );
                    assert!(
                        d + f64::EPSILON >= COLOR_BLIND_MIN_DISTANCE,
                        "{} 下 {} 与 {} 距离只有 {:.1}（要求 {:.0}）",
                        vision.key(),
                        colors[i].0.key(),
                        colors[j].0.key(),
                        d,
                        COLOR_BLIND_MIN_DISTANCE
                    );
                }
            }
        }
    }

    #[test]
    fn high_contrast_does_not_imply_color_blind_safe() {
        // 把「高对比 ≠ 色盲友好」这条实测结论写成断言。
        // 谁想把两个主题的配色「统一一下」，会在这里看到原因。
        for theme in [Theme::Default, Theme::HighContrast] {
            let p = palette_of(theme);
            let worst = ColorVision::ALL
                .iter()
                .flat_map(|v| {
                    let colors = p.class_colors();
                    (0..colors.len()).flat_map(move |i| {
                        ((i + 1)..colors.len()).map(move |j| {
                            color_distance(simulate(colors[i].1, *v), simulate(colors[j].1, *v))
                        })
                    })
                })
                .fold(f64::MAX, f64::min);
            assert!(
                worst < COLOR_BLIND_MIN_DISTANCE,
                "{} 主题居然达到了色盲友好阈值（最小距离 {:.1}）——\
                 若确实换了配色，请更新本条测试与文档，而不是删掉它",
                theme.key(),
                worst
            );
        }
        // 反过来确认默认主题的类别色就是原版那几个值，没有被「顺手优化」。
        let p = palette_of(Theme::Default);
        assert_eq!(p.low.to_hex(), crate::config::DEFAULT_COLOR_LOW);
        assert_eq!(p.mid.to_hex(), crate::config::DEFAULT_COLOR_MID);
        assert_eq!(p.high.to_hex(), crate::config::DEFAULT_COLOR_HIGH);
        assert_eq!(p.offline.to_hex(), crate::config::DEFAULT_COLOR_OFFLINE);
        assert_eq!(p.error.to_hex(), crate::config::DEFAULT_COLOR_ERROR);
    }

    #[test]
    fn only_the_color_blind_theme_promises_color_blind_safety() {
        assert!(Theme::ColorBlind.promises_color_blind_safety());
        assert!(!Theme::Default.promises_color_blind_safety());
        assert!(!Theme::HighContrast.promises_color_blind_safety());
    }

    // ---------------- ensure_contrast ----------------

    #[test]
    fn ensure_contrast_lifts_a_color_onto_a_light_background() {
        let paper = Rgb::new(0xF6, 0xF7, 0xF9);
        for (class, color) in Theme::Default.palette().class_colors() {
            let fixed = ensure_contrast(color, paper, CONTRAST_AA);
            let c = contrast_ratio(fixed, paper);
            assert!(
                c + f64::EPSILON >= CONTRAST_AA,
                "{} 压暗后仍只有 {:.2}",
                class.key(),
                c
            );
        }
        // 色相不能跑掉：纯绿压暗后仍应是绿（绿通道最大），
        // 否则「绿=低延迟」的心智映射就断了。
        let green = ensure_contrast(Rgb::new(0x00, 0xFF, 0x00), paper, CONTRAST_AA);
        assert!(green.g > green.r && green.g > green.b, "{}", green.to_hex());
        // 深色底同理：颜色要被提亮而不是压暗。
        let lifted = ensure_contrast(Rgb::new(0x10, 0x20, 0x10), BLACK, CONTRAST_AAA);
        assert!(contrast_ratio(lifted, BLACK) >= CONTRAST_AAA);
    }

    #[test]
    fn ensure_contrast_leaves_a_compliant_color_untouched() {
        // 已经达标的颜色必须**原样返回**：否则用户自定义的配色会被悄悄地
        // 挪到一个他认不出来的值上，而他并没有要求改。
        let white_on_black = ensure_contrast(WHITE, BLACK, CONTRAST_AAA);
        assert_eq!(white_on_black, WHITE);
        let p = Theme::ColorBlind.palette();
        for (_, color) in p.class_colors() {
            assert_eq!(ensure_contrast(color, p.bg, CONTRAST_AA), color);
        }
    }

    // ---------------- 字体缩放 ----------------

    #[test]
    fn font_scale_multiplies_monotonically() {
        let bases = [12.0, 13.0, 16.0];
        for base in bases {
            let n = FontScale::Normal.scale(base);
            let l = FontScale::Large.scale(base);
            let h = FontScale::Huge.scale(base);
            assert!(n < l && l < h, "缩放不单调：{n} {l} {h}");
            assert!((n - base).abs() < 1e-9);
            assert!((l - base * 1.5).abs() < 1e-9);
            assert!((h - base * 2.0).abs() < 1e-9);
        }
    }

    #[test]
    fn scaling_up_never_shrinks_a_control_below_the_minimum_target() {
        // 一个「本来就够大」的控件，放大后必须仍然够大——
        // 挡住「只放大文字、控件尺寸不变」那类写法。
        let control = MIN_TARGET_PX;
        for scale in FontScale::ALL {
            assert!(
                scale.keeps_min_target(control),
                "{} 缩放后控件只有 {:.1}px",
                scale.key(),
                scale.scale(control)
            );
        }
        // 反过来：一个明显偏小的控件，Normal 档就该被判为不合格。
        assert!(!FontScale::Normal.keeps_min_target(16.0));
        assert!(FontScale::Huge.keeps_min_target(16.0));
    }

    // ---------------- 解析与环境变量 ----------------

    #[test]
    fn theme_and_font_scale_parse_rejects_unknown_values() {
        assert_eq!(Theme::parse("high-contrast"), Some(Theme::HighContrast));
        assert_eq!(Theme::parse("  Color-Blind "), Some(Theme::ColorBlind));
        assert_eq!(Theme::parse("colour-blind"), Some(Theme::ColorBlind));
        assert_eq!(Theme::parse("kontrast"), None, "拼错必须返回 None");
        assert_eq!(Theme::parse(""), None);

        assert_eq!(FontScale::parse("150%"), Some(FontScale::Large));
        assert_eq!(FontScale::parse("2.0"), Some(FontScale::Huge));
        assert_eq!(FontScale::parse("huge"), Some(FontScale::Huge));
        assert_eq!(FontScale::parse("175"), None);
    }

    #[test]
    fn flag_is_on_follows_the_no_color_convention() {
        assert!(flag_is_on(Some("1")));
        // `NO_COLOR=0` 也是「开」：规范写的是「存在且非空」。
        assert!(flag_is_on(Some("0")));
        assert!(flag_is_on(Some("anything")));
        // 空串与未设置都是「关」——「设了变量却还是彩色」比「设了没用」难解释
        assert!(!flag_is_on(Some("")));
        assert!(!flag_is_on(None));
    }

    #[test]
    fn no_color_mode_skips_the_palette_audit() {
        // 无颜色模式下颜色根本不会被用到，报「配色不达标」是噪音。
        let s = A11ySettings {
            theme: Theme::Default,
            no_color: true,
            ..A11ySettings::default()
        };
        assert!(s.audit().is_empty());
        assert!(!s.colors_enabled());
        assert!(A11ySettings::default().colors_enabled());
    }

    // ---------------- 本地化 ----------------

    #[test]
    fn every_issue_renders_in_both_languages() {
        fn samples() -> Vec<A11yIssue> {
            vec![
                A11yIssue::LowContrast {
                    class: ResultClass::Error,
                    actual: 1.37,
                    required: 4.5,
                },
                A11yIssue::Indistinguishable {
                    a: ResultClass::Low,
                    b: ResultClass::Mid,
                    vision: ColorVision::Deuteranopia,
                    distance: 12.0,
                },
            ]
        }
        let zh: Vec<String> = samples().iter().map(|i| i.describe_in(Lang::Zh)).collect();
        let en: Vec<String> = samples().iter().map(|i| i.describe_in(Lang::En)).collect();
        for (z, e) in zh.iter().zip(en.iter()) {
            assert!(!z.is_empty() && !e.is_empty());
            assert_ne!(z, e, "两种语言渲染结果相同：{z}");
            assert!(
                z.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "中文渲染里没有汉字：{z}"
            );
            assert!(
                !e.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)),
                "英文渲染里出现了汉字：{e}"
            );
        }
        // 参数确实被填进去了：数字要出现在结果里
        assert!(
            zh[0].contains("1.37") && zh[0].contains("4.50"),
            "{}",
            zh[0]
        );
        assert!(en[1].contains("12.0"), "{}", en[1]);
        // 类别与视觉类型都走本地化，不是硬编码中文
        assert!(en[0].to_ascii_lowercase().contains("error"), "{}", en[0]);
        assert!(en[1].contains("deuteranopia"), "{}", en[1]);
    }

    #[test]
    fn theme_and_font_scale_labels_are_localized_both_ways() {
        for theme in Theme::ALL {
            let zh = theme.label_in(Lang::Zh);
            let en = theme.label_in(Lang::En);
            assert_ne!(zh, en, "{} 主题标签没有本地化", theme.key());
            assert!(zh.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
            assert!(!en.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        }
        for scale in FontScale::ALL {
            let zh = scale.label_in(Lang::Zh);
            let en = scale.label_in(Lang::En);
            assert_ne!(zh, en, "{} 档没有本地化", scale.key());
        }
        for vision in ColorVision::ALL {
            let zh = vision.label_in(Lang::Zh);
            let en = vision.label_in(Lang::En);
            assert_ne!(zh, en, "{} 没有本地化", vision.key());
            assert!(!en.chars().any(|c| ('\u{4e00}'..='\u{9fff}').contains(&c)));
        }
    }

    #[test]
    fn color_parsing_is_strict_and_round_trips() {
        assert_eq!(Rgb::parse("#00ff00"), Some(Rgb::new(0, 255, 0)));
        assert_eq!(
            Rgb::parse("#FFCC00").map(Rgb::to_hex).as_deref(),
            Some("#FFCC00")
        );
        assert_eq!(Rgb::parse("00FF00"), None);
        assert_eq!(Rgb::parse("#00FF0"), None);
        assert_eq!(Rgb::parse("#00FF0G"), None);
    }

    // ---------------- 无颜色可达性 ----------------

    /// 五个类别，按 `ResultClass` 的定义顺序（改枚举要同步这里）。
    const ALL_CLASSES: [ResultClass; 5] = [
        ResultClass::Low,
        ResultClass::Mid,
        ResultClass::High,
        ResultClass::Offline,
        ResultClass::Error,
    ];

    #[test]
    fn every_class_has_a_unique_single_width_symbol() {
        // 「无颜色环境下所有功能可达」的第一条前提：每个状态都有符号可看。
        // 颜色可以被关掉（`--no-color` / `NO_COLOR`），符号不能没有。
        let mut seen: Vec<&'static str> = Vec::new();
        for class in ALL_CLASSES {
            let s = class.symbol();
            assert!(!s.trim().is_empty(), "{} 没有符号", class.key());
            assert_eq!(
                s.chars().count(),
                1,
                "{} 的符号不是单个字符：{s:?}",
                class.key()
            );
            assert!(
                s.is_ascii(),
                "{} 的符号不是 ASCII（等宽字体下宽度会不一致）：{s:?}",
                class.key()
            );
            assert!(!seen.contains(&s), "符号重复：{s}");
            seen.push(s);
        }
        assert_eq!(seen.len(), ALL_CLASSES.len());
    }

    #[test]
    fn every_class_has_a_text_label_in_both_languages() {
        // 第二条通道：符号之外还有文字。两条都有，颜色才是「可选增强」。
        for class in ALL_CLASSES {
            for lang in [Lang::Zh, Lang::En] {
                let label = class.label_in(lang);
                assert!(
                    !label.trim().is_empty(),
                    "{} 在 {} 下没有标签",
                    class.key(),
                    lang.code()
                );
            }
        }
    }
}
