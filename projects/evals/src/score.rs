//! What a run gives, and how two runs compare.

use std::{collections::BTreeMap, fs, path::Path};

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};

/// The problem of a run that ran out of time. Its counts stop at the timeout, so they are
/// left out of the means; reports from before they were kept hold zeros.
pub const TIMED_OUT: &str = "timed out";

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct Outcome {
    pub task: String,
    pub passed: bool,
    /// Why there is no verdict on the work, such as a timeout or a dead provider.
    pub problem: Option<String>,
    /// What the check said about a failure.
    #[serde(default)]
    pub why: String,
    pub seconds: u64,
    pub model_calls: u64,
    /// Prompt and completion tokens of every model call, subagents included.
    pub tokens: u64,
    /// The prompt split by the provider's cache: served from it, not, and written to it.
    #[serde(default)]
    pub cached_prompt_tokens: u64,
    #[serde(default)]
    pub uncached_prompt_tokens: u64,
    #[serde(default)]
    pub cache_write_tokens: u64,
    /// What the model wrote, its reasoning included, and that reasoning alone.
    #[serde(default)]
    pub output_tokens: u64,
    #[serde(default)]
    pub reasoning_tokens: u64,
    pub tool_calls: u64,
    pub tool_errors: u64,
    /// Model calls tried again after a failure that may pass.
    #[serde(default)]
    pub retries: u64,
    /// Tool calls with the name and arguments of an earlier call of the same agent.
    #[serde(default)]
    pub reruns: u64,
    /// Times the core told a model, of the chat or an agent, that it repeats itself and
    /// let it go on.
    #[serde(default)]
    pub repeat_notices: u64,
    /// Turns the core ended because the same error kept coming back: the number that
    /// judges `limits.failure_ladder`.
    #[serde(default)]
    pub repeat_stops: u64,
    /// Times old tool results were put away (`limits.stub_results`), how many, and the
    /// estimated tokens each later request carries less, summed over the moves.
    #[serde(default)]
    pub stub_moves: u64,
    #[serde(default)]
    pub stubbed_results: u64,
    #[serde(default)]
    pub stub_saved_tokens: u64,
    /// Command results sent shaped (`limits.shape_output`), and the estimated tokens each
    /// later request carries less, summed over them.
    #[serde(default)]
    pub shaped_results: u64,
    #[serde(default)]
    pub shape_saved_tokens: u64,
    /// Episodes the core wrote down (`limits.episodes`), of the chat and its agents.
    #[serde(default)]
    pub episodes: u64,
    /// `read_history` calls that read one stored result back.
    #[serde(default)]
    pub retrievals: u64,
    /// Compactions that finished.
    #[serde(default)]
    pub compactions: u64,
    pub subagents: u64,
    pub questions: u64,
    /// The tool event after which the runner killed the core and started it again
    /// (`kill_after`); none when the task asks for no kill or the run ended first.
    #[serde(default)]
    pub killed_after: Option<u64>,
}

impl Outcome {
    fn timed_out(&self) -> bool {
        self.problem.as_deref() == Some(TIMED_OUT)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct Report {
    pub model: String,
    pub prompt: String,
    /// The `--set` keys of the run, by name.
    #[serde(default)]
    pub settings: BTreeMap<String, String>,
    /// Picks the kill point of each run of a task with `kill_after`.
    #[serde(default)]
    pub seed: u64,
    pub outcomes: Vec<Outcome>,
}

/// Sums over every run; means over the runs that did not time out.
struct Totals {
    runs: usize,
    passed: usize,
    broken: usize,
    timed_out: usize,
    tokens_spent: u64,
    calls: f64,
    tokens: f64,
    cached: f64,
    uncached: f64,
    cache_written: f64,
    output: f64,
    reasoning: f64,
    seconds: f64,
    errors: f64,
    retries: f64,
    reruns: f64,
    repeat_notices: f64,
    repeat_stops: f64,
    stub_moves: f64,
    stubbed_results: f64,
    stub_saved_tokens: f64,
    shaped_results: f64,
    shape_saved_tokens: f64,
    episodes: f64,
    retrievals: f64,
    compactions: f64,
}

/// Saturates: a report is a file anyone may have edited.
fn sum<'a>(outcomes: impl Iterator<Item = &'a Outcome>, value: fn(&Outcome) -> u64) -> u64 {
    outcomes.fold(0, |total, outcome| total.saturating_add(value(outcome)))
}

fn totals(outcomes: &[&Outcome]) -> Totals {
    let finished: Vec<&Outcome> = outcomes
        .iter()
        .copied()
        .filter(|outcome| !outcome.timed_out())
        .collect();
    let mean = |value: fn(&Outcome) -> u64| {
        sum(finished.iter().copied(), value) as f64 / finished.len().max(1) as f64
    };
    Totals {
        runs: outcomes.len(),
        passed: outcomes.iter().filter(|outcome| outcome.passed).count(),
        broken: outcomes
            .iter()
            .filter(|outcome| outcome.problem.is_some())
            .count(),
        timed_out: outcomes.len() - finished.len(),
        tokens_spent: sum(outcomes.iter().copied(), |outcome| outcome.tokens),
        calls: mean(|outcome| outcome.model_calls),
        tokens: mean(|outcome| outcome.tokens),
        cached: mean(|outcome| outcome.cached_prompt_tokens),
        uncached: mean(|outcome| outcome.uncached_prompt_tokens),
        cache_written: mean(|outcome| outcome.cache_write_tokens),
        output: mean(|outcome| outcome.output_tokens),
        reasoning: mean(|outcome| outcome.reasoning_tokens),
        seconds: mean(|outcome| outcome.seconds),
        errors: mean(|outcome| outcome.tool_errors),
        retries: mean(|outcome| outcome.retries),
        reruns: mean(|outcome| outcome.reruns),
        repeat_notices: mean(|outcome| outcome.repeat_notices),
        repeat_stops: mean(|outcome| outcome.repeat_stops),
        stub_moves: mean(|outcome| outcome.stub_moves),
        stubbed_results: mean(|outcome| outcome.stubbed_results),
        stub_saved_tokens: mean(|outcome| outcome.stub_saved_tokens),
        shaped_results: mean(|outcome| outcome.shaped_results),
        shape_saved_tokens: mean(|outcome| outcome.shape_saved_tokens),
        episodes: mean(|outcome| outcome.episodes),
        retrievals: mean(|outcome| outcome.retrievals),
        compactions: mean(|outcome| outcome.compactions),
    }
}

impl Totals {
    /// The headline: every token spent, failed and timed-out runs included, per task
    /// solved. None when nothing passed.
    fn tokens_per_solved(&self) -> Option<u64> {
        (self.passed > 0).then(|| self.tokens_spent / self.passed as u64)
    }

    fn headline(&self) -> String {
        let rate = 100.0 * self.passed as f64 / self.runs.max(1) as f64;
        format!(
            "passed {}/{} ({rate:.0}%), {} tokens per solved task",
            self.passed,
            self.runs,
            per_solved(self)
        )
    }

    fn means(&self) -> String {
        let left_out = match self.timed_out {
            0 => String::new(),
            count => format!(", {count} timed out and left out"),
        };
        format!(
            "means over {} runs{left_out}: {:.1} calls, {:.0} tokens (prompt {:.0} cached, \
             {:.0} uncached, {:.0} written to cache; {:.0} out of which {:.0} reasoning), \
             {:.1} tool errors, {:.1} retries, {:.1} re-runs, {:.1} repeat notices, {:.2} repeat stops, \
             {:.1} stub moves ({:.1} results, {:.0} tokens saved), {:.1} shaped results \
             ({:.0} tokens saved), {:.2} episodes, {:.1} retrievals, {:.2} compactions, {:.0} s",
            self.runs - self.timed_out,
            self.calls,
            self.tokens,
            self.cached,
            self.uncached,
            self.cache_written,
            self.output,
            self.reasoning,
            self.errors,
            self.retries,
            self.reruns,
            self.repeat_notices,
            self.repeat_stops,
            self.stub_moves,
            self.stubbed_results,
            self.stub_saved_tokens,
            self.shaped_results,
            self.shape_saved_tokens,
            self.episodes,
            self.retrievals,
            self.compactions,
            self.seconds
        )
    }
}

fn per_solved(sum: &Totals) -> String {
    sum.tokens_per_solved()
        .map_or_else(|| "n/a".to_owned(), |tokens| tokens.to_string())
}

fn settings(report: &Report) -> String {
    if report.settings.is_empty() {
        return "no --set".to_owned();
    }
    let pairs: Vec<String> = report
        .settings
        .iter()
        .map(|(name, value)| format!("{name}={value}"))
        .collect();
    format!("--set {}", pairs.join(" "))
}

pub fn print(report: &Report) {
    println!("{}", summary(report));
}

fn summary(report: &Report) -> String {
    let mut lines = vec![
        format!(
            "model {}, prompt {}, {}",
            report.model,
            report.prompt,
            settings(report)
        ),
        format!(
            "{:<24} {:>7} {:>6} {:>8} {:>8} {:>6} {:>5} {:>6} {:>5}",
            "task", "result", "calls", "tokens", "cached", "tools", "errs", "reruns", "secs"
        ),
    ];
    for outcome in &report.outcomes {
        let result = match (&outcome.problem, outcome.passed) {
            (Some(_), _) if outcome.timed_out() => "timeout",
            (Some(_), _) => "broken",
            (None, true) => "pass",
            (None, false) => "FAIL",
        };
        lines.push(format!(
            "{:<24} {result:>7} {:>6} {:>8} {:>8} {:>6} {:>5} {:>6} {:>5}",
            outcome.task,
            outcome.model_calls,
            outcome.tokens,
            outcome.cached_prompt_tokens,
            outcome.tool_calls,
            outcome.tool_errors,
            outcome.reruns,
            outcome.seconds
        ));
    }
    let all: Vec<&Outcome> = report.outcomes.iter().collect();
    let sum = totals(&all);
    lines.push(format!(
        "{}, {} without a verdict",
        sum.headline(),
        sum.broken
    ));
    lines.push(sum.means());
    lines.join("\n")
}

fn read(path: &Path) -> Result<Report> {
    let text =
        fs::read_to_string(path).with_context(|| format!("cannot read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("cannot parse {}", path.display()))
}

pub fn compare(before: &Path, after: &Path) -> Result<()> {
    println!("{}", comparison(&read(before)?, &read(after)?));
    Ok(())
}

fn comparison(before: &Report, after: &Report) -> String {
    let mut lines = vec![
        format!(
            "before: {} / {} / {}",
            before.model,
            before.prompt,
            settings(before)
        ),
        format!(
            "after:  {} / {} / {}",
            after.model,
            after.prompt,
            settings(after)
        ),
    ];
    let mut names: Vec<&String> = before.outcomes.iter().map(|o| &o.task).collect();
    names.sort();
    names.dedup();
    lines.push(format!(
        "{:<24} {:>16} {:>16}   (passed, tokens per solved task)",
        "task", "before", "after"
    ));
    for name in names {
        let task = |report: &Report| {
            let runs: Vec<&Outcome> = report.outcomes.iter().filter(|o| o.task == *name).collect();
            let sum = totals(&runs);
            format!("{}/{} {}", sum.passed, sum.runs, per_solved(&sum))
        };
        lines.push(format!(
            "{name:<24} {:>16} {:>16}",
            task(before),
            task(after)
        ));
    }
    for (label, report) in [("before", before), ("after", after)] {
        let all: Vec<&Outcome> = report.outcomes.iter().collect();
        let sum = totals(&all);
        lines.push(format!("{label:<7} {}", sum.headline()));
        lines.push(format!("{label:<7} {}", sum.means()));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn run(task: &str, passed: bool, problem: Option<&str>, tokens: u64) -> Outcome {
        Outcome {
            task: task.to_owned(),
            passed,
            problem: problem.map(str::to_owned),
            tokens,
            model_calls: 2,
            ..Outcome::default()
        }
    }

    fn report(settings: &[(&str, &str)], outcomes: Vec<Outcome>) -> Report {
        Report {
            model: "m".into(),
            prompt: "shipped".into(),
            settings: settings
                .iter()
                .map(|(name, value)| (name.to_string(), value.to_string()))
                .collect(),
            seed: 1,
            outcomes,
        }
    }

    #[test]
    fn tokens_per_solved_charges_every_run_to_the_passed_ones() {
        let outcomes = [
            run("a", true, None, 100),
            run("a", false, None, 300),
            run("b", true, None, 200),
        ];
        let all: Vec<&Outcome> = outcomes.iter().collect();
        assert_eq!(totals(&all).tokens_per_solved(), Some(300));
        assert!(
            totals(&all)
                .headline()
                .contains("passed 2/3 (67%), 300 tokens")
        );
    }

    #[test]
    fn nothing_passed_is_not_a_number() {
        let outcomes = [run("a", false, None, 100)];
        let all: Vec<&Outcome> = outcomes.iter().collect();
        assert_eq!(totals(&all).tokens_per_solved(), None);
        assert!(
            totals(&all)
                .headline()
                .contains("n/a tokens per solved task")
        );
        assert_eq!(totals(&[]).tokens_per_solved(), None);
    }

    #[test]
    fn timed_out_runs_count_in_the_spend_but_not_in_the_means() {
        let outcomes = [
            run("a", true, None, 100),
            run("a", false, Some(TIMED_OUT), 0),
            run("a", false, Some(TIMED_OUT), 900),
        ];
        let all: Vec<&Outcome> = outcomes.iter().collect();
        let sum = totals(&all);
        assert_eq!(sum.timed_out, 2);
        assert_eq!(sum.tokens, 100.0);
        assert_eq!(sum.calls, 2.0);
        assert_eq!(sum.tokens_per_solved(), Some(1000));
        assert!(
            sum.means()
                .contains("means over 1 runs, 2 timed out and left out")
        );
    }

    #[test]
    fn a_comparison_shows_what_differed_and_the_cost_of_a_solve() {
        let before = report(&[], vec![run("a", true, None, 100)]);
        let after = report(
            &[("limits.max_attempts", "1")],
            vec![run("a", false, Some(TIMED_OUT), 50)],
        );
        let text = comparison(&before, &after);
        assert!(text.contains("before: m / shipped / no --set"));
        assert!(text.contains("after:  m / shipped / --set limits.max_attempts=1"));
        assert!(text.contains("1/1 100"));
        assert!(text.contains("0/1 n/a"));
        assert!(text.contains("after   passed 0/1 (0%), n/a tokens per solved task"));
        assert!(text.contains("after   means over 0 runs, 1 timed out and left out"));
        assert!(
            text.contains("0.0 repeat notices, 0.00 repeat stops"),
            "{text}"
        );
        assert!(
            text.contains(
                "0.0 stub moves (0.0 results, 0 tokens saved), 0.0 shaped results (0 tokens \
                 saved), 0.00 episodes, 0.0 retrievals"
            ),
            "{text}"
        );
        assert!(summary(&after).contains("timeout"));
    }

    #[test]
    fn reports_without_the_new_fields_still_read() {
        let old = r#"{"model":"m","prompt":"p","outcomes":[{"task":"a","passed":true,
            "problem":null,"seconds":1,"model_calls":1,"tokens":5,"tool_calls":0,
            "tool_errors":0,"subagents":0,"questions":0}]}"#;
        let report: Report = serde_json::from_str(old).expect("an old report");
        assert!(report.settings.is_empty());
        assert_eq!(report.outcomes[0].reruns, 0);
        assert_eq!(report.outcomes[0].repeat_notices, 0);
        assert_eq!(report.outcomes[0].repeat_stops, 0);
        let outcome = &report.outcomes[0];
        let stubs = (outcome.stub_moves, outcome.stubbed_results);
        assert_eq!(stubs, (0, 0));
        assert_eq!(outcome.stub_saved_tokens, 0);
        assert_eq!((outcome.shaped_results, outcome.shape_saved_tokens), (0, 0));
        assert_eq!((outcome.retrievals, outcome.compactions), (0, 0));
        assert_eq!(outcome.episodes, 0);
    }

    #[test]
    fn the_means_show_stub_moves_retrievals_and_compactions() {
        let mut moved = run("a", true, None, 100);
        moved.stub_moves = 2;
        moved.stubbed_results = 7;
        moved.stub_saved_tokens = 9_000;
        moved.shaped_results = 3;
        moved.shape_saved_tokens = 1_200;
        moved.episodes = 1;
        moved.retrievals = 1;
        moved.compactions = 1;
        let still = run("a", true, None, 100);
        let outcomes = [moved, still];
        let all: Vec<&Outcome> = outcomes.iter().collect();
        let means = totals(&all).means();
        assert!(
            means.contains(
                "1.0 stub moves (3.5 results, 4500 tokens saved), 1.5 shaped results (600 \
                 tokens saved), 0.50 episodes, 0.5 retrievals, 0.50 compactions"
            ),
            "{means}"
        );
    }
}
