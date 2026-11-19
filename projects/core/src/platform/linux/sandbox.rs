//! Landlock rules made before the program starts and applied in its process just before
//! it runs: reading what is not hidden, writing only in the open folders, and as the
//! confinement says, starting only system programs and opening no TCP socket.
//!
//! Landlock grants and never denies, so a hidden folder is hidden by granting everything
//! around it: each folder on the way down to it is opened entry by entry, all but the
//! hidden one. What appears there later stays closed.
//!
//! Landlock cannot keep a program to one port on this machine, nor stop UDP or a unix
//! socket, so a confinement that allows only the broker is refused here; a program that
//! may reach its broker runs in a network namespace of its own instead (`netns`).

use std::path::{Path, PathBuf};

use anyhow::{Result, bail, ensure};
use landlock::{
    ABI, Access, AccessFs, AccessNet, BitFlags, CompatLevel, Compatible, Ruleset, RulesetAttr,
    RulesetCreated, RulesetCreatedAttr, path_beneath_rules,
};
use tokio::process::Command;

use crate::platform::{Confinement, Network};

/// The oldest Landlock, in kernels since 5.13, is enough for the file rules.
const ABI_VERSION: ABI = ABI::V1;
/// TCP rules came in 6.7.
const NET_ABI: ABI = ABI::V4;
/// Programs of the system, owned by root; whatever they do stays confined.
const SYSTEM_PROGRAMS: [&str; 7] = [
    "/bin",
    "/sbin",
    "/usr",
    "/lib",
    "/lib64",
    "/lib32",
    "/nix/store",
];

pub fn available() -> bool {
    rules(&Confinement::around(Path::new("/"), Path::new("/"), None)).is_ok()
}

pub fn command(program: &Path, arguments: &[String], confinement: &Confinement) -> Result<Command> {
    // Landlock leaves TCP open in a namespace; outside one that would be the machine's.
    ensure!(
        confinement.network != Network::Namespace,
        "a program kept to its own network namespace starts only in one"
    );
    let mut sandboxed = Command::new(program);
    sandboxed.args(arguments);
    confine(&mut sandboxed, confinement)?;
    Ok(sandboxed)
}

/// Applies the confinement to `command` just before it runs, after what it was given to
/// do before.
pub fn confine(command: &mut Command, confinement: &Confinement) -> Result<()> {
    // A file removed while the rules are made can fail them (some file systems fail
    // fstat on its open descriptor then): once more.
    let mut rules = Some(rules(confinement).or_else(|_| rules(confinement))?);
    // SAFETY: the closure runs in the forked child before exec and only makes the two
    // system calls that restrict it, on a ruleset made in the parent.
    unsafe {
        command.pre_exec(move || {
            if let Some(rules) = rules.take()
                && let Err(error) = rules.restrict_self()
            {
                // errno of the failed call, read before anything else runs; a check
                // of the crate's own sets none, so EPERM stands in. The error is
                // leaked: freeing it in the forked child could deadlock, and the
                // child ends here.
                let code = std::io::Error::last_os_error()
                    .raw_os_error()
                    .filter(|code| *code != 0)
                    .unwrap_or(nix::libc::EPERM);
                std::mem::forget(error);
                return Err(std::io::Error::from_raw_os_error(code));
            }
            Ok(())
        });
    }
    Ok(())
}

fn rules(confinement: &Confinement) -> Result<RulesetCreated> {
    let all = AccessFs::from_all(ABI_VERSION);
    let read: BitFlags<AccessFs> = match confinement.programs {
        Some(_) => AccessFs::ReadFile | AccessFs::ReadDir,
        None => AccessFs::from_read(ABI_VERSION),
    };
    let devices = ["/dev/null", "/dev/zero", "/dev/tty"];
    let mut ruleset = Ruleset::default()
        .set_compatibility(CompatLevel::HardRequirement)
        .handle_access(all)?;
    match confinement.network {
        Network::Open => {}
        // No rule follows: every TCP bind and connect is refused.
        Network::Closed => ruleset = ruleset.handle_access(AccessNet::from_all(NET_ABI))?,
        // Its own namespace has no interface but loopback, where only the broker listens.
        Network::Namespace => {}
        Network::Broker(_) => bail!(
            "Landlock cannot keep a program to one port on this machine, so a plugin that \
             connects anywhere does not start on Linux"
        ),
    }
    let mut created = ruleset
        .create()?
        .add_rules(path_beneath_rules(readable(&confinement.hidden), read))?
        .add_rules(path_beneath_rules(&confinement.reopened, read))?
        .add_rules(path_beneath_rules(devices, AccessFs::WriteFile))?
        .add_rules(path_beneath_rules(&confinement.writable, all))?;
    if let Some(programs) = &confinement.programs {
        let execute = AccessFs::Execute | AccessFs::ReadFile | AccessFs::ReadDir;
        created = created
            .add_rules(path_beneath_rules(SYSTEM_PROGRAMS, execute))?
            .add_rules(path_beneath_rules(programs, execute))?;
    }
    Ok(created)
}

/// What to grant so that everything but `hidden` can be read.
fn readable(hidden: &[PathBuf]) -> Vec<PathBuf> {
    let mut open = Vec::new();
    grant(Path::new("/"), hidden, &mut open);
    open
}

fn grant(folder: &Path, hidden: &[PathBuf], open: &mut Vec<PathBuf>) {
    if hidden.iter().any(|closed| folder.starts_with(closed)) {
        return;
    }
    if !hidden.iter().any(|closed| closed.starts_with(folder)) {
        open.push(folder.to_path_buf());
        return;
    }
    // A folder that cannot be listed stays closed.
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        // A link is granted where it leads, so one into or around a hidden folder stays
        // closed.
        if entry.file_type().is_ok_and(|kind| kind.is_symlink()) {
            let leads_in = path.canonicalize().map_or(true, |target| {
                hidden
                    .iter()
                    .any(|closed| target.starts_with(closed) || closed.starts_with(&target))
            });
            if leads_in {
                continue;
            }
        }
        grant(&path, hidden, open);
    }
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
        let closed = root.join("closed");
        std::fs::create_dir_all(&open).unwrap();
        std::fs::create_dir_all(&closed).unwrap();
        std::fs::write(closed.join("key"), "secret").unwrap();
        let confinement = Confinement {
            writable: vec![open.clone()],
            hidden: vec![closed.clone()],
            ..Confinement::around(&open, &open, None)
        };
        let run = |script: String| {
            let confinement = confinement.clone();
            async move {
                command(Path::new("sh"), &["-c".into(), script], &confinement)
                    .unwrap()
                    .status()
                    .await
                    .unwrap()
                    .success()
            }
        };
        assert!(run(format!("echo yes > {}/file", open.display())).await);
        assert!(!run(format!("echo no > {}/file", root.display())).await);
        assert!(!run(format!("cat {}/key", closed.display())).await);
    }
}
