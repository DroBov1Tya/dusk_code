//! What a client names before git sees it. A branch name is held to a strict allowlist,
//! so it can never be read as an option (`-x`), a forced refspec (`+x`) or a revision
//! expression (`a..b`, `x@{1}`); a path must name something inside the project.

use std::path::Path;

use anyhow::{Result, bail, ensure};
use dusk_protocol::project::Project;

const MAX_REF_CHARS: usize = 200;
const MAX_PATH_CHARS: usize = 4096;
/// The longest commit message and pull request body taken.
pub const MAX_MESSAGE_CHARS: usize = 20_000;

/// A branch name made of letters, digits and `-_./`, in parts separated by single `/`,
/// none starting with `-` or `.`, and none ending with `.lock`: a subset of what
/// `git check-ref-format --branch` takes.
pub fn branch(name: &str) -> Result<&str> {
    ensure!(!name.is_empty(), "a branch needs a name");
    ensure!(
        name.len() <= MAX_REF_CHARS,
        "a branch name is at most {MAX_REF_CHARS} characters"
    );
    let allowed = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.' | '/'));
    ensure!(
        allowed,
        "a branch name has letters, digits and - _ . / only"
    );
    for part in name.split('/') {
        ensure!(!part.is_empty(), "a branch name has no empty part");
        ensure!(
            !part.starts_with('-') && !part.starts_with('.'),
            "no part of a branch name starts with - or ."
        );
        ensure!(
            !part.ends_with(".lock") && !part.ends_with('.'),
            "no part of a branch name ends with .lock or ."
        );
    }
    ensure!(!name.contains(".."), "a branch name has no ..");
    Ok(name)
}

/// A path relative to the project, as git takes it after `--`. Refused when it is
/// absolute, climbs with `..`, starts with `-`, or leads out of the project through a
/// symbolic link; a path that no longer exists (a deleted file) is checked by the
/// nearest folder that does.
pub async fn path<'a>(project: &Project, relative: &'a str) -> Result<&'a str> {
    ensure!(!relative.is_empty(), "a path is needed");
    ensure!(relative.len() <= MAX_PATH_CHARS, "the path is too long");
    ensure!(!relative.starts_with('-'), "a path may not start with -");
    ensure!(
        !relative.chars().any(char::is_control),
        "a path has no control characters"
    );
    super::super::plain_relative(Path::new(relative))?;
    let mut probe = project.dir.join(relative);
    loop {
        match tokio::fs::canonicalize(&probe).await {
            Ok(real) => {
                ensure!(
                    real.starts_with(&project.dir),
                    "the path must stay inside the project"
                );
                return Ok(relative);
            }
            Err(_) => {
                if !probe.pop() || !probe.starts_with(&project.dir) {
                    bail!("the path must stay inside the project");
                }
            }
        }
    }
}

/// Text for a commit message or a pull request: not blank, no NUL, capped.
pub fn message(text: &str) -> Result<&str> {
    let text = text.trim();
    ensure!(!text.is_empty(), "the message is empty");
    ensure!(!text.contains('\0'), "the message has a NUL character");
    ensure!(
        text.chars().count() <= MAX_MESSAGE_CHARS,
        "the message is at most {MAX_MESSAGE_CHARS} characters"
    );
    Ok(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_branch_is_never_an_option_a_forced_refspec_or_a_range() {
        for good in ["main", "dusk/fix-1", "release/1.2", "a_b"] {
            assert!(branch(good).is_ok(), "{good}");
        }
        for bad in [
            "", "-f", "--force", "+main", "a..b", "x@{1}", "a b", "a\nb", "/a", "a/", "a//b",
            "a/-b", ".hidden", "x.lock", "x.", "HEAD~1", "a:b", "ü",
        ] {
            assert!(branch(bad).is_err(), "{bad:?}");
        }
        assert!(branch(&"a".repeat(201)).is_err());
    }
}
