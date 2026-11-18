//! Running git: an argument vector and never a shell, no prompts, no editor and no pager,
//! a time limit, and what it prints capped. Hooks and the repository's config are the
//! user's own and run as they would in a terminal.

use std::{path::Path, process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use tokio::{
    io::{AsyncRead, AsyncReadExt},
    process::Command,
};

/// Reads and local writes.
pub const LOCAL: Duration = Duration::from_secs(10);
/// Fetch, pull and push: the network and the user's credential helper.
pub const NETWORK: Duration = Duration::from_secs(120);
/// More output than this of one stream is dropped.
const MAX_OUTPUT: u64 = 1 << 20;
/// The longest message a client is given.
const MAX_MESSAGE_CHARS: usize = 4000;

/// What a finished command printed and whether it succeeded.
pub struct Output {
    pub success: bool,
    pub code: Option<i32>,
    pub stdout: String,
    pub stderr: String,
}

impl Output {
    /// What git said, both streams, as a plain message.
    pub fn message(&self) -> String {
        clean(&format!("{}\n{}", self.stdout, self.stderr))
    }
}

/// The command for `program` in `dir`, with the user's login PATH so credential helpers
/// are found, and every interactive part of git switched off.
pub fn command(program: &str, dir: &Path) -> Command {
    let mut command = Command::new(program);
    if let Some(path) = crate::platform::login_path::get() {
        command.env("PATH", path);
    }
    command
        .current_dir(dir)
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("GIT_EDITOR", "true")
        .env("GIT_SEQUENCE_EDITOR", "true")
        .env("GIT_MERGE_AUTOEDIT", "no")
        .env("GIT_PAGER", "cat")
        .env("PAGER", "cat")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        // Out of the terminal's foreground group: a program that still reads the tty is
        // stopped instead of taking the user's keys, and the time limit ends it.
        .process_group(0)
        .kill_on_drop(true);
    command
}

/// Runs `git -C dir <args>` within `limit`, whatever its exit status.
pub async fn run(dir: &Path, args: &[&str], limit: Duration) -> Result<Output> {
    let mut command = command("git", dir);
    command.args(["-c", "core.pager=cat", "-c", "color.ui=false"]);
    command.args(args);
    finish(command, limit).await
}

/// Runs a prepared command within `limit`, reading at most `MAX_OUTPUT` of each stream.
pub async fn finish(mut command: Command, limit: Duration) -> Result<Output> {
    let mut child = command.spawn().context("cannot start the program")?;
    let stdout = child.stdout.take().context("no output pipe")?;
    let stderr = child.stderr.take().context("no error pipe")?;
    let work = async {
        let (stdout, stderr) = tokio::join!(capped(stdout), capped(stderr));
        let status = child.wait().await?;
        anyhow::Ok((status, stdout?, stderr?))
    };
    let (status, stdout, stderr) = tokio::time::timeout(limit, work)
        .await
        .map_err(|_| anyhow::anyhow!("took longer than {} s", limit.as_secs()))??;
    Ok(Output {
        success: status.success(),
        code: status.code(),
        stdout: String::from_utf8_lossy(&stdout).into_owned(),
        stderr: String::from_utf8_lossy(&stderr).into_owned(),
    })
}

/// The first `MAX_OUTPUT` bytes; the rest is read and dropped so the program never
/// blocks on a full pipe.
async fn capped(mut stream: impl AsyncRead + Unpin) -> std::io::Result<Vec<u8>> {
    let mut kept = Vec::new();
    (&mut stream)
        .take(MAX_OUTPUT)
        .read_to_end(&mut kept)
        .await?;
    tokio::io::copy(&mut stream, &mut tokio::io::sink()).await?;
    Ok(kept)
}

/// Runs git and gives its output, or its cleaned message as the error.
pub async fn checked(dir: &Path, args: &[&str], limit: Duration) -> Result<Output> {
    let output = run(dir, args, limit).await.context("git")?;
    if !output.success {
        bail!("git: {}", output.message());
    }
    Ok(output)
}

/// A read: stdout as text. An error says what git said.
pub(in crate::project) async fn git(dir: &Path, args: &[&str]) -> Result<String> {
    Ok(checked(dir, args, LOCAL).await?.stdout)
}

/// Like `git`, but a status of 1 is no error: `diff --no-index` uses it for "differs".
pub(in crate::project) async fn git_differs(dir: &Path, args: &[&str]) -> Result<String> {
    let output = run(dir, args, LOCAL).await.context("git")?;
    if !matches!(output.code, Some(0 | 1)) {
        bail!("git: {}", output.message());
    }
    Ok(output.stdout)
}

/// Program output as a client may show it: control characters and color codes out,
/// credentials in URLs out, blank lines dropped, and capped.
pub fn clean(text: &str) -> String {
    let mut plain = String::with_capacity(text.len().min(MAX_MESSAGE_CHARS * 4));
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            // An escape sequence: ESC [ ... final letter.
            '\u{1b}' => {
                if chars.peek() == Some(&'[') {
                    chars.next();
                    for c in chars.by_ref() {
                        if c.is_ascii_alphabetic() {
                            break;
                        }
                    }
                }
            }
            '\r' | '\n' => plain.push('\n'),
            c if c.is_control() => {}
            c => plain.push(c),
        }
    }
    let lines: Vec<&str> = plain
        .lines()
        .map(str::trim_end)
        .filter(|line| !line.trim().is_empty())
        .collect();
    let joined = without_credentials(&lines.join("\n"));
    match joined.char_indices().nth(MAX_MESSAGE_CHARS) {
        Some((at, _)) => format!("{}...", &joined[..at]),
        None => joined,
    }
}

/// `scheme://user:secret@host` becomes `scheme://host`.
fn without_credentials(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(at) = rest.find("://") {
        let (before, after) = rest.split_at(at + 3);
        out.push_str(before);
        let end = after
            .find(|c: char| c.is_whitespace() || c == '/')
            .unwrap_or(after.len());
        let authority = &after[..end];
        match authority.rfind('@') {
            Some(sign) => out.push_str(&authority[sign + 1..]),
            None => out.push_str(authority),
        }
        rest = &after[end..];
    }
    out.push_str(rest);
    out
}
