//! Runs the tasks through the core, as a client would, each in a thread and folder of its
//! own. The config folder is a temporary one, so the user's stays as it is: only config.yml
//! is copied in, for the provider, and the rules are the ones in this repository.

use std::{
    collections::{BTreeSet, HashSet},
    fs,
    path::Path,
    sync::LazyLock,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use dusk_core::{
    app::{self, Paths},
    client::Client,
};
use dusk_protocol::{
    client::{Event, Hello, PROTOCOL_VERSION},
    model::{ModelEvent, Usage},
    thread::{RepeatRung, ThreadEvent},
};
use regex::Regex;
use tokio::sync::mpsc::UnboundedReceiver;

use crate::{
    args::Options,
    overrides::{self, Setting},
    sandbox::{self, TempHome},
    score::{self, Outcome, Report, TIMED_OUT},
    task::{self, Task},
};

const SOURCE: &str = "evals";
/// How long a timed-out run may take to stop its turn and report what it spent.
const SETTLE: Duration = Duration::from_secs(20);
static DELETES: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\b(rm|rmtree|unlink|rmdir)\b").expect("a valid pattern"));

pub async fn all(options: &Options) -> Result<()> {
    let tasks = task::load(&options.tasks, &options.only)?;
    let home = TempHome::new("dusk-evals")?;
    let paths = Paths::new(home.0.join("config"), home.0.join("workspace"));
    fs::create_dir(paths.config_root())?;
    sandbox::private(paths.config_root(), 0o700)?;
    let config = fs::read(&options.config)
        .with_context(|| format!("cannot read {}", options.config.display()))?;
    let config = overrides::apply(&config, &forced(&options.settings)?)?.into_bytes();
    fs::write(paths.config_file(), config)?;
    sandbox::private(&paths.config_file(), 0o600)?;
    let mut prompt = "shipped".to_owned();
    if let Some(file) = &options.main_prompt {
        fs::create_dir_all(paths.modes_dir())?;
        fs::copy(file, paths.main_prompt()).context("cannot read the prompt")?;
        prompt = file.display().to_string();
    }
    let hello = Hello {
        protocol: PROTOCOL_VERSION,
        client: SOURCE.to_owned(),
    };
    // The core refuses a key it does not know, so a misspelt --set ends the run here.
    let mut client = app::start(paths, None, hello).await.with_context(|| {
        if options.settings.is_empty() {
            "cannot start the core"
        } else {
            "cannot start the core with the --set keys"
        }
    })?;
    if let Some(mode) = &options.mode {
        client.settings().mode(mode).await?;
        prompt = format!("{prompt}, mode {mode}");
    }
    // The core looks for a model in the background when the config names none.
    while client.status.borrow_and_update().discovering_model {
        if client.status.changed().await.is_err() {
            break;
        }
    }
    let model = client
        .status
        .borrow()
        .model
        .clone()
        .unwrap_or_else(|| "none".to_owned());
    let mut outcomes = Vec::new();
    for task in &tasks {
        for run in 1..=options.runs {
            eprintln!("{} run {run}/{}", task.name, options.runs);
            let timeout = Duration::from_secs(options.timeout_seconds);
            let outcome = one(&mut client, task, &home.0, timeout).await;
            outcomes.push(outcome.unwrap_or_else(|error| Outcome {
                task: task.name.clone(),
                problem: Some(format!("{error:#}")),
                ..Outcome::default()
            }));
        }
    }
    client.shutdown().await;
    let report = Report {
        model,
        prompt,
        settings: overrides::recorded(&options.settings),
        outcomes,
    };
    score::print(&report);
    if let Some(parent) = options.out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.out, serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

/// A run's own settings: `settings` and, unless they set it, a turn that fails when its
/// provider cannot serve it, as before turns waited. A rate limit would otherwise hold a
/// task for hours, and a run measures the harness, not the provider's patience.
fn forced(settings: &[Setting]) -> Result<Vec<Setting>> {
    let fail = Setting::parse("limits.wait_for_provider=false").map_err(anyhow::Error::msg)?;
    let mut all = settings.to_vec();
    if !settings.iter().any(|setting| setting.name == fail.name) {
        all.push(fail);
    }
    Ok(all)
}

/// What is known of a task while it runs.
#[derive(Default)]
struct Watch {
    answer: String,
    turns: u64,
    idle: bool,
    agents_at_work: BTreeSet<String>,
    /// The chat's latest counts; they take in those of its subagents.
    spent: Usage,
    model_calls: u64,
    /// Token counts the chat sent; one comes after each of its turns.
    reports: u64,
    tool_calls: u64,
    tool_errors: u64,
    retries: u64,
    reruns: u64,
    repeat_notices: u64,
    repeat_stops: u64,
    stub_moves: u64,
    stubbed_results: u64,
    stub_saved_tokens: u64,
    shaped_results: u64,
    shape_saved_tokens: u64,
    retrievals: u64,
    compactions: u64,
    /// Every call so far, as the agent's thread ("" for the chat) and name and arguments.
    calls: HashSet<(String, String)>,
    subagents: u64,
    questions: u64,
    problem: Option<String>,
}

impl Watch {
    fn see(&mut self, event: ThreadEvent, thread: Option<&str>) {
        match event {
            ThreadEvent::Subagent { thread_id, event } => self.see(*event, Some(&thread_id)),
            ThreadEvent::SubagentStarted { thread_id, .. } => {
                self.subagents += 1;
                self.agents_at_work.insert(thread_id);
            }
            ThreadEvent::ToolStarted(call) => {
                self.tool_calls += 1;
                // A `result` is a retrieval of a stub; null is a plain read.
                let result = call
                    .arguments
                    .get("result")
                    .filter(|result| !result.is_null());
                let reads_back = call.name == "read_history" && result.is_some();
                self.retrievals += u64::from(reads_back);
                let key = format!("{} {}", call.name, call.arguments);
                self.reruns += u64::from(
                    !self
                        .calls
                        .insert((thread.unwrap_or_default().to_owned(), key)),
                );
                // Agents run on the real machine: what they delete is worth a record.
                let command = call
                    .arguments
                    .get("command")
                    .and_then(|command| command.as_str());
                if let Some(command) = command.filter(|command| DELETES.is_match(command)) {
                    eprintln!("deletes: {}", command.chars().take(200).collect::<String>());
                }
            }
            ThreadEvent::ToolFinished { result, .. } => {
                self.tool_errors += u64::from(result.is_error);
            }
            ThreadEvent::Retrying { .. } => self.retries += 1,
            ThreadEvent::RepeatNotice {
                rung: RepeatRung::Stop,
                ..
            } => self.repeat_stops += 1,
            ThreadEvent::RepeatNotice { .. } => self.repeat_notices += 1,
            ThreadEvent::ResultsStubbed {
                results,
                saved_tokens,
            } => {
                self.stub_moves += 1;
                self.stubbed_results += u64::from(results);
                self.stub_saved_tokens = self.stub_saved_tokens.saturating_add(saved_tokens);
            }
            ThreadEvent::ResultShaped { saved_tokens } => {
                self.shaped_results += 1;
                self.shape_saved_tokens = self.shape_saved_tokens.saturating_add(saved_tokens);
            }
            ThreadEvent::CompactionFinished(_) => self.compactions += 1,
            ThreadEvent::Question(_) => self.questions += 1,
            ThreadEvent::Tokens { tokens, .. } if thread.is_none() => {
                self.spent = tokens.spent;
                self.model_calls = tokens.model_calls;
                self.reports += 1;
            }
            ThreadEvent::Model(ModelEvent::Text(text)) if thread.is_none() => {
                self.answer.push_str(&text);
            }
            ThreadEvent::TurnStarted(_) => match thread {
                Some(thread) => {
                    self.agents_at_work.insert(thread.to_owned());
                }
                None => {
                    self.idle = false;
                    self.answer.clear();
                }
            },
            ThreadEvent::TurnFinished(_) if thread.is_none() => self.turns += 1,
            ThreadEvent::Idle => match thread {
                Some(thread) => {
                    self.agents_at_work.remove(thread);
                }
                None => self.idle = true,
            },
            ThreadEvent::NoModel if thread.is_none() => {
                self.problem = Some("no model is chosen".into());
            }
            ThreadEvent::Rejected { reason } if thread.is_none() => self.problem = Some(reason),
            _ => {}
        }
    }

    fn done(&self) -> bool {
        self.problem.is_some() || self.turns > 0 && self.idle && self.agents_at_work.is_empty()
    }
}

/// Feeds the thread's events to `watch` until `enough` holds or the thread ends.
async fn follow(
    events: &mut UnboundedReceiver<Event>,
    thread_id: &str,
    watch: &mut Watch,
    enough: impl Fn(&Watch) -> bool,
) {
    while !enough(watch) {
        match events.recv().await {
            Some(Event::Thread {
                thread_id: id,
                event,
            }) if id == thread_id => watch.see(*event, None),
            Some(Event::Ended { thread_id: id }) if id == thread_id => break,
            Some(_) => {}
            None => break,
        }
    }
}

/// A timed-out run would count as free: its turn is stopped, and the count the chat sends
/// when a turn ends is awaited a while. Calls still running then are not in it.
async fn settle(client: &mut Client, thread_id: &str, watch: &mut Watch) {
    let counted = watch.reports;
    if let Err(error) = client.threads().cancel(thread_id).await {
        eprintln!("cannot stop the timed-out turn: {error:#}");
        return;
    }
    let reported = |watch: &Watch| watch.reports > counted;
    let waited = tokio::time::timeout(
        SETTLE,
        follow(&mut client.events, thread_id, watch, reported),
    )
    .await;
    if waited.is_err() || watch.reports == counted {
        eprintln!("no token count after the timeout; the run keeps the last one");
    }
}

async fn one(client: &mut Client, task: &Task, home: &Path, timeout: Duration) -> Result<Outcome> {
    let project = home.join(format!("project-{}", uuid::Uuid::now_v7()));
    sandbox::prepare(task, &project)?;
    let started = Instant::now();
    let thread_id = client.threads().start(project.clone()).await?;
    client
        .threads()
        .send(&thread_id, task.prompt.clone(), false)
        .await?;
    let mut watch = Watch::default();
    let followed = follow(&mut client.events, &thread_id, &mut watch, Watch::done);
    let waited = tokio::time::timeout(timeout, followed).await;
    let seconds = started.elapsed().as_secs();
    // The state the run stopped in, before stopping the turn changes it.
    let state = format!(
        "turns {}, idle {}, agents at work {}",
        watch.turns,
        watch.idle,
        watch.agents_at_work.len()
    );
    let problem = match waited {
        Ok(()) => watch.problem.take(),
        Err(_) => {
            settle(client, &thread_id, &mut watch).await;
            Some(TIMED_OUT.to_owned())
        }
    };
    client.threads().close(&thread_id).await?;
    // Timed-out work is judged as it stands: it counts as a failure whatever it holds.
    let (passed, why) = match problem {
        None => sandbox::check(task, &project, &watch.answer, home)?,
        // Late work fails the task; the check tells stuck agents from a stuck count.
        Some(_) => {
            let (late, _) = sandbox::check(task, &project, &watch.answer, home)?;
            (false, format!("work passes: {late}; {state}"))
        }
    };
    let spent = watch.spent;
    Ok(Outcome {
        task: task.name.clone(),
        passed,
        problem,
        why,
        seconds,
        model_calls: watch.model_calls,
        tokens: spent.prompt_tokens.saturating_add(spent.completion_tokens),
        cached_prompt_tokens: spent.cached_prompt_tokens,
        uncached_prompt_tokens: spent
            .prompt_tokens
            .saturating_sub(spent.cached_prompt_tokens),
        cache_write_tokens: spent.cache_write_tokens,
        output_tokens: spent.completion_tokens,
        reasoning_tokens: spent.reasoning_tokens,
        tool_calls: watch.tool_calls,
        tool_errors: watch.tool_errors,
        retries: watch.retries,
        reruns: watch.reruns,
        repeat_notices: watch.repeat_notices,
        repeat_stops: watch.repeat_stops,
        stub_moves: watch.stub_moves,
        stubbed_results: watch.stubbed_results,
        stub_saved_tokens: watch.stub_saved_tokens,
        shaped_results: watch.shaped_results,
        shape_saved_tokens: watch.shape_saved_tokens,
        retrievals: watch.retrievals,
        compactions: watch.compactions,
        subagents: watch.subagents,
        questions: watch.questions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeat_notices_and_stops_are_counted_for_the_chat_and_its_agents() {
        let mut watch = Watch::default();
        let notice = |rung| ThreadEvent::RepeatNotice { rung, count: 3 };
        watch.see(notice(RepeatRung::SameCall), None);
        let from_agent = |event| ThreadEvent::Subagent {
            thread_id: "child".into(),
            event: Box::new(event),
        };
        watch.see(from_agent(notice(RepeatRung::Replan)), None);
        watch.see(from_agent(notice(RepeatRung::Stop)), None);
        assert_eq!((watch.repeat_notices, watch.repeat_stops), (2, 1));
    }

    #[test]
    fn stub_moves_retrievals_and_compactions_are_counted_for_the_chat_and_its_agents() {
        let mut watch = Watch::default();
        let stubbed = |results, saved_tokens| ThreadEvent::ResultsStubbed {
            results,
            saved_tokens,
        };
        let read = |arguments| {
            ThreadEvent::ToolStarted(dusk_protocol::model::ToolCall {
                id: "call".into(),
                name: "read_history".into(),
                arguments,
            })
        };
        let from_agent = |event| ThreadEvent::Subagent {
            thread_id: "child".into(),
            event: Box::new(event),
        };
        watch.see(stubbed(3, 9_000), None);
        watch.see(from_agent(stubbed(2, 5_000)), None);
        watch.see(read(serde_json::json!({ "result": 12 })), None);
        watch.see(
            from_agent(read(serde_json::json!({ "result": 4, "search": "x" }))),
            None,
        );
        watch.see(read(serde_json::json!({ "search": "error" })), None);
        watch.see(read(serde_json::json!({ "result": null, "from": 3 })), None);
        let compaction = dusk_protocol::thread::Compaction {
            compaction_id: 1,
            thread_id: "chat".into(),
            through_sequence: 9,
            summary: "so far".into(),
            summary_tokens: 2,
        };
        watch.see(ThreadEvent::CompactionFinished(compaction), None);
        watch.see(ThreadEvent::CompactionFailed { error: "x".into() }, None);
        let shaped = |saved_tokens| ThreadEvent::ResultShaped { saved_tokens };
        watch.see(shaped(300), None);
        watch.see(from_agent(shaped(200)), None);
        assert_eq!((watch.shaped_results, watch.shape_saved_tokens), (2, 500));
        let counts = (
            watch.stub_moves,
            watch.stubbed_results,
            watch.stub_saved_tokens,
            watch.retrievals,
            watch.compactions,
        );
        assert_eq!(counts, (2, 5, 14_000, 2, 1));
        assert_eq!(watch.tool_calls, 4);
    }

    /// A run never waits hours for a provider, unless `--set` asks it to.
    #[test]
    fn a_run_fails_on_a_provider_that_cannot_serve_it() {
        let config = b"model: a/b\n";
        let written = overrides::apply(config, &forced(&[]).unwrap()).unwrap();
        assert!(written.contains("wait_for_provider: false"), "{written}");
        let asked = [Setting::parse("limits.wait_for_provider=true").unwrap()];
        let written = overrides::apply(config, &forced(&asked).unwrap()).unwrap();
        assert!(written.contains("wait_for_provider: true"), "{written}");
    }
}
