//! What a listing holds, for its Contents tab: the files of the copy installed here, or
//! else of the package the last list read named, fetched and checked like an install.
//! A package is read in memory under the rules of `unpack` and never written out, so
//! nothing of it is installed, run or read by the model.
//!
//! A file is answered only by a path of the last contents listed for that listing,
//! never joined from what a client sends, and only as UTF-8 text with its control and
//! bidi characters escaped.

use std::{
    collections::{BTreeSet, VecDeque},
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, LazyLock},
};

use anyhow::{Context, Result, anyhow, bail, ensure};
use dusk_protocol::plugin::{Contents, FilePreview, FileRef, LOCAL_SOURCE, PREVIEW_BYTES};
use parking_lot::Mutex;

use super::{
    fetch::{self, Via},
    record::Record,
    source,
    unpack::{self, Item, MAX_FILES, MAX_PATH, MAX_UNPACKED_BYTES},
};
use crate::{
    config::Mirror,
    init::{files::open_no_follow, paths::Paths},
    plugins::{catalog, manifest::MANIFEST_FILE},
};

/// The listings whose contents are kept for their files to be asked for.
const KEPT: usize = 3;
/// What is read of a file to tell text from anything else.
const SNIFF: usize = 8 * 1024;

/// Where a listing's files are read from.
#[derive(Clone)]
enum Root {
    Folder(PathBuf),
    /// The checked `.tar.gz`.
    Package(Arc<Vec<u8>>),
}

struct Opened {
    source: String,
    name: String,
    root: Root,
    contents: Contents,
}

/// The last contents listed, newest first.
static OPENED: LazyLock<Mutex<VecDeque<Opened>>> = LazyLock::new(Mutex::default);

/// The files of `name` from `source`, kept so `file` can answer one of them.
pub async fn contents(
    paths: &Paths,
    mirrors: &[Mirror],
    source: &str,
    name: &str,
) -> Result<Contents> {
    ensure!(
        crate::init::paths::is_plain_folder_name(name),
        "invalid name"
    );
    let (root, sha256, installed) = match installed_folder(paths, source, name)? {
        Some((folder, sha256)) => (Root::Folder(folder), sha256, true),
        None => {
            let seen = source::seen(mirrors, source, name)?;
            let package = seen.package.with_context(|| {
                format!("{source} offers no package of {name} for this machine")
            })?;
            let bytes = match kept_package(&package.sha256) {
                Some(bytes) => bytes,
                None => Arc::new(
                    fetch::fetch_checked(&package.url, package.size, &package.sha256, Via::Package)
                        .await
                        .with_context(|| format!("the package of {name}"))?,
                ),
            };
            (Root::Package(bytes), package.sha256, false)
        }
    };
    let listed = root.clone();
    let files = tokio::task::spawn_blocking(move || match &listed {
        Root::Folder(folder) => list_folder(folder),
        Root::Package(bytes) => list_package(bytes),
    })
    .await
    .context("the listing stopped")??;
    let contents = Contents {
        files,
        package_sha256: sha256,
        installed,
    };
    let mut opened = OPENED.lock();
    opened.retain(|item| !(item.source == source && item.name == name));
    opened.push_front(Opened {
        source: source.to_owned(),
        name: name.to_owned(),
        root,
        contents: contents.clone(),
    });
    opened.truncate(KEPT);
    Ok(contents)
}

/// The text file at `path` of the last contents listed for `name` of `source`.
pub async fn file(source: &str, name: &str, path: &str) -> Result<FilePreview> {
    let (root, size) = {
        let opened = OPENED.lock();
        let item = opened
            .iter()
            .find(|item| item.source == source && item.name == name)
            .ok_or_else(|| anyhow!("open the contents of {name} again"))?;
        let file = item
            .contents
            .files
            .iter()
            .find(|file| file.path == path)
            .ok_or_else(|| anyhow!("{name} has no file {}", path.escape_debug()))?;
        ensure!(
            file.text,
            "{} is not text; it is shown by its size",
            file.path
        );
        (item.root.clone(), file.size)
    };
    let wanted = path.to_owned();
    let bytes = tokio::task::spawn_blocking(move || match &root {
        Root::Folder(folder) => read_folder_file(folder, &wanted),
        Root::Package(bytes) => read_package_file(bytes, &wanted),
    })
    .await
    .context("the read stopped")??;
    let truncated = size > PREVIEW_BYTES as u64 || bytes.len() > PREVIEW_BYTES;
    preview(&bytes, truncated)
}

/// The folder `source` installed `name` in, with the SHA-256 it was installed from; a
/// plugin of the plugins folder no source installed is `local`'s.
fn installed_folder(paths: &Paths, source: &str, name: &str) -> Result<Option<(PathBuf, String)>> {
    let record = Record::load(paths)?;
    if source == LOCAL_SOURCE {
        let folder = catalog::folder(paths).join(name);
        let local = !record.installed.contains_key(name) && folder.join(MANIFEST_FILE).is_file();
        return Ok(local.then(|| (folder, String::new())));
    }
    Ok(record
        .present(paths, name)
        .filter(|item| item.source == source)
        .map(|item| (item.path(paths, name), item.sha256.clone())))
}

/// A package held for its files to be read, by its hash.
fn kept_package(sha256: &str) -> Option<Arc<Vec<u8>>> {
    OPENED.lock().iter().find_map(|item| match &item.root {
        Root::Package(bytes) if item.contents.package_sha256 == sha256 => Some(Arc::clone(bytes)),
        _ => None,
    })
}

/// The files of a checked package under the rules of `unpack`; a path given twice
/// refuses it, as unpacking would.
pub(super) fn list_package(package: &[u8]) -> Result<Vec<FileRef>> {
    let mut files = Vec::new();
    let mut seen = BTreeSet::new();
    unpack::walk(package, |relative, item| {
        let Item::File { size, body, .. } = item else {
            return Ok(());
        };
        let path = slashed(relative)?;
        ensure!(seen.insert(path.clone()), "the package has {path} twice");
        let mut head = Vec::new();
        body.take(SNIFF as u64).read_to_end(&mut head)?;
        files.push(FileRef {
            text: is_text(&head, size as usize <= SNIFF),
            path,
            size,
        });
        Ok(())
    })?;
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// The plain files under `root`, within the caps of a package. Links are not followed
/// and not listed: they are not the item's own files.
fn list_folder(root: &Path) -> Result<Vec<FileRef>> {
    let mut files = Vec::new();
    let mut total = 0u64;
    let mut count = 0usize;
    let mut folders = vec![PathBuf::new()];
    while let Some(relative) = folders.pop() {
        let here = root.join(&relative);
        for entry in
            std::fs::read_dir(&here).with_context(|| format!("cannot list {}", here.display()))?
        {
            let entry = entry?;
            let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
                continue;
            };
            let path = relative.join(&name);
            let metadata = std::fs::symlink_metadata(entry.path())?;
            count += 1;
            ensure!(count <= MAX_FILES, "it has over {MAX_FILES} files");
            if metadata.is_dir() {
                folders.push(path);
            } else if metadata.is_file() {
                total = total.saturating_add(metadata.len());
                ensure!(
                    total <= MAX_UNPACKED_BYTES,
                    "it holds over {MAX_UNPACKED_BYTES} bytes"
                );
                let mut head = Vec::new();
                open_no_follow(std::fs::OpenOptions::new().read(true), &entry.path())?
                    .take(SNIFF as u64)
                    .read_to_end(&mut head)?;
                files.push(FileRef {
                    text: is_text(&head, metadata.len() as usize <= SNIFF),
                    path: slashed(&path)?,
                    size: metadata.len(),
                });
            }
        }
    }
    files.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(files)
}

/// At most `PREVIEW_BYTES` and one more of `relative` under `root`, once its real path is
/// found inside the real root.
fn read_folder_file(root: &Path, relative: &str) -> Result<Vec<u8>> {
    let real_root = root.canonicalize()?;
    let path = root.join(relative);
    let real = path.canonicalize().context("the file is gone")?;
    ensure!(
        real.starts_with(&real_root),
        "{} is outside its folder",
        relative.escape_debug()
    );
    let file = open_no_follow(std::fs::OpenOptions::new().read(true), &real)?;
    ensure!(file.metadata()?.is_file(), "not a regular file");
    let mut bytes = Vec::new();
    file.take(PREVIEW_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    Ok(bytes)
}

/// At most `PREVIEW_BYTES` and one more of the package's file at `relative`.
pub(super) fn read_package_file(package: &[u8], relative: &str) -> Result<Vec<u8>> {
    let mut found = None;
    unpack::walk(package, |path, item| {
        if let Item::File { body, .. } = item
            && found.is_none()
            && slashed(path)? == relative
        {
            let mut bytes = Vec::new();
            body.take(PREVIEW_BYTES as u64 + 1)
                .read_to_end(&mut bytes)?;
            found = Some(bytes);
        }
        Ok(())
    })?;
    found.with_context(|| format!("the package has no {}", relative.escape_debug()))
}

/// A relative path with `/` between its names, which must be UTF-8.
fn slashed(path: &Path) -> Result<String> {
    let mut names = Vec::new();
    for part in path.components() {
        let std::path::Component::Normal(name) = part else {
            bail!("a path out of its folder");
        };
        names.push(name.to_str().context("a name that is not UTF-8")?);
    }
    let path = names.join("/");
    ensure!(
        !path.is_empty() && path.len() <= MAX_PATH,
        "an empty or overlong path"
    );
    Ok(path)
}

/// UTF-8 without a NUL byte; the start of a longer file may end inside a character.
fn is_text(head: &[u8], whole: bool) -> bool {
    if head.contains(&0) {
        return false;
    }
    match std::str::from_utf8(head) {
        Ok(_) => true,
        Err(error) => !whole && error.error_len().is_none(),
    }
}

/// The first `PREVIEW_BYTES` of `bytes` as text a client draws as it is: control and bidi
/// characters, which could hide text or turn it around, shown as escapes; line breaks
/// and tabs kept, carriage returns dropped.
pub fn preview(bytes: &[u8], truncated: bool) -> Result<FilePreview> {
    let head = &bytes[..bytes.len().min(PREVIEW_BYTES)];
    let text = match std::str::from_utf8(head) {
        Ok(text) => text,
        Err(error) if truncated && error.error_len().is_none() => {
            std::str::from_utf8(&head[..error.valid_up_to()]).context("not UTF-8 text")?
        }
        Err(_) => bail!("not UTF-8 text"),
    };
    let bidi = |c: char| matches!(c, '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{200e}' | '\u{200f}' | '\u{061c}');
    let mut shown = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\t' => shown.push(c),
            '\r' => {}
            c if c.is_control() || bidi(c) => shown.extend(c.escape_unicode()),
            c => shown.push(c),
        }
    }
    Ok(FilePreview {
        text: shown,
        truncated,
    })
}
