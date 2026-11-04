//! The Messages request body, built from the conversation.

use serde::Serialize;
use serde_json::{Value, json};

use crate::{
    config::Effort,
    providers::{
        output::ToolCall,
        request::{ChatMessage, ChatRequest, Picture, ToolSpec},
    },
};

/// Messages requires a cap; this one every current model takes, for a model whose
/// catalogue entry gives none.
const DEFAULT_MAX_TOKENS: u64 = 32_000;

#[derive(Serialize)]
pub struct MessagesRequest {
    model: String,
    max_tokens: u64,
    #[serde(skip_serializing_if = "String::is_empty")]
    system: String,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool>,
    thinking: Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    output_config: Option<Value>,
    /// Caches the longest prefix the API can; every request repeats the last one.
    cache_control: Value,
    stream: bool,
}

#[derive(Serialize)]
struct WireMessage {
    role: &'static str,
    content: Vec<Value>,
}

#[derive(Serialize)]
struct WireTool {
    name: String,
    description: String,
    input_schema: Value,
}

impl MessagesRequest {
    pub fn new(request: ChatRequest<'_>) -> Self {
        let (system, messages) = wire_messages(request.messages);
        // Off is the only level without thinking; the rest set how much of it.
        let (thinking, effort) = match request.effort {
            Some(Effort::Off) => (json!({"type": "disabled"}), None),
            effort => (
                json!({"type": "adaptive", "display": "summarized"}),
                effort.map(wire_effort),
            ),
        };
        Self {
            model: request.model.to_owned(),
            max_tokens: request.max_output_tokens.unwrap_or(DEFAULT_MAX_TOKENS),
            system,
            messages,
            tools: request.tools.iter().map(WireTool::from).collect(),
            thinking,
            output_config: effort.map(|effort| json!({ "effort": effort })),
            cache_control: json!({"type": "ephemeral"}),
            stream: true,
        }
    }
}

fn wire_effort(effort: Effort) -> &'static str {
    match effort {
        Effort::Off | Effort::Minimal | Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::Xhigh => "xhigh",
        Effort::Max | Effort::Ultra => "max",
    }
}

/// The leading system messages become the system prompt; a later one joins the user's
/// turn as text. Turns of one role in a row are one message, so the tool results of a
/// step arrive together, as the API wants them.
fn wire_messages(messages: &[ChatMessage]) -> (String, Vec<WireMessage>) {
    let mut system = Vec::new();
    let mut wire: Vec<WireMessage> = Vec::new();
    for message in messages {
        let (role, blocks) = match message {
            ChatMessage::System(text) if wire.is_empty() => {
                system.push(text.as_str());
                continue;
            }
            ChatMessage::System(text) => ("user", text_block(&format!("[system] {text}"))),
            ChatMessage::User(text) => ("user", text_block(text)),
            ChatMessage::UserWithPictures { text, pictures } => {
                let mut blocks = text_block(text);
                blocks.extend(pictures.iter().map(image_block));
                ("user", blocks)
            }
            ChatMessage::Assistant { text, tool_calls } => {
                let mut blocks = text_block(text);
                blocks.extend(tool_calls.iter().map(tool_use_block));
                ("assistant", blocks)
            }
            ChatMessage::Tool {
                call_id,
                content,
                pictures,
            } => {
                // A result with pictures carries them as blocks after its text.
                let content = match pictures.is_empty() {
                    true => json!(content),
                    false => {
                        let text = json!({"type": "text", "text": content});
                        let images = pictures.iter().map(image_block);
                        Value::Array([text].into_iter().chain(images).collect())
                    }
                };
                (
                    "user",
                    vec![json!({
                        "type": "tool_result",
                        "tool_use_id": call_id,
                        "content": content,
                    })],
                )
            }
        };
        if blocks.is_empty() {
            continue;
        }
        match wire.last_mut() {
            Some(last) if last.role == role => last.content.extend(blocks),
            _ => wire.push(WireMessage {
                role,
                content: blocks,
            }),
        }
    }
    (system.join("\n\n"), wire)
}

fn image_block(picture: &Picture) -> Value {
    json!({
        "type": "image",
        "source": {
            "type": "base64",
            "media_type": picture.mime_type,
            "data": picture.base64,
        },
    })
}

/// The API refuses empty text blocks.
fn text_block(text: &str) -> Vec<Value> {
    match text.trim().is_empty() {
        true => Vec::new(),
        false => vec![json!({"type": "text", "text": text})],
    }
}

fn tool_use_block(call: &ToolCall) -> Value {
    // Input must be an object; arguments that were not JSON go back as none.
    let input = match &call.arguments {
        Value::Object(_) => call.arguments.clone(),
        _ => json!({}),
    };
    json!({"type": "tool_use", "id": call.id, "name": call.name, "input": input})
}

impl From<&ToolSpec> for WireTool {
    fn from(tool: &ToolSpec) -> Self {
        Self {
            name: tool.name.clone(),
            description: tool.description.clone(),
            input_schema: tool.parameters.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::providers::Picture;

    #[test]
    fn history_becomes_alternating_turns_with_a_system_prompt() {
        let call = ToolCall {
            id: "toolu_1".into(),
            name: "read_file".into(),
            arguments: json!({"path": "a"}),
        };
        let messages = [
            ChatMessage::System("be brief".into()),
            ChatMessage::User("look".into()),
            ChatMessage::Assistant {
                text: String::new(),
                tool_calls: vec![call],
            },
            ChatMessage::Tool {
                call_id: "toolu_1".into(),
                content: "text".into(),
                pictures: Vec::new(),
            },
            ChatMessage::UserWithPictures {
                text: "[sources]".into(),
                pictures: vec![Picture {
                    mime_type: "image/png",
                    base64: "AA==".into(),
                }],
            },
            ChatMessage::System("summarize".into()),
        ];
        let (system, wire) = wire_messages(&messages);
        assert_eq!(system, "be brief");
        assert_eq!(
            serde_json::to_value(&wire).unwrap(),
            json!([
                {"role": "user", "content": [{"type": "text", "text": "look"}]},
                {"role": "assistant", "content": [
                    {"type": "tool_use", "id": "toolu_1", "name": "read_file", "input": {"path": "a"}}
                ]},
                {"role": "user", "content": [
                    {"type": "tool_result", "tool_use_id": "toolu_1", "content": "text"},
                    {"type": "text", "text": "[sources]"},
                    {"type": "image", "source": {"type": "base64", "media_type": "image/png", "data": "AA=="}},
                    {"type": "text", "text": "[system] summarize"}
                ]}
            ])
        );
    }
}
