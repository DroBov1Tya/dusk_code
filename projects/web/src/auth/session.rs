//! The signed-in browsers of this run. Memory only: a restart signs everyone out.

use parking_lot::Mutex;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// A session ends this long after the browser last used it.
const IDLE: Duration = Duration::from_secs(12 * 3600);
/// A session ends this long after it began, however busy it is.
const LIFETIME: Duration = Duration::from_secs(24 * 3600);
const MAX_SESSIONS: usize = 32;

#[derive(Default)]
pub struct Sessions(Mutex<HashMap<String, Session>>);

struct Session {
    began: Instant,
    seen: Instant,
}

impl Session {
    fn live(&self) -> bool {
        self.seen.elapsed() < IDLE && self.began.elapsed() < LIFETIME
    }
}

impl Sessions {
    /// Starts a session under `id`; none when too many are open.
    pub fn open(&self, id: String) -> Option<String> {
        let mut sessions = self.0.lock();
        sessions.retain(|_, session| session.live());
        if sessions.len() >= MAX_SESSIONS {
            return None;
        }
        let now = Instant::now();
        sessions.insert(
            id.clone(),
            Session {
                began: now,
                seen: now,
            },
        );
        Some(id)
    }

    pub fn valid(&self, id: &str) -> bool {
        let mut sessions = self.0.lock();
        match sessions.get_mut(id) {
            Some(session) if session.live() => {
                session.seen = Instant::now();
                true
            }
            Some(_) => {
                sessions.remove(id);
                false
            }
            None => false,
        }
    }

    pub fn close(&self, id: &str) {
        self.0.lock().remove(id);
    }
}
