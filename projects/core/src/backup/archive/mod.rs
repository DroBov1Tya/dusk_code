//! Private portable archives. Version 1 is an uncompressed stream: magic, a little-endian
//! u64 JSON-manifest length, the manifest, then regular-file contents in manifest order.
//! No executable archive parser or external archiver is involved. Links and special files
//! are refused rather than followed; saved paths inside file contents are not rewritten.

mod export;
mod files;
mod inventory;
mod manifest;
mod overlap;
mod removal_guard;
mod restore;
mod restore_guard;
mod transaction;
mod verify;

pub use export::export;
pub use removal_guard::RemovalGuard;
pub use restore::restore;
pub use restore_guard::{RestoreGuard, resolve_state_db};
pub use transaction::recover;

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
