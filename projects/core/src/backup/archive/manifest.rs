//! The manifest of an archive and the rules its paths must satisfy.

use std::collections::BTreeMap;

use anyhow::{Context, Result, ensure};

use serde::{Deserialize, Serialize};

use super::{BACKUPS, FORMAT, MAX_DEPTH, MAX_ENTRIES, MAX_PATH_BYTES, MAX_PAYLOAD_BYTES, VERSION};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub application_version: String,
    pub created_at_ms: i64,
    /// Backups are deliberately not backed up again.
    pub excluded_workspace_paths: Vec<String>,
    pub entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Entry {
    /// Only `config[/...]` or `workspace[/...]`, with portable slash-separated names.
    pub path: String,
    pub kind: Kind,
    pub size: u64,
    pub sha256: Option<String>,
    pub executable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Directory,
    File,
}

pub fn validate_manifest(manifest: &Manifest) -> Result<u64> {
    ensure!(
        manifest.format == FORMAT && manifest.version == VERSION,
        "unsupported duskcode archive format or version"
    );
    ensure!(
        manifest.excluded_workspace_paths == vec![BACKUPS.to_owned()],
        "unsupported archive exclusions"
    );
    ensure!(
        !manifest.entries.is_empty() && manifest.entries.len() <= MAX_ENTRIES,
        "invalid archive entry count"
    );
    let mut seen = BTreeMap::new();
    let mut bytes = 0u64;
    for entry in &manifest.entries {
        validate_path(&entry.path)?;
        ensure!(
            !seen.contains_key(entry.path.as_str()),
            "duplicate archive path"
        );
        if let Some((parent, _)) = entry.path.rsplit_once('/') {
            ensure!(
                seen.get(parent) == Some(&Kind::Directory),
                "archive entry has no earlier directory parent"
            );
        }
        ensure!(
            entry.path != "workspace/backups" && !entry.path.starts_with("workspace/backups/"),
            "backup directory must not be inside the archive payload"
        );
        match entry.kind {
            Kind::Directory => ensure!(
                entry.size == 0 && entry.sha256.is_none() && !entry.executable,
                "invalid directory entry"
            ),
            Kind::File => {
                ensure!(entry.path.contains('/'), "archive root must be a directory");
                ensure!(
                    entry.sha256.as_ref().is_some_and(|hash| hash.len() == 64
                        && hash
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))),
                    "invalid archive checksum"
                );
                bytes = bytes
                    .checked_add(entry.size)
                    .context("archive payload size overflow")?;
                ensure!(
                    bytes <= MAX_PAYLOAD_BYTES,
                    "archive payload exceeds the 1 TiB limit"
                );
            }
        }
        seen.insert(entry.path.as_str(), entry.kind);
    }
    ensure!(
        seen.get("config") == Some(&Kind::Directory)
            && seen.get("workspace") == Some(&Kind::Directory),
        "archive must contain both data roots"
    );
    ensure!(
        seen.get("config/session/state.db") == Some(&Kind::File),
        "archive is missing the SQLite snapshot"
    );
    for suffix in ["-wal", "-shm", "-journal"] {
        ensure!(
            !seen.contains_key(format!("config/session/state.db{suffix}").as_str()),
            "archive contains an active database sidecar"
        );
    }
    Ok(bytes)
}

pub fn validate_path(path: &str) -> Result<()> {
    ensure!(
        path.len() <= MAX_PATH_BYTES,
        "archive path exceeds the 4096-byte limit"
    );
    let parts: Vec<_> = path.split('/').collect();
    ensure!(
        parts.len() <= MAX_DEPTH && matches!(parts[0], "config" | "workspace"),
        "invalid archive root or path depth"
    );
    for part in parts {
        validate_segment(part)?;
    }
    Ok(())
}

pub fn validate_segment(name: &str) -> Result<()> {
    ensure!(
        !name.is_empty()
            && name != "."
            && name != ".."
            && name.len() <= 255
            && !name.ends_with(['.', ' '])
            && !name.chars().any(|c| c.is_control()
                || matches!(c, '/' | '\\' | ':' | '<' | '>' | '"' | '|' | '?' | '*')),
        "archive contains a non-portable filename or unsafe path"
    );
    let device = name.split('.').next().unwrap_or("").to_ascii_uppercase();
    ensure!(
        !matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL")
            && !(device.len() == 4
                && (device.starts_with("COM") || device.starts_with("LPT"))
                && matches!(device.as_bytes()[3], b'1'..=b'9')),
        "archive contains a reserved device filename"
    );
    Ok(())
}
