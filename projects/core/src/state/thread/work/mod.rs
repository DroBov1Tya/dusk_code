//! What the agent does in a thread: its turns with their tool calls, the tokens each
//! model call used, the task board and the background jobs.

mod jobs;
mod tasks;
mod tokens;
mod turns;

pub use jobs::{BackgroundJob, JobSummary};
pub use tasks::{Task, TaskItem};
pub use tokens::{CallPurpose, LastPrompt, ModelCall, ThreadTokens, TokenCounts};
pub use turns::{Turn, TurnAnswer, TurnOutcome, TurnStatus};

use super::{Message, State, conversation::messages, timestamp_ms};
