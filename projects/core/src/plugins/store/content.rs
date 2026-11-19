//! Skills, packs and agents from the store: plain files, put where the core reads its
//! own. A single skill goes to `skills/<skill>/`, a pack to `skills/<pack>/<skill>/` or
//! `agents/<pack>/<agent>/`, as plugins put theirs. They are written through the same
//! record of hashes as a plugin's skills, so an update or a removal never replaces or
//! deletes a file the user edited since.

use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, bail, ensure};
use dusk_protocol::plugin::Kind;

use super::{
    STAGING,
    index::Entry,
    pack::{self, Pack},
    unpack,
};
use crate::{
    init::paths::Paths,
    plugins::{catalog, install},
    release::resources,
};

/// Far above any skill or agent file.
const MAX_FILE_BYTES: u64 = 4 * 1024 * 1024;

/// What a package writes: the folder it fills under the rules folder, and its files by
/// their path there.
#[derive(Debug)]
pub struct Files {
    pub folder: String,
    pub wanted: BTreeMap<PathBuf, Vec<u8>>,
}

/// Unpacks the checked package of `entry` and writes its files. `owned` is the folder the
/// store put this name in before, if it did; any other folder already there is the
/// user's and refuses the install. Returns the folder it filled.
#[cfg(test)]
pub fn place(paths: &Paths, entry: &Entry, package: &[u8], owned: Option<&str>) -> Result<String> {
    place_package(
        paths,
        entry,
        &super::Package::Archive(package.to_vec()),
        owned,
        false,
    )
}

/// As `place`, for a package or for files fetched from upstream and checked. What an
/// `unverified` source brings must be files pinned upstream, text only.
pub fn place_package(
    paths: &Paths,
    entry: &Entry,
    package: &super::Package,
    owned: Option<&str>,
    unverified: bool,
) -> Result<String> {
    let files = files_of(paths, entry, package, unverified)?;
    let target = paths.rules_root().join(&files.folder);
    if let Ok(metadata) = fs::symlink_metadata(&target) {
        ensure!(
            metadata.is_dir() && owned == Some(files.folder.as_str()),
            "{} is already here and not from the store",
            files.folder
        );
    }
    install::write_wanted(paths, &record_file(paths, &entry.name), &files.wanted)?;
    Ok(files.folder)
}

/// What `package` would write for `entry`, checked against it, without writing anything
/// under the rules folder; it is laid out in the store's staging folder and removed.
/// Called under `BUSY`.
pub fn files_of(
    paths: &Paths,
    entry: &Entry,
    package: &super::Package,
    unverified: bool,
) -> Result<Files> {
    ensure!(
        entry.consent().is_empty(),
        "a skill, pack or agent asks for no permissions; the list says it does"
    );
    ensure!(
        !unverified || matches!(package, super::Package::Files(_)),
        "an unverified source brings only files pinned upstream"
    );
    crate::init::create_private_dir(&catalog::folder(paths))?;
    let staging = catalog::folder(paths).join(STAGING);
    // Under `BUSY`, so what is here was left by an install that stopped.
    if fs::symlink_metadata(&staging).is_ok() {
        fs::remove_dir_all(&staging)?;
    }
    crate::init::create_private_dir(&staging)?;
    let unpacked = staging.join(&entry.name);
    let result = (|| {
        match package {
            super::Package::Archive(bytes) => unpack::unpack(bytes, &unpacked)?,
            super::Package::Files(files) => write_files(entry, files, &unpacked)?,
        }
        let files = read(entry, &unpacked)?;
        if unverified {
            super::guard::check_files(&files.wanted)?;
        }
        Ok(files)
    })();
    let _ = fs::remove_dir_all(&staging);
    result
}

/// Files fetched from upstream, laid out as a package would hold them; a pack gets the
/// `pack.yml` its entry describes, since its author's repository has none.
fn write_files(entry: &Entry, files: &BTreeMap<String, Vec<u8>>, folder: &Path) -> Result<()> {
    crate::init::create_private_dir(folder)?;
    let mut all = files.clone();
    if entry.kind == Kind::Pack {
        let text = |value: &str| serde_json::Value::from(value).to_string();
        let items: Vec<serde_json::Value> = entry
            .skills
            .iter()
            .map(|item| item.as_str().into())
            .collect();
        let yaml = format!(
            "name: {}\ntitle: {}\ndescription: {}\nvendor: {}\nkind: skills\nitems: {}\nlicense: {}\n",
            text(&entry.name),
            text(&entry.name),
            text(&entry.description),
            text(entry.author.as_deref().unwrap_or("")),
            serde_json::Value::from(items),
            text(entry.license.as_deref().unwrap_or("")),
        );
        ensure!(
            all.insert(pack::FILE.to_owned(), yaml.into_bytes())
                .is_none(),
            "an upstream pack names its own pack.yml"
        );
    }
    for (to, bytes) in &all {
        // `to` was checked to be plain and relative, so it stays under `folder`.
        let path = folder.join(to);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::write(&path, bytes).with_context(|| format!("cannot write {to}"))?;
    }
    Ok(())
}

/// Removes what the store wrote for `name`; files the user edited stay.
pub fn remove(paths: &Paths, name: &str) -> Result<()> {
    install::write_wanted(paths, &record_file(paths, name), &BTreeMap::new())
}

/// The files of an unpacked package, checked against the listing the user saw.
pub fn read(entry: &Entry, unpacked: &Path) -> Result<Files> {
    let top = top_level(unpacked)?;
    if top.contains(pack::FILE) {
        ensure!(
            matches!(entry.kind, Kind::Pack | Kind::Agent),
            "a {:?} package holds a pack.yml",
            entry.kind
        );
        let bytes = crate::init::files::read_regular(&unpacked.join(pack::FILE), 64 * 1024)?;
        let pack = Pack::parse(&bytes)?;
        ensure!(
            pack.name == entry.name,
            "the pack is named {}, the list says {}",
            pack.name,
            entry.name
        );
        ensure!(
            entry.kind != Kind::Agent || pack.kind == pack::PackKind::Agents,
            "an agent package holds a pack of skills"
        );
        let items: BTreeSet<&str> = pack.items.iter().map(String::as_str).collect();
        let listed: BTreeSet<&str> = entry.skills.iter().map(String::as_str).collect();
        ensure!(
            listed.is_empty() || listed == items,
            "the pack holds other items than the list shows"
        );
        let mut expected: BTreeSet<&str> = items.clone();
        expected.insert(pack::FILE);
        ensure!(
            top.iter().map(String::as_str).collect::<BTreeSet<_>>() == expected,
            "the pack holds other files than its items and pack.yml"
        );
        let (part, file) = pack.kind.place();
        let folder = format!("{part}/{}", entry.name);
        let mut wanted = BTreeMap::new();
        wanted.insert(Path::new(&folder).join(pack::FILE), bytes);
        for item in &pack.items {
            collect(
                &unpacked.join(item),
                file,
                &Path::new(&folder).join(item),
                &mut wanted,
            )?;
        }
        return Ok(Files { folder, wanted });
    }
    let (part, file) = match entry.kind {
        Kind::Skill => ("skills", "SKILL.md"),
        Kind::Agent => ("agents", "AGENT.md"),
        Kind::Pack => bail!("a pack has no pack.yml"),
        Kind::Plugin => bail!("a plugin is not placed as files"),
    };
    ensure!(
        top.len() == 1 && top.contains(&entry.name),
        "the package holds other files than the folder {}",
        entry.name
    );
    let folder = format!("{part}/{}", entry.name);
    let mut wanted = BTreeMap::new();
    collect(
        &unpacked.join(&entry.name),
        file,
        Path::new(&folder),
        &mut wanted,
    )?;
    Ok(Files { folder, wanted })
}

/// The names at the top of the package.
fn top_level(unpacked: &Path) -> Result<BTreeSet<String>> {
    let mut names = BTreeSet::new();
    for item in fs::read_dir(unpacked).context("cannot list the package")? {
        let name = item?.file_name();
        names.insert(
            name.to_str()
                .context("a name in the package is not UTF-8")?
                .to_owned(),
        );
    }
    Ok(names)
}

/// The files of one item folder, which must hold its `file`, under `to`.
fn collect(
    from: &Path,
    file: &str,
    to: &Path,
    wanted: &mut BTreeMap<PathBuf, Vec<u8>>,
) -> Result<()> {
    ensure!(
        fs::symlink_metadata(from.join(file)).is_ok_and(|metadata| metadata.is_file()),
        "{} has no {file}",
        to.display()
    );
    for relative in resources::files(from)? {
        let bytes = crate::init::files::read_regular(&from.join(&relative), MAX_FILE_BYTES)?;
        wanted.insert(to.join(relative), bytes);
    }
    Ok(())
}

/// Beside the plugins' records; `.store-` never begins a plugin's, `.installed-`.
fn record_file(paths: &Paths, name: &str) -> PathBuf {
    catalog::folder(paths).join(format!(".store-{name}"))
}
