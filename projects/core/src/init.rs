//! First-start initialization. Creates the missing parts of the layout in `paths`
//! with private permissions and never modifies existing files.

use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{ErrorKind, Write},
    path::Path,
};

use anyhow::{Context, Result, anyhow, bail, ensure};

use crate::paths::Paths;

const CONFIG_TEMPLATE: &str = "\
# dusk_code provider settings. Keep this file private.
# Add a provider when you are ready to connect, for example:
#
# providers:
#   main:
#     kind: openai_compatible
#     base_url: 'https://api.example.com/v1'
#     api_key: 'sk-...' # optional for local providers
providers: {}

thread:
  # full:   store user messages and the whole model output (reasoning, tool calls, answer)
  # answer: store user messages and model answers only
  record: full

limits:
  # A request larger than this is not sent; the context is compacted at 80% of it,
  # or of the model window when that is smaller.
  max_input_tokens: 100000
  # The model is stopped after writing this many tokens in one call.
  max_output_tokens: 100000

# Tools and skills are on unless switched off here or with :off in a client, for example:
# tools:
#   run_command: false
# skills:
#   git/release-notes: false
#
# The core rewrites this file when a setting changes at runtime; only its own header
# comment is kept then.
";

const MAIN_PROMPT_TEMPLATE: &str = "\
You are the main agent of dusk_code, working with the user in a chat.
Answer directly and concisely, in the language the user writes in.
";

/// Also used when COMPACT.md is missing or blank.
pub const COMPACT_PROMPT_TEMPLATE: &str = "\
Summarize the conversation above so that the work can go on from the summary alone:
the earlier messages are about to leave your context. Keep facts, decisions, exact
names, paths, commands, errors and numbers; drop small talk. Write in the language of
the conversation. Do not call tools. This request is not part of the conversation:
do not list it as a task.

Use exactly these sections:

1. Requested tasks: everything the user asked for, in order.
2. Completed tasks: what is done and how it was checked.
3. Open tasks: what is not done yet, with known problems.
4. Current task: what was in progress at the end, and the next step.
5. Paths and files: every file, directory, URL and command that matters, one line each
   on why.
6. Summary: everything else worth keeping, in a few sentences.
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initialization {
    /// True on first start: the config is a template without providers.
    pub config_created: bool,
}

pub fn initialize(paths: &Paths) -> Result<Initialization> {
    create_private_dir(paths.config_root())?;
    create_private_dir(&paths.records_dir())?;
    create_private_dir(paths.workspace_root())?;
    create_private_dir(&paths.skills_dir())?;
    create_private_dir(&paths.agents_dir())?;
    let config_created = ensure_config(&paths.config_file())?;
    create_private_file(&paths.main_prompt(), MAIN_PROMPT_TEMPLATE.as_bytes())?;
    create_private_file(&paths.compact_prompt(), COMPACT_PROMPT_TEMPLATE.as_bytes())?;
    // SQLite would create the file with umask permissions; an empty file is a valid new database.
    create_private_file(&paths.state_db(), b"")?;
    Ok(Initialization { config_created })
}

pub fn create_private_dir(directory: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .map_err(|_| anyhow!("cannot create application directory"))?;
    let metadata = fs::symlink_metadata(directory)
        .map_err(|_| anyhow!("cannot inspect application directory"))?;
    ensure!(
        metadata.is_dir(),
        "application directory must not be a symlink"
    );
    Ok(())
}

/// Creates the config template if the file is missing. Returns whether it was created.
fn ensure_config(path: &Path) -> Result<bool> {
    let directory = path.parent().context("config directory is missing")?;
    create_private_dir(directory)?;
    create_private_file(path, CONFIG_TEMPLATE.as_bytes())
}

/// Creates a mode 0600 file with `contents` if the path is free. Returns whether it was created.
fn create_private_file(path: &Path, contents: &[u8]) -> Result<bool> {
    let mut options = OpenOptions::new();
    // create_new never follows or replaces an existing path, including symlinks.
    options.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = match options.open(path) {
        Ok(file) => file,
        Err(error) if error.kind() == ErrorKind::AlreadyExists => return Ok(false),
        Err(_) => bail!("cannot create application file"),
    };
    let written = file.write_all(contents).and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = fs::remove_file(path);
        bail!("cannot write application file");
    }
    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_paths;

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        temp_paths(name).config_file()
    }

    #[cfg(unix)]
    #[test]
    fn initialize_creates_private_layout() {
        use std::os::unix::fs::PermissionsExt;

        let paths = temp_paths("layout");
        assert!(initialize(&paths).unwrap().config_created);
        assert!(!initialize(&paths).unwrap().config_created);
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(paths.config_root()), 0o700);
        assert_eq!(mode(paths.workspace_root()), 0o700);
        assert_eq!(mode(&paths.records_dir()), 0o700);
        assert_eq!(mode(&paths.config_file()), 0o600);
        assert_eq!(mode(&paths.main_prompt()), 0o600);
        assert_eq!(mode(&paths.compact_prompt()), 0o600);
        assert_eq!(mode(&paths.skills_dir()), 0o700);
        assert_eq!(mode(&paths.agents_dir()), 0o700);
        assert_eq!(mode(&paths.state_db()), 0o600);
    }

    #[cfg(unix)]
    #[test]
    fn does_not_follow_symlink() {
        let path = temp_config_path("symlink");
        let directory = path.parent().unwrap();
        fs::create_dir_all(directory).unwrap();
        let target = directory.join("target.yml");
        std::os::unix::fs::symlink(&target, &path).unwrap();
        assert!(!ensure_config(&path).unwrap());
        assert!(!target.exists());
    }
}
