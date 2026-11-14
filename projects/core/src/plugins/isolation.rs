//! How every program of a plugin runs: its MCP servers, hooks, platform and interface.
//! It gets a bare environment, so no key of the user's set in the core's environment
//! reaches it, and it runs confined by the system:
//!
//! - it writes only in its data folder and the temporary folders;
//! - the config folder, where the keys, the vault's copy and other plugins are, is
//!   closed, but for its own folder (read-only) and its data folder;
//! - the keychain is closed;
//! - it reaches no network but its broker, and that only when it may connect anywhere;
//! - it starts only the system's programs, its own and the ones it names.
//!
//! When the system cannot confine it, it does not start.

use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::plugin::Permissions;
use tokio::process::Command;

use super::broker::Broker;
use crate::{
    init::paths::{Paths, expand_home},
    platform::{self, Confinement, Network},
};

/// What is kept of the core's environment, besides `LC_*`.
const KEPT_ENV: [&str; 3] = ["HOME", "LANG", "TMPDIR"];

#[derive(Clone)]
pub struct Isolation {
    folder: PathBuf,
    /// Its variables, its settings among them: never logged.
    env: Vec<(String, String)>,
    confinement: Confinement,
}

impl fmt::Debug for Isolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Isolation")
            .field("folder", &self.folder)
            .field("confinement", &self.confinement)
            .finish_non_exhaustive()
    }
}

impl Isolation {
    /// The confinement of the plugin in `folder`, keeping its state in `data`. `env` is
    /// what it is given besides the bare environment; `broker` is its way out, when it
    /// may connect anywhere.
    pub fn new(
        paths: &Paths,
        folder: &Path,
        data: &Path,
        permissions: &Permissions,
        mut env: Vec<(String, String)>,
        broker: Option<&Broker>,
    ) -> Result<Self> {
        ensure!(
            platform::sandbox_available(),
            "this system cannot confine a plugin's programs, so they do not start"
        );
        let config_root = paths
            .config_root()
            .canonicalize()
            .context("cannot resolve the config folder")?;
        let folder = folder
            .canonicalize()
            .context("cannot resolve the plugin's folder")?;
        let data = data
            .canonicalize()
            .context("cannot resolve the plugin's data folder")?;
        let mut reopened = vec![folder.clone(), data.clone()];
        for wanted in &permissions.read {
            // A folder that is not there is not read.
            let Ok(path) = expand_home(wanted).canonicalize() else {
                continue;
            };
            ensure!(
                !path.starts_with(&config_root) && !config_root.starts_with(&path),
                "a plugin may not read duskcode's config folder: {wanted}"
            );
            reopened.push(path);
        }
        let network = match (permissions.network.is_empty(), broker) {
            (true, _) => Network::Closed,
            (false, Some(broker)) => Network::Broker(broker.port()),
            (false, None) => anyhow::bail!("the plugin may connect, but has no broker"),
        };
        #[cfg(target_os = "linux")]
        ensure!(
            network == Network::Closed,
            "Linux cannot keep a program to the core's broker, so a plugin that connects \
             does not start here"
        );
        let mut programs = vec![folder.clone()];
        for name in &permissions.exec {
            programs.extend(locate(name));
        }
        let mut writable = vec![data];
        writable.extend(platform::temp_folders());
        if let Some(broker) = broker {
            env.extend(broker.env());
        }
        Ok(Self {
            folder,
            env,
            confinement: Confinement {
                writable,
                hidden: vec![config_root],
                reopened,
                network,
                keychain: false,
                programs: Some(programs),
            },
        })
    }

    /// `line`, its program first, confined in the plugin's folder with a bare
    /// environment. Its own program may always start.
    pub fn command(&self, line: &[String]) -> Result<Command> {
        let (program, arguments) = line.split_first().context("no command")?;
        let found = locate(program);
        let mut confinement = self.confinement.clone();
        if let Some(programs) = &mut confinement.programs {
            programs.extend(found.iter().cloned());
        }
        let program = found.first().cloned().unwrap_or_else(|| program.into());
        let mut command = platform::confined_program(&program, arguments, &confinement)?;
        command.env_clear();
        for (key, value) in bare_env() {
            command.env(key, value);
        }
        command
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .current_dir(&self.folder);
        Ok(command)
    }

    /// Tells one confinement from another, to restart what runs under a changed one.
    /// Kept in memory only: it holds the settings.
    pub fn fingerprint(&self) -> String {
        format!("{:?} {:?}", self.confinement, self.env)
    }
}

/// The variables a program needs to run, and nothing that could be a key.
fn bare_env() -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(key, _)| {
            key.to_str()
                .is_some_and(|key| KEPT_ENV.contains(&key) || key.starts_with("LC_"))
        })
        .collect();
    let path = platform::login_path::get()
        .cloned()
        .or_else(|| std::env::var_os("PATH"));
    if let Some(path) = path {
        env.push(("PATH".into(), path));
    }
    if !env.iter().any(|(key, _)| key == "TMPDIR") {
        env.push(("TMPDIR".into(), std::env::temp_dir().into()));
    }
    env
}

/// The file `program` names, as written and resolved: a path, or a name looked up in
/// the PATH the programs get.
fn locate(program: &str) -> Vec<PathBuf> {
    let candidate = if program.contains('/') {
        Some(PathBuf::from(program))
    } else {
        let path = platform::login_path::get()
            .cloned()
            .or_else(|| std::env::var_os("PATH"))
            .unwrap_or_default();
        std::env::split_paths(&path)
            .map(|folder| folder.join(program))
            .find(|file| file.is_file())
    };
    let Some(file) = candidate else {
        return Vec::new();
    };
    let mut found = vec![file.clone()];
    if let Ok(resolved) = file.canonicalize()
        && resolved != file
    {
        found.push(resolved);
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A key in the core's environment never reaches a plugin's program.
    #[tokio::test]
    async fn a_program_sees_only_the_bare_environment() {
        // SAFETY: tests that read this variable do not run in this process at once.
        unsafe { std::env::set_var("OPENAI_API_KEY", "sk-planted") };
        let paths = crate::testing::temp_paths("isolation-env");
        let folder = paths.rules_root().join("plugins/p");
        let data = paths.config_root().join("plugin-data/p");
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::create_dir_all(&data).unwrap();
        let env = vec![("DUSKCODE_PLUGIN_DIR".to_owned(), "x".to_owned())];
        let isolation =
            Isolation::new(&paths, &folder, &data, &Permissions::default(), env, None).unwrap();
        let output = isolation
            .command(&["/usr/bin/env".to_owned()])
            .unwrap()
            .output()
            .await
            .unwrap();
        assert!(output.status.success());
        let printed = String::from_utf8(output.stdout).unwrap();
        assert!(!printed.contains("OPENAI_API_KEY"), "{printed}");
        assert!(!printed.contains("sk-planted"));
        assert!(printed.contains("DUSKCODE_PLUGIN_DIR=x"));
        for line in printed.lines() {
            let key = line.split('=').next().unwrap();
            assert!(
                ["PATH", "HOME", "LANG", "TMPDIR", "DUSKCODE_PLUGIN_DIR"].contains(&key)
                    || key.starts_with("LC_"),
                "{key} leaked"
            );
        }
    }
}
