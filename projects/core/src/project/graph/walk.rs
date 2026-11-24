//! Which files a project holds and what the core measures of one. In a git repository
//! the list is `git ls-files` (tracked and untracked, ignored left out), which is fast on
//! a large tree; elsewhere it is the change ledger's walk. Links are never followed and
//! nothing outside the folder is read.

use std::{
    collections::HashMap,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, UNIX_EPOCH},
};

use sha2::{Digest, Sha256};

use crate::{
    project::{
        git,
        ledger::{self, Stamp, may_keep_text},
    },
    state::graph::{GraphStamp, WalkedFile},
};

/// A project with more files than this is not walked: its graph would not stay current.
pub const MAX_FILES: usize = 300_000;
/// The ledger's walk of a folder that is not a repository may take this long.
const LEDGER_BUDGET: Duration = Duration::from_secs(3);
/// `git ls-files` may take this long.
const GIT_LIMIT: Duration = Duration::from_secs(20);
/// A larger file is listed but not read: no lines, no hash, no facts.
pub const MAX_READ_BYTES: u64 = 16 * 1024 * 1024;
/// A NUL in this many first bytes makes a file binary: it has no lines.
const BINARY_PROBE: usize = 8 * 1024;

/// Every regular file of `root` with its stamp, or `None` when it cannot be listed
/// whole: a partial list would drop the facts of files that were only not reached.
pub async fn list(root: &Path, skip: &[PathBuf]) -> Option<HashMap<String, GraphStamp>> {
    if let Some(paths) = git_files(root).await {
        let root = root.to_owned();
        let skip = skip.to_vec();
        return tokio::task::spawn_blocking(move || stamped(&root, &skip, paths))
            .await
            .ok()
            .flatten();
    }
    let root = root.to_owned();
    let skip = skip.to_vec();
    let snapshot =
        tokio::task::spawn_blocking(move || ledger::take_for(&root, &skip, LEDGER_BUDGET))
            .await
            .ok()
            .flatten()?;
    Some(
        snapshot
            .files
            .into_iter()
            .map(|(path, stamp)| (path, graph_stamp(stamp)))
            .collect(),
    )
}

/// The paths git lists under `root`, relative to it; `None` outside a repository.
async fn git_files(root: &Path) -> Option<Vec<String>> {
    let args = [
        "ls-files",
        "-z",
        "--cached",
        "--others",
        "--exclude-standard",
    ];
    let output = git::run(root, &args, GIT_LIMIT).await.ok()?;
    if !output.success {
        return None;
    }
    let mut paths: Vec<String> = output
        .stdout
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
        .collect();
    // A conflicted file is listed once per stage.
    paths.sort_unstable();
    paths.dedup();
    Some(paths)
}

fn stamped(
    root: &Path,
    skip: &[PathBuf],
    paths: Vec<String>,
) -> Option<HashMap<String, GraphStamp>> {
    if paths.len() > MAX_FILES {
        return None;
    }
    let mut files = HashMap::with_capacity(paths.len());
    for relative in paths {
        // git prints what it holds; a path that climbs out is not taken.
        if climbs_out(&relative) {
            continue;
        }
        let path = root.join(&relative);
        if skip.iter().any(|folder| path.starts_with(folder)) {
            continue;
        }
        // A link, a submodule or a file deleted since is not a file of the graph.
        let Some(stamp) = Stamp::of(&path) else {
            continue;
        };
        files.insert(relative, graph_stamp(stamp));
    }
    Some(files)
}

fn climbs_out(relative: &str) -> bool {
    crate::project::plain_relative(Path::new(relative)).is_err() || relative.contains('\\')
}

fn graph_stamp(stamp: Stamp) -> GraphStamp {
    GraphStamp {
        bytes: stamp.size,
        modified_ms: stamp
            .modified
            .duration_since(UNIX_EPOCH)
            .ok()
            .and_then(|since| i64::try_from(since.as_millis()).ok())
            .unwrap_or(0),
    }
}

/// What the core measures of the file `relative` under `root`: its lines (`\n` bytes)
/// and SHA-256. A file that holds secrets or is too large is measured by size only.
pub fn measure(root: &Path, relative: &str) -> std::io::Result<WalkedFile> {
    let path = root.join(relative);
    let meta = std::fs::symlink_metadata(&path)?;
    let stamp =
        Stamp::from_metadata(&meta).ok_or_else(|| std::io::Error::other("not a regular file"))?;
    let stamp = graph_stamp(stamp);
    let mut walked = WalkedFile {
        path: relative.to_owned(),
        lines: 0,
        bytes: stamp.bytes,
        hash: String::new(),
        modified_ms: stamp.modified_ms,
    };
    if !may_keep_text(relative) || stamp.bytes > MAX_READ_BYTES {
        return Ok(walked);
    }
    let mut file = std::fs::File::open(&path)?;
    let mut hasher = Sha256::new();
    let mut buffer = vec![0u8; 64 * 1024];
    let (mut lines, mut read, mut binary) = (0u64, 0usize, false);
    loop {
        let n = file.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        let chunk = &buffer[..n];
        if read < BINARY_PROBE {
            let probe = &chunk[..chunk.len().min(BINARY_PROBE - read)];
            binary |= probe.contains(&0);
        }
        read += n;
        hasher.update(chunk);
        lines += bytecount(chunk);
    }
    walked.lines = if binary { 0 } else { lines };
    walked.hash = dusk_protocol::text::hex(&hasher.finalize());
    Ok(walked)
}

fn bytecount(chunk: &[u8]) -> u64 {
    chunk.iter().filter(|byte| **byte == b'\n').count() as u64
}
