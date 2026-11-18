//! `git push`, `pull`, `fetch` and `clone`: only the transfer's progress goes, here or
//! from the remote. A line that names trouble stays, and so does everything else.

use super::troubled;

const TRANSFER_PROGRESS: &str = "transfer progress";

/// How each progress line starts; carriage returns may join several of one kind.
const PROGRESS: [&str; 10] = [
    "Enumerating objects:",
    "Counting objects:",
    "Compressing objects:",
    "Writing objects:",
    "Receiving objects:",
    "Resolving deltas:",
    "Unpacking objects:",
    "Updating files:",
    "Checking out files:",
    "Delta compression using up to ",
];

pub(super) fn left_out(line: &str) -> Option<&'static str> {
    let progress = line
        .split('\r')
        .map(|part| part.trim().trim_start_matches("remote:").trim_start())
        .filter(|part| !part.is_empty())
        .all(|part| PROGRESS.iter().any(|start| part.starts_with(start)));
    (progress && !line.trim().is_empty() && !troubled(line)).then_some(TRANSFER_PROGRESS)
}
