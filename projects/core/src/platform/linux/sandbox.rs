//! Landlock rules made before the command starts and applied in its process just before
//! `sh` runs: reading and running anything, writing only in the open folders. Landlock
//! grants and never denies, so a hidden folder stays readable here; writing to it is
//! still refused.

use std::path::PathBuf;

use anyhow::{Context, Result};
use landlock::{
    ABI, Access, AccessFs, CompatLevel, Compatible, Ruleset, RulesetAttr, RulesetCreated,
    RulesetCreatedAttr, path_beneath_rules,
};
use tokio::process::Command;

use crate::platform::Confinement;

/// The oldest Landlock, in kernels since 5.13, is enough for these rules.
const ABI_VERSION: ABI = ABI::V1;

pub fn available() -> bool {
    rules(&[]).is_ok()
}

pub fn command(command: &str, confinement: &Confinement) -> Result<Command> {
    let mut rules = Some(rules(&confinement.writable)?);
    let mut sandboxed = Command::new("sh");
    sandboxed.arg("-c").arg(command);
    // SAFETY: the closure runs in the forked child before exec and only makes the two
    // system calls that restrict it, on a ruleset made in the parent.
    unsafe {
        sandboxed.pre_exec(move || {
            if let Some(rules) = rules.take() {
                rules
                    .restrict_self()
                    .map_err(|error| std::io::Error::other(error.to_string()))?;
            }
            Ok(())
        });
    }
    Ok(sandboxed)
}

fn rules(writable: &[PathBuf]) -> Result<RulesetCreated> {
    let all = AccessFs::from_all(ABI_VERSION);
    let read = AccessFs::from_read(ABI_VERSION);
    let devices = ["/dev/null", "/dev/zero", "/dev/tty"];
    Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(all)?
        .create()?
        .add_rules(path_beneath_rules(["/"], read))?
        .add_rules(path_beneath_rules(devices, AccessFs::WriteFile))?
        .add_rules(path_beneath_rules(writable, all))
        .context("cannot make the sandbox rules")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_confined_command_writes_only_in_the_open_folder() {
        if !available() {
            eprintln!("no Landlock in this kernel; skipped");
            return;
        }
        let root = crate::testing::temp_dir("landlock");
        let open = root.join("open");
        std::fs::create_dir_all(&open).unwrap();
        let confinement = Confinement {
            writable: vec![open.clone()],
            hidden: Vec::new(),
        };
        let run = |script: String| {
            let confinement = confinement.clone();
            async move {
                command(&script, &confinement)
                    .unwrap()
                    .status()
                    .await
                    .unwrap()
                    .success()
            }
        };
        assert!(run(format!("echo yes > {}/file", open.display())).await);
        assert!(!run(format!("echo no > {}/file", root.display())).await);
    }
}
