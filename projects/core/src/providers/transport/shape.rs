//! The shape of a provider event that did not parse, safe to log: names of the fields
//! the parser knows and the types of their values, never a value or any other name.

use serde_json::{Map, Value};

const MAX_SHAPE_BYTES: usize = 2048;
const MAX_DEPTH: usize = 8;
const MAX_ITEMS: usize = 4;

/// `known` lists the field names that may be shown.
pub fn shape(data: &str, known: &[&str]) -> String {
    let Ok(value) = serde_json::from_str::<Value>(data) else {
        let kind = match data.trim_start().chars().next() {
            Some('{') => "object",
            Some('[') => "array",
            Some('<') => "markup",
            _ => "text",
        };
        return format!("not JSON: {} bytes, {kind}", data.len());
    };
    // Truncation below relies on every written byte being ASCII.
    debug_assert!(known.iter().all(|name| name.is_ascii()));
    let mut out = String::new();
    describe(&value, known, 0, &mut out);
    // Only ASCII is written, so any length is a character boundary.
    out.truncate(MAX_SHAPE_BYTES);
    out
}

fn describe(value: &Value, known: &[&str], depth: usize, out: &mut String) {
    if out.len() >= MAX_SHAPE_BYTES {
        return;
    }
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(_) => out.push_str("bool"),
        Value::Number(number) if number.is_u64() => out.push_str("integer"),
        Value::Number(number) if number.is_i64() => out.push_str("negative"),
        Value::Number(_) => out.push_str("float"),
        Value::String(_) => out.push_str("string"),
        Value::Array(_) | Value::Object(_) if depth == MAX_DEPTH => out.push_str("..."),
        Value::Array(items) => describe_array(items, known, depth, out),
        Value::Object(fields) => describe_object(fields, known, depth, out),
    }
}

fn describe_array(items: &[Value], known: &[&str], depth: usize, out: &mut String) {
    out.push('[');
    for (position, item) in items.iter().take(MAX_ITEMS).enumerate() {
        if position > 0 {
            out.push(',');
        }
        describe(item, known, depth + 1, out);
    }
    if items.len() > MAX_ITEMS {
        out.push_str(&format!(",+{} more", items.len() - MAX_ITEMS));
    }
    out.push(']');
}

/// Names come from `known`, never from the event: a server can echo input in a name.
fn describe_object(fields: &Map<String, Value>, known: &[&str], depth: usize, out: &mut String) {
    out.push('{');
    let mut shown = 0;
    for name in known {
        let Some(field) = fields.get(*name) else {
            continue;
        };
        if shown > 0 {
            out.push(',');
        }
        out.push_str(name);
        out.push(':');
        describe(field, known, depth + 1, out);
        shown += 1;
    }
    if fields.len() > shown {
        let comma = if shown > 0 { "," } else { "" };
        out.push_str(&format!("{comma}+{} other", fields.len() - shown));
    }
    out.push('}');
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    const KNOWN: &[&str] = &["choices", "delta", "content", "tool_calls", "index"];

    #[test]
    fn shows_types_and_never_values_or_unknown_names() {
        let data = r#"{"choices":[{"index":-1,"delta":{"content":"sk-secret","tool_calls":null},"sk-name":"x"}]}"#;
        assert_eq!(
            shape(data, KNOWN),
            "{choices:[{delta:{content:string,tool_calls:null},index:negative,+1 other}]}"
        );
    }

    #[test]
    fn text_that_is_not_json_shows_only_its_size_and_kind() {
        assert_eq!(
            shape("<html>secret</html>", KNOWN),
            "not JSON: 19 bytes, markup"
        );
        assert_eq!(
            shape(r#"{"content":"secret"#, KNOWN),
            "not JSON: 18 bytes, object"
        );
        assert_eq!(shape("ping", KNOWN), "not JSON: 4 bytes, text");
    }

    #[test]
    fn a_large_event_stays_short() {
        let row = format!("[{}]", ["1"; 50].join(","));
        let data = (0..6).fold(row, |inner, _| {
            format!("[{}]", [inner.as_str(); 4].join(","))
        });
        assert!(shape(&data, KNOWN).len() <= MAX_SHAPE_BYTES);
    }

    proptest! {
        #[test]
        fn any_string_value_stays_out(value in "[A-Z]{12}") {
            let data = format!(r#"{{"content":"{value}","{value}":["{value}"]}}"#);
            prop_assert!(!shape(&data, KNOWN).contains(&value));
        }

        #[test]
        fn any_text_is_described_without_panic(data in ".{0,200}") {
            prop_assert!(shape(&data, KNOWN).len() <= MAX_SHAPE_BYTES);
        }
    }
}
