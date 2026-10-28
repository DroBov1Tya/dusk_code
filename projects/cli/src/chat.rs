//! Line-based chat over a live session. Lines are sent as they are typed:
//! `text` queues a message, `!text` sends it now and interrupts the answer, `!` stops it.

use std::io::{self, IsTerminal, Write};

use anyhow::Result;
use dusk_core::{
    providers::{ModelEvent, ModelProvider},
    runtime::{SessionEvent, SessionInput, SessionOutput, SessionRunner},
    state::{Turn, TurnStatus},
};
use tokio::io::{AsyncBufReadExt, BufReader};

const PROMPT: &str = "> ";
const NOW_PREFIX: char = '!';

pub async fn run<P: ModelProvider>(
    runner: SessionRunner<'_, P>,
    input: SessionInput,
    output: SessionOutput,
) -> Result<()> {
    let (session, typing, ()) = tokio::join!(runner.run(), read_lines(input), print_events(output));
    typing?;
    session
}

/// Ends at end of input; dropping `input` lets the session finish its queue and stop.
async fn read_lines(input: SessionInput) -> Result<()> {
    let mut lines = BufReader::new(tokio::io::stdin()).lines();
    while let Some(line) = lines.next_line().await? {
        let line = line.trim();
        match line.strip_prefix(NOW_PREFIX).map(str::trim) {
            Some("") => input.cancel()?,
            Some(text) => input.send_now(text)?,
            None if line.is_empty() => {}
            None => input.send(line)?,
        }
    }
    Ok(())
}

async fn print_events(mut output: SessionOutput) {
    let mut printer = StreamPrinter::new();
    while let Some(event) = output.next_event().await {
        match event {
            SessionEvent::Idle => eprint!("{PROMPT}"),
            SessionEvent::Model(event) => printer.print(&event),
            SessionEvent::ToolStarted(call) => {
                printer.end_reasoning();
                eprintln!("[{}] {}", call.name, sanitize(&call.arguments.to_string()));
            }
            SessionEvent::ToolFinished { result, .. } => {
                let status = if result.is_error { "failed" } else { "done" };
                let first_line = result.content.lines().next().unwrap_or_default();
                eprintln!("  {status}: {}", sanitize(first_line));
            }
            SessionEvent::TurnFinished(turn) => {
                printer.finish();
                report(&turn);
            }
            SessionEvent::Rejected { reason } => eprintln!("rejected: {reason}"),
            SessionEvent::ContextTooLarge { tokens, limit } => eprintln!(
                "the context is about {tokens} tokens but the model accepts {limit}: \
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
            // Tool calls are shown from session events; usage and finish reasons are not.
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
