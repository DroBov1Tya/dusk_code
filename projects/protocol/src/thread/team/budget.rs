//! A team's budget as a person edits it: four fields of text, read back with the same
//! limits the core holds it to.

use anyhow::{Context, Result};

use super::TeamBudget;

/// The parts of a budget in the order a form shows them, with their names.
pub const BUDGET_FIELDS: [&str; 4] = ["Members at once", "Steps", "Tokens", "Minutes"];

impl TeamBudget {
    /// The budget as the text of its four fields, each read back by `read` to exactly
    /// what it is: tokens as `4M` or `250k` only when that is exact, else every digit, so
    /// a save that changes another field leaves the tokens as they were.
    pub fn fields(self) -> [String; 4] {
        [
            self.at_once().to_string(),
            self.steps().to_string(),
            exact_tokens(self.tokens()),
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

/// `4M`, `250k`, or every digit when no suffix is exact.
fn exact_tokens(tokens: u64) -> String {
    match tokens {
        0 => "0".into(),
        _ if tokens.is_multiple_of(1_000_000) => format!("{}M", tokens / 1_000_000),
        _ if tokens.is_multiple_of(1_000) => format!("{}k", tokens / 1_000),
        _ => tokens.to_string(),
    }
}

/// `4.1` of `4.1M` times `unit`, in integers so 4.1M is 4,100,000 exactly. A decimal finer
/// than the unit (`1.2345k`) is refused rather than rounded.
fn scaled(text: &str, unit: u64) -> Option<u64> {
    let (whole, fraction) = text.trim().split_once('.').unwrap_or((text.trim(), ""));
    let digits = |part: &str| part.bytes().all(|byte| byte.is_ascii_digit());
    if whole.is_empty() && fraction.is_empty() || !digits(whole) || !digits(fraction) {
        return None;
    }
    let places = u32::try_from(fraction.len()).ok()?;
    let scale = 10u64.checked_pow(places)?;
    if !unit.is_multiple_of(scale) {
        return None;
    }
    let whole: u64 = if whole.is_empty() {
        0
    } else {
        whole.parse().ok()?
    };
    let fraction: u64 = if fraction.is_empty() {
        0
    } else {
        fraction.parse().ok()?
    };
    whole
        .checked_mul(unit)?
        .checked_add(fraction.checked_mul(unit / scale)?)
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

    /// What a form shows reads back to the very budget, so saving after a change to one
    /// field leaves the others as they were.
    #[test]
    fn every_field_reads_back_exactly() {
        for tokens in [1_234_567, 1_250_000, 4_100_000, 4_000_000, 250_000, 999] {
            let budget = TeamBudget::new(3, 400, tokens, 120).unwrap();
            let fields = budget.fields();
            let texts = fields.each_ref().map(String::as_str);
            assert_eq!(TeamBudget::read(texts).unwrap(), budget, "{fields:?}");
            // Only "Members at once" changed: the tokens stay what they were.
            let edited = TeamBudget::read(["5", texts[1], texts[2], texts[3]]).unwrap();
            assert_eq!((edited.at_once(), edited.tokens()), (5, tokens));
        }
        assert_eq!(
            TeamBudget::new(3, 400, 1_234_567, 120).unwrap().fields()[2],
            "1234567"
        );
        let read = |tokens: &str| TeamBudget::read(["1", "1", tokens, "1"]).map(|b| b.tokens());
        assert_eq!(read("4.1M").unwrap(), 4_100_000);
        assert_eq!(read("1.15k").unwrap(), 1_150);
        assert_eq!(read(".5M").unwrap(), 500_000);
        assert!(read("1.2345k").is_err() && read("1.k.").is_err() && read("M").is_err());
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
