//! The bot's runtime: the token checked once, then polling and the outbox as two tasks
//! that a client starts, stops and starts again. The pairing outlives a stop.

use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU32, Ordering},
    },
    time::Instant,
};

use anyhow::{Result, anyhow};
use teloxide::{
    adaptors::throttle::Throttle,
    dispatching::{ShutdownToken, UpdateHandler},
    prelude::*,
    types::{MessageId, UserId},
};
use tokio::{sync::mpsc, task::JoinHandle};
use uuid::Uuid;

use crate::{
    bridge::{BotEvent, ToBot},
    config::LIMITS,
    handlers, outbox,
};

pub type MyBot = Throttle<Bot>;
pub type HandlerResult = Result<(), anyhow::Error>;

/// The Telegram user the bot answers, and the private chat with them.
#[derive(Clone)]
pub struct Owner {
    pub user: UserId,
    pub chat: ChatId,
    pub name: String,
}

/// An open question: its options, which are checked, and the message with its buttons.
pub struct OpenQuestion {
    pub options: Vec<String>,
    pub checked: Vec<bool>,
    pub multiple: bool,
    pub message: MessageId,
}

/// What the handlers and the outbox share, across stops and starts.
pub struct Shared {
    pub id: usize,
    pub username: String,
    pub code: String,
    /// Wrong codes sent so far; past a few, the code is spent.
    pub attempts: AtomicU32,
    /// Starts the data of every button this bot sends, so a button from an earlier run
    /// answers nothing.
    pub nonce: String,
    pub created: Instant,
    /// Set by a stop the client asked for, which needs no word back.
    pub stopping: AtomicBool,
    pub owner: Mutex<Option<Owner>>,
    pub question: Mutex<Option<OpenQuestion>>,
    pub working: AtomicBool,
    pub events: Mutex<Option<mpsc::UnboundedSender<(usize, BotEvent)>>>,
}

impl Shared {
    pub fn owner(&self) -> Option<Owner> {
        self.owner.lock().ok()?.clone()
    }

    pub fn emit(&self, event: BotEvent) {
        if let Some(events) = self.events.lock().ok().and_then(|events| events.clone()) {
            let _ = events.send((self.id, event));
        }
    }

    pub fn working(&self) -> bool {
        self.working.load(Ordering::Relaxed)
    }

    /// Callback data with this run's nonce in front.
    pub fn button(&self, data: &str) -> String {
        format!("{}|{data}", self.nonce)
    }
}

pub struct TelegramBot {
    bot: Bot,
    shared: Arc<Shared>,
    outgoing: Option<mpsc::UnboundedSender<ToBot>>,
    tasks: Vec<JoinHandle<()>>,
    polling: Option<ShutdownToken>,
}

impl TelegramBot {
    pub(crate) async fn connect(token: String, id: usize) -> Result<Self> {
        let bot = Bot::new(token);
        // The error would name the request; the token in it is redacted, but the
        // reason that matters is only this.
        let me = bot
            .get_me()
            .await
            .map_err(|_| anyhow!("Telegram did not accept the token"))?;
        Ok(Self {
            bot,
            shared: Arc::new(Shared {
                id,
                username: me.username().to_owned(),
                code: random_digits(8),
                attempts: AtomicU32::new(0),
                nonce: random_digits(6),
                created: Instant::now(),
                stopping: AtomicBool::new(false),
                owner: Mutex::new(None),
                question: Mutex::new(None),
                working: AtomicBool::new(false),
                events: Mutex::new(None),
            }),
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

    /// How long ago the bot was added.
    pub fn age(&self) -> std::time::Duration {
        self.shared.created.elapsed()
    }

    /// Polls for the owner's messages and sends them what the client hands over.
    pub fn start(&mut self, events: mpsc::UnboundedSender<(usize, BotEvent)>) {
        if self.running() {
            return;
        }
        if let Ok(mut slot) = self.shared.events.lock() {
            *slot = Some(events);
        }
        let bot = self.bot.clone().throttle(LIMITS);
        let (outgoing, incoming) = mpsc::unbounded_channel();
        self.outgoing = Some(outgoing);
        let shared = Arc::clone(&self.shared);
        shared.stopping.store(false, Ordering::Relaxed);
        let mut dispatcher = Dispatcher::builder(bot.clone(), schema())
            .dependencies(dptree::deps![Arc::clone(&shared)])
            .build();
        self.polling = Some(dispatcher.shutdown_token());
        let polling = tokio::spawn({
            let shared = Arc::clone(&shared);
            async move {
                dispatcher.dispatch().await;
                if !shared.stopping.load(Ordering::Relaxed) {
                    shared.emit(BotEvent::Stopped {
                        reason: "polling ended".into(),
                    });
                }
            }
        });
        let outbox = tokio::spawn(outbox::run(bot, shared, incoming));
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

/// `count` digits, up to 18, from the system's random source.
fn random_digits(count: u32) -> String {
    let bytes: [u8; 8] = Uuid::new_v4().as_bytes()[..8]
        .try_into()
        .unwrap_or_default();
    let random = u64::from_le_bytes(bytes) % 10u64.pow(count);
    format!("{random:0width$}", width = count as usize)
}

fn schema() -> UpdateHandler<anyhow::Error> {
    dptree::entry()
        .branch(Update::filter_message().endpoint(handlers::messages::on_message))
        .branch(Update::filter_callback_query().endpoint(handlers::callbacks::on_callback))
}
