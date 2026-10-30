//! Offline, version-matched release resources. Release archives carry these files
//! beside the binary; install/update activates their snapshot before the binary.

use std::{
    fs::{self, OpenOptions, TryLockError},
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};

use crate::{init::create_private_dir, release::update::VERSION};

const CATEGORIES: [&str; 4] = ["agents", "skills", "internal", "modes"];
const REQUIRED: [&str; 6] = [
    "internal/COMPACT.md",
    "internal/FORK.md",
    "internal/MEMORY.md",
    "modes/MAIN.md",
    "modes/PLAN.md",
    "modes/ORCHESTRATOR.md",
];

pub fn data_root() -> Result<PathBuf> {
    let root = match std::env::var_os("XDG_DATA_HOME").filter(|value| !value.is_empty()) {
        Some(root) => PathBuf::from(root),
        None => {
            PathBuf::from(std::env::var_os("HOME").context("HOME is not set")?).join(".local/share")
        }
    };
    ensure!(root.is_absolute(), "the data directory must be absolute");
    Ok(root.join("duskcode"))
}

pub fn root() -> Result<PathBuf> {
    if let Some(root) = std::env::var_os("DUSKCODE_RESOURCE_DIR") {
        let root = PathBuf::from(root);
        ensure!(root.is_absolute(), "DUSKCODE_RESOURCE_DIR must be absolute");
        validate(&root, None)?;
        return Ok(root);
    }
    // Only developer builds search the source tree. Release binaries contain no
    // build-machine path and never select resources from the current directory.
    #[cfg(debug_assertions)]
    {
        let starts = [std::env::current_dir().ok(), std::env::current_exe().ok()];
        for start in starts.into_iter().flatten() {
            for directory in start.ancestors() {
                let root = directory.join("resources");
                if directory.join("Cargo.toml").is_file() && validate(&root, None).is_ok() {
                    return Ok(root);
                }
            }
        }
    }
    let root = data_root()?.join("resources").join(VERSION);
    validate(&root, Some(VERSION)).with_context(|| {
        format!("resources for {VERSION} are missing or invalid; reinstall duskcode")
    })?;
    Ok(root)
}

pub fn text(relative: &str) -> Result<String> {
    let relative = Path::new(relative);
    ensure!(
        relative
            .components()
            .all(|part| matches!(part, Component::Normal(_))),
        "invalid resource path"
    );
    fs::read_to_string(root()?.join(relative)).context("cannot read resource as UTF-8 text")
}

/// Enumerates regular files only. Links and special files are never followed or
/// copied into a snapshot or the user's editable configuration.
pub fn files(root: &Path) -> Result<Vec<PathBuf>> {
    fn visit(root: &Path, relative: &Path, out: &mut Vec<PathBuf>) -> Result<()> {
        let path = root.join(relative);
        let metadata = fs::symlink_metadata(&path).context("cannot inspect resource")?;
        if metadata.is_file() {
            out.push(relative.to_owned());
        } else if metadata.is_dir() {
            for entry in fs::read_dir(path).context("cannot list resources")? {
                let entry = entry?;
                visit(root, &relative.join(entry.file_name()), out)?;
            }
        } else {
            bail!("resource must be a regular file or directory");
        }
        Ok(())
    }
    let mut out = Vec::new();
    visit(root, Path::new(""), &mut out)?;
    out.sort();
    Ok(out)
}

pub fn validate(root: &Path, version: Option<&str>) -> Result<()> {
    ensure!(
        fs::symlink_metadata(root)?.is_dir(),
        "resource root must be a directory"
    );
    ensure!(
        fs::symlink_metadata(root.join("FORMAT"))?.is_file(),
        "resource format must be a regular file"
    );
    ensure!(
        fs::read_to_string(root.join("FORMAT"))?.trim() == "1",
        "unsupported resource format"
    );
    for category in CATEGORIES {
        ensure!(
            fs::symlink_metadata(root.join(category))?.is_dir(),
            "missing resource category {category}"
        );
        files(&root.join(category))?;
    }
    for relative in REQUIRED {
        ensure!(
            !fs::read_to_string(root.join(relative))?.trim().is_empty(),
            "resource {relative} must not be blank"
        );
    }
    if let Some(version) = version {
        ensure!(
            fs::symlink_metadata(root.join("VERSION"))?.is_file(),
            "resource version must be a regular file"
        );
        ensure!(
            fs::read_to_string(root.join("VERSION"))?.trim() == version,
            "resources do not match binary version {version}"
        );
    }
    Ok(())
}

/// Publishes a complete immutable snapshot with one directory rename. An existing
/// snapshot must be identical; never replace files another running binary uses.
pub fn install_snapshot(source: &Path, version: &str) -> Result<PathBuf> {
    ensure!(
        !version.is_empty()
            && version
                .bytes()
                .all(|byte| byte.is_ascii_digit() || byte == b'.'),
        "invalid resource version"
    );
    validate(source, Some(version))?;
    let directory = data_root()?.join("resources");
    create_private_dir(&directory)?;
    // install.sh calls the new binary's resource-install command, so both paths
    // use this same OS lock. Its file stays in place; closing the handle releases
    // the lock even after SIGKILL or a machine restart. Never unlink lock files.
    let lock = directory.join(format!(".install-{version}.lock"));
    if let Ok(metadata) = fs::symlink_metadata(&lock) {
        ensure!(metadata.is_file(), "resource lock must be a regular file");
    }
    let mut options = OpenOptions::new();
    options.create(true).truncate(false).write(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let lock = options
        .open(&lock)
        .context("cannot open resource install lock")?;
    match lock.try_lock() {
        Ok(()) => {}
        Err(TryLockError::WouldBlock) => {
            bail!("another install is staging this version; retry later")
        }
        Err(TryLockError::Error(error)) => {
            return Err(error).context("cannot lock resource install");
        }
    }
    let target = directory.join(version);
    let same = |target: &Path| -> Result<bool> {
        validate(target, Some(version))?;
        let expected = files(source)?;
        if files(target)? != expected {
            return Ok(false);
        }
        for relative in expected {
            if fs::read(source.join(&relative))? != fs::read(target.join(relative))? {
                return Ok(false);
            }
        }
        Ok(true)
    };
    if fs::symlink_metadata(&target).is_ok() {
        ensure!(
            same(&target)?,
            "an incompatible resource snapshot already exists for {version}"
        );
        return Ok(target);
    }
    let staging = directory.join(format!(".staging-{}", uuid::Uuid::now_v7()));
    fs::create_dir(&staging).context("cannot stage resources")?;
    let installed = (|| -> Result<()> {
        for relative in files(source)? {
            let to = staging.join(&relative);
            create_private_dir(to.parent().context("resource has no directory")?)?;
            fs::copy(source.join(relative), to).context("cannot copy resource")?;
        }
        validate(&staging, Some(version))?;
        match fs::rename(&staging, &target) {
            Ok(()) => Ok(()),
            Err(_) if target.exists() && same(&target)? => Ok(()),
            Err(error) => Err(error).context("cannot activate resource snapshot"),
        }
    })();
    let _ = fs::remove_dir_all(&staging);
    installed?;
    Ok(target)
}
