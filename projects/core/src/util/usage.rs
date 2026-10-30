//! What the application costs the machine: its own process, sampled with `ps`, and
//! what its records take on disk.

use std::{fs, path::Path, process::Command};

/// The running process as `ps` sees it; `None` where `ps` is missing.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ProcessUsage {
    pub resident_bytes: u64,
    /// Of one core, averaged over the process lifetime on macOS and recently on Linux.
    pub cpu_percent: f32,
}

pub fn process() -> Option<ProcessUsage> {
    let output = Command::new("ps")
        .args(["-o", "rss=,%cpu=", "-p"])
        .arg(std::process::id().to_string())
        .output()
        .ok()?;
    let text = String::from_utf8(output.stdout).ok()?;
    let mut fields = text.split_whitespace();
    let resident_kib: u64 = fields.next()?.parse().ok()?;
    let cpu_percent = fields.next()?.replace(',', ".").parse().ok()?;
    Some(ProcessUsage {
        resident_bytes: resident_kib * 1024,
        cpu_percent,
    })
}

/// The size of a file, or of everything under a folder; links are not followed.
pub fn disk_bytes(path: &Path) -> u64 {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return 0;
    };
    if !meta.is_dir() {
        return meta.len();
    }
    fs::read_dir(path).map_or(0, |entries| {
        entries
            .flatten()
            .map(|entry| disk_bytes(&entry.path()))
            .sum()
    })
}
