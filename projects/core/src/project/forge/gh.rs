//! Running the GitHub CLI: an argument vector with every value in `--flag=value` form,
//! no prompts, no pager, a time limit. Its text is made plain by `untrusted`.

use std::{path::Path, time::Duration};

use anyhow::{Result, bail};

pub(super) use crate::plugins::contract::untrusted::{link, plain, time};
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
