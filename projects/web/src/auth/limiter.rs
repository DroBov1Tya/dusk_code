//! A cap on failed sign-ins per peer, so the token cannot be guessed by trying.

use parking_lot::Mutex;
use std::{
    collections::HashMap,
    time::{Duration, Instant},
};

const WINDOW: Duration = Duration::from_secs(60);
const MAX_FAILURES: usize = 5;
/// Peers remembered at once; a flood of addresses cannot grow the table.
const MAX_PEERS: usize = 1024;

#[derive(Default)]
pub struct Limiter(Mutex<HashMap<String, Vec<Instant>>>);

impl Limiter {
    /// Seconds until the peer may try again, if it must wait now.
    pub fn blocked(&self, peer: &str) -> Option<u64> {
        let mut peers = self.0.lock();
        let failures = peers.get_mut(peer)?;
        failures.retain(|at| at.elapsed() < WINDOW);
        if failures.len() < MAX_FAILURES {
            return None;
        }
        let oldest = failures.first()?;
        Some(WINDOW.saturating_sub(oldest.elapsed()).as_secs() + 1)
    }

    pub fn failed(&self, peer: &str) {
        let mut peers = self.0.lock();
        if peers.len() >= MAX_PEERS && !peers.contains_key(peer) {
            peers.retain(|_, failures| failures.iter().any(|at| at.elapsed() < WINDOW));
        }
        if peers.len() >= MAX_PEERS && !peers.contains_key(peer) {
            return;
        }
        peers
            .entry(peer.to_owned())
            .or_default()
            .push(Instant::now());
    }
}
