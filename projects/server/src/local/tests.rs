use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::{Path, PathBuf},
    sync::atomic::{AtomicUsize, Ordering},
};

use dusk_core::app::{Paths, shared_core};
use dusk_protocol::client::OwnerKind;

use super::files::{TOO_LONG, euid, locate_in, owned_by_us};
use super::{CoreLock, LockInfo, RunFiles, read_info, read_key, write_key};

/// A fresh folder, removed when dropped. Short, so the socket path stays under the limit.
struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!("dr-{}-{n}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
        Self(root)
    }

    fn db(&self) -> PathBuf {
        self.0.join("session/state.db")
    }

    /// The files with no folder counted as writable by commands: temporary folders are,
    /// and that rule has its own test.
    fn locate(&self, state_db: Option<&Path>) -> anyhow::Result<RunFiles> {
        locate_in(&self.0, &self.db(), state_db, None, &[])
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn mode(path: &Path) -> u32 {
    fs::symlink_metadata(path).unwrap().mode() & 0o777
}

#[test]
fn run_files_are_private() {
    let root = Root::new();
    let files = root.locate(None).unwrap();
    assert_eq!(files.dir, root.0.join("run"));
    assert_eq!(files.socket_path().unwrap(), root.0.join("run/core.sock"));
    assert_eq!(mode(&files.dir), 0o700);
    let lock = CoreLock::try_take(&files).unwrap().unwrap();
    // A temporary key file a crash left behind is cleared by the next write.
    fs::write(files.dir.join(".core.key.0011223344556677"), "stale").unwrap();
    let key = write_key(&lock, &files).unwrap();
    assert_eq!(mode(&files.key), 0o600);
    assert_eq!(read_key(&files).unwrap(), key);
    // Every start gets a new key, and no temporary file is left behind.
    assert_ne!(write_key(&lock, &files).unwrap(), key);
    let mut names: Vec<_> = fs::read_dir(&files.dir)
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect();
    names.sort();
    assert_eq!(names, ["core.key", "core.lock"]);
    // A key others can read is not trusted.
    fs::set_permissions(&files.key, fs::Permissions::from_mode(0o644)).unwrap();
    assert!(read_key(&files).is_err());
}

#[test]
fn a_wide_run_folder_is_narrowed() {
    let root = Root::new();
    let run = root.0.join("run");
    fs::create_dir(&run).unwrap();
    fs::set_permissions(&run, fs::Permissions::from_mode(0o755)).unwrap();
    root.locate(None).unwrap();
    assert_eq!(mode(&run), 0o700);
}

#[test]
fn a_run_folder_of_another_owner_is_refused() {
    let root = Root::new();
    let files = root.locate(None).unwrap();
    let meta = fs::symlink_metadata(&files.dir).unwrap();
    assert!(owned_by_us(&meta, euid()));
    assert!(!owned_by_us(&meta, euid().wrapping_add(1)));
}

#[test]
fn a_symlinked_run_folder_is_refused() {
    let root = Root::new();
    let elsewhere = root.0.join("elsewhere");
    fs::create_dir(&elsewhere).unwrap();
    std::os::unix::fs::symlink(&elsewhere, root.0.join("run")).unwrap();
    let error = root.locate(None).unwrap_err().to_string();
    assert!(error.contains("not a symlink"), "{error}");
}

#[test]
fn another_state_db_gets_its_own_files() {
    let root = Root::new();
    let default = root.locate(None).unwrap();
    assert_eq!(root.locate(Some(&root.db())).unwrap(), default);
    let other = root.locate(Some(&root.0.join("other.db"))).unwrap();
    for (path, extension) in [
        (&other.lock, "lock"),
        (&other.socket_path().unwrap().to_path_buf(), "sock"),
        (&other.key, "key"),
    ] {
        let name = path.file_name().unwrap().to_str().unwrap();
        let hash = name
            .strip_prefix("core-")
            .and_then(|rest| rest.strip_suffix(&format!(".{extension}")))
            .unwrap();
        assert_eq!(hash.len(), 12, "{name}");
        assert!(hash.bytes().all(|b| b.is_ascii_hexdigit()), "{name}");
    }
    assert_ne!(other.lock, default.lock);
    assert_ne!(other.socket, default.socket);
    assert_ne!(other.key, default.key);
}

#[test]
fn a_database_has_one_name_before_and_after_its_folder_exists() {
    let root = Root::new();
    // Through a symlinked folder, as /tmp is on macOS.
    let real = root.0.join("real");
    fs::create_dir(&real).unwrap();
    std::os::unix::fs::symlink(&real, root.0.join("link")).unwrap();
    let db = root.0.join("link/not-yet/x/state.db");
    let before = root.locate(Some(&db)).unwrap();
    fs::create_dir_all(real.join("not-yet/x")).unwrap();
    let after = root.locate(Some(&db)).unwrap();
    assert_eq!(before, after);
    // The same database spelled through the real folder or `.` is the same core.
    let spelled = root.0.join("real/./not-yet/x/state.db");
    assert_eq!(root.locate(Some(&spelled)).unwrap(), after);
    // `..` over a folder not made yet resolves once the folder is made.
    let around = root.0.join("real/missing/../not-yet/x/state.db");
    assert_eq!(root.locate(Some(&around)).unwrap(), after);
}

#[test]
fn two_spellings_of_a_new_folder_share_one_lock() {
    let root = Root::new();
    fs::write(root.0.join("Probe"), "").unwrap();
    if !root.0.join("probe").exists() {
        eprintln!("skipped: this file system tells case apart");
        return;
    }
    let upper = root.locate(Some(&root.0.join("New/state.db"))).unwrap();
    let lower = root.locate(Some(&root.0.join("new/state.db"))).unwrap();
    assert_eq!(upper.lock, lower.lock);
}

#[test]
fn only_the_holder_of_its_lock_writes_a_key() {
    let root = Root::new();
    let default = root.locate(None).unwrap();
    let other = root.locate(Some(&root.0.join("other.db"))).unwrap();
    let held = CoreLock::try_take(&default).unwrap().unwrap();
    assert!(write_key(&held, &other).is_err());
    assert!(!other.key.exists());
    write_key(&held, &default).unwrap();
}

#[test]
fn a_long_path_fails_without_xdg() {
    let root = Root::new();
    let long = root.0.join("a".repeat(100));
    let db = long.join("state.db");
    // The lock and the key are still had, so a second core stays off the database; only
    // serving fails, and only when asked for the socket.
    let files = locate_in(&long, &db, None, None, &[]).unwrap();
    assert_eq!(files.socket_path().unwrap_err().to_string(), TOO_LONG);
    assert_eq!(
        TOO_LONG,
        "the config folder path is too long for a socket; move it or set DUSKCODE_CONFIG to a \
         shorter path"
    );
    let lock = CoreLock::try_take(&files)
        .unwrap()
        .expect("the lock is taken");
    assert!(CoreLock::try_take(&files).unwrap().is_none());
    write_key(&lock, &files).unwrap();
    drop(lock);
    // With a private runtime folder (Linux's XDG_RUNTIME_DIR) only the socket moves there.
    let runtime = root.0.join("xdg");
    fs::create_dir(&runtime).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let files = locate_in(&long, &db, None, Some(&runtime), &[]).unwrap();
    assert_eq!(
        files.socket_path().unwrap(),
        runtime.join("duskcode/core.sock")
    );
    assert_eq!(mode(&runtime.join("duskcode")), 0o700);
    assert_eq!(files.lock, long.join("run/core.lock"));
    assert_eq!(files.key, long.join("run/core.key"));
    // A relative runtime folder, or one open to others, is not used.
    let relative = locate_in(&long, &db, None, Some(Path::new("x")), &[]).unwrap();
    assert_eq!(relative.socket_path().unwrap_err().to_string(), TOO_LONG);
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o755)).unwrap();
    let open = locate_in(&long, &db, None, Some(&runtime), &[]).unwrap();
    assert_eq!(open.socket_path().unwrap_err().to_string(), TOO_LONG);
}

#[test]
fn a_socket_folder_in_a_writable_folder_is_refused() {
    let root = Root::new();
    let error = locate_in(
        &root.0,
        &root.db(),
        None,
        None,
        std::slice::from_ref(&root.0),
    )
    .unwrap_err();
    assert!(error.to_string().contains("may write in"), "{error}");
    assert!(!root.0.join("run").exists(), "nothing is created there");
    // The temporary folders are writable by every command, so a config there is refused.
    let paths = Paths::new(&root.0, root.0.join("workspace"));
    assert!(RunFiles::locate(&paths, None).is_err());
    // So is a runtime folder inside one.
    let other = Root::new();
    let runtime = other.0.join("xdg");
    fs::create_dir(&runtime).unwrap();
    fs::set_permissions(&runtime, fs::Permissions::from_mode(0o700)).unwrap();
    let long = root.0.join("a".repeat(100));
    let files = locate_in(
        &long,
        &long.join("state.db"),
        None,
        Some(&runtime),
        std::slice::from_ref(&other.0),
    )
    .unwrap();
    let error = files.socket_path().unwrap_err();
    assert!(error.to_string().contains("may write in"), "{error}");
    assert!(
        !other.0.join("xdg/duskcode").exists(),
        "nothing is created there"
    );
}

#[test]
fn the_lock_is_taken_once() {
    let root = Root::new();
    let files = root.locate(None).unwrap();
    let first = CoreLock::try_take(&files).unwrap().expect("first take");
    assert_eq!(mode(&files.lock), 0o600);
    assert!(CoreLock::try_take(&files).unwrap().is_none());
    let info = LockInfo {
        pid: std::process::id(),
        version: "0.77.0".to_owned(),
        protocol: 36,
        owner: OwnerKind::Tui,
        started_at: 1,
        exe: PathBuf::from("/usr/bin/duskcode"),
    };
    first.write_info(&info).unwrap();
    assert_eq!(read_info(&files), Some(info.clone()));
    // A shorter line replaces the longer one whole.
    let short = LockInfo {
        version: "1".to_owned(),
        ..info
    };
    first.write_info(&short).unwrap();
    assert_eq!(read_info(&files), Some(short));
    drop(first);
    assert!(CoreLock::try_take(&files).unwrap().is_some());
}

#[test]
fn config_without_core_section_is_not_shared() {
    let root = Root::new();
    let paths = Paths::new(&root.0, root.0.join("workspace"));
    assert!(!shared_core(&paths).unwrap(), "no config file");
    let write = |text: &str| {
        fs::write(paths.config_file(), text).unwrap();
        fs::set_permissions(paths.config_file(), fs::Permissions::from_mode(0o600)).unwrap();
    };
    write("providers: {}\n");
    assert!(!shared_core(&paths).unwrap());
    write("providers: {}\ncore:\n  shared: true\n");
    assert!(shared_core(&paths).unwrap());
    write("providers: {}\ncore:\n  shared: false\n  other: 1\n");
    assert!(shared_core(&paths).is_err(), "unknown fields are refused");
}
