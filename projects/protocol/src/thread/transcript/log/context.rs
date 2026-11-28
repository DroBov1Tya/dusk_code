//! How full an agent's context is and how often it was compacted, in a line for the
//! lists of agents.

use super::ChatLog;
use crate::thread::{ContextReport, plural};

/// How full a context is, `used` of `limit` tokens, and how often it was compacted, as
/// every client shows it for an agent: `42% context · 2 compactions`.
pub fn context_line(tokens: Option<(u64, u64)>, compactions: u32) -> Option<String> {
    let counted = plural(compactions, "compaction");
    match tokens {
        Some((used, limit)) if limit > 0 => {
            let percent = (used.saturating_mul(100) / limit).min(100);
            Some(format!("{percent}% context \u{00B7} {counted}"))
        }
        _ if compactions > 0 => Some(counted),
        _ => None,
    }
}

impl<L> ChatLog<L> {
    /// `42% context · 2 compactions`; none until the context or a compaction is known.
    pub fn context_line(&self) -> Option<String> {
        context_line(self.tokens, self.compactions)
    }

    /// Whether the line still needs the core's report: no context was heard yet.
    pub fn context_unknown(&self) -> bool {
        self.tokens.is_none_or(|(_, limit)| limit == 0)
    }

    /// The core's report on the thread, asked for an agent whose events this client did
    /// not see: live events heard since stand over it.
    pub fn context_report(&mut self, report: &ContextReport) {
        if self.context_unknown() {
            self.tokens = Some((report.total, report.limit));
        }
        self.compactions = self.compactions.max(report.compactions);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::thread::ThreadEvent;

    #[test]
    fn the_line_counts_compactions_and_live_events_stand_over_the_report() {
        let mut chat = ChatLog::<std::convert::Infallible>::default();
        assert_eq!(chat.context_line(), None);
        let report = ContextReport {
            total: 50_000,
            limit: 200_000,
            sections: Vec::new(),
            sources: Vec::new(),
            compactions: 1,
        };
        chat.context_report(&report);
        assert_eq!(
            chat.context_line().unwrap(),
            "25% context \u{00B7} 1 compaction"
        );
        chat.apply(ThreadEvent::CompactionStarted, 0);
        chat.apply(
            ThreadEvent::CompactionFinished(crate::thread::Compaction {
                compaction_id: 1,
                thread_id: "t".into(),
                through_sequence: 4,
                summary: "so far".into(),
                summary_tokens: 3,
            }),
            0,
        );
        chat.apply(
            ThreadEvent::ModelSelected {
                name: "m".into(),
                input_limit: 100_000,
                effort: None,
            },
            0,
        );
        // A report read before the last compaction does not take the count back.
        chat.context_report(&report);
        assert_eq!(
            chat.context_line().unwrap(),
            "50% context \u{00B7} 2 compactions"
        );
    }
}
