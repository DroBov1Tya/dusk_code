//! Line-based chat over a live thread. Lines are sent as they are typed:
//! `text` queues a message, `!text` sends it now and interrupts the answer, `!` stops it,
//! `:command` changes settings and `/command`, such as `/compact`, goes to the core.

use std::io::{self, IsTerminal, Write};

use anyhow::Result;
use dusk_core::{
    providers::ModelEvent,
    runtime::{ThreadEvent, ThreadInput, ThreadOutput},
    state::{Message, MessageRole, ThreadTokens, Turn, TurnStatus},
};
use tokio::io::{AsyncBufReadExt, BufReader};

use crate::commands::{self, Settings};

const PROMPT: &str = "> ";
const NOW_PREFIX: char = '!';

/// Reads lines and prints events at the same time; ends when the thread has ended.
pub async fn run(input: ThreadInput, output: ThreadOutput, settings: Settings) -> Result<()> {
    let (typing, ()) = tokio::join!(read_lines(input, settings), print_events(output));
    typing
}

/// Ends at end of input; dropping `input` lets the thread finish its queue and stop.
async fn read_lines(input: ThreadInput, settings: Settings) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        if let Some(command) = line.strip_prefix(commands::PREFIX) {
            match commands::run(command, &settings) {
                Ok(reply) => eprintln!("{reply}"),
                Err(error) => eprintln!("error: {error:#}"),
            }
            continue;
        }
        match line.strip_prefix(NOW_PREFIX).map(str::trim) {
            Some("") => input.cancel()?,
            Some(text) => input.send_now(text).await?,
            None if line.is_empty() => {}
            None => input.send(line).await?,
        }
    }
    Ok(())
}

async fn print_events(mut output: ThreadOutput) {
    let mut printer = StreamPrinter::new();
    while let Some(event) = output.next_event().await {
        match event {
            ThreadEvent::Idle => eprint!("{PROMPT}"),
            ThreadEvent::Model(event) => printer.print(&event),
            ThreadEvent::ToolStarted(call) => {
                printer.end_reasoning();
                eprintln!("[{}] {}", call.name, sanitize(&call.arguments.to_string()));
            }
            ThreadEvent::ToolFinished { result, .. } => {
                let status = if result.is_error { "failed" } else { "done" };
                let first_line = result.content.lines().next().unwrap_or_default();
                eprintln!("  {status}: {}", sanitize(first_line));
            }
            ThreadEvent::TurnFinished(turn) => {
                printer.finish();
                report(&turn);
            }
            ThreadEvent::TitleChanged { title } => eprintln!("[title: {}]", sanitize(&title)),
            ThreadEvent::OutputLimitReached { limit } => {
                printer.end_reasoning();
                eprintln!("\n[stopped: the answer reached the output limit of {limit} tokens]");
            }
            ThreadEvent::CompactionStarted { reason } => {
                eprintln!("[compacting the context: {}]", reason.as_str());
            }
            ThreadEvent::CompactionFinished(compaction) => {
                printer.finish();
                eprintln!(
                    "[compacted messages up to #{} into a summary of ~{} tokens]",
                    compaction.through_sequence, compaction.summary_tokens
                );
            }
            ThreadEvent::CompactionFailed { error } => {
                printer.finish();
                eprintln!("compaction failed: {}", sanitize(&error));
            }
            ThreadEvent::Tokens { tokens, limit } => eprintln!("{}", describe(&tokens, limit)),
            ThreadEvent::Rejected { reason } => eprintln!("rejected: {reason}"),
            ThreadEvent::Retrying { attempt, error } => {
                printer.end_reasoning();
                eprintln!("retrying after attempt {attempt}: {}", sanitize(&error));
            }
            ThreadEvent::Failed(error) => eprintln!("thread stopped: {}", sanitize(&error)),
            ThreadEvent::ContextTooLarge { tokens, limit } => eprintln!(
                "the context is about {tokens} tokens but the input limit is {limit}: \
                 switch the model or compact the context"
            ),
            _ => {}
        }
    }
}

fn report(turn: &Turn) {
    match turn.status {
        TurnStatus::Interrupted => eprintln!("[interrupted]"),
        TurnStatus::Failed => eprintln!(
            "error: {}",
            turn.error_message.as_deref().unwrap_or("turn failed")
        ),
        _ => {}
    }
}

/// Context and history are estimates; the provider line is what it reported.
fn describe(tokens: &ThreadTokens, limit: u64) -> String {
    let current = &tokens.current;
    let spent = &tokens.spent;
    format!(
        "tokens: context {} of {limit}, estimated by author: system {}, user {}, agent {}, tool calls {}, \
         tool results {}, summary {}, tool definitions {}; history ~{}; provider {} in ({} cached), {} out in {} calls",
        tokens.context,
        current.system,
        current.user,
        current.assistant,
        current.tool_calls,
        current.tool_results,
        current.summary,
        current.tool_specs,
        tokens.all.total(),
        spent.prompt_tokens,
        spent.cached_prompt_tokens,
        spent.completion_tokens,
        tokens.model_calls,
    )
}

/// What was said so far: the user's messages after the prompt sign, the agent's answers
/// as they are. System messages and tool traffic are left out.
pub fn print_history(messages: &[Message]) {
    for message in messages {
        let text = sanitize(message.content.trim());
        match message.role {
            MessageRole::User => println!("{PROMPT}{text}"),
            MessageRole::Assistant if !text.is_empty() => println!("{text}\n"),
            _ => {}
        }
    }
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
    fn print(&mut self, event: &ModelEvent) {
        match event {
            ModelEvent::Reasoning(text) => {
                if !self.in_reasoning && self.dim_reasoning {
                    eprint!("\x1b[2m");
                }
                self.in_reasoning = true;
                let _ = io::stderr().write_all(sanitize(text).as_bytes());
            }
            ModelEvent::Text(text) => {
                self.end_reasoning();
                let mut stdout = io::stdout();
                let _ = stdout.write_all(sanitize(text).as_bytes());
                let _ = stdout.flush();
            }
            // Tool calls are shown from thread events; usage and finish reasons are not.
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
pub fn sanitize(text: &str) -> String {
    text.chars()
        .filter(|&c| c == '\n' || c == '\t' || !(c.is_control() || is_bidi_control(c)))
        .collect()
}

fn is_bidi_control(c: char) -> bool {
    matches!(c, '\u{200E}' | '\u{200F}' | '\u{202A}'..='\u{202E}' | '\u{2066}'..='\u{2069}')
}
