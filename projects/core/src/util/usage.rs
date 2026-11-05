//! What the application costs the machine: its own process, sampled with `ps`, and
//! what its records take on disk.

use std::{fs, path::Path, process::Command};

pub use dusk_protocol::system::ProcessUsage;

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
