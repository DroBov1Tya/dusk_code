//! What the application costs the machine: its own process and the ones it started,
//! sampled with `ps`, and what its records take on disk.

use std::{
    collections::{HashMap, HashSet},
    fs,
    path::Path,
    process::Command,
};

pub use dusk_protocol::system::{Children, ProcessUsage};

/// The app's own process and, summed, every process below it: MCP servers, plugin
/// programs, commands and terminals run as its children. Nothing where `ps` is missing.
pub fn process_tree() -> (Option<ProcessUsage>, Option<Children>) {
    let listed = Command::new("ps")
        .args(["-A", "-o", "pid=,ppid=,rss=,%cpu="])
        .output()
        .ok()
        .and_then(|output| String::from_utf8(output.stdout).ok());
    match listed {
        Some(text) => sum_tree(&text, std::process::id()),
        None => (None, None),
    }
}

/// Sums the processes below `root` in `ps` lines of `pid ppid rss %cpu`; a line that
/// does not read as four numbers is skipped.
fn sum_tree(text: &str, root: u32) -> (Option<ProcessUsage>, Option<Children>) {
    let mut own = None;
    let mut below: HashMap<u32, Vec<(u32, ProcessUsage)>> = HashMap::new();
    for line in text.lines() {
        let Some((pid, parent, usage)) = ps_line(line) else {
            continue;
        };
        if pid == root {
            own = Some(usage);
        } else {
            below.entry(parent).or_default().push((pid, usage));
        }
    }
    let mut seen = HashSet::from([root]);
    let mut waiting = vec![root];
    let (mut count, mut resident_bytes, mut cpu_percent) = (0u32, 0u64, 0f32);
    while let Some(parent) = waiting.pop() {
        for (pid, usage) in below.get(&parent).into_iter().flatten() {
            if seen.insert(*pid) {
                count += 1;
                resident_bytes = resident_bytes.saturating_add(usage.resident_bytes);
                cpu_percent += usage.cpu_percent;
                waiting.push(*pid);
            }
        }
    }
    let children = (count > 0).then_some(Children {
        count,
        usage: ProcessUsage {
            resident_bytes,
            cpu_percent,
        },
    });
    (own, children)
}

fn ps_line(line: &str) -> Option<(u32, u32, ProcessUsage)> {
    let mut fields = line.split_whitespace();
    let pid = fields.next()?.parse().ok()?;
    let parent = fields.next()?.parse().ok()?;
    let resident_kib: u64 = fields.next()?.parse().ok()?;
    let cpu_percent: f32 = fields.next()?.replace(',', ".").parse().ok()?;
    let usage = ProcessUsage {
        resident_bytes: resident_kib.saturating_mul(1024),
        cpu_percent: if cpu_percent.is_finite() {
            cpu_percent.max(0.)
        } else {
            0.
        },
    };
    Some((pid, parent, usage))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn children_and_theirs_are_summed_and_strangers_are_not() {
        let ps = "  10     1  2048  5.0\n  11    10  1024  1,5\n  12    11   512  0.5\n  \
                  13     1  9999 50.0\n  bad line\n  14    10  oops  1.0\n";
        let (own, children) = sum_tree(ps, 10);
        assert_eq!(own.unwrap().resident_bytes, 2048 * 1024);
        let children = children.unwrap();
        assert_eq!(children.count, 2);
        assert_eq!(children.usage.resident_bytes, 1536 * 1024);
        assert!((children.usage.cpu_percent - 2.0).abs() < 1e-6);
        assert_eq!(sum_tree("  10 1 2048 5.0\n", 10).1, None);
    }
}
