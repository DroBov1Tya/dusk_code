//! The Chat Completions request body, built from the conversation.

use serde::Serialize;
use serde_json::Value;

use crate::{
    config::Effort,
    providers::{
        chat::{ChatMessage, ChatRequest, Picture, ToolSpec},
        model_output::ToolCall,
    },
};

/// copying costs nothing next to the network call.
#[derive(Serialize)]
pub(super) struct CompletionRequest {
    model: String,
    messages: Vec<WireMessage>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<WireTool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_completion_tokens: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<&'static str>,
    stream: bool,
    stream_options: StreamOptions,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum WireMessage {
    System {
        content: String,
    },
    User {
        content: String,
    },
    #[serde(rename = "user")]
    UserParts {
        content: Vec<WirePart>,
    },
    Assistant {
        content: String,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<WireToolCall>,
    },
    Tool {
        tool_call_id: String,
        content: String,
    },
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum WirePart {
    Text { text: String },
    ImageUrl { image_url: WireImageUrl },
}

#[derive(Serialize)]
struct WireImageUrl {
    url: String,
}

#[derive(Serialize)]
struct WireToolCall {
    id: String,
    r#type: &'static str,
    function: WireFunctionCall,
}

#[derive(Serialize)]
struct WireFunctionCall {
    name: String,
    /// The API carries arguments as a JSON string, not an object.
    arguments: String,
}

#[derive(Serialize)]
struct WireTool {
    r#type: &'static str,
    function: WireFunction,
}

#[derive(Serialize)]
struct WireFunction {
    name: String,
    description: String,
    parameters: Value,
}

impl CompletionRequest {
    pub(super) fn new(request: ChatRequest<'_>, completion_tokens: bool) -> Self {
        let limit = Some(request.max_output_tokens);
        Self {
            model: request.model.to_owned(),
            messages: wire_messages(request.messages),
            tools: request.tools.iter().map(WireTool::from).collect(),
            max_tokens: limit.filter(|_| !completion_tokens),
            max_completion_tokens: limit.filter(|_| completion_tokens),
            reasoning_effort: request.effort.map(Effort::wire_name),
            stream: true,
            stream_options: StreamOptions {
                include_usage: true,
            },
        }
    }
}

/// Chat Completions takes no pictures in a tool message: those of a step's tool results
/// follow them as one user message.
fn wire_messages(messages: &[ChatMessage]) -> Vec<WireMessage> {
    let mut wire = Vec::new();
    let mut pending = Vec::new();
    for message in messages {
        let is_tool = matches!(message, ChatMessage::Tool { .. });
        if !is_tool && !pending.is_empty() {
            wire.push(tool_pictures(std::mem::take(&mut pending)));
        }
        if let ChatMessage::Tool { pictures, .. } = message {
            pending.extend(pictures.iter().map(image_part));
        }
        wire.push(WireMessage::from(message));
    }
    if !pending.is_empty() {
        wire.push(tool_pictures(pending));
    }
    wire
}

fn tool_pictures(pictures: Vec<WirePart>) -> WireMessage {
    let text = WirePart::Text {
        text: "[the pictures the tool calls above returned]".into(),
    };
    WireMessage::UserParts {
        content: [text].into_iter().chain(pictures).collect(),
    }
}

fn image_part(picture: &Picture) -> WirePart {
    WirePart::ImageUrl {
        image_url: WireImageUrl {
            url: format!("data:{};base64,{}", picture.mime_type, picture.base64),
        },
    }
}

impl From<&ChatMessage> for WireMessage {
    fn from(message: &ChatMessage) -> Self {
        match message.clone() {
            ChatMessage::System(content) => Self::System { content },
            ChatMessage::User(content) => Self::User { content },
            ChatMessage::UserWithPictures { text, pictures } => {
                let pictures = pictures.iter().map(image_part);
                Self::UserParts {
                    content: [WirePart::Text { text }]
                        .into_iter()
                        .chain(pictures)
                        .collect(),
                }
            }
            ChatMessage::Assistant { text, tool_calls } => Self::Assistant {
                content: text,
                tool_calls: tool_calls.iter().map(WireToolCall::from).collect(),
            },
            ChatMessage::Tool {
                call_id, content, ..
            } => Self::Tool {
                tool_call_id: call_id,
                content,
            },
        }
    }
}

impl From<&ToolCall> for WireToolCall {
    fn from(call: &ToolCall) -> Self {
        Self {
            id: call.id.clone(),
            r#type: "function",
            function: WireFunctionCall {
                name: call.name.clone(),
                // Arguments that were not JSON go back as the model wrote them.
                arguments: match &call.arguments {
                    Value::String(raw) => raw.clone(),
                    arguments => arguments.to_string(),
                },
            },
        }
    }
}

impl From<&ToolSpec> for WireTool {
    fn from(tool: &ToolSpec) -> Self {
        Self {
            r#type: "function",
            function: WireFunction {
                name: tool.name.clone(),
                description: tool.description.clone(),
                parameters: tool.parameters.clone(),
            },
        }
    }
}
