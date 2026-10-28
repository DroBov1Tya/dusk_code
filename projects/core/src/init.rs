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
# dusk_code provider settings. Keep this file private (mode 0600).
# Add at least one provider, for example:
#
# providers:
#   main:
#     kind: openai_compatible
#     base_url: 'https://api.example.com/v1'
#     api_key: 'sk-...'
providers: {}

session:
  # full:   store user messages and the whole model output (reasoning, tool calls, answer)
  # answer: store user messages and model answers only
  record: full
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initialization {
    /// True on first start: the config is a template without providers.
    pub config_created: bool,
}

pub fn initialize(paths: &Paths) -> Result<Initialization> {
    create_private_dir(paths.config_root())?;
    create_private_dir(&paths.session_dir())?;
    create_private_dir(paths.workspace_root())?;
    let config_created = ensure_config(&paths.config_file())?;
    // SQLite would create the file with umask permissions; an empty file is a valid new database.
    create_private_file(&paths.state_db(), b"")?;
    Ok(Initialization { config_created })
}

pub(crate) fn create_private_dir(directory: &Path) -> Result<()> {
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
    use crate::{config::Config, testing::temp_paths};

    fn temp_config_path(name: &str) -> std::path::PathBuf {
        temp_paths(name).config_file()
    }

    #[test]
    fn creates_private_template_once() {
        let path = temp_config_path("once");
        assert!(ensure_config(&path).unwrap());
        assert_eq!(fs::read_to_string(&path).unwrap(), CONFIG_TEMPLATE);

        fs::write(&path, "providers: {custom: {}}\n").unwrap();
        assert!(!ensure_config(&path).unwrap());
        assert_eq!(
            fs::read_to_string(&path).unwrap(),
            "providers: {custom: {}}\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn uses_private_permissions() {
        use std::os::unix::fs::PermissionsExt;

        let path = temp_config_path("modes");
        ensure_config(&path).unwrap();
        let mode = |p: &Path| fs::metadata(p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode(&path), 0o600);
        assert_eq!(mode(path.parent().unwrap()), 0o700);
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
        assert_eq!(mode(&paths.session_dir()), 0o700);
        assert_eq!(mode(&paths.config_file()), 0o600);
        assert_eq!(mode(&paths.state_db()), 0o600);
    }

    #[test]
    fn template_loads_but_requires_a_provider() {
        let path = temp_config_path("template");
        ensure_config(&path).unwrap();
        let error = Config::load_from(&path).unwrap_err();
        assert!(error.to_string().contains("at least one provider"));
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
