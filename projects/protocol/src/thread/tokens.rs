//! A thread's token counts.

use serde::{Deserialize, Serialize};

use crate::model::Usage;
/// Compacting starts once the context passes this share of the input limit. The rest is
/// room for the summary request and its answer, so 256k compacts at about 205k.
pub const AUTO_COMPACT_PERCENT: u64 = 80;

#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ThreadTokens {
    /// The best known size of the current context: what the provider counted for the
    /// last call plus estimates of what came after it. Before any call, `current.total()`.
    pub context: u64,
    /// What the model receives now: the system messages, the newest summary and the
    /// messages after it.
    pub current: TokenCounts,
    /// Every message and summary the thread ever had.
    pub all: TokenCounts,
    /// Sums of what the provider reported over every model call of the thread and of the
    /// subagents it asked.
    pub spent: Usage,
    pub model_calls: u64,
    /// The summed cost of the calls whose cost is known.
    #[serde(default)]
    pub cost_micro_usd: u64,
    /// Calls with no known cost: the provider did not report it and no price was set.
    #[serde(default)]
    pub unpriced_calls: u64,
}

impl ThreadTokens {
    /// Tokens sent and written over every call.
    pub fn spent_tokens(&self) -> u64 {
        self.spent
            .prompt_tokens
            .saturating_add(self.spent.completion_tokens)
    }

    /// The share of the prompt tokens served from the cache, from 0 to 100.
    pub fn cached_percent(&self) -> u64 {
        let cached = u128::from(self.spent.cached_prompt_tokens) * 100;
        let share = cached / u128::from(self.spent.prompt_tokens.max(1));
        u64::try_from(share).unwrap_or(u64::MAX)
    }

    /// Spent tokens per finished task; none before one is done.
    pub fn per_task(&self, done: u32) -> Option<u64> {
        (done > 0).then(|| self.spent_tokens() / u64::from(done))
    }

    /// What the thread cost, as `$0.42`; `+` when some calls have no known cost. None
    /// when no call has one, so free and local models show nothing.
    pub fn cost(&self) -> Option<String> {
        if self.unpriced_calls >= self.model_calls {
            return None;
        }
        let more = if self.unpriced_calls > 0 { "+" } else { "" };
        Some(format!(
            "{}{more}",
            crate::model::Cost::dollars(self.cost_micro_usd)
        ))
    }
}

/// "1 file", "2 files": a count with its noun in the plural unless it is one.
pub fn plural<N>(count: N, noun: &str) -> String
where
    N: std::fmt::Display + PartialEq + From<u8>,
{
    let ending = if count == N::from(1) { "" } else { "s" };
    format!("{count} {noun}{ending}")
}

/// A count as every client says it: "950", "18.4k", "1.2M"; a tenth of zero is left out,
/// so 12000 reads "12k".
pub fn short_count(value: u64) -> String {
    match value {
        0..1_000 => value.to_string(),
        1_000..1_000_000 => format!("{}k", tenths(value, 1_000)),
        _ => format!("{}M", tenths(value, 1_000_000)),
    }
}

/// A size in bytes as every client says it: "950 B", "1.5 KB", "12.0 MB", in steps of
/// 1024 up to TB.
pub fn short_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["KB", "MB", "GB", "TB"];
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64 / 1024.;
    let mut unit = 0;
    // Move up while the number would read 1024.0 or more at one decimal.
    while (value * 10.).round() >= 10_240. && unit + 1 < UNITS.len() {
        value /= 1024.;
        unit += 1;
    }
    format!("{value:.1} {}", UNITS[unit])
}

/// `value / unit` rounded to one decimal, dropped when it is zero.
fn tenths(value: u64, unit: u64) -> String {
    let tenths = (u128::from(value) * 10 + u128::from(unit) / 2) / u128::from(unit);
    match tenths % 10 {
        0 => (tenths / 10).to_string(),
        rest => format!("{}.{rest}", tenths / 10),
    }
}

/// Estimated tokens by who wrote them.
#[cfg_attr(feature = "schema", derive(schemars::JsonSchema))]
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TokenCounts {
    /// The mode's prompt and other system instructions.
    pub system: u64,
    /// What the client sent.
    pub user: u64,
    /// The agent's answers.
    pub assistant: u64,
    /// The agent's thinking; never sent back, so it is always 0 in the current context.
    pub reasoning: u64,
    pub tool_calls: u64,
    pub tool_results: u64,
    /// Compaction summaries, which stand in for the older messages.
    pub summary: u64,
    /// Tool definitions, sent with every request but never stored, so the agent counts
    /// them into the current context only.
    pub tool_specs: u64,
}

impl TokenCounts {
    pub fn total(&self) -> u64 {
        self.system
            + self.user
            + self.assistant
            + self.reasoning
            + self.tool_calls
            + self.tool_results
            + self.summary
            + self.tool_specs
    }

    pub fn add(&mut self, role: &str, content: u64, tool_calls: u64, reasoning: u64) {
        match role {
            "system" => self.system += content,
            "user" => self.user += content,
            "assistant" => self.assistant += content,
            _ => self.tool_results += content,
        }
        self.tool_calls += tool_calls;
        self.reasoning += reasoning;
    }
}

#[cfg(test)]
mod short_count_tests {
    use super::short_count;

    #[test]
    fn counts_read_short_with_one_tenth_at_most() {
        assert_eq!(short_count(0), "0");
        assert_eq!(short_count(999), "999");
        assert_eq!(short_count(1_000), "1k");
        assert_eq!(short_count(18_420), "18.4k");
        assert_eq!(short_count(999_949), "999.9k");
        assert_eq!(short_count(1_250_000), "1.3M");
    }

    #[test]
    fn a_count_names_its_noun_in_the_plural_unless_one() {
        use super::plural;
        assert_eq!(plural(0_u64, "file"), "0 files");
        assert_eq!(plural(1_usize, "file"), "1 file");
        assert_eq!(plural(2_u32, "tool run"), "2 tool runs");
    }

    #[test]
    fn sizes_read_in_steps_of_1024() {
        use super::short_size;
        assert_eq!(short_size(0), "0 B");
        assert_eq!(short_size(1023), "1023 B");
        assert_eq!(short_size(1024), "1.0 KB");
        assert_eq!(short_size(1536), "1.5 KB");
        assert_eq!(short_size(1_048_524), "1023.9 KB");
        assert_eq!(short_size(1_048_575), "1.0 MB");
        assert_eq!(short_size(1_048_576), "1.0 MB");
        assert_eq!(short_size((1 << 30) - 1), "1.0 GB");
        assert_eq!(short_size((1 << 40) - 1), "1.0 TB");
        assert_eq!(short_size(5 * 1024 * 1024), "5.0 MB");
        assert_eq!(short_size(3 << 40), "3.0 TB");
        assert_eq!(short_size(u64::MAX), "16777216.0 TB");
    }

    #[test]
    fn thread_figures_hold_with_nothing_counted() {
        use super::ThreadTokens;
        use crate::model::Usage;
        let mut tokens = ThreadTokens::default();
        assert_eq!(tokens.spent_tokens(), 0);
        assert_eq!(tokens.cached_percent(), 0, "no prompt is no share");
        assert_eq!(tokens.per_task(0), None);
        assert_eq!(tokens.per_task(3), Some(0));
        tokens.spent = Usage {
            prompt_tokens: 900,
            cached_prompt_tokens: 600,
            completion_tokens: 300,
            ..Usage::default()
        };
        assert_eq!(tokens.spent_tokens(), 1_200);
        assert_eq!(tokens.cached_percent(), 66, "rounded down");
        assert_eq!(tokens.per_task(0), None, "before a task is done");
        assert_eq!(tokens.per_task(5), Some(240));
        tokens.spent.prompt_tokens = u64::MAX;
        tokens.spent.cached_prompt_tokens = u64::MAX;
        assert_eq!(tokens.spent_tokens(), u64::MAX);
        assert_eq!(tokens.cached_percent(), 100);
    }
}
