//! Running the GitHub CLI: an argument vector with every value in `--flag=value` form,
//! no prompts, no pager, a time limit; and its text made plain.

use std::{path::Path, time::Duration};

use anyhow::{Result, bail};

use crate::project::git;

pub(super) const PROGRAM: &str = "gh";
const LIMIT: Duration = Duration::from_secs(60);
/// What is said when `gh` cannot be used, and nothing else.
pub(super) const MISSING: &str = "GitHub CLI is not installed or signed in";

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

/// `gh` is installed and signed in; otherwise `MISSING`, whatever the cause.
pub(super) async fn ready(program: &str, dir: &Path) -> Result<()> {
    match run(program, dir, &["auth", "status"]).await {
        Ok(output) if output.success => Ok(()),
        _ => bail!(MISSING),
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

/// Untrusted text as plain text: control characters out, capped.
pub(super) fn plain(text: &str, max: usize) -> String {
    let mut out: String = text
        .chars()
        .filter(|c| !c.is_control() || *c == '\n')
        .take(max)
        .collect();
    if text.chars().count() > max {
        out.push_str("...");
    }
    out
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
