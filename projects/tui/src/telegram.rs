//! The session's Telegram bots: what the open chat does, mirrored to their owners, and
//! what they write, turned into what the client would do with the same input.

use dusk_core::{
    providers::ModelEvent,
    runtime::ThreadEvent,
    state::{GoalStatus, TurnStatus},
    tools::Decision,
};
use dusk_telegram::{BotCommand, BotEvent, FromBot, TelegramBot, ToBot};
use tokio::sync::mpsc;

use crate::app::{Action, App, BotMenu, BotRow, Tone};

pub struct Telegram {
    bots: Vec<TelegramBot>,
    next_id: usize,
    sender: mpsc::UnboundedSender<(usize, BotEvent)>,
    receiver: mpsc::UnboundedReceiver<(usize, BotEvent)>,
    /// The chat's answer so far: its text since the last tool call.
    answer: String,
    question_sent: bool,
    approval_sent: Option<u64>,
}

impl Default for Telegram {
    fn default() -> Self {
        let (sender, receiver) = mpsc::unbounded_channel();
        Self {
            bots: Vec::new(),
            next_id: 0,
            sender,
            receiver,
            answer: String::new(),
            question_sent: false,
            approval_sent: None,
        }
    }
}

impl Telegram {
    /// Checks the token and starts the bot; returns its name and pairing code.
    pub async fn add(&mut self, token: String) -> anyhow::Result<(String, String)> {
        let mut bot = TelegramBot::connect(token, self.next_id).await?;
        self.next_id += 1;
        bot.start(self.sender.clone());
        let named = (bot.username().to_owned(), bot.code().to_owned());
        self.bots.push(bot);
        Ok(named)
    }

    pub fn toggle(&mut self, id: usize) {
        let sender = self.sender.clone();
        if let Some(bot) = self.bots.iter_mut().find(|bot| bot.id() == id) {
            match bot.running() {
                true => bot.stop(),
                false => bot.start(sender),
            }
        }
    }

    pub fn remove(&mut self, id: usize) {
        self.bots.retain(|bot| bot.id() != id);
    }

    pub fn rows(&self) -> Vec<BotRow> {
        self.bots
            .iter()
            .map(|bot| BotRow {
                id: bot.id(),
                username: bot.username().to_owned(),
                owner: bot.owner(),
                code: bot.code().to_owned(),
                running: bot.running(),
            })
            .collect()
    }

    pub async fn next(&mut self) -> Option<(usize, BotEvent)> {
        self.receiver.recv().await
    }

    pub fn menu(&self, id: usize, command: &str, title: String, options: Vec<String>) {
        if let Some(bot) = self.bots.iter().find(|bot| bot.id() == id) {
            bot.send(ToBot::Menu {
                command: command.into(),
                title,
                options,
            });
        }
    }

    pub fn reply(&self, id: usize, text: impl Into<String>) {
        if let Some(bot) = self.bots.iter().find(|bot| bot.id() == id) {
            bot.send(ToBot::Reply(text.into()));
        }
    }

    fn broadcast(&self, message: ToBot) {
        for bot in &self.bots {
            bot.send(message.clone());
        }
    }

    /// Follows the chat's own events; a subagent's stay in the client.
    pub fn observe(&mut self, event: &ThreadEvent) {
        if self.bots.is_empty() {
            return;
        }
        match event {
            ThreadEvent::TurnStarted(_) => {
                self.answer.clear();
                self.broadcast(ToBot::Working(true));
            }
            ThreadEvent::Model(ModelEvent::Text(text)) => self.answer.push_str(text),
            ThreadEvent::ToolStarted(_) => self.answer.clear(),
            ThreadEvent::TurnFinished(turn) => {
                let answer = std::mem::take(&mut self.answer);
                let message = match turn.status {
                    TurnStatus::Completed if answer.trim().is_empty() => None,
                    TurnStatus::Completed => Some(ToBot::Answer(answer)),
                    TurnStatus::Failed => Some(ToBot::Reply(format!(
                        "The turn failed: {}",
                        turn.error_message.as_deref().unwrap_or("no reason given")
                    ))),
                    TurnStatus::Interrupted => Some(ToBot::Reply("Stopped.".into())),
                    _ => None,
                };
                self.broadcast(ToBot::Working(false));
                if let Some(message) = message {
                    self.broadcast(message);
                }
            }
            _ => {}
        }
    }

    /// Sends the question and the approval request the client shows, once each.
    pub fn sync(&mut self, app: &App) {
        if self.bots.is_empty() {
            return;
        }
        match &app.poll {
            Some(poll) if !self.question_sent => {
                self.question_sent = true;
                self.broadcast(ToBot::Question {
                    question: poll.question.question.clone(),
                    options: poll.question.options.clone(),
                    multiple: poll.question.multiple,
                });
            }
            Some(_) => {}
            None => self.question_sent = false,
        }
        let first = app.approvals.first();
        let id = first.map(|asked| asked.request.id);
        if id != self.approval_sent {
            self.approval_sent = id;
            if let Some(asked) = first {
                self.broadcast(ToBot::Approval {
                    id: asked.request.id,
                    text: asked.title(),
                });
            }
        }
    }
}

/// What the client does with a bot's event, as if its owner sat at the keyboard.
pub fn handle(telegram: &mut Telegram, app: &mut App, id: usize, event: BotEvent) -> Action {
    let action = match event {
        BotEvent::Paired { user } => {
            app.transcript
                .notice(format!("Telegram bot paired with {user}"), Tone::Info);
            Action::None
        }
        BotEvent::Stopped { reason } => {
            app.transcript
                .notice(format!("Telegram bot stopped: {reason}"), Tone::Info);
            Action::None
        }
        BotEvent::Input(input) => input_action(telegram, app, id, input),
    };
    app.bots = telegram.rows();
    action
}

fn input_action(telegram: &Telegram, app: &mut App, id: usize, input: FromBot) -> Action {
    match input {
        // Written while a question is open, it is the owner's own answer.
        FromBot::Message(text) => match app.poll.take() {
            Some(poll) => send(app, poll.question.answer(&[], &text)),
            // The client's own commands open its panels and read files; from afar only
            // the bot's commands are taken.
            None if text.trim_start().starts_with('/') => {
                telegram.reply(
                    id,
                    "That command works only in duskcode; /help lists the bot's.",
                );
                Action::None
            }
            None => app.submit_text(&text),
        },
        FromBot::Chosen(chosen) => match app.poll.take() {
            Some(poll) => send(app, poll.question.answer(&chosen, "")),
            None => {
                telegram.reply(id, "That question is closed.");
                Action::None
            }
        },
        FromBot::Approval { id: request, allow } => {
            if !app
                .approvals
                .iter()
                .any(|asked| asked.request.id == request)
            {
                telegram.reply(id, "That request is no longer waiting.");
                return Action::None;
            }
            Action::Approve {
                id: request,
                decision: if allow {
                    Decision::Once
                } else {
                    Decision::Deny
                },
            }
        }
        FromBot::Command(command) => command_action(telegram, app, id, command),
    }
}

fn command_action(telegram: &Telegram, app: &mut App, id: usize, command: BotCommand) -> Action {
    match command {
        BotCommand::Stop if app.is_busy() || app.agents_at_work() => {
            telegram.reply(id, "Stopping.");
            Action::Cancel
        }
        BotCommand::Stop => {
            telegram.reply(id, "The agent is not working.");
            Action::None
        }
        BotCommand::Resume => {
            let paused = app
                .goal
                .as_ref()
                .is_some_and(|goal| goal.status == GoalStatus::Paused);
            match (paused, app.is_busy()) {
                (true, _) => app.submit_text("/goal resume"),
                (false, true) => {
                    telegram.reply(id, "The agent is already working.");
                    Action::None
                }
                (false, false) => app.submit_text("Continue where you stopped."),
            }
        }
        BotCommand::Status => {
            telegram.reply(id, status(app));
            Action::None
        }
        BotCommand::Approve | BotCommand::Deny => {
            // Only the request the owner was shown, never one that took its place.
            let shown = telegram.approval_sent;
            let Some(asked) = app
                .approvals
                .iter()
                .find(|asked| Some(asked.request.id) == shown)
            else {
                telegram.reply(id, "Nothing is waiting for your approval.");
                return Action::None;
            };
            let allow = command == BotCommand::Approve;
            Action::Approve {
                id: asked.request.id,
                decision: if allow {
                    Decision::Once
                } else {
                    Decision::Deny
                },
            }
        }
        BotCommand::Mode(None) => Action::BotMenu {
            bot: id,
            menu: BotMenu::Modes,
        },
        BotCommand::Mode(Some(name)) => Action::SetMode { bot: id, name },
        BotCommand::Model(None) => {
            telegram.reply(id, "Checking the providers.");
            Action::BotMenu {
                bot: id,
                menu: BotMenu::Models,
            }
        }
        BotCommand::Model(Some(name)) => {
            telegram.reply(id, format!("Switching to {name}; /status shows the model."));
            app.submit_text(&format!("/model {name}"))
        }
        BotCommand::Compact(instructions) => {
            telegram.reply(id, "Compacting the context.");
            app.submit_text(&format!("/compact {}", instructions.unwrap_or_default()))
        }
        BotCommand::Goal(None) => {
            telegram.reply(id, goal_line(app));
            Action::None
        }
        BotCommand::Goal(Some(text)) => app.submit_text(&format!("/goal {text}")),
    }
}

fn send(app: &mut App, text: String) -> Action {
    app.show_sent(&text);
    Action::Send(text)
}

fn status(app: &App) -> String {
    let agent = match (app.is_busy(), app.elapsed_ms()) {
        (true, Some(ms)) => format!("working for {}", duration(ms)),
        (true, None) => "working".into(),
        (false, _) if app.agents_at_work() => "subagents at work".into(),
        (false, _) => "idle".into(),
    };
    let waits = match (app.poll.is_some(), app.approvals.len()) {
        (true, _) => "\nWaits for: your answer to a question".to_owned(),
        (false, 0) => String::new(),
        (false, count) => format!("\nWaits for: {count} approval(s), /approve"),
    };
    format!(
        "Agent: {agent}\nMode: {} \u{00B7} model: {}\n{}{waits}",
        app.mode,
        app.model,
        goal_line(app)
    )
}

fn goal_line(app: &App) -> String {
    match &app.goal {
        Some(goal) => {
            let status = match goal.status {
                GoalStatus::Active if goal.approved => "active",
                GoalStatus::Active => "draft",
                GoalStatus::Paused => "paused",
                GoalStatus::Done => "done",
            };
            format!("Goal ({status}, round {}): {}", goal.rounds, goal.text)
        }
        None => "Goal: none".into(),
    }
}

fn duration(ms: u64) -> String {
    let seconds = ms / 1000;
    match seconds {
        0..60 => format!("{seconds}s"),
        60..3600 => format!("{}m {}s", seconds / 60, seconds % 60),
        _ => format!("{}h {}m", seconds / 3600, seconds % 3600 / 60),
    }
}
