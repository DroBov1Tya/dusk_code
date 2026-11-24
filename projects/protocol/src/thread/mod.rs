//! What happens in a thread: the events it tells a client, its messages, turns, tasks,
//! the link that names it,
//! the team that runs its plan and its page laid out, token counts, goal and context, the Overview, its pins, the blocks it shows, and the steps of the transcript a
//! client folds them into.

mod block;
mod context;
mod event;
mod goal;
mod job;
mod link;
mod live;
mod message;
mod name;
mod overview;
mod pin;
mod suggestion;
mod summary;
mod task;
mod team;
pub mod team_layout;
mod tokens;
pub mod transcript;
mod turn;

pub use block::diagram;
pub use block::{
    Block, CARD_LINE_CHARS, CARD_TITLE_CHARS, CHECK_DETAIL_CHARS, CHECK_NAME_CHARS, CHOICE_CHARS,
    Check, CheckState, Checks, Choices, Counts, DIAGRAM_TOOL, Diagram, Flow, FlowCard,
    LAYER_SUBTITLE_CHARS, LAYER_TITLE_CHARS, Layer, Line, MAX_CARDS, MAX_CHECKS, MAX_CHOICES,
    MAX_LAYERS, MIN_CHOICES, MIN_LAYERS, SHOW_TOOL, Show,
};
pub use context::{ContextItemDetail, ContextReport, ContextSection, ContextSource};
pub use event::{EpisodeKind, RepeatRung, ThreadEvent};
pub use goal::{Goal, GoalCheck, GoalCriterion, GoalEvidence, GoalStatus};
pub use job::{JobInfo, JobResult, job_message};
pub use link::{THREAD_LINK_PREFIX, links_in, thread_link, thread_of_link};
pub use live::{LiveTurn, STALLED_AFTER_MS, Work, WorkKind, activity};
pub use message::{
    Compaction, LocalUnsent, Message, MessageRole, Part, Source, UndoPoint, has_pasted, labelled,
    plain, sound_tag, source_tag, split, wrap,
};
pub use name::{ARBITER_THREAD, agent_name, fnv1a, name_seed};
pub use overview::{
    Activity, ChangeSum, ChangedPath, EVENTS_PAGE, LOG_CHARS, LogEntry, LogKind, OVERVIEW_LOG,
    OVERVIEW_PATHS, OVERVIEW_TOOLS, Overview, ToolRuns,
};
pub use pin::{MAX_PINS, PIN_CHARS, Pin, excerpt};
pub use suggestion::{SUGGEST_NEXT_TOOL, SUGGESTION_CHARS, SuggestNext, Suggestion};
pub use summary::{Found, Thread};
pub use task::{
    PLAN_PAGE_DONE, PlanCounts, PlanPage, Task, TaskItem, TaskOwner, TaskStatus, WaitsOn,
    current_task,
};
pub use team::{
    BUDGET_FIELDS, BUDGET_NEAR, BudgetPart, GENERAL_ROLE, Joining, KnownAgent, MAX_TEAM_AT_ONCE,
    MAX_TEAM_MEMBERS, MAX_TEAM_MINUTES, MAX_TEAM_STEPS, MAX_TEAM_TOKENS, MemberRow, TaskCard, Team,
    TeamBudget, TeamChange, TeamEnd, TeamMember, TeamStatus, TeamUsed, find_task, plan_aside,
    role_name, short_model, task_card, task_in_work, team_call_id,
};
pub use tokens::{
    AUTO_COMPACT_PERCENT, ThreadTokens, TokenCounts, plural, short_count, short_size,
};
pub use transcript::{
    AgentState, Applied, ChatLog, DETAIL_KEYS, FAILED_PREFIXES, MAX_MARK_CHARS, Note, Retry,
    RetryRow, Run, RunCard, RunKind, RunMark, SHOWN_CHARS, Span, Subagent, Tone, Tool, ToolState,
    TurnSpend, clip, marks, note, retry_row, stored_failure, tool_detail, turn_spend, unmarked,
};
pub use turn::{
    Turn, TurnFailure, TurnStatus, TurnWait, age_words, ago_words, clock_words, duration_words,
    duration_words_secs,
};
