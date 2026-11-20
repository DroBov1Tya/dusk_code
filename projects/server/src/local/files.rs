//! The run folder: where the lock, the socket and the key of a core are, created private
//! and outside every folder a sandboxed command may write in; and the key itself.

use std::{
    fs::{self, DirBuilder, Metadata, OpenOptions},
    io::{Read, Write},
    os::unix::{
        ffi::OsStrExt,
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    },
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, ensure};
use dusk_core::app::{Paths, command_writable_folders};
use dusk_protocol::text::hex;
use nix::libc::{O_DIRECTORY, O_NOFOLLOW, O_NONBLOCK};
use sha2::{Digest, Sha256};
use tracing::warn;

use super::CoreLock;

const RUN_DIR: &str = "run";
/// `sun_path` holds 104 bytes on macOS and 108 on Linux; a longer socket path cannot bind.
const MAX_SOCKET_PATH: usize = 100;
pub(super) const TOO_LONG: &str = "the config folder path is too long for a socket; move it or \
                                   set DUSKCODE_CONFIG to a shorter path";
const KEY_LEN: usize = 32;

/// The files of one core: one per state database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunFiles {
    /// `run/` under the config root; holds the lock and the key, and the socket unless
    /// its path is too long.
    pub dir: PathBuf,
    pub lock: PathBuf,
    /// Where the socket goes, or why it cannot be had; the lock and the key do not depend
    /// on it, so a core that cannot serve still keeps a second one off the database.
    pub socket: std::result::Result<PathBuf, String>,
    pub key: PathBuf,
}

impl RunFiles {
    /// The files for the core over `state_db` (the default database when `None`), with
    /// their folders created private and checked.
    pub fn locate(paths: &Paths, state_db: Option<&Path>) -> Result<Self> {
        // Per-user and 0700 by the system; elsewhere there is no such folder to fall back on.
        #[cfg(target_os = "linux")]
        let runtime_dir = std::env::var_os("XDG_RUNTIME_DIR").map(PathBuf::from);
        #[cfg(not(target_os = "linux"))]
        let runtime_dir: Option<PathBuf> = None;
        locate_in(
            paths.config_root(),
            &paths.state_db(),
            state_db,
            runtime_dir.as_deref(),
            &command_writable_folders(paths),
        )
    }

    /// The socket's path, or the reason there is none, such as a config path too long.
    pub fn socket_path(&self) -> Result<&Path> {
        self.socket.as_deref().map_err(|reason| anyhow!("{reason}"))
    }
}

/// `locate` with what it reads from the environment given, so each rule can be tested.
pub(super) fn locate_in(
    config_root: &Path,
    default_db: &Path,
    state_db: Option<&Path>,
    runtime_dir: Option<&Path>,
    writable: &[PathBuf],
) -> Result<RunFiles> {
    let stem = match state_db {
        Some(db) => {
            // The folders are made first so the file system spells the name: two spellings
            // of a folder not made yet (case, Unicode form, `..`) must not get two locks.
            // The core makes the same folder when it opens the database.
            create_parent(db)?;
            create_parent(default_db)?;
            let db = physical(db).context("cannot resolve the state database path")?;
            if db == physical(default_db).context("cannot resolve the state database path")? {
                "core".to_owned()
            } else {
                format!("core-{}", short_hash(&db))
            }
        }
        None => "core".to_owned(),
    };
    let dir = config_root.join(RUN_DIR);
    let uid = euid();
    prepare(&dir, uid, writable)?;
    let socket = socket_in(&dir, &format!("{stem}.sock"), runtime_dir, uid, writable)
        .map_err(|error| format!("{error:#}"));
    Ok(RunFiles {
        lock: dir.join(format!("{stem}.lock")),
        key: dir.join(format!("{stem}.key")),
        socket,
        dir,
    })
}

/// The socket in `run/`, or in the runtime folder when that path does not fit `sun_path`.
fn socket_in(
    dir: &Path,
    name: &str,
    runtime_dir: Option<&Path>,
    uid: u32,
    writable: &[PathBuf],
) -> Result<PathBuf> {
    let socket = dir.join(name);
    if socket.as_os_str().len() <= MAX_SOCKET_PATH {
        return Ok(socket);
    }
    // Never a temporary folder: a sandboxed command could unlink and rebind it there.
    let runtime = runtime_dir
        .filter(|folder| is_private_runtime(folder, uid))
        .ok_or_else(|| anyhow!(TOO_LONG))?;
    let folder = runtime.join("duskcode");
    let socket = folder.join(name);
    ensure!(socket.as_os_str().len() <= MAX_SOCKET_PATH, TOO_LONG);
    prepare(&folder, uid, writable)?;
    Ok(socket)
}

/// The runtime folder is only used as the system makes it: absolute, ours and closed to
/// others.
fn is_private_runtime(folder: &Path, uid: u32) -> bool {
    if !folder.is_absolute() {
        return false;
    }
    let usable = fs::symlink_metadata(folder)
        .is_ok_and(|meta| meta.is_dir() && owned_by_us(&meta, uid) && meta.mode() & 0o077 == 0);
    if !usable {
        warn!(folder = %folder.display(), "XDG_RUNTIME_DIR is not a private folder of this user; not used");
    }
    usable
}

/// Checks `folder` lies outside the writable folders before creating it, then creates it
/// private and checks again on the real path.
fn prepare(folder: &Path, uid: u32, writable: &[PathBuf]) -> Result<()> {
    refuse_writable(&physical(folder)?, writable)?;
    private_dir(folder, uid)?;
    let real =
        fs::canonicalize(folder).with_context(|| format!("cannot resolve {}", folder.display()))?;
    refuse_writable(&real, writable)
}

fn create_parent(db: &Path) -> Result<()> {
    match db.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        Some(parent) => DirBuilder::new()
            .recursive(true)
            .mode(0o700)
            .create(parent)
            .with_context(|| format!("cannot create {}", parent.display())),
        None => Ok(()),
    }
}

pub(super) fn euid() -> u32 {
    nix::unistd::geteuid().as_raw()
}

pub(super) fn owned_by_us(meta: &Metadata, uid: u32) -> bool {
    meta.uid() == uid
}

/// The path with symlinks resolved, whether or not it exists yet: the deepest existing
/// folder is resolved and the rest appended, so one path gets one name before and after
/// its folders are created. `..` past an existing folder is refused, as it could only be
/// resolved once the folders exist.
pub(super) fn physical(path: &Path) -> Result<PathBuf> {
    let absolute =
        std::path::absolute(path).with_context(|| format!("cannot resolve {}", path.display()))?;
    let existing = absolute
        .ancestors()
        .find(|ancestor| fs::symlink_metadata(ancestor).is_ok())
        .ok_or_else(|| anyhow!("cannot resolve {}", path.display()))?;
    let mut real = fs::canonicalize(existing)
        .with_context(|| format!("cannot resolve {}", existing.display()))?;
    let rest = absolute
        .strip_prefix(existing)
        .with_context(|| format!("cannot resolve {}", path.display()))?;
    for component in rest.components() {
        match component {
            Component::Normal(name) => real.push(name),
            Component::CurDir => {}
            _ => anyhow::bail!(
                "{} uses `..` in folders that do not exist yet",
                path.display()
            ),
        }
    }
    Ok(real)
}

/// 12 hex digits of the SHA-256 of the path.
fn short_hash(path: &Path) -> String {
    hex(&Sha256::digest(path.as_os_str().as_bytes())[..6])
}

/// Creates `folder` as 0700, refuses one of another user or a symlink and narrows a wider
/// mode, all through one handle so the folder checked is the folder changed.
fn private_dir(folder: &Path, uid: u32) -> Result<()> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(folder)
        .with_context(|| format!("cannot create {}", folder.display()))?;
    let handle = OpenOptions::new()
        .read(true)
        .custom_flags(O_DIRECTORY | O_NOFOLLOW)
        .open(folder)
        .with_context(|| format!("{} must be a folder, not a symlink", folder.display()))?;
    let meta = handle
        .metadata()
        .with_context(|| format!("cannot inspect {}", folder.display()))?;
    ensure!(
        meta.is_dir(),
        "{} must be a folder, not a symlink",
        folder.display()
    );
    ensure!(
        owned_by_us(&meta, uid),
        "{} belongs to another user; refusing to use it",
        folder.display()
    );
    if meta.mode() & 0o077 != 0 {
        warn!(folder = %folder.display(), "the run folder was open to others; narrowed to 0700");
        handle
            .set_permissions(fs::Permissions::from_mode(0o700))
            .with_context(|| format!("cannot narrow {}", folder.display()))?;
    }
    Ok(())
}

/// A folder a sandboxed command may write in would let it replace the socket or the key.
fn refuse_writable(real: &Path, writable: &[PathBuf]) -> Result<()> {
    for open in writable {
        let open = fs::canonicalize(open).unwrap_or_else(|_| open.clone());
        ensure!(
            !real.starts_with(&open),
            "refusing the run folder {}: commands the agent runs may write in {}",
            real.display(),
            open.display()
        );
    }
    Ok(())
}

/// A new random key for this core, written 0600 through a file of its own that replaces
/// the old key at once, so a reader never sees half a key. Only the holder of the lock
/// may replace the key of a core.
pub fn write_key(held: &CoreLock, files: &RunFiles) -> Result<[u8; KEY_LEN]> {
    ensure!(
        held.path() == files.lock,
        "the key of a core is written only by the holder of its lock"
    );
    let mut key = [0u8; KEY_LEN];
    let mut tag = [0u8; 8];
    getrandom::fill(&mut key).map_err(|error| anyhow!("no randomness: {error}"))?;
    getrandom::fill(&mut tag).map_err(|error| anyhow!("no randomness: {error}"))?;
    let folder = files
        .key
        .parent()
        .ok_or_else(|| anyhow!("the key path has no folder"))?;
    let name = files
        .key
        .file_name()
        .ok_or_else(|| anyhow!("the key path has no name"))?
        .to_string_lossy();
    let prefix = format!(".{name}.");
    remove_stale(folder, &prefix);
    let temp = folder.join(format!("{prefix}{}", hex(&tag)));
    let written = (|| -> std::io::Result<()> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .custom_flags(O_NOFOLLOW)
            .open(&temp)?;
        file.write_all(hex(&key).as_bytes())?;
        file.sync_all()?;
        fs::rename(&temp, &files.key)
    })();
    if let Err(error) = written {
        // The write already failed; a leftover temporary file is only untidy.
        if let Err(cleanup) = fs::remove_file(&temp) {
            warn!(%cleanup, "cannot remove the temporary key file");
        }
        return Err(error).context("cannot write the core key");
    }
    Ok(key)
}

/// Temporary key files a crash left behind; under the lock no other write is under way.
fn remove_stale(folder: &Path, prefix: &str) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        if entry.file_name().as_bytes().starts_with(prefix.as_bytes())
            && let Err(error) = fs::remove_file(entry.path())
        {
            warn!(%error, "cannot remove a stale temporary key file");
        }
    }
}

/// The key of the running core. Refused unless it is a private regular file of this user.
pub fn read_key(files: &RunFiles) -> Result<[u8; KEY_LEN]> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(&files.key)
        .context("cannot open the core key")?;
    let meta = file.metadata().context("cannot inspect the core key")?;
    ensure!(
        meta.is_file() && owned_by_us(&meta, euid()) && meta.mode() & 0o077 == 0,
        "the core key must be a private file of this user"
    );
    let mut text = Vec::new();
    file.take(2 * KEY_LEN as u64 + 1)
        .read_to_end(&mut text)
        .context("cannot read the core key")?;
    unhex(&text).ok_or_else(|| anyhow!("the core key is malformed"))
}

fn unhex(text: &[u8]) -> Option<[u8; KEY_LEN]> {
    if text.len() != 2 * KEY_LEN || !text.iter().all(u8::is_ascii_hexdigit) {
        return None;
    }
    let mut key = [0u8; KEY_LEN];
    for (byte, pair) in key.iter_mut().zip(text.as_chunks::<2>().0) {
        let pair = std::str::from_utf8(pair).ok()?;
        *byte = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(key)
}
