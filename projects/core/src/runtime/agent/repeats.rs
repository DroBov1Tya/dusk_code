//! A model that is stuck repeats a call and gets the same answer. With no cap on steps,
//! this is what stops it from burning a turn: from the third identical call with an
//! identical result in a row, the result carries a note to change course.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
};

use serde_json::Value;

/// Identical calls with identical results, in a row, before the note appears.
const NOTICE_AT: u32 = 3;

#[derive(Default)]
pub struct Repeats {
    turn: String,
    last: Option<(u64, u64)>,
    count: u32,
}

impl Repeats {
    /// Notes a finished call; the text to add to its result when it is the third or
    /// later of the same kind in a row.
    pub fn note(
        &mut self,
        turn_id: &str,
        name: &str,
        arguments: &Value,
        result: &str,
    ) -> Option<String> {
        if self.turn != turn_id {
            *self = Self {
                turn: turn_id.to_owned(),
                ..Self::default()
            };
        }
        let call = hash((name, arguments.to_string()));
        let answer = hash(result);
        if self.last == Some((call, answer)) {
            self.count += 1;
        } else {
            self.last = Some((call, answer));
            self.count = 1;
        }
        (self.count >= NOTICE_AT).then(|| {
            format!(
                "[this exact call has returned this same result {} times in a row; repeating it \
                 will not change anything. Read the result again, try a different command, or \
                 ask the user.]",
                self.count
            )
        })
    }
}

fn hash(value: impl Hash) -> u64 {
    let mut hasher = DefaultHasher::new();
    value.hash(&mut hasher);
    hasher.finish()
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_third_identical_call_with_the_same_result_gets_a_note() {
        let mut repeats = Repeats::default();
        let args = json!({"command": "make"});
        assert!(repeats.note("t", "run_command", &args, "error").is_none());
        assert!(repeats.note("t", "run_command", &args, "error").is_none());
        assert!(repeats.note("t", "run_command", &args, "error").is_some());
        // A different result, or call, or turn starts over.
        assert!(repeats.note("t", "run_command", &args, "ok").is_none());
        assert!(
            repeats
                .note("t", "run_command", &json!({"command": "ls"}), "ok")
                .is_none()
        );
        for _ in 0..3 {
            repeats.note("a", "x", &args, "r");
        }
        assert!(repeats.note("b", "x", &args, "r").is_none());
    }
}
