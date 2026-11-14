//! The wire between the core and a platform: lines of JSON, one message each.
//!
//! They talk in lines of JSON. The core sends what the user should see:
//!
//! - `{"type": "answer", "text": "…"}` — the agent's final answer, in Markdown;
//! - `{"type": "question", "question": "…", "options": […], "multiple": false}`;
//! - `{"type": "approval", "id": 3, "text": "the agent wants to run: …"}`;
//! - `{"type": "reply", "text": "…"}` — a short answer to a command, or a note;
//! - `{"type": "menu", "command": "mode", "title": "…", "options": […]}`;
//! - `{"type": "image", "path": "/…", "mime_type": "image/png", "caption": "…"}` — an
//!   image card the agent showed, its file to send on;
//! - `{"type": "working", "on": true}`.
//!
//! The platform sends what the user did:
//!
//! - `{"type": "message", "text": "…"}` — a message, or the answer to an open question;
//! - `{"type": "chosen", "options": [0, 2]}` — options picked for the open question;
//! - `{"type": "approval", "id": 3, "allow": true}`;
//! - `{"type": "command", "name": "status", "argument": null}`, where the name is one of
//!   stop, resume, status, approve, deny, mode, model, compact, goal and new;
//! - `{"type": "status", "text": "…"}` — what `/settings` shows about it, such as the code
//!   to pair a bot with.

use serde::{Deserialize, Serialize};

/// A line longer than this is no message of the protocol.
pub(super) const MAX_LINE_BYTES: usize = 1024 * 1024;

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(super) enum ToPlatform<'a> {
    Answer {
        text: &'a str,
    },
    Question {
        question: &'a str,
        options: &'a [String],
        multiple: bool,
    },
    Approval {
        id: u64,
        text: String,
    },
    Reply {
        text: &'a str,
    },
    Menu {
        command: &'a str,
        title: String,
        options: Vec<String>,
    },
    Image {
        path: String,
        mime_type: &'a str,
        caption: Option<String>,
    },
    Working {
        on: bool,
    },
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum FromPlatform {
    Message {
        text: String,
    },
    Chosen {
        options: Vec<usize>,
    },
    Approval {
        id: u64,
        allow: bool,
    },
    Command {
        name: String,
        argument: Option<String>,
    },
    Status {
        text: String,
    },
}
