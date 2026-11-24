//! `fake=diagrams`: a chat with a diagram of each kind, made-up data only, to look at
//! them without a model, each read from a `draw_diagram` call as a chat reads it; `fake=diagram-KIND` shows just the one (`flow`, `architecture`,
//! `sequence`, `timeline`, `tree`, `state`).

use dusk_protocol::{
    model::ToolCall,
    thread::{Block, DIAGRAM_TOOL},
};
use gpui_kit::*;

use crate::{
    pages::chat::transcript::{Item, Transcript},
    workspace::{Center, Workspace},
};

/// The diagrams, by name, as the model would give them to `draw_diagram`.
const DIAGRAMS: &str = include_str!("diagrams.json");

pub fn fake(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    let only = value.strip_prefix("diagram-");
    let samples: Vec<serde_json::Value> =
        serde_json::from_str(DIAGRAMS).expect("the made-up diagrams are JSON");
    let mut items = vec![Item::User {
        text: "Draw me how it fits together.".into(),
        sequence: Some(1),
        answered: Vec::new(),
    }];
    for sample in samples {
        let name = sample["name"].as_str().unwrap_or_default();
        if only.is_some_and(|only| only != name) {
            continue;
        }
        let call = ToolCall {
            id: name.to_owned(),
            name: DIAGRAM_TOOL.into(),
            arguments: sample["diagram"].clone(),
        };
        let block = Block::asked(&call).expect("a made-up diagram is within caps");
        items.push(Item::Assistant {
            text: format!("The {name}, as a diagram:"),
            reasoning: String::new(),
        });
        items.push(Item::Block(block));
    }
    let mut chat = Transcript::default();
    chat.extend(items, 0);
    ws.chats.insert("shot".into(), chat);
    ws.shown = Some("shot".into());
    ws.center = Center::Chat;
    // A home without a provider would show the welcome over the chat.
    ws.welcome.page = None;
    cx.notify();
}
