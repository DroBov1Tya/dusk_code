//! `duskcode remove`: what taking duskcode off this machine deletes, worked out and
//! checked before the client asks, then deleted in an order that never leaves a removed
//! program beside the data it was meant to take along. Thread files in the workspace
//! folder are the user's work and stay unless asked for.

use std::{
    env, fs, io,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};

use super::archive::RemovalGuard;
use crate::init::paths::Paths;

const BINARY: &str = "duskcode";

/// A removal under way: the lock that keeps other duskcode processes out, and what it
/// deletes. Dropped without `run`, it deletes nothing.
pub struct Uninstall {
    guard: RemovalGuard,
    install_dir: PathBuf,
    config_file: PathBuf,
    /// This program, resolved.
    pub exe: PathBuf,
    /// Links named duskcode that lead to it.
    pub links: Vec<PathBuf>,
    /// The application data folder, when there is one.
    pub data: Option<PathBuf>,
    /// Config, history, skills and agents.
    pub config: PathBuf,
    /// What the desktop client's installer left outside the data folder.
    pub extras: Vec<PathBuf>,
    pub workspace: PathBuf,
    /// Whether the chats' files in `workspace` go too.
    pub files: bool,
}

impl Uninstall {
    /// Takes the lock and works out what goes. `wait`: started by the desktop client,
    /// this process may begin while the window is still closing, so it waits a while.
    pub fn plan(paths: &Paths, files: bool, wait: bool) -> Result<Uninstall> {
        let guard = acquire(wait)?;
        let home = PathBuf::from(env::var_os("HOME").context("HOME is not set")?);
        let exe = env::current_exe().context("cannot find this binary")?;
        let exe = fs::canonicalize(&exe).unwrap_or(exe);
        let links = links_to(&exe, &home);
        let install_dir = crate::release::resources::data_root()?;
        guard.validate_application_data_root(&install_dir)?;
        let config = paths.config_root();
        ensure!(
            config.ends_with("duskcode"),
            "unexpected config folder {}",
            config.display()
        );
        let data = match fs::symlink_metadata(&install_dir) {
            Ok(meta) => {
                ensure!(meta.is_dir(), "install directory must not be a symlink");
                Some(install_dir.clone())
            }
            Err(_) => None,
        };
        let extras = desktop_leftovers(&home, &install_dir);
        let workspace = paths.workspace_root();
        if files {
            ensure!(
                workspace.ends_with("duskcode") && workspace.starts_with(&home),
                "unexpected workspace folder {}",
                workspace.display()
            );
        }
        Ok(Uninstall {
            guard,
            install_dir,
            config_file: paths.config_file(),
            exe,
            links,
            data,
            config: config.to_path_buf(),
            extras,
            workspace: workspace.to_path_buf(),
            files,
        })
    }

    /// The coordination lock, which stays.
    pub fn lock_path(&self) -> &Path {
        self.guard.lock_path()
    }

    /// Deletes everything listed. Keys the keychain would not give up do not stop the
    /// removal; `keychain_failed` hears of them so the user can delete them by hand.
    pub fn run(self, keychain_failed: impl FnOnce(&anyhow::Error)) -> Result<()> {
        // What the user wants gone goes first and the program last, and one failure does
        // not stop the rest: a run that could not finish leaves a program that can be
        // asked again.
        let mut failures: Vec<String> = Vec::new();
        let mut step = |result: Result<()>| {
            if let Err(error) = result {
                failures.push(format!("{error:#}"));
            }
        };
        // While the config still names the providers.
        if let Err(error) = crate::secrets::forget_all(&self.config_file) {
            keychain_failed(&error);
        }
        step(remove_path(&self.config));
        if self.files {
            step(remove_path(&self.workspace));
        }
        step(self.guard.remove_application_data(&self.install_dir));
        for extra in &self.extras {
            step(remove_path(extra));
        }
        for link in &self.links {
            step(
                fs::remove_file(link).with_context(|| format!("cannot remove {}", link.display())),
            );
        }
        if failures.is_empty() {
            return fs::remove_file(&self.exe)
                .with_context(|| format!("cannot remove {}", self.exe.display()));
        }
        // Started by the desktop client nobody reads this process's output, so what
        // failed is also kept beside the lock, which stays.
        let report = failures.join("\n");
        if let Some(folder) = self.lock_path().parent() {
            let _ = fs::write(folder.join("removal.log"), format!("{report}\n"));
        }
        anyhow::bail!("the removal did not finish; run `duskcode remove` again:\n{report}")
    }
}

fn acquire(wait: bool) -> Result<RemovalGuard> {
    let mut tries = 0;
    loop {
        match RemovalGuard::acquire() {
            Err(_) if wait && tries < 60 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            result => return result,
        }
    }
}

/// The app bundle on a Mac, the desktop entry and the icon elsewhere. Only what exists
/// is listed.
fn desktop_leftovers(home: &Path, install_dir: &Path) -> Vec<PathBuf> {
    let data_home = install_dir.parent().unwrap_or(home);
    let candidates = [
        home.join("Applications/Duskcode.app"),
        data_home.join("applications/duskcode.desktop"),
        data_home.join("icons/hicolor/512x512/apps/duskcode.png"),
    ];
    candidates
        .into_iter()
        .filter(|path| fs::symlink_metadata(path).is_ok())
        .collect()
}

/// A file, a link or a folder, links not followed.
fn remove_path(path: &Path) -> Result<()> {
    let Ok(meta) = fs::symlink_metadata(path) else {
        return Ok(());
    };
    if !meta.is_dir() {
        return fs::remove_file(path).with_context(|| format!("cannot remove {}", path.display()));
    }
    // A process that is still closing may write one more file into the folder while it is
    // emptied, which fails the removal of the folder itself: a few tries outlast that.
    let mut tries = 0;
    loop {
        match fs::remove_dir_all(path) {
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty && tries < 10 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(300));
            }
            result => {
                return result.with_context(|| format!("cannot remove {}", path.display()));
            }
        }
    }
}

/// Links named duskcode on PATH and in the usual places that lead to `exe`.
fn links_to(exe: &Path, home: &Path) -> Vec<PathBuf> {
    let mut folders = vec![PathBuf::from("/usr/local/bin"), home.join(".local/bin")];
    if let Some(path) = env::var_os("PATH") {
        folders.extend(env::split_paths(&path));
    }
    let mut links: Vec<PathBuf> = Vec::new();
    for link in folders.into_iter().map(|folder| folder.join(BINARY)) {
        let is_link = fs::symlink_metadata(&link).is_ok_and(|meta| meta.file_type().is_symlink());
        let target = fs::canonicalize(&link).ok();
        if is_link && target.as_deref() == Some(exe) && !links.contains(&link) {
            links.push(link);
        }
    }
    links
}
