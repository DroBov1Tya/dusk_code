//! The bot a client owns: its token checked once, then polling and the outbox as two
//! tasks that the client starts, stops and starts again. The pairing outlives a stop.

pub mod bot;
pub mod bridge;
pub mod context;
pub mod workers;

use std::sync::{Arc, atomic::Ordering};

use anyhow::{Result, anyhow};
use teloxide::{dispatching::ShutdownToken, prelude::*, utils::command::BotCommands};
use tokio::{sync::mpsc, task::JoinHandle};

use crate::config::{Command, LIMITS};
use bridge::{BotEvent, ToBot};
use context::Shared;

pub struct TelegramBot {
    bot: Bot,
    shared: Arc<Shared>,
    outgoing: Option<mpsc::UnboundedSender<ToBot>>,
    tasks: Vec<JoinHandle<()>>,
    polling: Option<ShutdownToken>,
}

impl TelegramBot {
    /// Checks the token with Telegram and lists the commands there; nothing is sent to
    /// anyone until [`TelegramBot::start`].
    pub async fn connect(token: String, id: usize) -> Result<Self> {
        let bot = Bot::new(token);
        // The error would name the request; the token in it is redacted, but the
        // reason that matters is only this.
        let me = bot
            .get_me()
            .await
            .map_err(|_| anyhow!("Telegram did not accept the token"))?;
        // The menu of commands shows in Telegram from the first look at the bot.
        let _ = bot.set_my_commands(Command::bot_commands()).await;
        Ok(Self {
            bot,
            shared: Arc::new(Shared::new(id, me.username().to_owned())),
            outgoing: None,
            tasks: Vec::new(),
            polling: None,
        })
    }

    pub fn id(&self) -> usize {
        self.shared.id
    }

    pub fn username(&self) -> &str {
        &self.shared.username
    }

    /// What the owner sends to pair: `/start <code>`.
    pub fn code(&self) -> &str {
        &self.shared.code
    }

    pub fn owner(&self) -> Option<String> {
        self.shared.owner().map(|owner| owner.name)
    }

    pub fn running(&self) -> bool {
        self.tasks.iter().any(|task| !task.is_finished())
    }

    /// Polls for the owner's messages and sends them what the client hands over.
    pub fn start(&mut self, events: mpsc::UnboundedSender<(usize, BotEvent)>) {
        if self.running() {
            return;
        }
        *self.shared.events.lock() = Some(events);
        self.shared.stopping.store(false, Ordering::Relaxed);
        let bot = self.bot.clone().throttle(LIMITS);
        let (outgoing, incoming) = mpsc::unbounded_channel();
        self.outgoing = Some(outgoing);
        let (polling, token) = bot::runner::start_bot(bot.clone(), Arc::clone(&self.shared));
        self.polling = Some(token);
        let outbox = tokio::spawn(workers::outbox::run(
            bot,
            Arc::clone(&self.shared),
            incoming,
        ));
        self.tasks = vec![polling, outbox];
    }

    /// Polling ends after the updates in hand are handled and confirmed, so none comes
    /// again on the next start.
    pub fn stop(&mut self) {
        self.shared.stopping.store(true, Ordering::Relaxed);
        let mut tasks = self.tasks.drain(..);
        let polling = tasks.next();
        let stopped = self
            .polling
            .take()
            .is_some_and(|token| token.shutdown().is_ok());
        if let Some(polling) = polling
            && !stopped
        {
            polling.abort();
        }
        for task in tasks {
            task.abort();
        }
        self.outgoing = None;
    }

    pub fn send(&self, message: ToBot) {
        if let Some(outgoing) = &self.outgoing {
            let _ = outgoing.send(message);
        }
    }
}

impl Drop for TelegramBot {
    fn drop(&mut self) {
        self.stop();
    }
}
