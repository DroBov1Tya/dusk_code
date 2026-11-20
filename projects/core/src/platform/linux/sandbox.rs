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

/// Landlock handles truncation since this ABI (Linux 6.2); before it any file the user
/// owns can be emptied by a confined program.
const TRUNCATE_ABI: i32 = 3;
/// The newest ABI whose file rights are known here; later ones add no file right but
/// `ResolveUnix`, which would close every Unix socket.
const NEWEST_FILE_ABI: i32 = 8;

/// The Landlock ABI of the running kernel; 0 when it has none. Asked once.
fn running_abi() -> i32 {
    static ABI_FOUND: std::sync::OnceLock<i32> = std::sync::OnceLock::new();
    *ABI_FOUND.get_or_init(|| {
        // SAFETY: the version query takes no ruleset and writes nothing.
        let version = unsafe {
            libc::syscall(
                libc::SYS_landlock_create_ruleset,
                std::ptr::null::<libc::c_void>(),
                0usize,
                1u32, // LANDLOCK_CREATE_RULESET_VERSION
            )
        };
        i32::try_from(version).unwrap_or(0).max(0)
    })
}

/// Whether a confinement that must handle every file right can be made here: why not,
/// in brackets after "this Linux cannot stop a program from emptying files".
pub fn every_file_right() -> Result<()> {
    file_rights(running_abi()).map(|_| ())
}

/// The file rights ABI `abi` handles, when it closes truncation.
fn file_rights(abi: i32) -> Result<ABI> {
    ensure!(
        abi >= TRUNCATE_ABI,
        "this Linux cannot stop a program from emptying files (Landlock here is older than \
         Linux 6.2)"
    );
    Ok(ABI::from(abi.min(NEWEST_FILE_ABI)))
}

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
    // A rule handled only grants: what this ABI does not name stays open, truncation
    // and device ioctls among it before 6.2 and 6.10.
    let abi = match confinement.every_file_right {
        true => file_rights(running_abi())?,
        false => ABI_VERSION,
    };
    let all = AccessFs::from_all(abi);
    let read: BitFlags<AccessFs> = match confinement.programs {
        Some(_) => AccessFs::ReadFile | AccessFs::ReadDir,
        None => AccessFs::from_read(ABI_VERSION),
    };
    // `> /dev/null` opens it with O_TRUNC; ioctls on the terminal stay closed.
    let device_rights: BitFlags<AccessFs> = all & (AccessFs::WriteFile | AccessFs::Truncate);
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
        .add_rules(path_beneath_rules(devices, device_rights))?
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

    #[test]
    fn a_landlock_without_truncation_is_refused() {
        for abi in [0, 1, 2] {
            let error = file_rights(abi).unwrap_err().to_string();
            assert!(error.contains("older than Linux 6.2"), "{error}");
        }
        assert_eq!(file_rights(3).unwrap(), ABI::V3);
        assert_eq!(file_rights(5).unwrap(), ABI::V5);
        assert_eq!(file_rights(9).unwrap(), ABI::V8, "no ResolveUnix");
    }

    /// With every file right handled, a file it may read or may not see cannot be
    /// emptied: not by truncate(2), not by an open with O_TRUNC, not through a
    /// read-only descriptor it was given. Without, the first two can (ABI v1 alone).
    #[tokio::test]
    async fn every_file_right_closes_truncation() {
        if every_file_right().is_err() || !Path::new("/usr/bin/perl").exists() {
            eprintln!("no Landlock with truncation, or no perl; skipped");
            return;
        }
        let root = crate::testing::temp_dir("landlock-truncate");
        let open = root.join("open");
        let closed = root.join("closed");
        std::fs::create_dir_all(&open).unwrap();
        std::fs::create_dir_all(&closed).unwrap();
        let readable = root.join("readable");
        let hidden = closed.join("key");
        let confinement = |every_file_right| Confinement {
            writable: vec![open.clone()],
            hidden: vec![closed.clone()],
            every_file_right,
            ..Confinement::around(&open, &open, None)
        };
        let perl = |code: &str, file: &Path, every| {
            let mut command = command(
                Path::new("/usr/bin/perl"),
                &[
                    "-MFcntl".into(),
                    "-e".into(),
                    code.into(),
                    file.display().to_string(),
                ],
                &confinement(every),
            )
            .unwrap();
            command.stdin(std::fs::File::open(&readable).unwrap());
            async move { command.status().await.unwrap().success() }
        };
        let by_path = "exit(truncate($ARGV[0], 0) ? 0 : 1)";
        let by_open = "exit(sysopen(my $f, $ARGV[0], O_RDONLY | O_TRUNC) ? 0 : 1)";
        let by_descriptor = "exit(truncate(STDIN, 0) ? 0 : 1)";
        for file in [&readable, &hidden] {
            for code in [by_path, by_open, by_descriptor] {
                std::fs::write(file, "kept").unwrap();
                assert!(
                    !perl(code, file, true).await,
                    "{code} on {}",
                    file.display()
                );
                assert_eq!(std::fs::read_to_string(file).unwrap(), "kept");
            }
        }
        // Writable files still can be.
        let own = open.join("own");
        std::fs::write(&own, "gone").unwrap();
        assert!(perl(by_path, &own, true).await);
        assert_eq!(std::fs::read_to_string(&own).unwrap(), "");
        // The gap it closes, as every other confinement on Linux still has it.
        std::fs::write(&readable, "kept").unwrap();
        assert!(perl(by_open, &readable, false).await);
        assert_eq!(std::fs::read_to_string(&readable).unwrap(), "");
    }
}
