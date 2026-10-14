//! 零依赖 JSON 值模型与渲染（M10）。
//!
//! 为什么自己写：核心 crate 的「零第三方运行时依赖」是一条硬约束
//! （见 README 与 `docs/plan.md`），而引入 `serde_json` 会把它打破。
//! 本项目需要的 JSON 能力很小，但**转义必须绝对正确** —— 一处漏转义就能让
//! 整份报告变成非法 JSON，所以这里把转义单独抽成 [`escape_into`] 并全覆盖测试。
//!
//! 设计取舍：
//! - 对象用 `Vec<(String, Json)>` 而不是 `HashMap`：**键序稳定**，
//!   同一份数据两次渲染字节级一致，便于测试与 diff。
//! - 数字分 `UInt` / `Int` / `Float` 三种，避免把 `255` 渲染成 `255.0`。
//! - 非 ASCII 字符原样输出（UTF-8 本身就是合法 JSON），不做 `\uXXXX` 转义。

use std::fmt::Write as _;

/// 一个 JSON 值。
#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    UInt(u64),
    Int(i64),
    Float(f64),
    Str(String),
    Array(Vec<Json>),
    Object(Vec<(String, Json)>),
}

impl Json {
    pub fn str(value: impl Into<String>) -> Self {
        Json::Str(value.into())
    }

    pub fn opt_str(value: Option<impl Into<String>>) -> Self {
        match value {
            Some(v) => Json::Str(v.into()),
            None => Json::Null,
        }
    }

    pub fn array(items: impl IntoIterator<Item = Json>) -> Self {
        Json::Array(items.into_iter().collect())
    }

    pub fn object(pairs: impl IntoIterator<Item = (impl Into<String>, Json)>) -> Self {
        Json::Object(pairs.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// 紧凑渲染（无空白），适合机器消费与网络传输。
    pub fn render(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out, None, 0);
        out
    }

    /// 两空格缩进渲染，适合人读与入库。
    pub fn render_pretty(&self) -> String {
        let mut out = String::new();
        self.write_into(&mut out, Some(2), 0);
        out
    }

    fn write_into(&self, out: &mut String, indent: Option<usize>, depth: usize) {
        match self {
            Json::Null => out.push_str("null"),
            Json::Bool(true) => out.push_str("true"),
            Json::Bool(false) => out.push_str("false"),
            Json::UInt(v) => {
                let _ = write!(out, "{v}");
            }
            Json::Int(v) => {
                let _ = write!(out, "{v}");
            }
            Json::Float(v) => {
                if v.is_finite() {
                    // JSON 没有 NaN/Infinity，非有限值一律降级为 null（下面分支）
                    let _ = write!(out, "{v}");
                } else {
                    out.push_str("null");
                }
            }
            Json::Str(s) => {
                out.push('"');
                escape_into(s, out);
                out.push('"');
            }
            Json::Array(items) => {
                if items.is_empty() {
                    out.push_str("[]");
                    return;
                }
                out.push('[');
                for (i, item) in items.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline_indent(out, indent, depth + 1);
                    item.write_into(out, indent, depth + 1);
                }
                newline_indent(out, indent, depth);
                out.push(']');
            }
            Json::Object(pairs) => {
                if pairs.is_empty() {
                    out.push_str("{}");
                    return;
                }
                out.push('{');
                for (i, (key, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    newline_indent(out, indent, depth + 1);
                    out.push('"');
                    escape_into(key, out);
                    out.push('"');
                    out.push(':');
                    if indent.is_some() {
                        out.push(' ');
                    }
                    value.write_into(out, indent, depth + 1);
                }
                newline_indent(out, indent, depth);
                out.push('}');
            }
        }
    }

    /// 按键取值（仅对象有意义）。
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }
}

fn newline_indent(out: &mut String, indent: Option<usize>, depth: usize) {
    if let Some(width) = indent {
        out.push('\n');
        for _ in 0..width * depth {
            out.push(' ');
        }
    }
}

/// 转义 JSON 字符串内容（不含两侧引号），追加到 `out`。
///
/// 规则来自 RFC 8259 §7：
/// - `"` `\` 必须转义；
/// - U+0000–U+001F 必须转义（用 `\u00XX`，其中 `\b \f \n \r \t` 用短写）；
/// - 其余字符（含所有非 ASCII）原样输出。
///
/// 注意 **`/` 不转义**（它不是必须的，转了反而不利于人读）。
pub fn escape_into(s: &str, out: &mut String) {
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{8}' => out.push_str("\\b"),
            '\u{c}' => out.push_str("\\f"),
            c if (c as u32) < 0x20 => {
                let _ = write!(out, "\\u{:04x}", c as u32);
            }
            c => out.push(c),
        }
    }
}

/// 便捷入口：转义成不含引号的 JSON 字符串体。
pub fn escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    escape_into(s, &mut out);
    out
}

/// 是否是一个语法上完整的 JSON 文本（仅做括号/引号配平与常见错误检查）。
///
/// 这不是完整的解析器——本项目不需要解析 JSON——但足以在测试里守住
/// 「渲染出来的东西至少是配平的」这条底线。
pub fn is_balanced(text: &str) -> bool {
    let mut stack: Vec<char> = Vec::new();
    let mut in_string = false;
    let mut escaped = false;
    for ch in text.chars() {
        if in_string {
            if escaped {
                escaped = false;
            } else if ch == '\\' {
                escaped = true;
            } else if ch == '"' {
                in_string = false;
            }
            continue;
        }
        match ch {
            '"' => in_string = true,
            '{' | '[' => stack.push(ch),
            '}' if stack.pop() != Some('{') => return false,
            ']' if stack.pop() != Some('[') => return false,
            _ => {}
        }
    }
    !in_string && stack.is_empty()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scalars_render_without_type_confusion() {
        assert_eq!(Json::Null.render(), "null");
        assert_eq!(Json::Bool(true).render(), "true");
        assert_eq!(Json::UInt(255).render(), "255");
        assert_eq!(Json::Int(-7).render(), "-7");
        assert_eq!(Json::Float(1.5).render(), "1.5");
        assert_eq!(Json::str("hi").render(), "\"hi\"");
    }

    #[test]
    fn non_finite_floats_become_null() {
        assert_eq!(Json::Float(f64::NAN).render(), "null");
        assert_eq!(Json::Float(f64::INFINITY).render(), "null");
    }

    #[test]
    fn escaping_covers_every_mandatory_character() {
        assert_eq!(escape("a\"b"), "a\\\"b");
        assert_eq!(escape("a\\b"), "a\\\\b");
        assert_eq!(escape("a\nb"), "a\\nb");
        assert_eq!(escape("a\rb"), "a\\rb");
        assert_eq!(escape("a\tb"), "a\\tb");
        assert_eq!(escape("\u{8}"), "\\b");
        assert_eq!(escape("\u{c}"), "\\f");
        assert_eq!(escape("\u{0}"), "\\u0000");
        assert_eq!(escape("\u{1f}"), "\\u001f");
    }

    #[test]
    fn slash_and_non_ascii_are_left_alone() {
        assert_eq!(escape("a/b"), "a/b");
        assert_eq!(escape("厂商：中兴"), "厂商：中兴");
        assert_eq!(Json::str("C:\\tmp ").render(), "\"C:\\\\tmp \"");
    }

    #[test]
    fn object_keys_keep_insertion_order_for_stable_output() {
        let obj = Json::object([
            ("z", Json::UInt(1)),
            ("a", Json::UInt(2)),
            ("m", Json::UInt(3)),
        ]);
        assert_eq!(obj.render(), "{\"z\":1,\"a\":2,\"m\":3}");
        // 两次渲染必须字节级一致
        assert_eq!(obj.render(), obj.render());
    }

    #[test]
    fn empty_containers_render_tightly() {
        assert_eq!(Json::Array(vec![]).render(), "[]");
        assert_eq!(Json::Object(vec![]).render(), "{}");
        assert_eq!(Json::Array(vec![]).render_pretty(), "[]");
    }

    #[test]
    fn pretty_print_is_indented_and_still_balanced() {
        let value = Json::object([
            (
                "targets",
                Json::array([
                    Json::object([("ip", Json::str("10.0.0.1"))]),
                    Json::object([("ip", Json::str("10.0.0.2"))]),
                ]),
            ),
            ("total", Json::UInt(2)),
        ]);
        let pretty = value.render_pretty();
        assert!(pretty.contains("\n  \"targets\": ["), "{pretty}");
        assert!(pretty.contains("\n    {"), "{pretty}");
        assert!(is_balanced(&pretty));
        assert!(is_balanced(&value.render()));
    }

    #[test]
    fn tricky_strings_stay_balanced() {
        let nasty = "引号\" 反斜杠\\ 换行\n 制表\t 控制\u{1} 结束";
        let rendered = Json::str(nasty).render();
        assert!(is_balanced(&rendered), "{rendered}");
        assert!(!rendered.contains('\n'));
    }

    #[test]
    fn get_reaches_into_objects() {
        let obj = Json::object([("a", Json::UInt(1)), ("b", Json::str("x"))]);
        assert_eq!(obj.get("a"), Some(&Json::UInt(1)));
        assert_eq!(obj.get("b"), Some(&Json::Str("x".into())));
        assert_eq!(obj.get("c"), None);
        assert_eq!(Json::UInt(1).get("a"), None);
    }

    #[test]
    fn opt_str_maps_none_to_null() {
        assert_eq!(Json::opt_str(Some("x")).render(), "\"x\"");
        assert_eq!(Json::opt_str(None::<String>).render(), "null");
    }

    #[test]
    fn is_balanced_rejects_obvious_breakage() {
        assert!(is_balanced("{\"a\":[1,2]}"));
        assert!(!is_balanced("{\"a\":[1,2}"));
        assert!(!is_balanced("{\"a\":\"unterminated}"));
        assert!(!is_balanced("]"));
        assert!(is_balanced("{\"a\":\"\\\"}\"}"));
    }
}
