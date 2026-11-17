//! What the agent does in a thread: its turns with their tool calls, the tokens each
//! model call used, the task board and the background jobs.

mod failure;
mod jobs;
mod resume;
mod tasks;
mod tokens;
mod turns;
mod waits;

pub use failure::{ProviderFailure, TurnFailure};
pub use jobs::{BackgroundJob, JobSummary};
pub use resume::{CHECK_NOTE, CUT_OFF, Settle, Unfinished};
pub use tasks::Task;
pub use tokens::{CallPurpose, LastPrompt, ModelCall};
pub use turns::{Turn, TurnAnswer, TurnOutcome, TurnStatus};

use super::{Message, State, conversation::messages, timestamp_ms};
