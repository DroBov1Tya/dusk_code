//! Tests run again with nothing changed. On a large project a stuck model re-runs the
//! suite, or a reshuffled slice of it, hoping for another answer. A test run is a
//! command whose program and first word name a test runner (`cargo test`, `pytest`,
//! `npm test`, ...); the runs of each runner since a project file last changed are kept
//! with the tests that failed and the change ledger's count at the time. A run that
//! repeats one of them, as the same command or with the same tests failing, gets a note
//! with what that run found. A narrower run to look at one failure fails fewer tests and
//! is not a repeat. The command still runs: tests can be flaky, and a change outside the
//! project (an env var, a service) is not seen.
//!
//! "Changed" reads the ledger when it can be trusted: recording is on and it has recorded
//! a change in this thread before. It sees only this thread's foreground commands, so a
//! call that may write in a way it cannot see (a subagent, a background job, an MCP tool)
//! is a change too. Without a ledger to trust, any call that may write, other than a test
//! run, counts as a change, so a project too large to watch never gets a wrong note.
//!
//! The failing test ids are read from untrusted output: only its last `OUTPUT_BYTES`,
//! line by line, with at most `MAX_IDS` kept, each cut to `MAX_ID_CHARS`.

use std::collections::{BTreeSet, HashMap};

use dusk_protocol::{text::clip_chars, thread::RepeatRung};

use super::signature::{self, RUN_COMMAND, Signature};
use crate::{
    config::Limits, providers::ToolCall, tools::ToolResult,
    tools::console::command_line::program_name,
};

/// The tool that reads and waits on background jobs.
const JOBS: &str = "jobs";
/// Failing tests are listed near the end; this bounds the work on any output.
const OUTPUT_BYTES: usize = 1 << 20;
/// Failing test ids kept per run.
const MAX_IDS: usize = 64;
const MAX_ID_CHARS: usize = 200;
/// Ids a note names.
const SHOWN_IDS: usize = 5;
/// Test runners a turn keeps runs of.
const MAX_RUNNERS: usize = 16;
/// Runs kept per runner since a project file last changed.
const MAX_RUNS: usize = 8;

/// A finished test run.
pub struct TestRun {
    /// The runner, as `cargo test` or `pytest`.
    pub runner: String,
    pub command: String,
    pub exit_code: i32,
    /// Sorted, each once; empty when it passed or none could be read.
    pub failing: Vec<String>,
}

impl TestRun {
    /// The run `call` made, when it ran tests in the foreground and ended.
    pub fn of(call: &ToolCall, result: &ToolResult) -> Option<Self> {
        if call.name != RUN_COMMAND {
            return None;
        }
        let exit_code = result.exit_code?;
        let command = call.arguments["command"].as_str()?;
        let runner = runner(command)?;
        let failing = if exit_code == 0 {
            Vec::new()
        } else {
            failing_ids(&result.content)
        };
        Some(Self {
            runner,
            command: command.split_whitespace().collect::<Vec<_>>().join(" "),
            exit_code,
            failing,
        })
    }

    /// The failure as the ladder counts it: the runner and the failing ids, so a run
    /// filtered or ordered differently with the same failures is the same failure. None
    /// when no failing id was read.
    pub fn signature(&self) -> Option<Signature> {
        if self.failing.is_empty() {
            return None;
        }
        let list = self.failing.join("\n");
        Some(Signature {
            key: format!("tests\n{}\n{list}", self.runner),
            excerpt: clip_chars(&format!("failing tests:\n{list}"), 400, "…"),
            error: format!("failing tests:\n{list}"),
        })
    }

    fn outcome(&self) -> String {
        match (self.exit_code, self.failing.len()) {
            (0, _) => "passed".to_owned(),
            (code, 0) => format!("failed with exit code {code}"),
            (_, count) => {
                let mut ids: Vec<&str> = self
                    .failing
                    .iter()
                    .take(SHOWN_IDS)
                    .map(String::as_str)
                    .collect();
                let more = count.saturating_sub(SHOWN_IDS);
                let and_more = format!("and {more} more");
                if more > 0 {
                    ids.push(&and_more);
                }
                let noun = if count == 1 { "failure" } else { "failures" };
                format!("had {count} {noun}: {}", ids.join(", "))
            }
        }
    }
}

/// A run of one runner, with what had changed by then.
struct Last {
    run: TestRun,
    step: u32,
    ledger: Option<u64>,
    writes: u64,
    unseen: u64,
    /// Repeats in a row, this run included, with nothing changed between.
    streak: u32,
}

impl Last {
    /// Whether `run` repeats this one: the same command, or the same tests failing.
    fn repeated_by(&self, run: &TestRun) -> bool {
        self.run.command == run.command
            || (!self.run.failing.is_empty() && self.run.failing == run.failing)
    }
}

#[derive(Default)]
pub struct TestRuns {
    /// Per runner, its runs since a project file last changed, oldest first.
    runs: HashMap<String, Vec<Last>>,
    /// Calls that may write, test runs left out, so far in the turn.
    writes: u64,
    /// Calls that may write where the ledger does not look, so far in the turn.
    unseen: u64,
    /// A background job was started in the turn: it may write while it runs.
    jobs_started: bool,
}

impl TestRuns {
    /// Notes a call that is not a test run.
    pub fn other(&mut self, call: &ToolCall, result: &ToolResult, read_only: bool) {
        let job_call = call.name == JOBS && self.jobs_started;
        if read_only && !job_call {
            return;
        }
        self.writes += 1;
        // The ledger records only what a foreground command of this thread changed. A
        // look at the jobs is when a job's writes show.
        let foreground = call.name == RUN_COMMAND
            && call.arguments["background"].as_bool() != Some(true)
            && result.exit_code.is_some();
        if !foreground {
            self.unseen += 1;
            self.jobs_started |= call.name == RUN_COMMAND;
        }
    }

    /// Whether a call of the turn may have written where the ledger does not look.
    pub fn unseen_writes(&self) -> bool {
        self.unseen > 0
    }

    /// Notes a test run at `step`, with the ledger's count after it; the rung, count and
    /// note when it repeats a run of its runner with nothing changed since.
    pub fn run(
        &mut self,
        step: u32,
        run: TestRun,
        ledger: Option<u64>,
        limits: &Limits,
    ) -> Option<(RepeatRung, u32, String)> {
        if let Some(runs) = self.runs.get_mut(&run.runner)
            && runs
                .last()
                .is_some_and(|newest| changed(newest, ledger, self.writes, self.unseen))
        {
            runs.clear();
        }
        let repeated = self
            .runs
            .get(&run.runner)
            .and_then(|runs| runs.iter().rev().find(|last| last.repeated_by(&run)));
        let streak = repeated.map_or(1, |last| last.streak.saturating_add(1));
        let found = repeated
            .filter(|_| streak >= limits.test_rerun_note_at)
            .map(|last| {
                if streak >= limits.test_rerun_replan_at {
                    let note = format!(
                        "[the tests ran {streak} times in a row with no project file changed \
                     between; the last run {}. Before your next call, write one sentence with \
                     a different plan, then follow it.]",
                        last.run.outcome()
                    );
                    (RepeatRung::TestRerunReplan, streak, note)
                } else {
                    let note = format!(
                        "[no project file changed since the `{}` run at step {}; it {}. \
                     Running it again gives the same result unless a test is flaky: read that \
                     output, or change the code first.]",
                        run.runner,
                        last.step,
                        last.run.outcome()
                    );
                    (RepeatRung::TestRerun, streak, note)
                }
            });
        if self.runs.len() < MAX_RUNNERS || self.runs.contains_key(&run.runner) {
            let last = Last {
                writes: self.writes,
                unseen: self.unseen,
                run,
                step,
                ledger,
                streak,
            };
            let runs = self.runs.entry(last.run.runner.clone()).or_default();
            if runs.len() == MAX_RUNS {
                runs.remove(0);
            }
            runs.push(last);
        }
        found
    }
}

/// Whether a project file may have changed since `last`, with the ledger at `ledger` and
/// the turn's counts of calls that may write at `writes` and `unseen` now.
fn changed(last: &Last, ledger: Option<u64>, writes: u64, unseen: u64) -> bool {
    let moved = matches!((last.ledger, ledger), (Some(before), Some(now)) if before != now);
    let trusted = ledger.is_some_and(|now| now > 0);
    moved || unseen != last.unseen || (!trusted && writes != last.writes)
}

/// Whether `call` runs tests, so the ledger is read for it.
pub fn is_test_run(call: &ToolCall) -> bool {
    call.name == RUN_COMMAND
        && call.arguments["command"]
            .as_str()
            .and_then(runner)
            .is_some()
}

/// The test runner a command runs, named by its program and the word that makes it run
/// tests; None for any other command.
pub fn runner(command: &str) -> Option<String> {
    let (program, rest) = signature::command_words(command)?;
    let rest: Vec<&str> = rest
        .iter()
        .map(String::as_str)
        .filter(|word| !word.is_empty())
        .collect();
    runner_of(&program, &rest)
}

fn runner_of(program: &str, rest: &[&str]) -> Option<String> {
    let words: Vec<&str> = rest
        .iter()
        .copied()
        .filter(|word| !word.starts_with('-') && !word.starts_with('+'))
        .collect();
    let first = words.first().copied();
    let test_word = |name: &str| (first == Some("test")).then(|| format!("{name} test"));
    match program {
        "cargo" => match first? {
            "test" | "t" => Some("cargo test".to_owned()),
            "nextest" => Some("cargo nextest".to_owned()),
            _ => None,
        },
        "pytest" | "py.test" => Some("pytest".to_owned()),
        "jest" | "vitest" | "mocha" | "rspec" | "phpunit" | "tox" | "ctest" => {
            Some(program.to_owned())
        }
        "npm" | "pnpm" | "yarn" | "bun" => match first? {
            "test" | "t" => Some(format!("{program} test")),
            "run" | "run-script" => {
                let script = words.get(1)?;
                script
                    .contains("test")
                    .then(|| format!("{program} run {script}"))
            }
            _ => None,
        },
        "npx" | "pnpx" | "bunx" => runner_of(
            first?,
            &rest[rest.iter().position(|w| Some(*w) == first)? + 1..],
        ),
        "uv" | "poetry" | "pipenv" if first == Some("run") => {
            let at = rest.iter().position(|w| *w == "run")?;
            let (program, rest) = rest[at + 1..].split_first()?;
            runner_of(program_name(program), rest)
        }
        "bundle" if first == Some("exec") => {
            let at = rest.iter().position(|w| *w == "exec")?;
            let (program, rest) = rest[at + 1..].split_first()?;
            runner_of(program_name(program), rest)
        }
        "make" => words
            .iter()
            .any(|word| matches!(*word, "test" | "tests" | "check"))
            .then(|| "make test".to_owned()),
        "go" | "dotnet" | "mvn" | "gradle" | "gradlew" | "swift" | "deno" | "mix" | "zig" => {
            test_word(program)
        }
        python if python == "python" || python == "python3" || python.starts_with("python3.") => {
            let at = rest.iter().position(|w| *w == "-m")?;
            match *rest.get(at + 1)? {
                "pytest" => Some("pytest".to_owned()),
                "unittest" => Some("python unittest".to_owned()),
                _ => None,
            }
        }
        _ => None,
    }
}

/// The tests a run's output says failed, sorted and each once, in the forms of libtest,
/// nextest, pytest, go test, jest, vitest and rspec.
pub fn failing_ids(output: &str) -> Vec<String> {
    let start = output.ceil_char_boundary(output.len().saturating_sub(OUTPUT_BYTES));
    let mut ids = BTreeSet::new();
    for line in output[start..].lines() {
        if ids.len() >= MAX_IDS {
            break;
        }
        if let Some(id) = failing_id(line.trim())
            && !id.is_empty()
        {
            ids.insert(clip_chars(id, MAX_ID_CHARS, "…"));
        }
    }
    ids.into_iter().collect()
}

fn first_word(text: &str) -> &str {
    text.split_whitespace().next().unwrap_or_default()
}

fn failing_id(line: &str) -> Option<&str> {
    // libtest: `test tests::adds ... FAILED`
    if let Some(name) = line
        .strip_prefix("test ")
        .and_then(|rest| rest.strip_suffix(" ... FAILED"))
    {
        return Some(name.trim());
    }
    // nextest: `FAIL [   0.004s] core tests::adds`
    if let Some(rest) = line.strip_prefix("FAIL [") {
        return rest.split_once(']').map(|(_, name)| name.trim());
    }
    // go test: `--- FAIL: TestAdds (0.00s)`
    if let Some(rest) = line.strip_prefix("--- FAIL: ") {
        return Some(first_word(rest));
    }
    // pytest summary: `FAILED tests/test_a.py::test_x - assert 1 == 2`
    if let Some(rest) = line.strip_prefix("FAILED ") {
        return Some(first_word(rest));
    }
    // pytest: `ERROR tests/test_a.py - ImportError`, `ERROR tests/a.py::test_x`
    if let Some(rest) = line.strip_prefix("ERROR ") {
        let id = first_word(rest);
        return (id.contains("::") || id.ends_with(".py")).then_some(id);
    }
    // rspec: `rspec ./spec/a_spec.rb:12 # adds`
    if let Some(rest) = line.strip_prefix("rspec ") {
        return Some(first_word(rest));
    }
    // jest: `FAIL src/a.test.js (5 s)`, vitest: `FAIL  src/a.test.ts > adds`,
    // go: `FAIL	example.com/pkg	0.01s`
    if let Some(rest) = line.strip_prefix("FAIL")
        && rest.starts_with(char::is_whitespace)
    {
        let rest = rest.trim();
        let rest = rest.split('\t').next().unwrap_or(rest);
        let rest = rest.split(" (").next().unwrap_or(rest);
        return Some(rest.trim());
    }
    // pytest -v: `tests/test_a.py::test_x FAILED [ 50%]`
    let id = first_word(line);
    (id.contains("::") && (line.ends_with(" FAILED") || line.contains(" FAILED ["))).then_some(id)
}
