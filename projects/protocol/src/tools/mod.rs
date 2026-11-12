//! What tools hand back and ask for: results, approvals, questions and cards.

mod approval;
mod card;
mod phrase;
mod question;
mod result;

pub use approval::{ApprovalRequest, Decision, Request};
pub use card::{Card, CardKind};
pub use phrase::{is_agent_tool, phrase};
pub use question::{MAX_OPTION_CHARS, MAX_OPTIONS, MAX_QUESTION_CHARS, MIN_OPTIONS, Question};
pub use result::{ToolResult, WindowAccess};
