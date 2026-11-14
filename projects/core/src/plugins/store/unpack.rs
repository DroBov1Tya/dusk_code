//! Unpacking a package into an empty folder. Its hash is checked before this, so a
//! package that gets here is the one the signed list names; the rules still hold, so a
//! list signed by mistake cannot write outside the folder either: only plain files and
//! folders, paths without `..`, roots or links, within a size and a count.

use std::{
    fs::{self, OpenOptions},
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::{Component, Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use flate2::read::GzDecoder;
use tar::{Archive, EntryType};

const MAX_UNPACKED_BYTES: u64 = 256 * 1024 * 1024;
const MAX_FILES: usize = 4000;
const MAX_PATH: usize = 300;

/// Unpacks the `.tar.gz` in `package` into `into`, which must not exist yet.
pub fn unpack(package: &[u8], into: &Path) -> Result<()> {
    fs::create_dir(into).with_context(|| format!("cannot create {}", into.display()))?;
    set_mode(into, 0o700)?;
    let mut archive = Archive::new(GzDecoder::new(package));
    let mut total = 0u64;
    let mut count = 0usize;
    for entry in archive.entries().context("the package is not a tar.gz")? {
        let mut entry = entry.context("the package is damaged")?;
        let kind = entry.header().entry_type();
        if matches!(
            kind,
            EntryType::XGlobalHeader | EntryType::XHeader | EntryType::GNULongName
        ) {
            continue;
        }
        let relative = relative(&entry.path().context("a path in the package")?)?;
        count += 1;
        ensure!(count <= MAX_FILES, "the package has over {MAX_FILES} files");
        let target = into.join(&relative);
        match kind {
            EntryType::Directory => {
                fs::create_dir_all(&target)?;
                set_mode(&target, 0o700)?;
            }
            EntryType::Regular | EntryType::Continuous => {
                let size = entry.header().size()?;
                total = total.saturating_add(size);
                ensure!(
                    total <= MAX_UNPACKED_BYTES,
                    "the package unpacks to over {MAX_UNPACKED_BYTES} bytes"
                );
                if let Some(parent) = target.parent() {
                    fs::create_dir_all(parent)?;
                }
                let executable = entry.header().mode().unwrap_or(0) & 0o111 != 0;
                let mut file = OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .mode(if executable { 0o700 } else { 0o600 })
                    .open(&target)
                    .with_context(|| format!("cannot write {}", relative.display()))?;
                let copied = io::copy(&mut (&mut entry).take(size), &mut file)?;
                ensure!(copied == size, "{} is cut short", relative.display());
            }
            other => bail!(
                "{} is a {other:?}; a package holds only files and folders",
                relative.display()
            ),
        }
    }
    Ok(())
}

/// The path as a plain relative one: only names, none of `..`, `/` at the start or a
/// drive; a leading `./` is dropped.
fn relative(path: &Path) -> Result<PathBuf> {
    let mut clean = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => clean.push(name),
            Component::CurDir => {}
            _ => bail!(
                "the package has a path out of its folder: {}",
                path.display()
            ),
        }
    }
    ensure!(
        !clean.as_os_str().is_empty() && clean.as_os_str().len() <= MAX_PATH,
        "the package has an empty or overlong path"
    );
    Ok(clean)
}

fn set_mode(path: &Path, mode: u32) -> Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
        .with_context(|| format!("cannot set the mode of {}", path.display()))
}
