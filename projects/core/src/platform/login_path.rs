//! The PATH of the user's own shell. An app started from the Dock or a desktop menu gets
//! a bare PATH without cargo, rg and the rest, so commands take the one the user's
//! terminal would have. Read once; without it commands keep the inherited PATH. The shell
//! is `$SHELL` only when that names a program by its full path.

use std::{
    ffi::OsString,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use tokio::{process::Command, sync::OnceCell};

static PATH: OnceCell<Option<OsString>> = OnceCell::const_new();

/// Shell start-up files may print; the value is taken from between these.
const MARK: &str = "__dusk_path__";
const WAIT: Duration = Duration::from_secs(5);

/// Reads the PATH on the first call; later calls return at once.
pub async fn load() {
    PATH.get_or_init(read).await;
}

/// The PATH read by `load`, if it was read.
pub fn get() -> Option<&'static OsString> {
    PATH.get()?.as_ref()
}

/// The PATH a program the core starts gets: the login shell's, else the inherited one.
pub fn effective() -> Option<OsString> {
    get().cloned().or_else(|| std::env::var_os("PATH"))
}

/// `$SHELL` when it names a program by its full path; anything else is not run.
pub fn user_shell() -> Option<PathBuf> {
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|shell| is_program(shell))
}

fn is_program(path: &Path) -> bool {
    let Ok(metadata) = std::fs::metadata(path) else {
        return false;
    };
    #[cfg(unix)]
    let runs = {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    };
    #[cfg(not(unix))]
    let runs = true;
    path.is_absolute() && metadata.is_file() && runs
}

async fn read() -> Option<OsString> {
    let shell = user_shell()?;
    // Interactive too, since many users set PATH in .zshrc or .bashrc.
    let script = format!("printf '{MARK}%s{MARK}' \"$PATH\"");
    let mut command = Command::new(shell);
    command
        .args(["-i", "-l", "-c", &script])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    let output = match tokio::time::timeout(WAIT, command.output()).await {
        Ok(Ok(output)) => output,
        _ => {
            tracing::warn!("cannot read the login shell's PATH; commands keep the inherited one");
            return None;
        }
    };
    let text = String::from_utf8(output.stdout).ok()?;
    let path = text.split(MARK).nth(1)?;
    (!path.is_empty()).then(|| path.into())
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::is_program;

    #[test]
    fn only_a_program_named_by_its_full_path_is_a_shell() {
        assert!(is_program(Path::new("/bin/sh")));
        assert!(!is_program(Path::new("sh")));
        assert!(!is_program(Path::new("bin/sh")));
        assert!(!is_program(Path::new("/bin")));
        assert!(!is_program(Path::new("/etc/hosts")));
        assert!(!is_program(Path::new("/bin/sh -c 'echo no'")));
    }

    #[tokio::test]
    async fn reads_a_path_from_the_shell() {
        // Nothing is run without a shell named by its full path.
        if super::user_shell().is_none() {
            return;
        }
        super::load().await;
        assert!(super::get().is_some_and(|path| !path.is_empty()));
    }
}
