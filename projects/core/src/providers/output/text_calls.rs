//! Tool calls a model wrote as text. Some servers pass a model's call markup through as
//! plain text when their parser misses it; the call then never runs and the turn ends
//! as if the model had answered. Only blocks that run to the very end of the answer
//! count, and only for tools the model was offered, so prose that shows the markup is
//! left alone. Markup that ends the text but cannot be read gets a note back instead
//! (`stranded`), so the model tries again with a real call.
//!
//! Two forms are read: `<tool_call><function=NAME><parameter=KEY>value</parameter>
//! </function></tool_call>` and `<tool_call>{"name": ..., "arguments": {...}}</tool_call>`.
//! Inside a block closed by `</tool_call>`, a missing `</parameter>` or `</function>`
//! is forgiven: models drop them.

use serde_json::{Map, Value};

use super::ToolCall;
use crate::providers::ToolSpec;

const OPEN: &str = "<tool_call>";
const CLOSE: &str = "</tool_call>";
const FUNCTION: &str = "<function=";
const FUNCTION_CLOSE: &str = "</function>";
const PARAMETER: &str = "<parameter=";
const PARAMETER_CLOSE: &str = "</parameter>";

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
        let (body, after, closed) = match rest.split_once(CLOSE) {
            Some((body, after)) => (body, after, true),
            None => (rest, "", false),
        };
        let (name, arguments) = call(body.trim(), closed, tools)?;
        calls.push(ToolCall {
            id: format!("text_call_{}", calls.len()),
            name,
            arguments,
        });
        rest = after;
    }
}

fn call(body: &str, closed: bool, tools: &[ToolSpec]) -> Option<(String, Value)> {
    let (name, arguments) = match body.strip_prefix(FUNCTION) {
        Some(function) => markup(function, closed, tools)?,
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

/// `NAME>` then parameters, then `</function>`. Well-formed markup is read strictly
/// first, so a value may hold tags of its own (source of this parser, say).
fn markup(function: &str, closed: bool, tools: &[ToolSpec]) -> Option<(String, Value)> {
    strict(function, tools).or_else(|| tolerant(function, closed, tools))
}

/// Every value closed by `</parameter>`, the call by `</function>`.
fn strict(function: &str, tools: &[ToolSpec]) -> Option<(String, Value)> {
    let (name, mut rest) = function.split_once('>')?;
    let name = name.trim().to_owned();
    let schema = &tools.iter().find(|tool| tool.name == name)?.parameters;
    let mut arguments = Map::new();
    loop {
        rest = rest.trim_start();
        if let Some(after) = rest.strip_prefix(FUNCTION_CLOSE) {
            return after
                .trim()
                .is_empty()
                .then_some((name, Value::Object(arguments)));
        }
        let (key, after) = rest.strip_prefix(PARAMETER)?.split_once('>')?;
        let (value, after) = after.split_once(PARAMETER_CLOSE)?;
        let key = key.trim();
        let value = value.strip_prefix('\n').unwrap_or(value);
        let value = value.strip_suffix('\n').unwrap_or(value);
        arguments.insert(key.to_owned(), typed(value, &schema["properties"][key]));
        rest = after;
    }
}

/// Closers may be missing. A missing one is forgiven only when `</tool_call>` closed the
/// block, so a cut-off call is never run.
fn tolerant(function: &str, closed: bool, tools: &[ToolSpec]) -> Option<(String, Value)> {
    let (name, mut rest) = function.split_once('>')?;
    let name = name.trim().to_owned();
    let schema = &tools.iter().find(|tool| tool.name == name)?.parameters;
    let mut arguments = Map::new();
    loop {
        rest = rest.trim_start();
        let ended = match rest.strip_prefix(FUNCTION_CLOSE) {
            Some(after) => Some(after.trim().is_empty()),
            None => rest.is_empty().then_some(closed),
        };
        if let Some(ended) = ended {
            return ended.then_some((name, Value::Object(arguments)));
        }
        let (key, after) = rest.strip_prefix(PARAMETER)?.split_once('>')?;
        let key = key.trim();
        let (value, after) = match after.split_once(PARAMETER_CLOSE) {
            Some((value, after))
                if !value.contains(PARAMETER) && !value.contains(FUNCTION_CLOSE) =>
            {
                let value = value.strip_prefix('\n').unwrap_or(value);
                (value.strip_suffix('\n').unwrap_or(value), after)
            }
            // No closer of its own: the value runs to the next tag, or to the end.
            _ => {
                let end = [PARAMETER, FUNCTION_CLOSE]
                    .iter()
                    .filter_map(|tag| after.find(tag))
                    .min();
                if end.is_none() && !closed {
                    return None;
                }
                let (value, after) = after.split_at(end.unwrap_or(after.len()));
                (value.trim(), after)
            }
        };
        arguments.insert(key.to_owned(), typed(value, &schema["properties"][key]));
        rest = after;
    }
}

/// The call markup that ends `text`, if any: from an opening tag that starts a line and
/// reads as a call, with no prose after the last closing one. Markup followed by prose,
/// or mentioned inside a sentence, was shown, not called.
fn trailing_markup(text: &str) -> Option<&str> {
    let start = [OPEN, FUNCTION]
        .iter()
        .flat_map(|tag| text.match_indices(tag).map(|(at, _)| at))
        .filter(|&at| at == 0 || text[..at].trim_end_matches([' ', '\t']).ends_with('\n'))
        .filter(|&at| looks_like_call(&text[at..]))
        .min()?;
    let tail = &text[start..];
    let after_last_closer = [CLOSE, FUNCTION_CLOSE]
        .iter()
        .filter_map(|tag| tail.rfind(tag).map(|at| &tail[at + tag.len()..]))
        .min_by_key(|after| after.len())
        .unwrap_or("");
    after_last_closer.trim().is_empty().then_some(tail)
}

/// `<function=NAME>`, or `<tool_call>` then such a tag or a JSON object.
fn looks_like_call(markup: &str) -> bool {
    if let Some(body) = markup.strip_prefix(OPEN) {
        let body = body.trim_start();
        return body.starts_with('{') || looks_like_call(body);
    }
    let Some(rest) = markup.strip_prefix(FUNCTION) else {
        return false;
    };
    let name_end = rest.find(|c: char| !(c.is_alphanumeric() || "_-.:".contains(c)));
    name_end.is_some_and(|end| end > 0 && rest[end..].starts_with('>'))
}

/// Whether call markup ends `text`, so a step should read it rather than drop it.
pub fn ends_in_call(text: &str) -> bool {
    trailing_markup(text).is_some()
}

/// The note for a model whose call markup ends `text` but was not recovered, naming the
/// closest offered tool when it called one that does not exist. `None` when no call
/// markup ends the text, so prose that shows the markup gets no note.
pub fn stranded(text: &str, tools: &[ToolSpec]) -> Option<String> {
    let tail = trailing_markup(text)?;
    let mut note = String::from("You wrote a tool call as text, so it did not run.");
    if let Some(name) = called_name(tail).filter(|name| !tools.iter().any(|t| t.name == *name)) {
        note.push_str(&format!(" There is no tool named {name}"));
        if let Some(closest) = closest(&name, tools) {
            note.push_str(&format!("; the closest is {closest}"));
        }
        note.push('.');
    }
    note.push_str(" Call a tool through the tool interface, not as text.");
    Some(note)
}

/// The tool name in call markup, short enough to quote back.
fn called_name(markup: &str) -> Option<String> {
    let name = match markup.find(FUNCTION) {
        Some(at) => {
            let rest = &markup[at + FUNCTION.len()..];
            let end = rest.find(|c: char| c == '>' || c == '<' || c.is_whitespace());
            rest[..end.unwrap_or(rest.len())].to_owned()
        }
        None => {
            let body = markup.strip_prefix(OPEN)?;
            let body = body.split_once(CLOSE).map_or(body, |(body, _)| body);
            let call: Map<String, Value> = serde_json::from_str(body.trim()).ok()?;
            call.get("name")?.as_str()?.to_owned()
        }
    };
    let name: String = name.trim().chars().take(64).collect();
    (!name.is_empty()).then_some(name)
}

/// The offered tool whose name is fewest edits away, if it is near enough to be meant.
fn closest<'a>(name: &str, tools: &'a [ToolSpec]) -> Option<&'a str> {
    let (distance, tool) = tools
        .iter()
        .map(|tool| (edits(name, &tool.name), tool.name.as_str()))
        .min_by_key(|(distance, _)| *distance)?;
    (distance <= name.chars().count().div_ceil(2)).then_some(tool)
}

/// Levenshtein distance over chars.
fn edits(a: &str, b: &str) -> usize {
    let b: Vec<char> = b.chars().collect();
    let mut row: Vec<usize> = (0..=b.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut diagonal = row[0];
        row[0] = i + 1;
        for (j, cb) in b.iter().enumerate() {
            let above = row[j + 1];
            row[j + 1] = (above + 1)
                .min(row[j] + 1)
                .min(diagonal + usize::from(ca != *cb));
            diagonal = above;
        }
    }
    row[b.len()]
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

    fn named(names: &[&str]) -> Vec<ToolSpec> {
        let schema = json!({ "type": "object", "properties": { "search": { "type": "string" } } });
        names
            .iter()
            .map(|name| ToolSpec {
                name: (*name).into(),
                description: String::new(),
                parameters: schema.clone(),
            })
            .collect()
    }

    #[test]
    fn missing_inner_closers_are_forgiven_inside_a_closed_block() {
        let history = named(&["read_history"]);
        let text =
            "<tool_call> <function=read_history> <parameter=search> panel-web —   </tool_call>";
        let (rest, calls) = recover(text, &history).unwrap();
        assert_eq!(rest, "");
        assert_eq!(calls[0].name, "read_history");
        assert_eq!(calls[0].arguments, json!({ "search": "panel-web —" }));

        let text = "<tool_call>\n<function=write_file>\n<parameter=path>\nrun.py\n\
            <parameter=content>\nx\n</function>\n</tool_call>";
        let (_, calls) = recover(text, &tools()).unwrap();
        assert_eq!(
            calls[0].arguments,
            json!({ "path": "run.py", "content": "x" })
        );
        // Without </tool_call> the call may be cut off: only a closer ends a value.
        let open = "<tool_call>\n<function=write_file>\n<parameter=path>\na\n</function>";
        assert!(recover(open, &tools()).is_some());
        let cut = "<tool_call>\n<function=write_file>\n<parameter=path>\na\n<parameter=content>\nb";
        assert!(recover(cut, &tools()).is_none());
    }

    #[test]
    fn well_formed_values_may_hold_tags() {
        let text = "<tool_call>\n<function=write_file>\n<parameter=path>\np.rs\n</parameter>\n\
            <parameter=content>\nlet a = \"</function>\";\nlet b = \"<parameter=x>\";\n\
            </parameter>\n</function>\n</tool_call>";
        let (_, calls) = recover(text, &tools()).unwrap();
        assert_eq!(
            calls[0].arguments,
            json!({
                "path": "p.rs",
                "content": "let a = \"</function>\";\nlet b = \"<parameter=x>\";"
            })
        );
    }

    #[test]
    fn stranded_markup_gets_a_note_naming_the_closest_tool() {
        let tools = named(&["read_history", "read_file", "run_command"]);
        let text = "Let me look.\n<tool_call> <function=run_history> <parameter=search> panel-web —   </tool_call>";
        assert!(recover(text, &tools).is_none());
        assert_eq!(
            stranded(text, &tools).unwrap(),
            "You wrote a tool call as text, so it did not run. There is no tool \
             named run_history; the closest is read_history. Call a tool through the tool \
             interface, not as text."
        );
        let json_call = "<tool_call>{\"name\": \"zzzzzzzz\", \"arguments\": {}}</tool_call>";
        assert_eq!(
            stranded(json_call, &tools).unwrap(),
            "You wrote a tool call as text, so it did not run. There is no tool \
             named zzzzzzzz. Call a tool through the tool interface, not as text."
        );
        let cut = "<tool_call>\n<function=read_file>\n<parameter=search>\na";
        assert_eq!(
            stranded(cut, &tools).unwrap(),
            "You wrote a tool call as text, so it did not run. Call a tool \
             through the tool interface, not as text."
        );
        let shown = "Write <tool_call>{\"name\": \"x\"}</tool_call> like this.";
        assert!(stranded(shown, &tools).is_none());
        assert!(stranded("No markup here.", &tools).is_none());
        assert!(stranded("<function=x>", &[]).is_some());
        // Prose that mentions the tags is not a call.
        let mention = "I changed the parser to handle `<function=` blocks.";
        assert!(stranded(mention, &tools).is_none());
        let inline = "Done. It now reads <tool_call> and <function=name> tags";
        assert!(stranded(inline, &tools).is_none());
        assert!(stranded("<function= is a tag", &tools).is_none());
    }

    proptest::proptest! {
        #[test]
        fn arbitrary_markup_never_panics(
            pieces in proptest::collection::vec(
                proptest::prop_oneof![
                    proptest::strategy::Just("<tool_call>".to_owned()),
                    proptest::strategy::Just("</tool_call>".to_owned()),
                    proptest::strategy::Just("<function=write_file>".to_owned()),
                    proptest::strategy::Just("</function>".to_owned()),
                    proptest::strategy::Just("<parameter=path>".to_owned()),
                    proptest::strategy::Just("</parameter>".to_owned()),
                    ".{0,6}",
                ],
                0..12,
            ),
        ) {
            let text = pieces.concat();
            let _ = recover(&text, &tools());
            let _ = stranded(&text, &tools());
        }
    }
}
