//! `duskcode run`: one prompt in a new chat, the answer on standard output. Progress goes
//! to standard error, so the answer can be piped. Nobody can approve or answer, so a
//! request for approval is denied and a question gets a note that nobody is there.

use std::{
    io::{Read, Write},
    time::{Duration, Instant},
};

use anyhow::{Context, Result, bail};
use dusk_core::app::Paths;
use dusk_protocol::{
    client::{Event, Hello, PROTOCOL_VERSION},
    config::ApprovalPolicy,
    model::ModelEvent,
    thread::{ThreadEvent, TurnStatus},
    tools::Decision,
};

use super::args::Headless;

const SOURCE: &str = "cli";
/// Questions answered with the note before the run gives up on getting a result.
const MAX_NOTES: u32 = 3;
const NOBODY: &str =
    "Nobody can answer questions in this run. Choose the most reasonable option and go on.";

enum Next {
    Event(Option<Event>),
    Interrupted,
    TimedOut,
}

/// What the run learned, printed at the end.
#[derive(Default)]
struct Result_ {
    answer: String,
    turns: u64,
    model_calls: u64,
    prompt_tokens: u64,
    cached_tokens: u64,
    completion_tokens: u64,
    reasoning_tokens: u64,
    tool_calls: u64,
    tool_errors: u64,
    denied: u64,
    /// Subagents still working; the chat is not done while any is.
    agents: std::collections::BTreeSet<String>,
    error: Option<String>,
}

pub async fn run(options: Headless) -> Result<()> {
    let prompt = match options.prompt.as_str() {
        "-" => {
            let mut text = String::new();
            std::io::stdin()
                .read_to_string(&mut text)
                .context("cannot read the prompt from standard input")?;
            text
        }
        text => text.to_owned(),
    };
    if prompt.trim().is_empty() {
        bail!("the prompt is empty");
    }
    let project = match &options.project {
        Some(dir) => dir.clone(),
        None => std::env::current_dir().context("cannot tell the current folder")?,
    };
    let project = project
        .canonicalize()
        .with_context(|| format!("no such folder: {}", project.display()))?;
    let paths = Paths::from_home()?;
    let _guard = dusk_core::app::RestoreGuard::startup(&paths, None)?;
    let hello = Hello {
        protocol: PROTOCOL_VERSION,
        client: SOURCE.to_owned(),
    };
    let mut client = dusk_core::app::start(paths, None, hello).await?;
    // The core looks for a model in the background when the config names none.
    while client.status.borrow_and_update().discovering_model {
        if client.status.changed().await.is_err() {
            break;
        }
    }
    let thread_id = client.threads().start(project).await?;
    if let Some(mode) = &options.access {
        let policy = match mode.as_str() {
            "ask" => ApprovalPolicy::Ask,
            "sandbox" => ApprovalPolicy::Sandbox,
            "arbiter" => ApprovalPolicy::Arbiter,
            _ => ApprovalPolicy::FullAccess,
        };
        client
            .threads()
            .set_approvals(&thread_id, Some(policy))
            .await?;
    }
    let started = Instant::now();
    client.threads().send(&thread_id, prompt, false).await?;
    let mut seen = Result_::default();
    let mut asked = false;
    let mut notes = 0;
    let mut interrupted = false;
    let limit = options.timeout_seconds.map(Duration::from_secs);
    loop {
        let next = {
            let wait = async {
                tokio::select! {
                    event = client.events.recv() => Next::Event(event),
                    _ = tokio::signal::ctrl_c() => Next::Interrupted,
                }
            };
            match limit.map(|limit| limit.saturating_sub(started.elapsed())) {
                Some(left) => tokio::time::timeout(left, wait)
                    .await
                    .unwrap_or(Next::TimedOut),
                None => wait.await,
            }
        };
        let event = match next {
            Next::Event(Some(event)) => event,
            Next::Event(None) => break,
            Next::Interrupted => {
                interrupted = true;
                break;
            }
            Next::TimedOut => {
                seen.error = Some("timed out".to_owned());
                break;
            }
        };
        match event {
            Event::Thread {
                thread_id: id,
                event,
            } if id == thread_id => match *event {
                ThreadEvent::Approval(request) => {
                    seen.denied += 1;
                    eprintln!(
                        "denied: {:?}; run with --access to allow it",
                        request.request
                    );
                    client.threads().approve(request.id, Decision::Deny).await?;
                }
                ThreadEvent::Question(_) => asked = true,
                ThreadEvent::Idle if asked && notes < MAX_NOTES => {
                    asked = false;
                    notes += 1;
                    client
                        .threads()
                        .send(&thread_id, NOBODY.to_owned(), false)
                        .await?;
                }
                ThreadEvent::Idle if seen.turns > 0 && seen.agents.is_empty() => break,
                ThreadEvent::SubagentStarted { thread_id, .. } => {
                    seen.agents.insert(thread_id);
                }
                ThreadEvent::Subagent { thread_id, event } => match *event {
                    ThreadEvent::TurnStarted(_) => {
                        seen.agents.insert(thread_id);
                    }
                    ThreadEvent::Idle => {
                        seen.agents.remove(&thread_id);
                    }
                    _ => {}
                },
                other => see(&mut seen, other),
            },
            Event::Ended { thread_id: id } if id == thread_id => break,
            _ => {}
        }
        if seen.error.is_some() {
            break;
        }
    }
    if interrupted || seen.error.is_some() {
        let _ = client.threads().cancel(&thread_id).await;
    }
    let _ = client.threads().close(&thread_id).await;
    client.shutdown().await;
    let seconds = started.elapsed().as_secs();
    print_result(&options, &thread_id, &seen, seconds)?;
    match (interrupted, &seen.error) {
        (true, _) => std::process::exit(130),
        (_, Some(_)) => std::process::exit(1),
        _ => Ok(()),
    }
}

fn see(seen: &mut Result_, event: ThreadEvent) {
    match event {
        ThreadEvent::Model(ModelEvent::Text(text)) => seen.answer.push_str(&text),
        ThreadEvent::TurnStarted(_) => seen.answer.clear(),
        ThreadEvent::TurnFinished(turn) => {
            seen.turns += 1;
            if turn.status == TurnStatus::Failed {
                let reason = turn
                    .error_message
                    .unwrap_or_else(|| "the turn failed".into());
                seen.error = Some(reason);
            }
        }
        ThreadEvent::ToolStarted(call) => {
            seen.tool_calls += 1;
            eprintln!("tool: {}", call.name);
        }
        ThreadEvent::ToolFinished { result, .. } => seen.tool_errors += u64::from(result.is_error),
        ThreadEvent::Tokens { tokens, .. } => {
            seen.model_calls = tokens.model_calls;
            seen.prompt_tokens = tokens.spent.prompt_tokens;
            seen.cached_tokens = tokens.spent.cached_prompt_tokens;
            seen.completion_tokens = tokens.spent.completion_tokens;
            seen.reasoning_tokens = tokens.spent.reasoning_tokens;
        }
        ThreadEvent::ProviderUnavailable => {
            seen.error = Some("the provider is unavailable".into());
        }
        ThreadEvent::Rejected { reason } => seen.error = Some(reason),
        _ => {}
    }
}

fn print_result(options: &Headless, thread_id: &str, seen: &Result_, seconds: u64) -> Result<()> {
    let mut out = std::io::stdout().lock();
    if options.json {
        let report = serde_json::json!({
            "thread": thread_id,
            "answer": seen.answer.trim(),
            "error": seen.error,
            "seconds": seconds,
            "turns": seen.turns,
            "model_calls": seen.model_calls,
            "tokens": {
                "prompt": seen.prompt_tokens,
                "cached": seen.cached_tokens,
                "completion": seen.completion_tokens,
                "reasoning": seen.reasoning_tokens,
            },
            "tool_calls": seen.tool_calls,
            "tool_errors": seen.tool_errors,
            "denied": seen.denied,
        });
        writeln!(out, "{report}")?;
    } else {
        writeln!(out, "{}", seen.answer.trim())?;
        if let Some(error) = &seen.error {
            eprintln!("error: {error}");
        }
    }
    Ok(())
}
