//! The change ledger's detector: what a tool call changed in a project, found without
//! git. `snapshot` walks the project by size and change time, `compare` turns two walks
//! into edits with diffs, and `seen` keeps the texts a diff needs as its "before".

mod compare;
pub mod seen;
mod snapshot;

use std::path::{Component, Path};

#[cfg(test)]
pub use compare::Diff;
pub use compare::{Edit, compare, sha256};
pub use snapshot::{Snapshot, Stamp, take, take_for};

/// Whether the text of a project file may be kept and diffed: not for `.env` files or
/// anything under `.ssh`, which hold secrets. Such a change is recorded without a diff.
pub fn may_keep_text(relative: &str) -> bool {
    !Path::new(relative).components().any(|part| match part {
        Component::Normal(name) => {
            let name = name.to_string_lossy();
            name.starts_with(".env") || name == ".ssh"
        }
        _ => false,
    })
}

#[cfg(test)]
mod tests {
    use super::may_keep_text;

    #[test]
    fn env_files_and_ssh_keys_are_never_kept() {
        for secret in [
            ".env",
            ".env.local",
            "app/.env.prod",
            ".ssh/id_ed25519",
            "a/.ssh/x",
        ] {
            assert!(!may_keep_text(secret), "{secret}");
        }
        for plain in ["src/env.rs", "environment.md", "ssh/notes.txt", "a/b.env"] {
            assert!(may_keep_text(plain), "{plain}");
        }
    }
}
