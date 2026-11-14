//! Running the tools behind the simulator: a program with its arguments, never a
//! shell, with the user's PATH so a Homebrew `axe` is found from the Dock too.

use std::{process::Stdio, time::Duration};

use anyhow::{Context, Result, bail};
use tokio::process::Command;

pub const LIMIT: Duration = Duration::from_secs(30);

/// A UDID in the one form simctl prints, so nothing else reaches the arguments.
pub fn device(udid: &str) -> Result<String> {
    let parsed = uuid::Uuid::try_parse(udid).context("not a simulator device")?;
    Ok(parsed.hyphenated().to_string().to_uppercase())
}

pub async fn simctl(args: &[&str], limit: Duration) -> Result<Vec<u8>> {
    let mut all = vec!["simctl"];
    all.extend_from_slice(args);
    run("xcrun", &all, limit).await
}

/// AXe, which sends touches, keys and buttons to a device.
pub async fn axe(args: &[&str]) -> Result<Vec<u8>> {
    run("axe", args, LIMIT)
        .await
        .context("AXe could not reach the device")
}

pub async fn run(program: &str, args: &[&str], limit: Duration) -> Result<Vec<u8>> {
    let mut command = command(program);
    command.args(args);
    let output = tokio::time::timeout(limit, command.output())
        .await
        .with_context(|| format!("{program} took too long"))?
        .with_context(|| format!("{program} did not start"))?;
    if !output.status.success() {
        bail!("{}", last_line(&output.stderr));
    }
    Ok(output.stdout)
}

pub fn command(program: &str) -> Command {
    let mut command = Command::new(program);
    command.stdin(Stdio::null()).kill_on_drop(true);
    if let Some(path) = crate::platform::login_path::get() {
        command.env("PATH", path);
    }
    command
}

/// The last line a tool wrote to stderr, which is where it says why.
pub fn last_line(stderr: &[u8]) -> String {
    let text = String::from_utf8_lossy(stderr);
    let line = text.lines().rev().find(|line| !line.trim().is_empty());
    line.unwrap_or("failed").trim().chars().take(300).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_udid_reaches_simctl() {
        assert_eq!(
            device("543c1e74-f9cc-4eb5-81a0-e1470375459b").unwrap(),
            "543C1E74-F9CC-4EB5-81A0-E1470375459B"
        );
        for bad in ["booted", "--help", "all", "", "543C1E74 F9CC"] {
            assert!(device(bad).is_err(), "{bad}");
        }
    }
}
