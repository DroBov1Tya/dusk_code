//! The signed-in browsers of this run. Memory only: a restart signs everyone out.

use parking_lot::Mutex;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

/// A session ends this long after the browser last used it.
const IDLE: Duration = Duration::from_secs(12 * 3600);
const MAX_SESSIONS: usize = 32;

#[derive(Default)]
pub struct Sessions(Mutex<HashMap<String, Instant>>);

impl Sessions {
    /// Starts a session under `id`; none when too many are open.
    pub fn open(&self, id: String) -> Option<String> {
        let mut sessions = self.0.lock();
        sessions.retain(|_, seen| seen.elapsed() < IDLE);
        if sessions.len() >= MAX_SESSIONS {
            return None;
        }
        sessions.insert(id.clone(), Instant::now());
        Some(id)
    }

    pub fn valid(&self, id: &str) -> bool {
        let mut sessions = self.0.lock();
        match sessions.get_mut(id) {
            Some(seen) if seen.elapsed() < IDLE => {
                *seen = Instant::now();
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
