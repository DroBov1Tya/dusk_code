//! `duskcode remove`: takes duskcode off this machine after one question. Thread files in
//! the workspace folder are the user's work and stay.

use std::{
    env, fs,
    io::{self, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use dusk_core::paths::Paths;

const BINARY: &str = "duskcode";
/// Where the install script puts the binary, under HOME.
const INSTALL_DIR: &str = ".local/share/duskcode";

pub fn run(paths: &Paths) -> Result<()> {
    let home = PathBuf::from(env::var_os("HOME").context("HOME is not set")?);
    let exe = env::current_exe().context("cannot find this binary")?;
    let exe = fs::canonicalize(&exe).unwrap_or(exe);
    let links = links_to(&exe, &home);
    let install_dir = home.join(INSTALL_DIR);
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
    if install_dir.exists() && !exe.starts_with(&install_dir) {
        println!("  {}", install_dir.display());
    }
    println!(
        "  {} (config, API keys, history, skills, agents)",
        config.display()
    );
    println!(
        "Thread files in {} are kept.",
        paths.workspace_root().display()
    );
    print!("Remove duskcode? [y/N] ");
    io::stdout().flush()?;
    let mut answer = String::new();
    io::stdin().read_line(&mut answer)?;
    if !matches!(answer.trim(), "y" | "Y" | "yes") {
        println!("Nothing removed.");
        return Ok(());
    }

    for link in &links {
        fs::remove_file(link).with_context(|| format!("cannot remove {}", link.display()))?;
    }
    fs::remove_file(&exe).with_context(|| format!("cannot remove {}", exe.display()))?;
    if install_dir.exists() {
        fs::remove_dir_all(&install_dir)
            .with_context(|| format!("cannot remove {}", install_dir.display()))?;
    }
    if fs::symlink_metadata(config).is_ok() {
        fs::remove_dir_all(config)
            .with_context(|| format!("cannot remove {}", config.display()))?;
    }
    println!("duskcode is removed.");
    Ok(())
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
