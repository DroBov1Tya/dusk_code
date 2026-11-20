//! A team's budget as a person edits it: four fields of text, read back with the same
//! limits the core holds it to.

use anyhow::{Context, Result};

use super::TeamBudget;

/// The parts of a budget in the order a form shows them, with their names.
pub const BUDGET_FIELDS: [&str; 4] = ["Members at once", "Steps", "Tokens", "Minutes"];

impl TeamBudget {
    /// The budget as the text of its four fields: tokens as `4M` or `250k`.
    pub fn fields(self) -> [String; 4] {
        [
            self.at_once().to_string(),
            self.steps().to_string(),
            crate::thread::short_count(self.tokens()),
            self.minutes().to_string(),
        ]
    }

    /// Reads the four fields back. Tokens may end in `k` or `M`; every part must be
    /// within the largest ceilings, and the core holds it to its own, lower ones.
    pub fn read(fields: [&str; 4]) -> Result<Self> {
        let [at_once, steps, tokens, minutes] = fields.map(str::trim);
        let number = |name: &str, text: &str| -> Result<u64> {
            text.replace(['_', ','], "")
                .parse::<u64>()
                .with_context(|| format!("{name} must be a whole number, not \"{text}\""))
        };
        let small = |name: &str, text: &str| -> Result<u32> {
            u32::try_from(number(name, text)?).with_context(|| format!("{name} is too large"))
        };
        let tokens = match tokens.char_indices().last() {
            Some((at, 'k' | 'K')) => scaled(&tokens[..at], 1_000),
            Some((at, 'm' | 'M')) => scaled(&tokens[..at], 1_000_000),
            _ => number("tokens", tokens).ok(),
        }
        .with_context(|| format!("tokens must be a number such as 400k or 4M, not \"{tokens}\""))?;
        Self::new(
            small("members at once", at_once)?,
            small("steps", steps)?,
            tokens,
            small("minutes", minutes)?,
        )
    }
}

/// `1.5` of `1.5M` times `unit`, rounded down.
fn scaled(text: &str, unit: u64) -> Option<u64> {
    let value: f64 = text.trim().parse().ok()?;
    (value.is_finite() && value >= 0.0).then_some((value * unit as f64) as u64)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_budget_reads_back_from_its_fields() {
        let budget = TeamBudget::new(3, 400, 4_000_000, 120).unwrap();
        let fields = budget.fields();
        assert_eq!(fields, ["3", "400", "4M", "120"]);
        let texts = [&*fields[0], &*fields[1], &*fields[2], &*fields[3]];
        assert_eq!(TeamBudget::read(texts).unwrap(), budget);
        let raised = TeamBudget::read([" 4 ", "1_000", "1.5M", "90"]).unwrap();
        assert_eq!(raised, TeamBudget::new(4, 1_000, 1_500_000, 90).unwrap());
        assert_eq!(
            TeamBudget::read(["2", "10", "250k", "5"]).unwrap().tokens(),
            250_000
        );
    }

    #[test]
    fn a_budget_past_its_ceilings_or_not_a_number_is_refused() {
        for fields in [
            ["0", "400", "4M", "120"],
            ["9", "400", "4M", "120"],
            ["3", "lots", "4M", "120"],
            ["3", "400", "-1k", "120"],
            ["3", "400", "4G", "120"],
            ["3", "400", "4M", "99999999999"],
            ["3", "400", "4M", ""],
        ] {
            assert!(TeamBudget::read(fields).is_err(), "{fields:?}");
        }
    }
}
