//! The bot's settings: its commands, the texts it writes itself and its keyboards.

use std::time::Duration;

use teloxide::{adaptors::throttle::Limits, utils::command::BotCommands};

/// How often the owner hears that the bot is alive while the agent works.
pub const ALIVE_EVERY: Duration = Duration::from_secs(5 * 60);
/// How long the pairing code is good for after the bot starts.
pub const PAIRING_WINDOW: Duration = Duration::from_secs(10 * 60);
/// Wrong pairing codes before the code is spent and the bot is added again.
pub const MAX_PAIRING_ATTEMPTS: u32 = 10;
/// An answer longer than this many messages is cut; the rest stays in duskcode.
pub const MAX_ANSWER_MESSAGES: usize = 10;
pub const CUT: &str = "(The answer goes on in duskcode.)";
/// Telegram takes up to 4096 characters a message; HTML tags count too.
pub const MAX_CHUNK_CHARS: usize = 3500;

pub const LIMITS: Limits = Limits {
    messages_per_sec_chat: 1,
    messages_per_sec_overall: 20,
    messages_per_min_chat: 20,
    messages_per_min_channel_or_supergroup: 20,
};

pub const PAIRED: &str = "Paired. Messages here go to the duskcode chat; the agent's \
                          answers come back. /help lists the commands.";
pub const ALIVE: &str = "The bot is alive; the agent is still preparing the answer.";
pub const ALIVE_IDLE: &str = "The bot is alive; the agent is not working.";
pub const QUESTION: &str = "The agent asks:";
pub const MULTIPLE: &str = "Tap the options, then Send; or write your own answer.";
pub const SINGLE: &str = "Tap an option, or write your own answer.";
pub const APPROVAL: &str = "The agent waits for your approval:";
pub const SEND: &str = "Send";
pub const ONLY_TEXT: &str = "Only text reaches the chat for now.";
pub const ALLOW: &str = "Allow";
pub const DENY: &str = "Deny";

#[derive(BotCommands, Clone, Debug, PartialEq, Eq)]
#[command(rename_rule = "lowercase", description = "Commands:")]
pub enum Command {
    #[command(description = "pair with the code duskcode shows")]
    Start(String),
    #[command(description = "list the commands")]
    Help,
    #[command(description = "stop the agent")]
    Stop,
    #[command(description = "go on with a paused goal, or where the agent stopped")]
    Resume,
    #[command(description = "what the agent does, its goal and time")]
    Status,
    #[command(description = "allow what the agent waits on")]
    Approve,
    #[command(description = "deny what the agent waits on")]
    Deny,
    #[command(description = "switch to the next mode")]
    Mode,
    #[command(description = "show the model, or switch to provider/model")]
    Model(String),
    #[command(description = "summarize the context")]
    Compact(String),
    #[command(description = "show the goal, or set one")]
    Goal(String),
    #[command(description = "check that the bot is alive")]
    Alive,
}
