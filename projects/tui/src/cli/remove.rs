//! `duskcode remove`: takes duskcode off this machine after one question. Thread files in
//! the workspace folder are the user's work and stay, unless `--files` says otherwise. The
//! desktop client runs it as `remove --yes` after it has closed, having asked the user
//! itself.

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use dusk_core::app::{self, Paths};

use super::args::RemoveOptions;

const BINARY: &str = "duskcode";

pub fn run(paths: &Paths, options: &RemoveOptions) -> Result<()> {
    let removal = acquire(options.assume_yes)?;
    let home = PathBuf::from(env::var_os("HOME").context("HOME is not set")?);
    let exe = env::current_exe().context("cannot find this binary")?;
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    let links = links_to(&exe, &home);
    let install_dir = app::data_root()?;
    removal.validate_application_data_root(&install_dir)?;
    let config = paths.config_root();
    ensure!(
        config.ends_with("duskcode"),
        "unexpected config folder {}",
        config.display()
    );

    println!("This removes:");
    println!("  {}", exe.display());
    for link in &links {
        println!("  {} (link)", link.display());
    }
    if fs::symlink_metadata(&install_dir).is_ok() {
        ensure!(
            fs::symlink_metadata(&install_dir)?.is_dir(),
            "install directory must not be a symlink"
        );
        println!("  {} (application data)", install_dir.display());
        println!(
            "    {} (all versioned resources)",
            install_dir.join("resources").display()
        );
    }
    println!(
        "  {} (config, history, skills, agents) and the keys in the system keychain",
        config.display()
    );
    let extras = desktop_leftovers(&home, &install_dir);
    for extra in &extras {
        println!("  {} (desktop client)", extra.display());
    }
    let workspace = paths.workspace_root();
    if options.files {
        ensure!(
            workspace.ends_with("duskcode") && workspace.starts_with(&home),
            "unexpected workspace folder {}",
            workspace.display()
        );
        println!("  {} (the chats' files)", workspace.display());
    } else {
        println!("Thread files in {} are kept.", workspace.display());
    }
    println!(
        "The coordination lock {} is kept.",
        removal.lock_path().display()
    );
    if !options.assume_yes {
        print!("Remove duskcode? [y/N] ");
        io::stdout().flush()?;
        let mut answer = String::new();
        io::stdin().read_line(&mut answer)?;
        if !matches!(answer.trim(), "y" | "Y" | "yes") {
            println!("Nothing removed.");
            return Ok(());
        }
    }

    // First, while the config still names the providers; a locked keychain must not stop
    // the removal, so what it leaves is only reported.
    if let Err(error) = app::forget_secrets(&paths.config_file()) {
        eprintln!("duskcode: {error:#}; delete them in the keychain app");
    }
    for link in &links {
        fs::remove_file(link).with_context(|| format!("cannot remove {}", link.display()))?;
    }
    fs::remove_file(&exe).with_context(|| format!("cannot remove {}", exe.display()))?;
    removal.remove_application_data(&install_dir)?;
    for extra in &extras {
        remove_path(extra)?;
    }
    remove_path(config)?;
    if options.files {
        remove_path(workspace)?;
    }
    println!("duskcode is removed.");
    Ok(())
}

/// The lock that keeps other duskcode processes out. Started by the desktop client, this
/// process may begin while the window is still closing, so it waits for it a while.
fn acquire(wait: bool) -> Result<app::RemovalGuard> {
    let mut tries = 0;
    loop {
        match app::RemovalGuard::acquire() {
            Err(_) if wait && tries < 60 => {
                tries += 1;
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            result => return result,
        }
    }
}

/// What the desktop client's installer left outside the data folder: the app bundle on a
/// Mac, the desktop entry and the icon elsewhere. Only what exists is listed.
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
    match meta.is_dir() {
        true => fs::remove_dir_all(path),
        false => fs::remove_file(path),
    }
    .with_context(|| format!("cannot remove {}", path.display()))
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
