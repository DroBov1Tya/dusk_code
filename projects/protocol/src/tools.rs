//! A tool's result, the approvals a tool asks for, and the question an agent asks.

use anyhow::ensure;
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use crate::artifact::ImageArtifact;

/// How long a question and its options may be, and how many options it has.
pub const MAX_QUESTION_CHARS: usize = 300;
pub const MAX_OPTION_CHARS: usize = 120;
pub const MIN_OPTIONS: usize = 2;
pub const MAX_OPTIONS: usize = 8;

/// What the model gets back from a call.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolResult {
    pub content: String,
    pub is_error: bool,
    /// Set only by the foreground shell runner, never inferred from output text.
    pub exit_code: Option<i32>,
    pub artifacts: Vec<ImageArtifact>,
}

impl ToolResult {
    pub fn text(content: String) -> Self {
        Self {
            content,
            is_error: false,
            exit_code: None,
            artifacts: Vec::new(),
        }
    }
}

/// Window inventory or one kind of access to a retained window selection.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum WindowAccess {
    List,
    Capture(String),
    Activate(String),
}

/// What a tool asks to do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Request {
    /// A path outside the working directory and the workspace.
    Path { path: PathBuf, write: bool },
    /// A shell command line.
    Command { command: String },
    /// Seeing the screen and independent accessibility activation. This never grants
    /// control of the user's system pointer.
    Screen { action: String },
    /// Moving and clicking/scrolling with the shared system pointer. This needs its
    /// own session grant, separate from screen access and independent activation.
    SystemPointer { action: String },
    /// Window metadata, capture or activation. None is a request to end confinement.
    Window {
        grant: Option<WindowAccess>,
        action: String,
    },
    /// Typing or pressing keys, which can do anything a shell can: asked every time,
    /// with what it sends.
    Keyboard { action: String },
}

impl Request {
    /// What `who` asks for, in a line, for any client to show.
    pub fn describe(&self, who: &str) -> String {
        match self {
            Self::Path { path, write: true } => format!("{who} wants to write {}", path.display()),
            Self::Path { path, write: false } => format!("{who} wants to read {}", path.display()),
            Self::Command { command } => format!("{who} wants to run: {command}"),
            Self::Screen { action } => format!("{who} wants to use the screen: {action}"),
            Self::SystemPointer { action } => {
                format!("{who} wants to control your system pointer: {action}")
            }
            Self::Keyboard { action } => format!("{who} wants to use the keyboard: {action}"),
            Self::Window { action, .. } => {
                format!("{who} wants to use window tools: {action}")
            }
        }
    }

    pub fn what(&self) -> String {
        match self {
            Self::Path { path, write: true } => format!("writing {}", path.display()),
            Self::Path { path, write: false } => format!("reading {}", path.display()),
            Self::Command { command } => format!("running `{command}`"),
            Self::Screen { action } => format!("using the screen: {action}"),
            Self::SystemPointer { action } => format!("controlling the system pointer: {action}"),
            Self::Keyboard { action } => format!("using the keyboard: {action}"),
            Self::Window { action, .. } => format!("using window tools: {action}"),
        }
    }
}

/// A request as the client sees it; `id` is what its answer names.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalRequest {
    pub id: u64,
    pub request: Request,
    /// What a yes for the session covers, as the core decides it: the folder of the
    /// path, or the program of a plain command. `None` when it cannot be offered.
    pub session: Option<String>,
}

/// A question as the client shows it. The client always adds a field for the user's
/// own answer below the options.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Question {
    pub question: String,
    #[schemars(length(min = MIN_OPTIONS, max = MAX_OPTIONS))]
    pub options: Vec<String>,
    /// Several options may be chosen; otherwise exactly one, or the user's own answer.
    #[serde(default)]
    pub multiple: bool,
}

impl Question {
    /// The message that answers it: the chosen options, by index, then the user's own
    /// words, if any.
    pub fn answer(&self, chosen: &[usize], own: &str) -> String {
        let mut parts: Vec<&str> = chosen
            .iter()
            .filter_map(|&index| self.options.get(index))
            .map(String::as_str)
            .collect();
        let own = own.trim();
        if !own.is_empty() {
            parts.push(own);
        }
        self.reply(&parts.join("; "))
    }

    /// The message saying the user would not answer, ahead of what they wrote instead.
    pub fn declined(&self, message: &str) -> String {
        let reply = self.reply("(declined)");
        match message.trim() {
            "" => reply,
            message => format!("{reply}\n\n{message}"),
        }
    }

    pub fn reply(&self, answer: &str) -> String {
        format!("QUESTION: {}\nANSWER: {answer}", self.question)
    }

    pub fn check(&self) -> anyhow::Result<()> {
        let question = self.question.trim();
        ensure!(!question.is_empty(), "the question is empty");
        ensure!(
            question.chars().count() <= MAX_QUESTION_CHARS,
            "the question is over {MAX_QUESTION_CHARS} characters"
        );
        ensure!(
            (MIN_OPTIONS..=MAX_OPTIONS).contains(&self.options.len()),
            "give {MIN_OPTIONS} to {MAX_OPTIONS} options"
        );
        for (index, option) in self.options.iter().enumerate() {
            let option = option.trim();
            ensure!(!option.is_empty(), "option {} is empty", index + 1);
            ensure!(
                option.chars().count() <= MAX_OPTION_CHARS,
                "option {} is over {MAX_OPTION_CHARS} characters",
                index + 1
            );
            ensure!(
                !self.options[..index]
                    .iter()
                    .any(|other| other.trim() == option),
                "option {} repeats an earlier one",
                index + 1
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Decision {
    Once,
    /// Also yes to the same folder or program until the core stops.
    Session,
    Deny,
}
