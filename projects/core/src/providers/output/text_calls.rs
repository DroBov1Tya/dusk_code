//! Tool calls a model wrote as text. Some servers pass a model's call markup through as
//! plain text when their parser misses it; the call then never runs and the turn ends
//! as if the model had answered. Only blocks that run to the very end of the answer
//! count, and only for tools the model was offered, so prose that shows the markup is
//! left alone.
//!
//! Two forms are read: `<tool_call><function=NAME><parameter=KEY>value</parameter>
//! </function></tool_call>` and `<tool_call>{"name": ..., "arguments": {...}}</tool_call>`.

use serde_json::{Map, Value};

use super::ToolCall;
use crate::providers::ToolSpec;

const OPEN: &str = "<tool_call>";
const CLOSE: &str = "</tool_call>";

/// The answer without its trailing call blocks, and the calls they hold.
pub fn recover(text: &str, tools: &[ToolSpec]) -> Option<(String, Vec<ToolCall>)> {
    let starts = text
        .match_indices(OPEN)
        .map(|(at, _)| at)
        .filter(|&at| text[..at].trim_end_matches([' ', '\t']).ends_with('\n') || at == 0);
    for start in starts {
        if let Some(calls) = blocks(&text[start..], tools) {
            return Some((text[..start].trim_end().to_owned(), calls));
        }
    }
    None
}

/// Every block from here to the end, or nothing.
fn blocks(mut rest: &str, tools: &[ToolSpec]) -> Option<Vec<ToolCall>> {
    let mut calls = Vec::new();
    loop {
        rest = rest.trim_start();
        if rest.is_empty() {
            return (!calls.is_empty()).then_some(calls);
        }
        rest = rest.strip_prefix(OPEN)?;
        // A server that stops on the closing tag drops it.
        let (body, after) = rest.split_once(CLOSE).unwrap_or((rest, ""));
        let (name, arguments) = call(body.trim(), tools)?;
        calls.push(ToolCall {
            id: format!("text_call_{}", calls.len()),
            name,
            arguments,
        });
        rest = after;
    }
}

fn call(body: &str, tools: &[ToolSpec]) -> Option<(String, Value)> {
    let (name, arguments) = match body.strip_prefix("<function=") {
        Some(function) => markup(function, tools)?,
        None => json(body)?,
    };
    tools
        .iter()
        .any(|tool| tool.name == name)
        .then_some((name, arguments))
}

/// `{"name": …, "arguments": …}`, the arguments as an object or a string holding one;
/// the inline tag filter reads the same form.
pub fn json(body: &str) -> Option<(String, Value)> {
    let mut call: Map<String, Value> = serde_json::from_str(body).ok()?;
    let name = call.remove("name")?.as_str()?.to_owned();
    let arguments = match call.remove("arguments").unwrap_or_default() {
        Value::String(raw) => serde_json::from_str(&raw).ok()?,
        Value::Null => Value::Object(Map::new()),
        arguments => arguments,
    };
    arguments.is_object().then_some((name, arguments))
}

/// `NAME>` then parameters, then `</function>`.
fn markup(function: &str, tools: &[ToolSpec]) -> Option<(String, Value)> {
    let (name, mut rest) = function.split_once('>')?;
    let name = name.trim().to_owned();
    let schema = &tools.iter().find(|tool| tool.name == name)?.parameters;
    let mut arguments = Map::new();
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix("</function>") {
            return after
                .trim()
                .is_empty()
                .then_some((name, Value::Object(arguments)));
        }
        let (key, after) = rest.strip_prefix("<parameter=")?.split_once('>')?;
        let (value, after) = after.split_once("</parameter>")?;
        let key = key.trim();
        let value = value.strip_prefix('\n').unwrap_or(value);
        let value = value.strip_suffix('\n').unwrap_or(value);
        arguments.insert(key.to_owned(), typed(value, &schema["properties"][key]));
        rest = after;
    }
}

/// Markup has only text; a value goes in as the type its schema asks for.
fn typed(value: &str, schema: &Value) -> Value {
    if schema["type"] == "string" || schema["type"].is_null() {
        return Value::String(value.to_owned());
    }
    serde_json::from_str(value.trim()).unwrap_or_else(|_| Value::String(value.to_owned()))
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    fn tools() -> Vec<ToolSpec> {
        let schema = json!({ "type": "object", "properties": {
            "path": { "type": "string" },
            "content": { "type": "string" },
            "timeout_seconds": { "type": "integer" }
        }});
        ["write_file", "run_command"]
            .map(|name| ToolSpec {
                name: name.into(),
                description: String::new(),
                parameters: schema.clone(),
            })
            .into()
    }

    #[test]
    fn markup_at_the_end_becomes_calls() {
        let text = "Now the script.\n<tool_call>\n<function=write_file>\n\
            <parameter=path>\nrun.py\n</parameter>\n<parameter=content>\n{\"a\": 1}\n\
            </parameter>\n</function>\n</tool_call>\n<tool_call>\n\
            {\"name\": \"run_command\", \"arguments\": {\"timeout_seconds\": 5}}\n";
        let (text, calls) = recover(text, &tools()).unwrap();
        assert_eq!(text, "Now the script.");
        assert_eq!(calls[0].name, "write_file");
        // A string parameter stays text even when it looks like JSON.
        assert_eq!(
            calls[0].arguments,
            json!({ "path": "run.py", "content": "{\"a\": 1}" })
        );
        assert_eq!(calls[1].arguments, json!({ "timeout_seconds": 5 }));
        assert_ne!(calls[0].id, calls[1].id);
    }

    #[test]
    fn prose_and_unknown_tools_stay_text() {
        let tools = tools();
        let shown = "Write <tool_call>{\"name\": \"write_file\"}</tool_call> like this.";
        assert!(recover(shown, &tools).is_none());
        let then_more = "<tool_call>\n<function=write_file>\n</function>\n</tool_call>\nDone.";
        assert!(recover(then_more, &tools).is_none());
        let unknown = "<tool_call>\n<function=rm_rf>\n</function>\n</tool_call>";
        assert!(recover(unknown, &tools).is_none());
        let unclosed = "<tool_call>\n<function=write_file>\n<parameter=path>\na";
        assert!(recover(unclosed, &tools).is_none());
    }
}
