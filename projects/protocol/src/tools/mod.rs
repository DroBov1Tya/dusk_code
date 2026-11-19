//! What tools hand back and ask for: results, approvals, questions and cards.

mod answered;
mod approval;
mod card;
mod phrase;
mod question;
mod result;

pub use answered::{Answered, Asked};
pub use approval::{ApprovalRequest, ApprovalStep, Decision, Request};
pub use card::{Card, CardKind};
pub use phrase::{is_agent_tool, phrase};
pub use question::{
    DECLINED, MAX_OPTION_CHARS, MAX_OPTIONS, MAX_QUESTION_CHARS, MIN_OPTIONS, Question, SKIPPED,
};
pub use result::{ToolResult, WindowAccess};
