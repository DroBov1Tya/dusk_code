//! What happens in a thread: the events it tells a client, its messages, turns, tasks,
//! token counts, goal and context, and the steps of the transcript a client folds them into.

mod context;
mod event;
mod goal;
mod job;
mod message;
mod name;
mod summary;
mod task;
mod tokens;
mod transcript;
mod turn;

pub use context::{ContextItemDetail, ContextReport, ContextSection, ContextSource};
pub use event::ThreadEvent;
pub use goal::{Goal, GoalCheck, GoalCriterion, GoalEvidence, GoalStatus};
pub use job::JobInfo;
pub use message::{
    Compaction, LocalUnsent, Message, MessageRole, Part, Source, UndoPoint, has_pasted, plain,
    source_tag, split, wrap,
};
pub use name::{ARBITER_THREAD, agent_name, fnv1a, name_seed};
pub use summary::{Found, Thread};
pub use task::{Task, TaskItem};
pub use tokens::{AUTO_COMPACT_PERCENT, ThreadTokens, TokenCounts};
pub use transcript::{
    AgentState, DETAIL_KEYS, Note, SHOWN_CHARS, Tool, ToolState, clip, note, tool_detail,
};
pub use turn::{Turn, TurnStatus, duration_words};
