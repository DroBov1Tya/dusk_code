//! Which shell a terminal runs and how it starts. Nothing a client sends is part of the
//! command: the program is the user's login shell and it gets no command to run.

use std::path::{Path, PathBuf};

use portable_pty::CommandBuilder;

#[cfg(target_os = "macos")]
const FALLBACK: &str = "/bin/zsh";
#[cfg(not(target_os = "macos"))]
const FALLBACK: &str = "/bin/sh";

/// `$SHELL` when it names a program by its full path, else the system's own shell.
pub fn login_shell() -> PathBuf {
    std::env::var_os("SHELL")
        .map(PathBuf::from)
        .filter(|shell| is_program(shell))
        .unwrap_or_else(|| PathBuf::from(FALLBACK))
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

/// `shell` as a login shell in `cwd`. It is interactive because it stands on a terminal;
/// the rest of the environment is the core's own, which holds no secrets of duskcode.
pub fn command(shell: &Path, cwd: &Path) -> CommandBuilder {
    let mut command = CommandBuilder::new(shell);
    command.arg("-l");
    command.cwd(cwd);
    command.env("TERM", "xterm-256color");
    command.env("SHELL", shell);
    command
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_program_named_by_its_full_path_is_a_shell() {
        assert!(is_program(Path::new("/bin/sh")));
        assert!(!is_program(Path::new("sh")));
        assert!(!is_program(Path::new("bin/sh")));
        assert!(!is_program(Path::new("/bin")));
        assert!(!is_program(Path::new("/etc/hosts")));
        assert!(!is_program(Path::new("/bin/sh -c 'echo no'")));
    }
}
