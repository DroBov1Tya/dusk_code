//! The same call with the same result: from the third in a row, the result carries a
//! note to change course. A different call or result starts the count over.

use serde_json::Value;

use super::hash;

/// Identical calls with identical results, in a row, before the note appears.
const NOTICE_AT: u32 = 3;

#[derive(Default)]
pub struct SameCall {
    last: Option<(u64, u64)>,
    count: u32,
}

impl SameCall {
    /// Notes a finished call; when it is the third or later of the same kind in a row,
    /// how many times it came back the same and the text to add to its result.
    pub fn note(&mut self, name: &str, arguments: &Value, result: &str) -> Option<(u32, String)> {
        let call = hash((name, arguments.to_string()));
        let answer = hash(result);
        if self.last == Some((call, answer)) {
            self.count += 1;
        } else {
            self.last = Some((call, answer));
            self.count = 1;
        }
        (self.count >= NOTICE_AT).then(|| {
            let note = format!(
                "[this exact call has returned this same result {} times in a row; repeating it \
                 will not change anything. Read the result again, try a different command, or \
                 ask the user.]",
                self.count
            );
            (self.count, note)
        })
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[test]
    fn the_third_identical_call_with_the_same_result_gets_a_note() {
        let mut repeats = SameCall::default();
        let args = json!({"command": "make"});
        assert!(repeats.note("run_command", &args, "error").is_none());
        assert!(repeats.note("run_command", &args, "error").is_none());
        assert!(repeats.note("run_command", &args, "error").is_some());
        // A different result, or call, starts over; a new turn is tested with `Repeats`.
        assert!(repeats.note("run_command", &args, "ok").is_none());
        assert!(
            repeats
                .note("run_command", &json!({"command": "ls"}), "ok")
                .is_none()
        );
    }
}
