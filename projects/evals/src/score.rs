//! What a run gives, and how two runs compare.

use std::{fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Serialize, Deserialize)]
pub struct Outcome {
    pub task: String,
    pub passed: bool,
    /// Why there is no verdict on the work, such as a timeout or a dead provider.
    pub problem: Option<String>,
    pub seconds: u64,
    pub model_calls: u64,
    pub tokens: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    pub subagents: u64,
    pub questions: u64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub model: String,
    pub prompt: String,
    pub outcomes: Vec<Outcome>,
}

struct Totals {
    runs: usize,
    passed: usize,
    broken: usize,
    calls: f64,
    tokens: f64,
    seconds: f64,
    errors: f64,
}

fn totals(outcomes: &[&Outcome]) -> Totals {
    let runs = outcomes.len();
    let mean = |value: fn(&Outcome) -> u64| {
        outcomes.iter().map(|outcome| value(outcome)).sum::<u64>() as f64 / runs.max(1) as f64
    };
    Totals {
        runs,
        passed: outcomes.iter().filter(|outcome| outcome.passed).count(),
        broken: outcomes
            .iter()
            .filter(|outcome| outcome.problem.is_some())
            .count(),
        calls: mean(|outcome| outcome.model_calls),
        tokens: mean(|outcome| outcome.tokens),
        seconds: mean(|outcome| outcome.seconds),
        errors: mean(|outcome| outcome.tool_errors),
    }
}

pub fn print(report: &Report) {
    println!("model {}, prompt {}", report.model, report.prompt);
    println!(
        "{:<24} {:>6} {:>6} {:>8} {:>6} {:>5} {:>5}",
        "task", "result", "calls", "tokens", "tools", "errs", "secs"
    );
    for outcome in &report.outcomes {
        let result = match (&outcome.problem, outcome.passed) {
            (Some(_), _) => "broken",
            (None, true) => "pass",
            (None, false) => "FAIL",
        };
        println!(
            "{:<24} {result:>6} {:>6} {:>8} {:>6} {:>5} {:>5}",
            outcome.task,
            outcome.model_calls,
            outcome.tokens,
            outcome.tool_calls,
            outcome.tool_errors,
            outcome.seconds
        );
    }
    let all: Vec<&Outcome> = report.outcomes.iter().collect();
    let sum = totals(&all);
    println!(
        "passed {}/{}, {} without a verdict; mean {:.1} calls, {:.0} tokens, {:.1} tool errors, {:.0} s",
        sum.passed, sum.runs, sum.broken, sum.calls, sum.tokens, sum.errors, sum.seconds
    );
}

fn read(path: &Path) -> Result<Report> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))
}

pub fn compare(before: &Path, after: &Path) -> Result<()> {
    let (before, after) = (read(before)?, read(after)?);
    println!("before: {} / {}", before.model, before.prompt);
    println!("after:  {} / {}", after.model, after.prompt);
    let mut names: Vec<&String> = before.outcomes.iter().map(|o| &o.task).collect();
    names.sort();
    names.dedup();
    println!("{:<24} {:>9} {:>9}", "task", "before", "after");
    for name in names {
        let passed = |report: &Report| {
            let runs: Vec<&Outcome> = report.outcomes.iter().filter(|o| o.task == *name).collect();
            let sum = totals(&runs);
            format!("{}/{}", sum.passed, sum.runs)
        };
        println!("{name:<24} {:>9} {:>9}", passed(&before), passed(&after));
    }
    for (label, report) in [("before", &before), ("after", &after)] {
        let all: Vec<&Outcome> = report.outcomes.iter().collect();
        let sum = totals(&all);
        println!(
            "{label:<7} passed {}/{}; mean {:.1} calls, {:.0} tokens, {:.1} tool errors, {:.0} s",
            sum.passed, sum.runs, sum.calls, sum.tokens, sum.errors, sum.seconds
        );
    }
    Ok(())
}
