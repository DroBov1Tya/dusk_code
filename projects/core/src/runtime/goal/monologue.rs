//! Goal rounds that only talk. A round is meant to act on the goal, yet a model can
//! answer each one with a plan or a report and call no tool, and with the rounds no
//! longer capped nothing else sees it: a round that changes no file and runs no test is
//! not measured by `stall`. As OpenHands counts answers with no action in a row, rounds
//! in a row that called no tool, with no message from the user between, get a note in
//! the next round's start at `limits.monologue_note_at`, a request for a different plan
//! at `monologue_replan_at`, and again every `monologue_note_at` rounds after. A round
//! that talks after the re-plan changed nothing either: with `limits.goal_stall` on,
//! the goal then pauses (`talked_out`).

use dusk_protocol::thread::RepeatRung;

use crate::config::Limits;

#[derive(Default)]
pub struct Monologue {
    /// Rounds in a row that called no tool.
    rounds: u32,
    /// The count the next note waits for; 0 before the first since a round acted.
    next_at: u32,
}

impl Monologue {
    /// The user or another agent wrote: what came before is no monologue any more.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The rounds in a row that only talked, once one more did after the request for a
    /// different plan; None before.
    pub fn talked_out(&self, limits: &Limits) -> Option<u32> {
        (limits.monologue && self.rounds > limits.monologue_replan_at).then_some(self.rounds)
    }

    /// A round ended, having called a tool or not (`acted`); the rung, count and note for
    /// the next round when the rounds only talk.
    pub fn round(&mut self, acted: bool, limits: &Limits) -> Option<(RepeatRung, u32, String)> {
        if !limits.monologue || acted {
            self.reset();
            return None;
        }
        self.rounds = self.rounds.saturating_add(1);
        let at = match self.next_at {
            0 => limits.monologue_note_at,
            at => at,
        };
        let count = self.rounds;
        if count < at {
            return None;
        }
        Some(if count >= limits.monologue_replan_at {
            self.next_at = count.saturating_add(limits.monologue_note_at);
            (RepeatRung::NoProgressReplan, count, replan(count))
        } else {
            self.next_at = limits.monologue_replan_at;
            (RepeatRung::NoProgress, count, note(count))
        })
    }
}

fn note(count: u32) -> String {
    format!(
        "[your last {count} rounds only wrote text and called no tool. This round, act on \
         the goal: make the change, run the check, or ask the user what is missing.]"
    )
}

fn replan(count: u32) -> String {
    format!(
        "[{count} rounds in a row have only written text; the approach is not working. \
         Write one sentence with a different plan, then call the tool it needs, or ask the \
         user.]"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The rungs of `rounds` rounds, each acting as said.
    fn rungs(
        monologue: &mut Monologue,
        acted: &[bool],
        limits: &Limits,
    ) -> Vec<Option<RepeatRung>> {
        acted
            .iter()
            .map(|acted| monologue.round(*acted, limits).map(|(rung, ..)| rung))
            .collect()
    }

    #[test]
    fn rounds_that_only_talk_get_a_note_then_a_replan() {
        let limits = Limits::default();
        let mut monologue = Monologue::default();
        let found = rungs(&mut monologue, &[false; 10], &limits);
        use RepeatRung::{NoProgress, NoProgressReplan};
        assert_eq!(
            found,
            [
                None,
                None,
                Some(NoProgress),
                None,
                Some(NoProgressReplan),
                None,
                None,
                Some(NoProgressReplan),
                None,
                None
            ]
        );
    }

    #[test]
    fn a_round_that_acts_or_a_user_message_starts_over() {
        let limits = Limits::default();
        let mut monologue = Monologue::default();
        let found = rungs(&mut monologue, &[false, false, true, false, false], &limits);
        assert!(found.iter().all(Option::is_none), "{found:?}");
        monologue.reset();
        let found = rungs(&mut monologue, &[false, false], &limits);
        assert!(found.iter().all(Option::is_none), "{found:?}");
        // Rounds that each call a tool, research included, never get a note.
        let found = rungs(&mut monologue, &[true; 20], &limits);
        assert!(found.iter().all(Option::is_none), "{found:?}");
    }

    #[test]
    fn the_switch_turns_it_off() {
        let mut limits = Limits::default();
        limits.monologue = false;
        let mut monologue = Monologue::default();
        let found = rungs(&mut monologue, &[false; 10], &limits);
        assert!(found.iter().all(Option::is_none), "{found:?}");
    }
}
