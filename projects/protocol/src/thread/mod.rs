//! What happens in a thread: the events it tells a client, its messages, turns, tasks,
//! token counts, goal and context, the blocks it shows, and the steps of the transcript a
//! client folds them into.

mod block;
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

pub use block::{
    Block, CARD_LINE_CHARS, CARD_TITLE_CHARS, CHECK_DETAIL_CHARS, CHECK_NAME_CHARS, CHOICE_CHARS,
    Check, CheckState, Checks, Choices, Counts, Flow, FlowCard, LAYER_SUBTITLE_CHARS,
    LAYER_TITLE_CHARS, Layer, Line, MAX_CARDS, MAX_CHECKS, MAX_CHOICES, MAX_LAYERS, MIN_CHOICES,
    MIN_LAYERS, SHOW_TOOL, Show,
};
pub use context::{ContextItemDetail, ContextReport, ContextSection, ContextSource};
pub use event::ThreadEvent;
pub use goal::{Goal, GoalCheck, GoalCriterion, GoalEvidence, GoalStatus};
pub use job::{JobInfo, JobResult, job_message};
pub use message::{
    Compaction, LocalUnsent, Message, MessageRole, Part, Source, UndoPoint, has_pasted, plain,
    sound_tag, source_tag, split, wrap,
};
pub use name::{ARBITER_THREAD, agent_name, fnv1a, name_seed};
pub use summary::{Found, Thread};
pub use task::{Task, TaskItem};
pub use tokens::{AUTO_COMPACT_PERCENT, ThreadTokens, TokenCounts};
pub use transcript::{
    AgentState, DETAIL_KEYS, MAX_MARK_CHARS, Note, Retry, RetryRow, SHOWN_CHARS, Span, Tone, Tool,
    ToolState, clip, marks, note, retry_row, tool_detail, unmarked,
};
pub use turn::{Turn, TurnFailure, TurnStatus, duration_words};
