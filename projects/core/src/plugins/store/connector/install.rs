//! Putting a release connector's program on this computer: the archive for this platform
//! is downloaded from GitHub with the store's client (https, public addresses, redirects
//! only to GitHub's asset hosts, capped), its SHA-256 is checked before anything is
//! unpacked, and only the program is taken out of it, under the rules of `unpack`, into
//! `<data>/connectors/<name>/<tag>/`. Nothing of the listing reaches a shell.

use std::{
    fs::{self, OpenOptions},
    io,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use dusk_protocol::text::sha256_hex;

use super::release::{self, Asset, MAX_DOWNLOAD, Release};
use crate::{
    init::paths::Paths,
    plugins::store::{
        fetch::{self, Via},
        unpack::{Item, walk},
    },
};

/// Where an install in progress is put together, inside the connector's folder; the dot
/// keeps it from reading as a tag.
const STAGING: &str = ".staging";

/// Downloads, checks and places the program of `name`; answers where it is. Called under
/// the store's lock.
pub async fn install(paths: &Paths, name: &str, release: &Release) -> Result<PathBuf> {
    if let Some(why) = release.unfit(name) {
        bail!(why);
    }
    let asset = release.asset().context("no build for this computer")?;
    let url = release.url(asset);
    let archive = fetch::fetch(&url, asset.size.unwrap_or(MAX_DOWNLOAD), Via::Package)
        .await
        .with_context(|| format!("the program of {name}"))?;
    let (paths, name, release, asset) = (
        paths.clone(),
        name.to_owned(),
        release.clone(),
        asset.clone(),
    );
    tokio::task::spawn_blocking(move || place(&paths, &name, &release, &asset, &archive))
        .await
        .map_err(|error| anyhow!("the install of the program stopped: {error}"))?
}

/// Checks `archive` against `asset` and puts its program in place: nothing is unpacked
/// until its size and SHA-256 are the listing's.
pub fn place(
    paths: &Paths,
    name: &str,
    release: &Release,
    asset: &Asset,
    archive: &[u8],
) -> Result<PathBuf> {
    check(asset, archive)?;
    let root = release::folder(paths, name);
    crate::init::create_private_dir(&root)?;
    let staging = root.join(STAGING);
    // Under the store's lock, so what is here was left by an install that stopped.
    if fs::symlink_metadata(&staging).is_ok() {
        fs::remove_dir_all(&staging)?;
    }
    crate::init::create_private_dir(&staging)?;
    let program = Release::program(asset);
    let target = root.join(&release.tag);
    let result = (|| {
        take_program(archive, &asset.binary, &staging.join(program))?;
        unquarantine(&staging.join(program))?;
        // Ours: the same tag added again is replaced whole.
        if fs::symlink_metadata(&target).is_ok() {
            fs::remove_dir_all(&target)
                .with_context(|| format!("cannot replace {}", target.display()))?;
        }
        fs::rename(&staging, &target)
            .with_context(|| format!("cannot put the program in {}", target.display()))?;
        Ok(target.join(program))
    })();
    if fs::symlink_metadata(&staging).is_ok() {
        let _ = fs::remove_dir_all(&staging);
    }
    result
}

/// The archive is the one the listing names: its size when given, under the cap, and
/// its SHA-256.
pub fn check(asset: &Asset, archive: &[u8]) -> Result<()> {
    let length = archive.len() as u64;
    ensure!(
        length <= MAX_DOWNLOAD,
        "{} is over {MAX_DOWNLOAD} bytes",
        asset.file
    );
    if let Some(size) = asset.size {
        ensure!(
            length == size,
            "{} has {length} bytes, the list says {size}",
            asset.file
        );
    }
    ensure!(
        sha256_hex(archive) == asset.sha256,
        "{} is not the file the list names: its SHA-256 differs",
        asset.file
    );
    Ok(())
}

/// Writes the file at `binary` in the `.tar.gz` `archive` to `to`, executable. Every
/// entry is held to `unpack`'s rules, so an archive with a link, a path out of its
/// folder or too many bytes is refused whole, even where the program is fine.
fn take_program(archive: &[u8], binary: &str, to: &Path) -> Result<()> {
    let wanted = Path::new(binary);
    let mut found = false;
    walk(archive, |relative, item| {
        if relative != wanted {
            return Ok(());
        }
        let Item::File { size, body, .. } = item else {
            bail!("{binary} in the archive is a folder, not a program");
        };
        ensure!(!found, "the archive holds {binary} twice");
        found = true;
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o700)
            .open(to)
            .with_context(|| format!("cannot write {}", to.display()))?;
        let copied = io::copy(body, &mut file)?;
        ensure!(copied == size, "{binary} is cut short");
        file.sync_all()?;
        Ok(())
    })?;
    ensure!(found, "the archive has no {binary}");
    Ok(())
}

/// Takes the quarantine mark off the program alone, so macOS starts it without asking:
/// the user agreed to it in duskcode. A file without the mark is fine as it is.
#[cfg(target_os = "macos")]
pub fn unquarantine(program: &Path) -> Result<()> {
    use std::{ffi::CString, os::unix::ffi::OsStrExt};
    let path = CString::new(program.as_os_str().as_bytes()).context("a path with a NUL")?;
    // SAFETY: both are NUL-terminated strings that outlive the call.
    let done = unsafe {
        libc::removexattr(
            path.as_ptr(),
            c"com.apple.quarantine".as_ptr(),
            libc::XATTR_NOFOLLOW,
        )
    };
    if done == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    match error.raw_os_error() {
        Some(libc::ENOATTR) => Ok(()),
        _ => Err(error).context("cannot clear the quarantine mark of the program"),
    }
}

#[cfg(not(target_os = "macos"))]
pub fn unquarantine(_program: &Path) -> Result<()> {
    Ok(())
}

/// Keeps of `name`'s folder only the version its recorded server runs, `keep`; with
/// none, the whole folder goes. Called once a connector was added, failed to be added
/// or was removed, so an update leaves no old version and a failed add no new one.
pub fn tidy(paths: &Paths, name: &str, keep: Option<&str>) -> Result<()> {
    let root = release::folder(paths, name);
    let Ok(meta) = fs::symlink_metadata(&root) else {
        return Ok(());
    };
    ensure!(
        meta.is_dir(),
        "{} is not a folder; remove it yourself",
        root.display()
    );
    let Some(keep) = keep else {
        return fs::remove_dir_all(&root)
            .with_context(|| format!("cannot remove {}", root.display()));
    };
    for item in fs::read_dir(&root).with_context(|| format!("cannot read {}", root.display()))? {
        let item = item?;
        if item.file_name() == keep {
            continue;
        }
        let path = item.path();
        let removed = match item.file_type()?.is_dir() {
            true => fs::remove_dir_all(&path),
            false => fs::remove_file(&path),
        };
        removed.with_context(|| format!("cannot remove {}", path.display()))?;
    }
    Ok(())
}
