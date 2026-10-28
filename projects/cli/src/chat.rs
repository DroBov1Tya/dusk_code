//! Line-based chat: every input line becomes a turn whose answer streams to stdout.

use std::{
    env,
    io::{self, IsTerminal, Write},
};

use anyhow::{Result, bail};
use dusk_core::{
    providers::ChatDelta,
    receivers::{UserMessage, UserReceiver},
    runtime::AgentRuntime,
    state::{RejectionCode, State, TurnStatus},
};
use tokio::io::{AsyncBufReadExt, BufReader};
use uuid::Uuid;

const PROMPT: &str = "> ";

pub async fn run(state: &State, runtime: &AgentRuntime<'_>, thread_id: &str) -> Result<()> {
    let users = UserReceiver::new(state);
    let cwd = env::current_dir()?.to_string_lossy().into_owned();
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    loop {
        eprint!("{PROMPT}");
        let Some(line) = lines.next_line().await? else {
            return Ok(());
        };
        let content = line.trim();
        if content.is_empty() {
            continue;
        }
        if let Err(error) = ask(&users, runtime, thread_id, &cwd, content).await {
            eprintln!("error: {error:#}");
        }
    }
}

async fn ask(
    users: &UserReceiver<'_>,
    runtime: &AgentRuntime<'_>,
    thread_id: &str,
    cwd: &str,
    content: &str,
) -> Result<()> {
    let receipt = users
        .submit_message(UserMessage {
            request_uid: Uuid::now_v7().to_string(),
            thread_id: thread_id.to_owned(),
            content: content.to_owned(),
            cwd: cwd.to_owned(),
            attachments: Vec::new(),
        })
        .await?;
    let Some(turn) = receipt.turn else {
        let code = receipt.execution.error_code.map(RejectionCode::as_str);
        bail!("request rejected: {}", code.unwrap_or("unknown"));
    };

    let mut printer = StreamPrinter::new();
    let receipt = runtime.run_turn(&turn, |delta| printer.print(delta)).await;
    printer.finish();
    let receipt = receipt?;
    if receipt.turn.status == TurnStatus::Failed {
        bail!("{}", receipt.turn.error_message.unwrap_or_default());
    }
    Ok(())
}

/// Reasoning goes to stderr (dimmed on a terminal), the answer to stdout.
struct StreamPrinter {
    dim_reasoning: bool,
    in_reasoning: bool,
}

impl StreamPrinter {
    fn new() -> Self {
        Self {
            dim_reasoning: io::stderr().is_terminal(),
            in_reasoning: false,
        }
    }

    // Write errors are ignored: a closed terminal must not abort a turn whose answer is
    // still recorded in the thread.
    fn print(&mut self, delta: &ChatDelta) {
        match delta {
            ChatDelta::Reasoning(text) => {
                if !self.in_reasoning && self.dim_reasoning {
                    eprint!("\x1b[2m");
                }
                self.in_reasoning = true;
                let _ = io::stderr().write_all(sanitize(text).as_bytes());
            }
            ChatDelta::Text(text) => {
                self.end_reasoning();
                let mut stdout = io::stdout();
                let _ = stdout.write_all(sanitize(text).as_bytes());
                let _ = stdout.flush();
            }
            // Kinds this client does not render yet.
            _ => {}
        }
    }

    fn finish(&mut self) {
        self.end_reasoning();
        println!();
    }

    fn end_reasoning(&mut self) {
        if self.in_reasoning {
            if self.dim_reasoning {
                eprint!("\x1b[0m");
            }
            eprintln!();
            self.in_reasoning = false;
        }
    }
}

/// Model output is untrusted: drop control characters that could drive the terminal
/// and bidi controls that could make the displayed text misleading.
fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\n' || c == '\t' || !(c.is_control() || is_bidi_control(c)))
        .collect()
}

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}
