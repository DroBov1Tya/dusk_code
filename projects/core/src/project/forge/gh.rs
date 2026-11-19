//! Running the GitHub CLI: an argument vector with every value in `--flag=value` form,
//! no prompts, no pager, a time limit; and its text made plain.

use std::{path::Path, time::Duration};

use anyhow::{Result, bail};

use crate::project::git;

pub(super) const PROGRAM: &str = "gh";
const LIMIT: Duration = Duration::from_secs(60);

/// Runs `gh` in `dir` once it is known to be there and signed in; an error carries
/// what it said, plain.
pub(super) async fn gh(program: &str, dir: &Path, args: &[&str]) -> Result<String> {
    ready(program, dir).await?;
    output(program, dir, args).await
}

/// Runs `gh` without asking first whether it is signed in, for a caller that did.
pub(super) async fn output(program: &str, dir: &Path, args: &[&str]) -> Result<String> {
    let output = run(program, dir, args).await?;
    if !output.success {
        bail!("gh: {}", output.message());
    }
    Ok(output.stdout)
}

/// `gh` is installed and signed in; otherwise an error, whatever the cause.
pub(super) async fn ready(program: &str, dir: &Path) -> Result<()> {
    match run(program, dir, &["auth", "status"]).await {
        Ok(output) if output.success => Ok(()),
        _ => bail!("GitHub CLI is not installed or signed in"),
    }
}

async fn run(program: &str, dir: &Path, args: &[&str]) -> Result<git::Output> {
    let mut command = git::command(program, dir);
    command
        .args(args)
        .env("GH_PROMPT_DISABLED", "1")
        .env("GH_NO_UPDATE_NOTIFIER", "1")
        .env("GH_SPINNER_DISABLED", "1")
        .env("GH_PAGER", "cat")
        .env("NO_COLOR", "1");
    git::finish(command, LIMIT).await
}

/// Untrusted text as plain text: control and direction-changing characters out, capped.
pub(super) fn plain(text: &str, max: usize) -> String {
    let mut out: String = text
        .chars()
        .filter(|c| (!c.is_control() || *c == '\n') && !is_bidi(*c))
        .take(max)
        .collect();
    if text.chars().count() > max {
        out.push_str("...");
    }
    out
}

/// Marks that reorder text on screen, so a name could show other than it reads.
fn is_bidi(c: char) -> bool {
    matches!(c, '\u{061c}' | '\u{200e}' | '\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}')
}

/// Unix seconds of a time GitHub gives, such as `2026-11-18T12:07:21Z`; `None` for
/// anything else, and for the zero time it gives a check not started.
pub(super) fn time(text: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(text)
        .ok()
        .map(|time| time.timestamp())
        .filter(|seconds| *seconds > 0)
}

/// An https link to GitHub, or nothing.
pub(super) fn link(url: &str) -> String {
    let safe = url.starts_with("https://")
        && url.len() <= 500
        && url
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "-._~:/?#[]@!$&'()*+,;=%".contains(c));
    if safe { url.to_owned() } else { String::new() }
}
