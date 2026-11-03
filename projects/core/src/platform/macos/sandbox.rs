//! A Seatbelt profile written for each command: everything allowed but writing outside
//! the open folders, and any access to the hidden ones. `sandbox-exec` is deprecated in
//! name only; the system and Apple's own tools still use it.

use std::path::Path;

use anyhow::{Result, ensure};
use tokio::process::Command;

use crate::platform::Confinement;

const SANDBOX_EXEC: &str = "/usr/bin/sandbox-exec";

pub fn available() -> bool {
    Path::new(SANDBOX_EXEC).is_file()
}

pub fn command(command: &str, confinement: &Confinement) -> Result<Command> {
    let mut sandboxed = Command::new(SANDBOX_EXEC);
    sandboxed
        .arg("-p")
        .arg(profile(confinement)?)
        .args(["sh", "-c", command]);
    Ok(sandboxed)
}

fn profile(confinement: &Confinement) -> Result<String> {
    let mut profile = String::from(
        "(version 1)\n(allow default)\n(deny file-write*)\n(allow file-write*\n  \
         (literal \"/dev/null\") (literal \"/dev/zero\") (literal \"/dev/dtracehelper\")\n  \
         (regex #\"^/dev/tty\") (regex #\"^/dev/fd/\")",
    );
    for folder in &confinement.writable {
        profile.push_str(&format!("\n  (subpath {})", quoted(folder)?));
    }
    profile.push_str(")\n");
    for folder in &confinement.hidden {
        profile.push_str(&format!(
            "(deny file-read* file-write* (subpath {}))\n",
            quoted(folder)?
        ));
    }
    Ok(profile)
}

/// A path as a profile string; one that could break out of the string is refused.
fn quoted(path: &Path) -> Result<String> {
    let text = path.to_string_lossy();
    ensure!(
        !text
            .chars()
            .any(|c| c.is_control() || c == '"' || c == '\\'),
        "{} cannot be named in a sandbox profile",
        path.display()
    );
    Ok(format!("\"{text}\""))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_confined_command_writes_only_in_the_open_folder() {
        let root = crate::testing::temp_dir("seatbelt");
        std::fs::create_dir_all(&root).unwrap();
        let root = root.canonicalize().unwrap();
        let (open, closed) = (root.join("open"), root.join("closed"));
        for folder in [&open, &closed] {
            std::fs::create_dir_all(folder).unwrap();
        }
        let confinement = Confinement {
            writable: vec![open.clone()],
            hidden: vec![closed.clone()],
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
        assert!(!run(format!("ls {}", closed.display())).await);
        assert!(quoted(Path::new("/a\"b")).is_err());
    }
}
