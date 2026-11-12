//! The `generateContent` request body, built from the conversation.

use std::collections::HashMap;

use serde::Serialize;
use serde_json::{Map, Value, json};

use super::signatures::{PLACEHOLDER, Signatures};
use crate::{
    config::Effort,
    providers::request::{ChatMessage, ChatRequest, Picture, ToolSpec},
};

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GenerateRequest {
    #[serde(skip_serializing_if = "Option::is_none")]
    system_instruction: Option<Value>,
    contents: Vec<Content>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Value>,
    #[serde(skip_serializing_if = "Map::is_empty")]
    generation_config: Map<String, Value>,
}

#[derive(Serialize)]
struct Content {
    role: &'static str,
    parts: Vec<Value>,
}

impl GenerateRequest {
    /// `thinks` is false for a model whose catalogue entry says it cannot think: such a
    /// model refuses a thinking config.
    pub fn new(request: ChatRequest<'_>, signatures: &Signatures, thinks: bool) -> Self {
        let levels = takes_levels(request.model);
        let (system, contents) = contents(request.messages, signatures, levels);
        let mut config = Map::new();
        if let Some(limit) = request.max_output_tokens {
            config.insert("maxOutputTokens".into(), json!(limit));
        }
        if thinks {
            let mut thinking = json!({"includeThoughts": true});
            if let Some(level) = request.effort.and_then(level).filter(|_| levels) {
                thinking["thinkingLevel"] = json!(level);
            }
            config.insert("thinkingConfig".into(), thinking);
        }
        let declarations: Vec<Value> = request.tools.iter().map(declaration).collect();
        Self {
            system_instruction: (!system.is_empty()).then(|| json!({"parts": [{"text": system}]})),
            contents,
            tools: match declarations.is_empty() {
                true => Vec::new(),
                false => vec![json!({ "functionDeclarations": declarations })],
            },
            generation_config: config,
        }
    }
}

/// Gemini 3 and later take a thinking level and check thought signatures; the 2.x models
/// refuse a level and know nothing of the placeholder signature.
fn takes_levels(model: &str) -> bool {
    model.starts_with("gemini-") && !model.starts_with("gemini-1") && !model.starts_with("gemini-2")
}

/// Only `low` and `high` are taken by every model that takes levels, and none of them
/// turns thinking off; the middle is the model's own default.
fn level(effort: Effort) -> Option<&'static str> {
    match effort {
        Effort::Off | Effort::Minimal | Effort::Low => Some("low"),
        Effort::Medium => None,
        Effort::High | Effort::Xhigh | Effort::Max | Effort::Ultra => Some("high"),
    }
}

/// The leading system messages become the system instruction; a later one joins the
/// user's turn as text. Turns of one role in a row are one content, so the results of
/// a step's calls arrive together, as the API wants them.
fn contents(
    messages: &[ChatMessage],
    signatures: &Signatures,
    placeholder: bool,
) -> (String, Vec<Content>) {
    let mut system = Vec::new();
    let mut wire: Vec<Content> = Vec::new();
    // A function response names its function; the history names only the call.
    let mut names: HashMap<&str, &str> = HashMap::new();
    for message in messages {
        let (role, parts) = match message {
            ChatMessage::System(text) if wire.is_empty() => {
                system.push(text.as_str());
                continue;
            }
            ChatMessage::System(text) => ("user", text_part(&format!("[system] {text}"))),
            ChatMessage::User(text) => ("user", text_part(text)),
            ChatMessage::UserWithPictures { text, pictures } => {
                let mut parts = text_part(text);
                parts.extend(pictures.iter().map(picture_part));
                ("user", parts)
            }
            ChatMessage::Assistant { text, tool_calls } => {
                let mut parts = text_part(text);
                for (index, call) in tool_calls.iter().enumerate() {
                    names.insert(&call.id, &call.name);
                    // Arguments must be an object; ones that were not JSON go back as none.
                    let args = match &call.arguments {
                        Value::Object(_) => call.arguments.clone(),
                        _ => json!({}),
                    };
                    let mut part = json!({
                        "functionCall": {"id": call.id, "name": call.name, "args": args},
                    });
                    // The first call of a step must carry a signature.
                    let signature = signatures
                        .of(&call.id)
                        .or_else(|| (index == 0 && placeholder).then(|| PLACEHOLDER.to_owned()));
                    if let Some(signature) = signature {
                        part["thoughtSignature"] = json!(signature);
                    }
                    parts.push(part);
                }
                ("model", parts)
            }
            ChatMessage::Tool {
                call_id,
                content,
                pictures,
            } => {
                let mut parts = match names.get(call_id.as_str()) {
                    Some(name) => vec![json!({
                        "functionResponse": {
                            "id": call_id,
                            "name": name,
                            "response": {"result": content},
                        },
                    })],
                    // A result whose call is gone from the history is still worth reading.
                    None => text_part(&format!("[tool result] {content}")),
                };
                // Plain parts after the result: every model reads those.
                parts.extend(pictures.iter().map(picture_part));
                ("user", parts)
            }
        };
        if parts.is_empty() {
            continue;
        }
        match wire.last_mut() {
            Some(last) if last.role == role => last.parts.extend(parts),
            _ => wire.push(Content { role, parts }),
        }
    }
    (system.join("\n\n"), wire)
}

fn picture_part(picture: &Picture) -> Value {
    json!({"inlineData": {"mimeType": picture.mime_type, "data": picture.base64}})
}

fn text_part(text: &str) -> Vec<Value> {
    match text.trim().is_empty() {
        true => Vec::new(),
        false => vec![json!({ "text": text })],
    }
}

/// `parametersJsonSchema` takes JSON Schema as it is, so nothing is rewritten.
fn declaration(tool: &ToolSpec) -> Value {
    json!({
        "name": tool.name,
        "description": tool.description,
        "parametersJsonSchema": tool.parameters,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::{Picture, ToolCall};

    fn call(id: &str, path: &str) -> ToolCall {
        ToolCall {
            id: id.into(),
            name: "read_file".into(),
            arguments: json!({ "path": path }),
        }
    }

    #[test]
    fn a_conversation_with_calls_and_results_becomes_contents() {
        let signatures = Signatures::default();
        signatures.remember("c1", "SIG".into());
        let messages = [
            ChatMessage::System("be brief".into()),
            ChatMessage::User("look".into()),
            ChatMessage::Assistant {
                text: "Reading.".into(),
                tool_calls: vec![call("c1", "a"), call("c2", "b")],
            },
            ChatMessage::Tool {
                call_id: "c1".into(),
                content: "one".into(),
                pictures: Vec::new(),
            },
            ChatMessage::Tool {
                call_id: "c2".into(),
                content: "two".into(),
                pictures: vec![Picture {
                    mime_type: "image/png",
                    base64: "AA==".into(),
                }],
            },
            ChatMessage::Assistant {
                text: String::new(),
                tool_calls: vec![call("old", "c")],
            },
            ChatMessage::Tool {
                call_id: "gone".into(),
                content: "lost".into(),
                pictures: Vec::new(),
            },
        ];
        let tools = [ToolSpec {
            name: "read_file".into(),
            description: "Reads a file".into(),
            parameters: json!({"type": "object", "additionalProperties": false}),
        }];
        let request = ChatRequest {
            model: "gemini-3-flash-preview",
            messages: &messages,
            tools: &tools,
            max_output_tokens: Some(1000),
            effort: Some(Effort::Low),
            cache_key: None,
        };
        let body = serde_json::to_value(GenerateRequest::new(request, &signatures, true)).unwrap();
        let read =
            |id: &str, path: &str| json!({"id": id, "name": "read_file", "args": {"path": path}});
        assert_eq!(
            body,
            json!({
                "systemInstruction": {"parts": [{"text": "be brief"}]},
                "contents": [
                    {"role": "user", "parts": [{"text": "look"}]},
                    {"role": "model", "parts": [
                        {"text": "Reading."},
                        {"functionCall": read("c1", "a"), "thoughtSignature": "SIG"},
                        {"functionCall": read("c2", "b")},
                    ]},
                    {"role": "user", "parts": [
                        {"functionResponse": {"id": "c1", "name": "read_file", "response": {"result": "one"}}},
                        {"functionResponse": {"id": "c2", "name": "read_file", "response": {"result": "two"}}},
                        {"inlineData": {"mimeType": "image/png", "data": "AA=="}},
                    ]},
                    {"role": "model", "parts": [
                        {"functionCall": read("old", "c"), "thoughtSignature": PLACEHOLDER},
                    ]},
                    {"role": "user", "parts": [{"text": "[tool result] lost"}]},
                ],
                "tools": [{"functionDeclarations": [{
                    "name": "read_file",
                    "description": "Reads a file",
                    "parametersJsonSchema": {"type": "object", "additionalProperties": false},
                }]}],
                "generationConfig": {
                    "maxOutputTokens": 1000,
                    "thinkingConfig": {"includeThoughts": true, "thinkingLevel": "low"},
                },
            })
        );
    }

    #[test]
    fn an_older_model_gets_no_level_and_no_placeholder() {
        let messages = [
            ChatMessage::User("look".into()),
            ChatMessage::Assistant {
                text: String::new(),
                tool_calls: vec![call("c1", "a")],
            },
        ];
        let request = ChatRequest {
            model: "gemini-2.5-flash",
            messages: &messages,
            tools: &[],
            max_output_tokens: None,
            effort: Some(Effort::High),
            cache_key: None,
        };
        let body =
            serde_json::to_value(GenerateRequest::new(request, &Signatures::default(), true))
                .unwrap();
        assert_eq!(
            body["generationConfig"],
            json!({"thinkingConfig": {"includeThoughts": true}})
        );
        assert!(
            body["contents"][1]["parts"][0]
                .get("thoughtSignature")
                .is_none()
        );
        let plain =
            serde_json::to_value(GenerateRequest::new(request, &Signatures::default(), false))
                .unwrap();
        assert!(plain.get("generationConfig").is_none());
    }
}
