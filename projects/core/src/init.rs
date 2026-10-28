//! First-start initialization. Creates missing user files with private permissions
//! and never modifies existing ones.

use std::{
    fs::{self, DirBuilder, OpenOptions},
    io::{ErrorKind, Write},
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, bail, ensure};

use crate::config::Config;

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
";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Initialization {
    pub config_path: PathBuf,
    /// True on first start: the config is a template without providers.
    pub config_created: bool,
}

pub fn initialize() -> Result<Initialization> {
    let config_path = Config::path()?;
    let config_created = ensure_config(&config_path)?;
    Ok(Initialization {
        config_path,
        config_created,
    })
}

/// Creates the config template if the file is missing. Returns whether it was created.
pub fn ensure_config(path: &Path) -> Result<bool> {
    let directory = path.parent().context("config directory is missing")?;
    create_private_dir(directory)?;

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
        Err(_) => bail!("cannot create config file"),
    };
    let written = file
        .write_all(CONFIG_TEMPLATE.as_bytes())
        .and_then(|()| file.sync_all());
    if written.is_err() {
        drop(file);
        let _ = fs::remove_file(path);
        bail!("cannot write config file");
    }
    Ok(true)
}

fn create_private_dir(directory: &Path) -> Result<()> {
    let mut builder = DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .map_err(|_| anyhow!("cannot create config directory"))?;
    let metadata =
        fs::symlink_metadata(directory).map_err(|_| anyhow!("cannot inspect config directory"))?;
    ensure!(metadata.is_dir(), "config directory must not be a symlink");
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    fn temp_config_path(name: &str) -> PathBuf {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir()
            .join(format!("dusk_init_{name}_{}_{nanos}", std::process::id()))
            .join("duskcode/config.yml")
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
