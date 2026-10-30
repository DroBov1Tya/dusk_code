//! The built-in tools.

use std::{
    fs::{File, OpenOptions},
    io::{Read, Write},
    path::Path,
};

use anyhow::{Context, Result, ensure};
use serde::de::DeserializeOwned;
use serde_json::Value;

use super::{
    console::RunCommand,
    files::{DeleteFile, EditFile, ReadFile, WriteFile},
    harness::LoadSkill,
    tool::Tool,
};

/// `skills` is the directory the skill graph is read from.
pub fn all(skills: &Path) -> Vec<Box<dyn Tool>> {
    vec![
        Box::new(ReadFile),
        Box::new(WriteFile),
        Box::new(EditFile),
        Box::new(DeleteFile),
        Box::new(RunCommand::default()),
        Box::new(LoadSkill::new(skills.to_owned())),
    ]
}

/// Strict on purpose: a misspelled field is reported to the model instead of ignored.
pub fn parse_arguments<T: DeserializeOwned>(arguments: Value) -> Result<T> {
    serde_json::from_value(arguments).context("invalid arguments")
}

/// Reads a regular file. The last path component is not followed if it is a symlink and a
/// FIFO does not block, so a file swapped after the scope check cannot redirect the read.
pub fn read_regular(path: &Path, max_bytes: u64) -> Result<Vec<u8>> {
    let file = open_no_follow(OpenOptions::new().read(true), path)?;
    let metadata = file.metadata().context("cannot read file")?;
    ensure!(metadata.is_file(), "not a regular file");
    ensure!(
        metadata.len() <= max_bytes,
        "file is larger than {max_bytes} bytes"
    );
    let mut bytes = Vec::new();
    // The file may grow after the size check.
    file.take(max_bytes + 1)
        .read_to_end(&mut bytes)
        .context("cannot read file")?;
    ensure!(
        bytes.len() as u64 <= max_bytes,
        "file is larger than {max_bytes} bytes"
    );
    Ok(bytes)
}

/// Creates or replaces a regular file, with the same symlink and FIFO guard as `read_regular`.
pub fn write_regular(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = open_no_follow(
        OpenOptions::new().write(true).create(true).truncate(true),
        path,
    )?;
    ensure!(
        file.metadata().context("cannot write file")?.is_file(),
        "not a regular file"
    );
    file.write_all(bytes).context("cannot write file")
}

/// Control characters in names or lines would reach the model and terminals as markup.
pub fn printable(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_control() && c != '\t' { '?' } else { c })
        .collect()
}

fn open_no_follow(options: &mut OpenOptions, path: &Path) -> Result<File> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.custom_flags(nix::libc::O_NOFOLLOW | nix::libc::O_NONBLOCK);
    }
    options.open(path).context("cannot open file")
}
