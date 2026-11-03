//! What passes between the bot and the client that owns the chat.

/// From the owner in Telegram to the client.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FromBot {
    /// A message for the chat, or the owner's own answer to an open question.
    Message(String),
    Command(BotCommand),
    /// The options chosen for the open question, by index.
    Chosen(Vec<usize>),
    /// The answer to the approval request `id`.
    Approval {
        id: u64,
        allow: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotCommand {
    Stop,
    Resume,
    Status,
    /// Allows the request the agent waits on, if it waits on one.
    Approve,
    Deny,
    /// The next mode, as Tab in the client.
    Mode,
    Model(Option<String>),
    Compact(Option<String>),
    Goal(Option<String>),
}

/// From the client to the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ToBot {
    /// The agent's final answer, in Markdown.
    Answer(String),
    Question {
        question: String,
        options: Vec<String>,
        multiple: bool,
    },
    /// The agent waits for a yes to what `text` says.
    Approval { id: u64, text: String },
    /// A short plain answer to a command.
    Reply(String),
    /// The agent started or stopped working; while it works the owner hears that the
    /// bot is alive every few minutes.
    Working(bool),
}

/// What a bot tells the client, with the bot's id.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum BotEvent {
    Paired {
        user: String,
    },
    Input(FromBot),
    /// Polling ended, such as on a revoked token.
    Stopped {
        reason: String,
    },
}
