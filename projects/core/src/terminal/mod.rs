//! Terminals: the user's own shell on a pseudo-terminal the core runs for a client that
//! draws it. A terminal belongs to the client that opened it: its bytes go to that client
//! alone, no other client can name it, and it ends when that client goes away.
//!
//! The agents have no part in it: no tool reaches a terminal, and what is typed into one
//! or printed by one is neither stored nor logged.
//!
//! - `shell`: which shell runs, and how it is started;
//! - `pty`: one running terminal and the threads that carry its bytes;
//! - `table`: the open terminals, each with its owner, and the limits.

mod pty;
mod shell;
mod table;

use std::path::{Path, PathBuf};

use anyhow::{Result, ensure};

pub use shell::login_shell;
pub use table::Terminals;

/// The folder a shell may start in: `cwd` resolved, when it is inside one of `roots`.
pub async fn working_dir(cwd: &Path, roots: &[PathBuf]) -> Result<PathBuf> {
    let cwd = crate::project::within(cwd, roots).await?;
    ensure!(cwd.is_dir(), "a terminal opens in a folder");
    Ok(cwd)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[tokio::test]
    async fn a_shell_starts_only_inside_a_project_or_the_workspace() {
        let base = temp_dir("terminal-cwd");
        std::fs::create_dir_all(base.join("project/src")).unwrap();
        std::fs::create_dir_all(base.join("outside")).unwrap();
        std::fs::write(base.join("project/file"), "").unwrap();
        let base = std::fs::canonicalize(base).unwrap();
        let project = base.join("project");
        let roots = vec![project.clone()];

        assert_eq!(working_dir(&project, &roots).await.unwrap(), project);
        assert_eq!(
            working_dir(&project.join("src/../src"), &roots)
                .await
                .unwrap(),
            project.join("src")
        );
        let mut refused = vec![
            base.join("outside"),
            project.join("../outside"),
            project.join(".."),
            project.join("file"),
            project.join("absent"),
            "/".into(),
        ];
        #[cfg(unix)]
        {
            std::os::unix::fs::symlink(base.join("outside"), project.join("link")).unwrap();
            refused.push(project.join("link"));
        }
        for cwd in refused {
            assert!(
                working_dir(&cwd, &roots).await.is_err(),
                "{}",
                cwd.display()
            );
        }
    }
}
