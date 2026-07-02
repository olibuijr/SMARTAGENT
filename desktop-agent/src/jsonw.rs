pub use httpc::json::Value;

pub fn write(v: &Value) -> String {
    match v {
        Value::Null => "null".to_string(),
        Value::Bool(x) => x.to_string(),
        Value::Num(x) => write_num(*x),
        Value::Str(s) => format!("\"{}\"", httpc::json::escape(s)),
        Value::Arr(items) => {
            let mut out = String::from("[");
            for (idx, item) in items.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push_str(&write(item));
            }
            out.push(']');
            out
        }
        Value::Obj(pairs) => {
            let mut out = String::from("{");
            for (idx, (key, val)) in pairs.iter().enumerate() {
                if idx > 0 {
                    out.push(',');
                }
                out.push('"');
                out.push_str(&httpc::json::escape(key));
                out.push_str("\":");
                out.push_str(&write(val));
            }
            out.push('}');
            out
        }
    }
}

fn write_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        (n as i64).to_string()
    } else {
        n.to_string()
    }
}

pub fn s(t: &str) -> Value {
    Value::Str(t.to_string())
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn n(x: f64) -> Value {
    Value::Num(x)
}

pub fn b(x: bool) -> Value {
    Value::Bool(x)
}

pub fn obj(pairs: Vec<(&str, Value)>) -> Value {
    Value::Obj(
        pairs
            .into_iter()
            .map(|(key, val)| (key.to_string(), val))
            .collect(),
    )
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn arr(items: Vec<Value>) -> Value {
    Value::Arr(items)
}

pub fn truncate_chars(s: &str, max: usize) -> String {
    let mut out = String::new();
    let mut truncated = false;

    for (idx, ch) in s.chars().enumerate() {
        if idx >= max {
            truncated = true;
            break;
        }
        out.push(ch);
    }

    if truncated {
        out.push('…');
    }

    out
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn compact_preview(v: &Value, max: usize) -> String {
    truncate_chars(&write(v), max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_nested_json_round_trip() {
        let v = obj(vec![
            ("name", s("Ólafur á Akureyri")),
            ("count", n(3.0)),
            ("active", b(true)),
            (
                "items",
                arr(vec![
                    s("kaffi"),
                    obj(vec![("nested", s("já")), ("score", n(4.5))]),
                ]),
            ),
        ]);

        let written = write(&v);
        assert!(written.contains("\"count\":3"));
        assert!(!written.contains("3.0"));
        assert_eq!(httpc::json::parse(&written).unwrap(), v);
    }

    #[test]
    fn truncates_on_char_boundaries() {
        let text = "Ólafur á Akureyri";
        let truncated = truncate_chars(text, 7);
        assert_eq!(truncated, "Ólafur …");
        assert!(truncated.is_char_boundary(truncated.len()));
    }

    #[test]
    fn compact_preview_writes_then_truncates() {
        let v = obj(vec![("text", s("Ólafur á Akureyri"))]);
        assert_eq!(compact_preview(&v, 10), "{\"text\":\"Ó…");
    }
}
