//! Python's semantics for the few things ruvsarpur's code relies on, over
//! schedule items kept as JSON (a Python dict: a missing key and `None` are
//! different things there, and some branches depend on it).

use serde_json::Value;

/// Python's `str.isspace()`: Rust's `char::is_whitespace` plus the four
/// information separators U+001C to U+001F, which Python also strips.
pub fn is_space(c: char) -> bool {
    c.is_whitespace() || ('\u{1c}'..='\u{1f}').contains(&c)
}

/// `s.strip()`.
pub fn strip(s: &str) -> &str {
    s.trim_matches(is_space)
}

/// `str(value)` for the JSON values a schedule holds.
pub fn str_of(value: &Value) -> String {
    match value {
        Value::Null => "None".into(),
        Value::Bool(true) => "True".into(),
        Value::Bool(false) => "False".into(),
        Value::String(s) => s.clone(),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                i.to_string()
            } else if let Some(u) = n.as_u64() {
                u.to_string()
            } else {
                float_repr(n.as_f64().unwrap_or(0.0))
            }
        }
        other => other.to_string(),
    }
}

/// `repr(float)` for ordinary values: `3.0`, `2.5`.
fn float_repr(f: f64) -> String {
    if f.fract() == 0.0 && f.abs() < 1e16 {
        format!("{f:.1}")
    } else {
        f.to_string()
    }
}

/// `str(d[key])` with a missing key read as `None`.
pub fn str_at(item: &Value, key: &str) -> String {
    item.get(key).map(str_of).unwrap_or_else(|| "None".into())
}

/// `key in d`.
pub fn has(item: &Value, key: &str) -> bool {
    item.get(key).is_some()
}

/// `key in d and d[key] is not None`.
pub fn has_some(item: &Value, key: &str) -> bool {
    item.get(key).is_some_and(|v| !v.is_null())
}

/// Python truthiness.
pub fn truthy(value: &Value) -> bool {
    match value {
        Value::Null => false,
        Value::Bool(b) => *b,
        Value::Number(n) => n.as_f64().is_some_and(|f| f != 0.0),
        Value::String(s) => !s.is_empty(),
        Value::Array(a) => !a.is_empty(),
        Value::Object(o) => !o.is_empty(),
    }
}

/// `bool(d.get(key))`.
pub fn truthy_at(item: &Value, key: &str) -> bool {
    item.get(key).is_some_and(truthy)
}

/// `d[key] is True` (a real boolean, not just truthy).
pub fn is_true(item: &Value, key: &str) -> bool {
    item.get(key) == Some(&Value::Bool(true))
}

/// `int(value)`: whole numbers, booleans and decimal strings (surrounding
/// whitespace and `_` separators aside, which RÚV never sends). `None`
/// where Python would raise.
pub fn int_of(value: &Value) -> Option<i64> {
    match value {
        Value::Bool(b) => Some(i64::from(*b)),
        Value::Number(n) => n.as_i64().or_else(|| n.as_f64().map(|f| f.trunc() as i64)),
        Value::String(s) => {
            let s = strip(s);
            let digits = s.strip_prefix(['+', '-']).unwrap_or(s);
            if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
                return None;
            }
            s.parse().ok()
        }
        _ => None,
    }
}

/// `s.zfill(width)`.
pub fn zfill(s: &str, width: usize) -> String {
    let len = s.chars().count();
    if len >= width {
        return s.to_string();
    }
    let pad = "0".repeat(width - len);
    match s.chars().next() {
        Some(sign @ ('+' | '-')) => format!("{sign}{pad}{}", &s[1..]),
        _ => format!("{pad}{s}"),
    }
}

/// `s[start:end]` by characters, clamped like Python's slicing.
pub fn slice(s: &str, start: usize, end: usize) -> String {
    s.chars()
        .skip(start)
        .take(end.saturating_sub(start))
        .collect()
}

/// `s.lower()`.
pub fn lower(s: &str) -> String {
    s.to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn conversions() {
        assert_eq!(str_of(&json!(null)), "None");
        assert_eq!(str_of(&json!(true)), "True");
        assert_eq!(str_of(&json!(7)), "7");
        assert_eq!(str_of(&json!(2.0)), "2.0");
        assert_eq!(int_of(&json!(" 12 ")), Some(12));
        assert_eq!(int_of(&json!("1.5")), None);
        assert_eq!(int_of(&json!("")), None);
        assert_eq!(zfill("3", 2), "03");
        assert_eq!(zfill("123", 2), "123");
        assert_eq!(zfill("-3", 3), "-03");
        assert_eq!(slice("2024-05-01T10:00:00", 0, 10), "2024-05-01");
        assert_eq!(slice("ab", 5, 10), "");
        assert_eq!(strip("\u{1f} a \t"), "a");
    }
}
