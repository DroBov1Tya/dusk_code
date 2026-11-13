//! Opening, feeding and closing threads.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::config::ApprovalPolicy;
use crate::tools::Decision;
/// Opening, feeding and closing threads. Events of an open thread arrive as
/// `Event::Thread` until `Event::Ended`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ThreadRequest {
    New {
        cwd: PathBuf,
    },
    /// A new chat that is never written to disk: its messages stay in memory and its
    /// files in the system's temporary folder, and all of it goes when the core stops.
    NewTemporary {
        cwd: PathBuf,
    },
    Resume {
        thread_id: String,
        cwd: PathBuf,
    },
    /// A new thread that starts from a /fork summary.
    Fork {
        summary: String,
        cwd: PathBuf,
    },
    List,
    /// Searches what was said in every chat; at least three characters.
    Search {
        text: String,
    },
    Messages {
        thread_id: String,
    },
    Tasks {
        thread_id: String,
    },
    LocalUnsent {
        thread_id: String,
    },
    ResendLocalUnsent {
        thread_id: String,
        id: String,
    },
    /// Keeps long pastes and named pictures among the thread's sources, then queues the
    /// message; `now` asks for it before what is queued.
    Send {
        thread_id: String,
        text: String,
        now: bool,
    },
    Cancel {
        thread_id: String,
    },
    /// Sends a command the agent runs in the foreground, by its tool call, to the
    /// background: the agent goes on and gets the result when it ends.
    Background {
        thread_id: String,
        call_id: String,
    },
    /// Runs a command the user confirmed, in the chat's folder; answers with its output.
    RunCommand {
        thread_id: String,
        command: String,
    },
    /// Stops the thread's turn and waits for it to end, so it can be opened again.
    Close {
        thread_id: String,
    },
    StopAgent {
        thread_id: String,
    },
    Rename {
        thread_id: String,
        title: String,
    },
    /// Erases the chat, its agents' threads and its files folder for good.
    Delete {
        thread_id: String,
    },
    /// Answers a tool's request for a yes; `false` when it no longer waits.
    Approve {
        id: u64,
        decision: Decision,
    },
    /// The thread's own approval mode; `None` goes back to the default in settings.
    SetApprovals {
        thread_id: String,
        policy: Option<ApprovalPolicy>,
    },
    /// Answers with the thread's own approval mode, or none when it uses the default.
    GetApprovals {
        thread_id: String,
    },
}
