//! The PATH of the user's own shell. An app started from the Dock or a desktop menu gets
//! a bare PATH without cargo, rg and the rest, so commands take the one the user's
//! terminal would have. Read once; without it commands keep the inherited PATH.

use std::{ffi::OsString, process::Stdio, time::Duration};

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

async fn read() -> Option<OsString> {
    let shell = std::env::var_os("SHELL")?;
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
    #[tokio::test]
    async fn reads_a_path_from_the_shell() {
        if std::env::var_os("SHELL").is_none() {
            return;
        }
        super::load().await;
        assert!(super::get().is_some_and(|path| !path.is_empty()));
    }
}
