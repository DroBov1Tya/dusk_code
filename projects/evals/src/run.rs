//! Runs the tasks through the core, as a client would, each in a thread and folder of its
//! own. The config folder is a temporary one, so the user's stays as it is: only config.yml
//! is copied in, for the provider, and the rules are the ones in this repository.

use std::{
    collections::BTreeSet,
    fs,
    path::Path,
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use dusk_core::{
    config::Config,
    init::paths::Paths,
    providers::ModelEvent,
    runtime::{Runtime, ThreadEvent},
};

use crate::{
    args::Options,
    sandbox::{self, TempHome},
    score::{self, Outcome, Report},
    task::{self, Task},
};

const SOURCE: &str = "evals";

pub async fn all(options: &Options) -> Result<()> {
    let tasks = task::load(&options.tasks, &options.only)?;
    let home = TempHome::new("dusk-evals")?;
    let paths = Paths::new(home.0.join("config"), home.0.join("workspace"));
    fs::create_dir(paths.config_root())?;
    sandbox::private(paths.config_root(), 0o700)?;
    fs::copy(&options.config, paths.config_file())
        .with_context(|| format!("cannot read {}", options.config.display()))?;
    sandbox::private(&paths.config_file(), 0o600)?;
    let mut prompt = "shipped".to_owned();
    if let Some(file) = &options.main_prompt {
        fs::create_dir_all(paths.modes_dir())?;
        fs::copy(file, paths.main_prompt()).context("cannot read the prompt")?;
        prompt = file.display().to_string();
    }
    let runtime = Runtime::start(paths, None).await?;
    if let Some(mode) = &options.mode {
        runtime
            .config()
            .update(|config: &mut Config| config.set_mode(mode.clone()))?;
        prompt = format!("{prompt}, mode {mode}");
    }
    // The core looks for a model in the background when the config names none.
    while runtime.discovering_model() {
        tokio::time::sleep(Duration::from_millis(100)).await;
    }
    let model = runtime
        .model()
        .map_or("none".to_owned(), |choice| choice.name());
    let mut outcomes = Vec::new();
    for task in &tasks {
        for run in 1..=options.runs {
            eprintln!("{} run {run}/{}", task.name, options.runs);
            let timeout = Duration::from_secs(options.timeout_seconds);
            let outcome = one(&runtime, task, &home.0, timeout).await;
            outcomes.push(outcome.unwrap_or_else(|error| Outcome {
                task: task.name.clone(),
                passed: false,
                problem: Some(format!("{error:#}")),
                seconds: 0,
                model_calls: 0,
                tokens: 0,
                tool_calls: 0,
                tool_errors: 0,
                subagents: 0,
                questions: 0,
            }));
        }
    }
    runtime.shutdown().await;
    let report = Report {
        model,
        prompt,
        outcomes,
    };
    score::print(&report);
    if let Some(parent) = options.out.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(&options.out, serde_json::to_string_pretty(&report)?)?;
    Ok(())
}

/// What is known of a task while it runs.
#[derive(Default)]
struct Watch {
    answer: String,
    turns: u64,
    idle: bool,
    agents_at_work: BTreeSet<String>,
    /// The chat's latest counts; they take in those of its subagents.
    spent: u64,
    model_calls: u64,
    tool_calls: u64,
    tool_errors: u64,
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
            ThreadEvent::ToolStarted(_) => self.tool_calls += 1,
            ThreadEvent::ToolFinished { result, .. } => {
                self.tool_errors += u64::from(result.is_error);
            }
            ThreadEvent::Question(_) => self.questions += 1,
            ThreadEvent::Tokens { tokens, .. } if thread.is_none() => {
                self.spent = tokens.spent.prompt_tokens + tokens.spent.completion_tokens;
                self.model_calls = tokens.model_calls;
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
            ThreadEvent::ProviderUnavailable if thread.is_none() => {
                self.problem = Some("the provider is unavailable".into());
            }
            ThreadEvent::Rejected { reason } if thread.is_none() => self.problem = Some(reason),
            _ => {}
        }
    }

    fn done(&self) -> bool {
        self.problem.is_some() || self.turns > 0 && self.idle && self.agents_at_work.is_empty()
    }
}

async fn one(runtime: &Runtime, task: &Task, home: &Path, timeout: Duration) -> Result<Outcome> {
    let project = home.join(format!("project-{}", uuid::Uuid::now_v7()));
    sandbox::prepare(task, &project)?;
    let started = Instant::now();
    let mut handle = runtime.new_thread(SOURCE, &project).await?;
    handle.input.send(task.prompt.clone()).await?;
    let mut watch = Watch::default();
    let waited = tokio::time::timeout(timeout, async {
        while !watch.done() {
            match handle.output.next_event().await {
                Some(event) => watch.see(event, None),
                None => break,
            }
        }
    })
    .await;
    let seconds = started.elapsed().as_secs();
    let problem = match waited {
        Ok(()) => watch.problem.take(),
        Err(_) => Some("timed out".to_owned()),
    };
    drop(handle);
    // Timed-out work is judged as it stands: it counts as a failure whatever it holds.
    let passed = problem.is_none() && sandbox::check(task, &project, &watch.answer, home)?;
    Ok(Outcome {
        task: task.name.clone(),
        passed,
        problem,
        seconds,
        model_calls: watch.model_calls,
        tokens: watch.spent,
        tool_calls: watch.tool_calls,
        tool_errors: watch.tool_errors,
        subagents: watch.subagents,
        questions: watch.questions,
    })
}
