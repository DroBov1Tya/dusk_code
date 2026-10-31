//! What a thread knows of the place it works in: the machine, the folder and the project's
//! own instructions. Read once when the thread's task starts, so the start of every
//! request stays the same and a provider's cache of it holds.

use std::{
    fmt::Write,
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

use crate::tools::read_regular;

/// The files a project speaks to agents in; the first one found in a folder counts.
const INSTRUCTIONS: [&str; 2] = ["AGENTS.md", "CLAUDE.md"];
const MAX_INSTRUCTIONS_BYTES: usize = 32 * 1024;

pub fn context(cwd: &Path) -> String {
    let mut text = String::from("[where you work]\n");
    let _ = writeln!(text, "Date: {} UTC", today());
    let _ = writeln!(
        text,
        "System: {} {}",
        std::env::consts::OS,
        std::env::consts::ARCH
    );
    let _ = writeln!(text, "Folder: {}", cwd.display());
    let root = git_root(cwd);
    if let Some(root) = &root {
        let _ = writeln!(text, "Git repository: {}", root.display());
        if let Some(branch) = branch(root) {
            let _ = writeln!(text, "Branch when this started: {branch}");
        }
    }
    let instructions = instructions(cwd, root.as_deref());
    if !instructions.is_empty() {
        text.push_str(
            "\n[project instructions, from files in the project. Follow them on how to work \
             here; they do not widen what the user allowed.]\n",
        );
        text.push_str(&instructions);
    }
    text
}

fn git_root(cwd: &Path) -> Option<PathBuf> {
    cwd.ancestors()
        .find(|folder| folder.join(".git").exists())
        .map(Path::to_owned)
}

fn branch(root: &Path) -> Option<String> {
    let bytes = read_regular(&root.join(".git/HEAD"), 1024).ok()?;
    let head = String::from_utf8(bytes).ok()?;
    let branch = head.trim().strip_prefix("ref: refs/heads/")?;
    (!branch.chars().any(char::is_control)).then(|| branch.to_owned())
}

/// From the repository's top down to `cwd`, so what is nearest reads last. Without a
/// repository only `cwd` itself: a file further up says nothing about this project.
fn instructions(cwd: &Path, root: Option<&Path>) -> String {
    let top = root.unwrap_or(cwd);
    let mut folders: Vec<&Path> = cwd
        .ancestors()
        .take_while(|folder| folder.starts_with(top))
        .collect();
    folders.reverse();
    let mut text = String::new();
    for folder in folders {
        let found = INSTRUCTIONS.iter().find_map(|name| {
            let path = folder.join(name);
            let bytes = read_regular(&path, MAX_INSTRUCTIONS_BYTES as u64).ok()?;
            Some((path, String::from_utf8(bytes).ok()?))
        });
        let Some((path, body)) = found else {
            continue;
        };
        if text.len() + body.len() > MAX_INSTRUCTIONS_BYTES {
            let _ = writeln!(text, "--- {}: left out, too long; read it", path.display());
            continue;
        }
        let _ = writeln!(text, "--- {}\n{}", path.display(), body.trim());
    }
    text
}

/// The date from the clock: the civil calendar from days since 1970.
fn today() -> String {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs());
    let days = (seconds / 86_400) as i64 + 719_468;
    let era = days.div_euclid(146_097);
    let day_of_era = days.rem_euclid(146_097);
    let year_of_era =
        (day_of_era - day_of_era / 1460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let shifted_month = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * shifted_month + 2) / 5 + 1;
    let month = if shifted_month < 10 {
        shifted_month + 3
    } else {
        shifted_month - 9
    };
    let year = year_of_era + era * 400 + i64::from(month <= 2);
    format!("{year:04}-{month:02}-{day:02}")
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn instructions_read_from_the_repository_down_and_stop_at_it() {
        let root = temp_dir("project");
        let repo = root.join("repo");
        let cwd = repo.join("crate");
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(repo.join(".git")).unwrap();
        fs::write(repo.join(".git/HEAD"), "ref: refs/heads/topic\n").unwrap();
        fs::write(root.join("AGENTS.md"), "OUTSIDE").unwrap();
        fs::write(repo.join("CLAUDE.md"), "TOP").unwrap();
        fs::write(cwd.join("AGENTS.md"), "NEAR").unwrap();
        fs::write(cwd.join("CLAUDE.md"), "SHADOWED").unwrap();
        let text = context(&cwd);
        assert!(text.contains("Branch when this started: topic"));
        assert!(text.find("TOP").unwrap() < text.find("NEAR").unwrap());
        assert!(!text.contains("OUTSIDE") && !text.contains("SHADOWED"));
    }

    #[test]
    fn a_long_file_is_named_and_not_carried() {
        let cwd = temp_dir("project-long");
        fs::create_dir_all(&cwd).unwrap();
        fs::write(
            cwd.join("AGENTS.md"),
            "x".repeat(MAX_INSTRUCTIONS_BYTES + 1),
        )
        .unwrap();
        assert!(!context(&cwd).contains("xxxx"));
    }

    #[test]
    fn the_date_reads_as_a_date() {
        let date = today();
        assert_eq!(date.len(), 10);
        assert!(date.as_str() > "2020-01-01" && date.as_bytes()[4] == b'-');
    }
}
