//! Private portable archives. Version 1 is an uncompressed stream: magic, a little-endian
//! u64 JSON-manifest length, the manifest, then regular-file contents in manifest order.
//! No executable archive parser or external archiver is involved. Links and special files
//! are refused rather than followed; saved paths inside file contents are not rewritten.

use serde::{Deserialize, Serialize};

mod export;
mod files;
mod guard;
mod restore;
mod verify;

pub use export::export;
pub use guard::{RemovalGuard, RestoreGuard, resolve_state_db};
pub use restore::{recover, restore};

const MAGIC: &[u8] = b"DUSKCODE-ARCHIVE\n";
const FORMAT: &str = "duskcode-portable";
const VERSION: u32 = 1;
const BACKUPS: &str = "backups";
const MAX_MANIFEST_BYTES: u64 = 16 * 1024 * 1024;
const MAX_ENTRIES: usize = 100_000;
const MAX_DEPTH: usize = 128;
const MAX_PATH_BYTES: usize = 4096;
const MAX_PAYLOAD_BYTES: u64 = 1024 * 1024 * 1024 * 1024;
const BUFFER_BYTES: usize = 64 * 1024;
const PENDING_RESTORE: &str = ".restore-pending";
const RESTORE_LOCK: &str = ".restore.lock";
const MAX_TRANSACTION_BYTES: u64 = 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Manifest {
    format: String,
    version: u32,
    application_version: String,
    created_at_ms: i64,
    /// Backups are deliberately not backed up again.
    excluded_workspace_paths: Vec<String>,
    entries: Vec<Entry>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    /// Only `config[/...]` or `workspace[/...]`, with portable slash-separated names.
    path: String,
    kind: Kind,
    size: u64,
    sha256: Option<String>,
    executable: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Kind {
    Directory,
    File,
}
