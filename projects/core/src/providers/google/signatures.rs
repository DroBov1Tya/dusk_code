//! The thought signatures of recent function calls, by call ID. The API checks them only
//! for the turn in progress, so a bounded memory of this run is enough; a call it has
//! forgotten goes back with the placeholder the API documents for such history.

use std::collections::VecDeque;

use parking_lot::Mutex;

/// What the API accepts in place of a signature it did not issue.
pub const PLACEHOLDER: &str = "skip_thought_signature_validator";

const MAX_BYTES: usize = 4 * 1024 * 1024;

#[derive(Default)]
pub struct Signatures {
    /// Oldest first, with the bytes they hold.
    kept: Mutex<(VecDeque<(String, String)>, usize)>,
}

impl Signatures {
    pub fn remember(&self, call_id: &str, signature: String) {
        let mut kept = self.kept.lock();
        kept.1 += signature.len();
        kept.0.push_back((call_id.to_owned(), signature));
        while kept.1 > MAX_BYTES {
            let Some((_, dropped)) = kept.0.pop_front() else {
                break;
            };
            kept.1 -= dropped.len();
        }
    }

    pub fn of(&self, call_id: &str) -> Option<String> {
        let kept = self.kept.lock();
        kept.0
            .iter()
            .rev()
            .find(|(id, _)| id == call_id)
            .map(|(_, signature)| signature.clone())
    }
}
