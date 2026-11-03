//! What the handlers and the outbox of one bot share, across stops and starts.

use std::{
    sync::atomic::{AtomicBool, AtomicU32, Ordering},
    time::Instant,
};

use parking_lot::Mutex;
use teloxide::types::{ChatId, MessageId, UserId};
use tokio::sync::mpsc;
use uuid::Uuid;

use super::bridge::BotEvent;

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
    pub fn new(id: usize, username: String) -> Self {
        Self {
            id,
            username,
            code: random_digits(8),
            attempts: AtomicU32::new(0),
            nonce: random_digits(6),
            created: Instant::now(),
            stopping: AtomicBool::new(false),
            owner: Mutex::new(None),
            question: Mutex::new(None),
            working: AtomicBool::new(false),
            events: Mutex::new(None),
        }
    }

    pub fn owner(&self) -> Option<Owner> {
        self.owner.lock().clone()
    }

    pub fn emit(&self, event: BotEvent) {
        if let Some(events) = self.events.lock().as_ref() {
            let _ = events.send((self.id, event));
        }
    }

    pub fn working(&self) -> bool {
        self.working.load(Ordering::Relaxed)
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
