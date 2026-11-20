//! How every program of a plugin runs: its MCP servers, hooks, platform and interface.
//! It gets a bare environment, so no key of the user's set in the core's environment
//! reaches it, and it runs confined by the system:
//!
//! - it writes only in its data folder and the temporary folders;
//! - the config folder, where the keys, the vault's copy and other plugins are, is
//!   closed, but for its own folder (read-only) and its data folder;
//! - the keychain is closed;
//! - a program that serves a contract gets, for one call, the folder of that call's
//!   project (read-only) with home and every other saved project hidden: see
//!   [`Isolation::for_call`];
//! - it reaches no network but its broker, and that only when it may connect anywhere;
//!   on Linux only a program that serves a contract may, in a network namespace of its
//!   own whose only socket is the broker's (Landlock cannot keep a program to one
//!   address); where this Linux cannot make one, the plugin does not start;
//! - it starts only the system's programs, its own and the ones it names; `xcrun` brings
//!   the developer tools of Xcode it starts;
//! - it connects to the Unix sockets only in the folders it names.
//!
//! When the system cannot confine it, it does not start.

use std::{
    ffi::OsString,
    fmt,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use dusk_protocol::plugin::{Permissions, ProjectAccess};
use tokio::process::Command;

use super::broker::Broker;
#[cfg(target_os = "linux")]
use super::broker::Gate;
use crate::{
    init::paths::{Paths, expand_home},
    platform::{self, Confinement, Network},
};

/// What is kept of the core's environment, besides `LC_*`.
const KEPT_ENV: [&str; 3] = ["HOME", "LANG", "TMPDIR"];

#[derive(Clone)]
pub struct Isolation {
    folder: PathBuf,
    config_root: PathBuf,
    /// Its data folder, which a call keeps open as its own.
    data: PathBuf,
    /// The project of the call this confinement is for; the program runs in it.
    cwd: Option<PathBuf>,
    project: ProjectAccess,
    /// Its variables, its settings among them: never logged.
    env: Vec<(String, String)>,
    confinement: Confinement,
    /// The broker as served in each program's own network namespace.
    #[cfg(target_os = "linux")]
    gate: Option<Gate>,
}

impl fmt::Debug for Isolation {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("Isolation")
            .field("folder", &self.folder)
            .field("cwd", &self.cwd)
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
        env: Vec<(String, String)>,
        broker: Option<&Broker>,
    ) -> Result<Self> {
        Self::build(paths, folder, data, permissions, env, broker, false)
    }

    /// The same for a plugin that serves a contract. On Linux its programs reach the
    /// broker from a network namespace of their own; where this Linux cannot make one,
    /// it fails, and the plugin does not start.
    pub fn for_contract(
        paths: &Paths,
        folder: &Path,
        data: &Path,
        permissions: &Permissions,
        env: Vec<(String, String)>,
        broker: Option<&Broker>,
    ) -> Result<Self> {
        Self::build(paths, folder, data, permissions, env, broker, true)
    }

    fn build(
        paths: &Paths,
        folder: &Path,
        data: &Path,
        permissions: &Permissions,
        mut env: Vec<(String, String)>,
        broker: Option<&Broker>,
        contract: bool,
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
        let mut sockets = Vec::new();
        for wanted in &permissions.sockets {
            let Ok(path) = expand_home(wanted).canonicalize() else {
                continue;
            };
            ensure!(
                !path.starts_with(&config_root) && !config_root.starts_with(&path),
                "a plugin may not reach sockets in duskcode's config folder: {wanted}"
            );
            sockets.push(path);
        }
        let network = match (permissions.network.is_empty(), broker) {
            (true, _) => Network::Closed,
            (false, Some(broker)) => Network::Broker(broker.port()),
            (false, None) => anyhow::bail!("the plugin may connect, but has no broker"),
        };
        // A call promises a read-only project and a hidden home: a Landlock that cannot
        // close truncation cannot keep that.
        #[cfg(target_os = "linux")]
        if contract {
            platform::every_file_right().map_err(|error| {
                anyhow::anyhow!("{error}, so a plugin that serves a contract cannot start here")
            })?;
        }
        // Landlock cannot keep a program to one address: only a namespace can, and only
        // a contract's programs get one.
        #[cfg(target_os = "linux")]
        let (network, gate) = match (network, broker) {
            (Network::Closed, _) => (Network::Closed, None),
            (_, Some(broker)) if contract => {
                if let Some(missing) = platform::netns::missing() {
                    anyhow::bail!(
                        "this Linux does not let a program have a network of its own ({}), \
                         so a plugin that serves a contract cannot start here",
                        missing.reason()
                    );
                }
                (Network::Namespace, Some(broker.gate()))
            }
            _ => anyhow::bail!(
                "Linux cannot keep a program to the core's broker, so a plugin that connects \
                 does not start here"
            ),
        };
        #[cfg(not(target_os = "linux"))]
        let _ = contract;
        let mut programs = vec![folder.clone()];
        for name in &permissions.exec {
            programs.extend(locate(name));
            if name == "xcrun" {
                programs.extend(platform::developer_tools());
            }
        }
        let mut writable = vec![data.clone()];
        writable.extend(platform::temp_folders());
        if let Some(broker) = broker {
            match network {
                #[cfg(target_os = "linux")]
                Network::Namespace => env.extend(broker.env_at(platform::netns::PROXY_PORT)),
                _ => env.extend(broker.env()),
            }
        }
        Ok(Self {
            folder,
            config_root: config_root.clone(),
            data: data.clone(),
            cwd: None,
            project: permissions.project,
            env,
            confinement: Confinement {
                writable,
                hidden: vec![config_root],
                reopened,
                network,
                keychain: false,
                programs: Some(programs),
                sockets,
                hand_off: true,
                stat: Vec::new(),
                listed: Vec::new(),
                every_file_right: false,
            },
            #[cfg(target_os = "linux")]
            gate,
        })
    }

    /// `line`, its program first, confined in the plugin's folder with a bare
    /// environment. Its own program may always start.
    ///
    /// On Linux a contract plugin's program gets its broker only if it is spawned
    /// within five seconds: build the command once the call holds its slot, spawn it at
    /// once and drop it, which closes the core's copy of the program's end of the pair,
    /// so a program that dies before it hands its socket over is seen at once
    /// (`contract::call` does so).
    pub fn command(&self, line: &[String]) -> Result<Command> {
        let (program, arguments) = line.split_first().context("no command")?;
        let found = locate(program);
        let mut confinement = self.confinement.clone();
        if let Some(programs) = &mut confinement.programs {
            programs.extend(found.iter().cloned());
        }
        let program = found.first().cloned().unwrap_or_else(|| program.into());
        #[cfg(target_os = "linux")]
        let mut command = match &self.gate {
            Some(gate) if confinement.network == Network::Namespace => {
                let (command, receiver) =
                    platform::namespaced_program(&program, arguments, &confinement)?;
                serve_inside(gate.clone(), receiver)?;
                command
            }
            _ => platform::confined_program(&program, arguments, &confinement)?,
        };
        #[cfg(not(target_os = "linux"))]
        let mut command = platform::confined_program(&program, arguments, &confinement)?;
        command.env_clear();
        for (key, value) in bare_env() {
            command.env(key, value);
        }
        command
            .envs(self.env.iter().map(|(key, value)| (key, value)))
            .current_dir(self.cwd.as_ref().unwrap_or(&self.folder));
        Ok(command)
    }

    /// The confinement of one call of a contract plugin about the project in `project`:
    /// home and every folder of `others` (the other saved projects) hidden, `project`
    /// reopened read-only and the program run in it, git's own config readable, and the
    /// keychain closed. Hiding home alone would leave
    /// projects outside it open, so each other project is hidden by name.
    ///
    /// Build it anew for every call: the folders of temporary chats, taken when it is
    /// built, are hidden too, and the program cannot hand work
    /// to launchd, LaunchServices or Apple events (macOS), which run it unconfined.
    ///
    /// It asks git, outside any sandbox, where the project's git dir is, and waits for
    /// it: async callers run it through `spawn_blocking`. A git dir outside the project
    /// is reopened read-only only in three cases; otherwise git fails in the call:
    ///
    /// - the project is a sub-folder of a repository whose `.git` folder is a real
    ///   folder, whose top is not home nor holds home or the config folder, and holds
    ///   no other saved project or temporary chat (its objects show every file of the
    ///   repository); the names in the top folder are listed too, as git needs them
    ///   (macOS), and with them the names of entries git does not track or ignores;
    /// - the project is a submodule whose git dir's `core.worktree` names it;
    /// - the project is a worktree whose link checks out both ways. Its repository's
    ///   common dir is then reopened even inside another saved project B. That shows
    ///   more than the shared history: B's index (the paths, blob ids and file times
    ///   of B's working tree), its logs and reflogs, `COMMIT_EDITMSG`, hooks, config
    ///   (remote URLs among it), stashes and the metadata of B's other worktrees. It
    ///   is accepted because the worktree is a checkout of that same repository: its
    ///   objects, refs and branches are the project's own already, and what B adds is
    ///   metadata about B's checkout, not its working files, which stay hidden.
    ///
    /// Left open, by nature or design: a project under the temporary folders stays
    /// writable there, as they are for every plugin (projects are not kept there); a
    /// hard link inside the project to a file elsewhere is read as the project's own;
    /// the plugin's data folder is the same for every call, so one call can leave a
    /// file the next reads.
    pub fn for_call(&self, home: &Path, project: &Path, others: &[PathBuf]) -> Result<Isolation> {
        // The second lock after the manifest: a plugin that did not ask for a project
        // folder never gets one, and a call's confinement is never widened again.
        ensure!(
            self.project == ProjectAccess::Read,
            "this plugin may not read a project's folder"
        );
        ensure!(self.cwd.is_none(), "this confinement is already one call's");
        let home = home_folder(home)?;
        let project = project
            .canonicalize()
            .context("cannot resolve the project's folder")?;
        ensure!(
            !home.starts_with(&project)
                && !self.config_root.starts_with(&project)
                && !project.starts_with(&self.config_root),
            "this project's folder holds duskcode's config folder or your home folder"
        );
        let mut call = self.clone();
        let confinement = &mut call.confinement;
        let mut hidden = vec![home.clone()];
        // A folder that is not there cannot be read.
        hidden.extend(
            others
                .iter()
                .filter_map(|other| other.canonicalize().ok())
                .chain(crate::runtime::temporary_folders())
                .filter(|other| *other != project),
        );
        // Reopening this project would open one saved inside it again: neither system
        // closes a folder inside an open one.
        ensure!(
            !hidden[1..].iter().any(|other| other.starts_with(&project)),
            "another saved project lies inside this project's folder"
        );
        // A grant reopened after them would open another project again.
        let overlaps = |path: &Path| {
            hidden[1..]
                .iter()
                .any(|other| path.starts_with(other) || other.starts_with(path))
        };
        for granted in &confinement.reopened {
            let own = *granted == self.folder || *granted == self.data;
            ensure!(
                own || !overlaps(granted),
                "this plugin's read permission reaches another project's folder: {}",
                granted.display()
            );
        }
        // A link such as `~/.config/git -> ~/.config` must not reopen the config
        // folder, home or another project.
        let safe_from_core = |path: &Path| {
            !path.starts_with(&self.config_root)
                && !self.config_root.starts_with(path)
                && !home.starts_with(path)
        };
        let safe = |path: &Path| safe_from_core(path) && !overlaps(path);
        let mut reopened = vec![project.clone()];
        // A worktree's, a submodule's or a sub-folder's git dir lies outside the
        // project. It is reopened only when it holds nothing but this project's
        // repository; otherwise it stays closed, and git fails.
        let mut gits = Vec::new();
        if let Some(repository) = repository(&project) {
            let linked = worktree_common_dir(&project, &repository).filter(|common| {
                safe_from_core(common) && !hidden[1..].iter().any(|other| other.starts_with(common))
            });
            if let Some(common) = linked {
                // Its link holds both ways, so it may lie in another saved project.
                gits.extend(
                    [repository.git_dir, repository.common]
                        .into_iter()
                        .filter(|git| git.starts_with(&common) && safe_from_core(git)),
                );
            } else {
                // An enclosing repository is reopened only when it holds no other
                // saved project or temporary chat and is not home's (a dotfiles repo
                // would show all of home through its objects).
                let toplevel = &repository.toplevel;
                let own = toplevel.join(".git");
                let enclosing = repository.git_dir == own
                    && repository.common == own
                    && std::fs::symlink_metadata(&own).is_ok_and(|meta| meta.is_dir())
                    && project.starts_with(toplevel)
                    && !home.starts_with(toplevel)
                    && safe_from_core(toplevel)
                    && !hidden[1..].iter().any(|other| other.starts_with(toplevel));
                // A submodule's git dir names the project as its work tree.
                let submodule = repository.git_dir == repository.common
                    && *toplevel == project
                    && core_worktree(&repository.git_dir).as_ref() == Some(&project);
                if (enclosing || submodule) && safe(&repository.git_dir) {
                    if enclosing && *toplevel != project {
                        // Git walks up from the project, then moves to the top and
                        // asks its own path, which needs the top's list of entries.
                        confinement.stat.extend(
                            project
                                .ancestors()
                                .skip(1)
                                .take_while(|folder| folder != toplevel)
                                .map(Path::to_path_buf),
                        );
                        confinement.listed.push(toplevel.clone());
                    }
                    gits.push(repository.git_dir);
                }
            }
        }
        for git in gits {
            if !git.starts_with(&project) && !project.starts_with(&git) && !reopened.contains(&git)
            {
                // Git resolves the path one folder at a time.
                confinement
                    .stat
                    .extend(git.ancestors().skip(1).map(Path::to_path_buf));
                reopened.push(git);
            }
        }
        for git in [".gitconfig", ".config/git"] {
            if let Ok(path) = home.join(git).canonicalize()
                && safe(&path)
            {
                reopened.push(path);
            }
        }
        // The keychain files go with home, and the services stay closed: a call never
        // signs in through the keychain.
        confinement.hidden.extend(hidden);
        confinement.reopened.extend(reopened);
        confinement.keychain = false;
        confinement.hand_off = false;
        confinement.every_file_right = true;
        call.cwd = Some(project);
        Ok(call)
    }

    /// Tells one confinement from another, to restart what runs under a changed one.
    /// Kept in memory only: it holds the settings.
    pub fn fingerprint(&self) -> String {
        format!("{:?} {:?}", self.confinement, self.env)
    }
}

/// Serves the broker on the socket the program about to start makes in its namespace,
/// until its process ends. A program never spawned leaves nothing behind: its socket
/// pair closes with the command.
#[cfg(target_os = "linux")]
fn serve_inside(gate: Gate, receiver: platform::netns::Receiver) -> Result<()> {
    let runtime = tokio::runtime::Handle::try_current()
        .context("a program in a network of its own starts inside the runtime")?;
    runtime.spawn(async move {
        let inside = match tokio::task::spawn_blocking(move || receiver.listener()).await {
            Ok(Ok(inside)) => inside,
            Ok(Err(error)) => {
                tracing::debug!("plugin program in a namespace: {error:#}");
                return;
            }
            Err(_) => return,
        };
        let serving = match gate.serve(inside.listener) {
            Ok(serving) => serving,
            Err(error) => {
                tracing::debug!("cannot serve a plugin program's broker: {error:#}");
                return;
            }
        };
        // A pidfd turns readable when its process has ended.
        use tokio::io::{Interest, unix::AsyncFd};
        // SAFETY: an OwnedFd keeps its one descriptor open until dropped.
        match unsafe { AsyncFd::register_with_interest(inside.process, Interest::READABLE) } {
            Ok(ended) => drop(ended.readable().await),
            Err(error) => tracing::debug!("cannot watch a plugin program: {error}"),
        }
        serving.abort();
    });
    Ok(())
}

/// Where git keeps the repository a project is in, resolved.
struct Repository {
    git_dir: PathBuf,
    common: PathBuf,
    /// The top of its work tree.
    toplevel: PathBuf,
}

/// The repository `project` is in, read by the core outside any sandbox. None when it
/// is no repository, has no work tree or git fails.
fn repository(project: &Path) -> Option<Repository> {
    let printed = read_git(
        project,
        &[
            "rev-parse",
            "--absolute-git-dir",
            "--git-common-dir",
            "--show-toplevel",
        ],
    )?;
    // The common dir may be printed relative to the project.
    let resolved: Vec<PathBuf> = printed
        .lines()
        .map(|line| project.join(line).canonicalize().ok())
        .collect::<Option<_>>()?;
    let [git_dir, common, toplevel] = <[PathBuf; 3]>::try_from(resolved).ok()?;
    Some(Repository {
        git_dir,
        common,
        toplevel,
    })
}

/// The work tree the config of the git dir `git_dir` names, resolved from it.
fn core_worktree(git_dir: &Path) -> Option<PathBuf> {
    let config = git_dir.join("config");
    let config = config.to_str()?;
    // Read from `/`, so git does not take that git dir for the repository it is in.
    let args = ["config", "--file", config, "--get", "core.worktree"];
    let printed = read_git(Path::new("/"), &args)?;
    git_dir.join(printed.lines().next()?).canonicalize().ok()
}

/// What `git -C <dir> <args>` prints, run with no shell, no prompt, no system config,
/// no fsmonitor hook and git's other variables removed, within git's usual time for a
/// read. None when git fails.
fn read_git(dir: &Path, args: &[&str]) -> Option<String> {
    use std::{io::Read, process::Stdio, time::Instant};
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(10);
    let mut command = std::process::Command::new("git");
    if let Some(path) = platform::login_path::get() {
        command.env("PATH", path);
    }
    // The user's GIT_CEILING_DIRECTORIES is kept: git stops where the user said.
    for key in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_CONFIG",
        "GIT_CONFIG_COUNT",
        "GIT_CONFIG_PARAMETERS",
    ] {
        command.env_remove(key);
    }
    command
        .args(["-c", "core.fsmonitor=false", "-C"])
        .arg(dir)
        .args(args)
        .env("GIT_CONFIG_NOSYSTEM", "1")
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let mut child = command.spawn().ok()?;
    let deadline = Instant::now() + LIMIT;
    let finished = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status.success(),
            Ok(None) if Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(5));
            }
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                break false;
            }
        }
    };
    let mut printed = String::new();
    let read = child
        .stdout
        .take()
        .map(|stdout| stdout.take(64 * 1024).read_to_string(&mut printed));
    (finished && matches!(read, Some(Ok(_)))).then_some(printed)
}

/// The common git dir of the worktree in `project`, when git's link holds both ways:
/// the project's `.git` file names `<common>/worktrees/<name>`, and that folder's
/// `gitdir` names the project's `.git` and its `commondir` names `<common>`.
fn worktree_common_dir(project: &Path, repository: &Repository) -> Option<PathBuf> {
    let Repository {
        git_dir, common, ..
    } = repository;
    let dot_git = project.join(".git");
    if !std::fs::symlink_metadata(&dot_git).ok()?.is_file() {
        return None;
    }
    let named = |file: &Path, prefix: &str, base: &Path| -> Option<PathBuf> {
        if std::fs::metadata(file).ok()?.len() > 4096 {
            return None;
        }
        let text = std::fs::read_to_string(file).ok()?;
        let line = text.lines().next()?;
        let path = line.strip_prefix(prefix)?.trim();
        base.join(path).canonicalize().ok()
    };
    let linked = named(&dot_git, "gitdir:", project)?;
    let back = named(&linked.join("gitdir"), "", &linked)?;
    let shared = named(&linked.join("commondir"), "", &linked)?;
    (linked == *git_dir
        && linked.parent() == Some(common.join("worktrees").as_path())
        && back == dot_git.canonicalize().ok()?
        && shared == *common)
        .then(|| common.clone())
}

/// The variables a program needs to run, and nothing that could be a key.
fn bare_env() -> Vec<(OsString, OsString)> {
    let mut env: Vec<(OsString, OsString)> = std::env::vars_os()
        .filter(|(key, _)| {
            key.to_str()
                .is_some_and(|key| KEPT_ENV.contains(&key) || key.starts_with("LC_"))
        })
        .collect();
    if let Some(path) = platform::login_path::effective() {
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
        let path = platform::login_path::effective().unwrap_or_default();
        std::env::split_paths(&path)
            .map(|folder| folder.join(program))
            .find(|file| file.is_file())
    };
    let extras: Vec<PathBuf> = match program {
        #[cfg(target_os = "macos")]
        "axe" => super::ui::phone::AXE_PATHS.map(PathBuf::from).to_vec(),
        #[cfg(target_os = "macos")]
        "vphone-launchpad-cli" => {
            use super::ui::phone::{APP_NAME, CLI_SUBPATH, user_app};
            let system_app = PathBuf::from("/Applications").join(APP_NAME);
            user_app()
                .into_iter()
                .chain([system_app])
                .map(|app| app.join(CLI_SUBPATH))
                .collect()
        }
        _ => Vec::new(),
    };
    let mut found = Vec::new();
    for file in candidate
        .into_iter()
        .chain(extras.into_iter().filter(|file| file.is_file()))
    {
        if found.contains(&file) {
            continue;
        }
        let resolved = file.canonicalize();
        found.push(file);
        if let Ok(resolved) = resolved
            && !found.contains(&resolved)
        {
            found.push(resolved);
        }
    }
    found
}

/// The home folder a call hides, resolved; refused when hiding it would hide the whole
/// system (`/`) or nothing that is there (empty, missing, not a folder).
fn home_folder(home: &Path) -> Result<PathBuf> {
    ensure!(
        !home.as_os_str().is_empty(),
        "HOME is empty, so a plugin's call cannot be kept out of the home folder"
    );
    let resolved = home
        .canonicalize()
        .with_context(|| format!("cannot resolve the home folder {}", home.display()))?;
    ensure!(
        resolved.is_dir(),
        "HOME ({}) is not a folder, so a plugin's call cannot be kept out of it",
        home.display()
    );
    ensure!(
        resolved.parent().is_some(),
        "HOME is /: hiding it from a plugin's call would hide the whole system"
    );
    Ok(resolved)
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

    /// A project, another project outside home, gh's config and a second plugin, all
    /// under a root kept out of the temporary folders: those stay writable for every
    /// plugin, so a folder in them could not be closed.
    struct Call {
        root: PathBuf,
        paths: Paths,
        home: PathBuf,
        a: PathBuf,
        b: PathBuf,
        gh: PathBuf,
        isolation: Isolation,
    }

    impl Drop for Call {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.root);
        }
    }

    fn call_setup(name: &str, permissions: Permissions, broker: Option<&Broker>) -> Option<Call> {
        if !platform::sandbox_available() {
            eprintln!("no sandbox on this system; skipped");
            return None;
        }
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../target/isolation-tests")
            .join(format!("{name}_{}_{id}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let paths = Paths::new(root.join("config"), root.join("workspace"));
        let home = root.join("home");
        let (a, b, gh) = (home.join("a"), root.join("b"), home.join(".config/gh"));
        let folder = paths.rules_root().join("plugins/p");
        let other_plugin = paths.rules_root().join("plugins/q");
        let data = paths.config_root().join("plugin-data/p");
        for dir in [&a, &b, &gh, &folder, &other_plugin, &data] {
            std::fs::create_dir_all(dir).unwrap();
        }
        std::fs::create_dir_all(home.join("Library/Keychains")).unwrap();
        std::fs::write(a.join("x"), "a").unwrap();
        std::fs::write(b.join("x"), "b").unwrap();
        std::fs::write(gh.join("hosts.yml"), "gh").unwrap();
        std::fs::write(home.join(".gitconfig"), "[user]").unwrap();
        std::fs::write(home.join("private"), "home").unwrap();
        std::fs::write(other_plugin.join("plugin.yml"), "q").unwrap();
        std::fs::write(paths.config_root().join("config.yml"), "key: secret").unwrap();
        std::os::unix::fs::symlink(b.join("x"), a.join("link")).unwrap();
        let root = root.canonicalize().unwrap();
        let (home, a, b, gh) = (
            home.canonicalize().unwrap(),
            a.canonicalize().unwrap(),
            b.canonicalize().unwrap(),
            gh.canonicalize().unwrap(),
        );
        let permissions = Permissions {
            read: vec![gh.display().to_string()],
            ..permissions
        };
        let isolation =
            Isolation::for_contract(&paths, &folder, &data, &permissions, Vec::new(), broker)
                .unwrap();
        Some(Call {
            root,
            paths,
            home,
            a,
            b,
            gh,
            isolation,
        })
    }

    fn reads_project() -> Permissions {
        Permissions {
            project: ProjectAccess::Read,
            ..Permissions::default()
        }
    }

    async fn run(isolation: &Isolation, line: &[&str]) -> std::process::Output {
        let line: Vec<String> = line.iter().map(|word| (*word).to_owned()).collect();
        isolation.command(&line).unwrap().output().await.unwrap()
    }

    /// Spec 8, test 2: a call reads its project and gh's config, and nothing of another
    /// project, the config folder, another plugin or the rest of home.
    #[tokio::test]
    async fn a_call_reads_only_its_project() {
        let Some(setup) = call_setup("call-reads", reads_project(), None) else {
            return;
        };
        let Call {
            paths,
            home,
            a,
            b,
            gh,
            ..
        } = &setup;
        let call = setup
            .isolation
            .for_call(home, a, std::slice::from_ref(b))
            .unwrap();
        let ok = async |path: &Path| {
            run(&call, &["/bin/cat", &path.display().to_string()])
                .await
                .status
                .success()
        };
        for open in [a.join("x"), gh.join("hosts.yml"), home.join(".gitconfig")] {
            assert!(ok(&open).await, "{open:?} was not readable");
        }
        let closed = [
            b.join("x"),
            a.join("link"),
            paths.rules_root().join("plugins/q/plugin.yml"),
            paths.config_root().join("config.yml"),
            home.join("private"),
        ];
        for closed in closed {
            assert!(!ok(&closed).await, "{closed:?} was readable");
        }
        // It runs in the project, which it cannot change.
        let wrote = run(&call, &["/bin/sh", "-c", "pwd; touch new"]).await;
        let printed = String::from_utf8_lossy(&wrote.stdout);
        assert!(printed.starts_with(&a.display().to_string()), "{printed}");
        assert!(!a.join("new").exists());
        assert!(
            Isolation::new(
                paths,
                &paths.rules_root().join("plugins/q"),
                &paths.config_root().join("plugin-data/p"),
                &Permissions::default(),
                Vec::new(),
                None,
            )
            .unwrap()
            .for_call(home, a, &[])
            .is_err(),
            "a plugin without `project: read` gets no project"
        );
    }

    #[test]
    fn a_call_for_home_or_the_config_folder_is_refused() {
        let Some(setup) = call_setup("call-refused", reads_project(), None) else {
            return;
        };
        let Call {
            root,
            paths,
            home,
            a,
            b,
            isolation,
            ..
        } = &setup;
        assert!(isolation.for_call(home, home, &[]).is_err());
        assert!(
            isolation.for_call(home, root, &[]).is_err(),
            "contains home"
        );
        // A home of / would hide everything; an empty, missing or file home hides
        // nothing that is there: each is refused, saying why.
        let file = root.join("a-file");
        std::fs::write(&file, "").unwrap();
        for (bad, why) in [
            (Path::new("/"), "HOME is /"),
            (Path::new(""), "HOME is empty"),
            (&*root.join("missing"), "cannot resolve the home folder"),
            (&*file, "is not a folder"),
        ] {
            let error = isolation.for_call(bad, a, &[]).unwrap_err();
            assert!(format!("{error:#}").contains(why), "{error:#}");
        }
        assert!(isolation.for_call(home, paths.config_root(), &[]).is_err());
        assert!(
            isolation.for_call(home, &paths.rules_root(), &[]).is_err(),
            "inside it"
        );
        // Another project inside this one could not be closed.
        assert!(
            isolation
                .for_call(home, root, std::slice::from_ref(a))
                .is_err()
        );
        // Nor can a read grant reopen another project, nor a call be widened.
        let reads_b = Permissions {
            read: vec![b.display().to_string()],
            ..reads_project()
        };
        let folder = paths.rules_root().join("plugins/p");
        let data = paths.config_root().join("plugin-data/p");
        let wide = Isolation::new(paths, &folder, &data, &reads_b, Vec::new(), None).unwrap();
        assert!(wide.for_call(home, a, std::slice::from_ref(b)).is_err());
        let call = isolation.for_call(home, a, &[]).unwrap();
        assert!(call.for_call(home, b, &[]).is_err());
    }

    /// A call never opens the keychain, whatever its permissions say: Seatbelt closes
    /// `~/Library/Keychains` with home and the keychain services stay closed.
    #[tokio::test]
    async fn a_call_never_opens_the_keychain() {
        let permissions = Permissions {
            keychain: true,
            ..reads_project()
        };
        let Some(setup) = call_setup("call-keychain", permissions, None) else {
            return;
        };
        let call = setup
            .isolation
            .for_call(&setup.home, &setup.a, &[])
            .unwrap();
        let keychains = setup.home.join("Library/Keychains");
        let shown = format!("{call:?}");
        assert!(shown.contains("keychain: false"), "{shown}");
        assert!(!shown.contains(&format!("{keychains:?}")), "{shown}");
        let listed = run(&call, &["/bin/ls", &keychains.display().to_string()]).await;
        assert!(!listed.status.success());
    }

    /// Spec 8, test 4.
    #[tokio::test]
    async fn a_call_sees_no_secret_of_the_core() {
        // SAFETY: as in a_program_sees_only_the_bare_environment.
        unsafe {
            for key in ["GH_TOKEN", "GITHUB_TOKEN", "ANTHROPIC_API_KEY"] {
                std::env::set_var(key, "planted");
            }
        }
        let Some(setup) = call_setup("call-env", reads_project(), None) else {
            return;
        };
        let call = setup
            .isolation
            .for_call(&setup.home, &setup.a, &[])
            .unwrap();
        let output = run(&call, &["/usr/bin/env"]).await;
        assert!(output.status.success());
        let printed = String::from_utf8(output.stdout).unwrap();
        for line in printed.lines() {
            assert!(
                !line.contains("planted") && !line.starts_with("GH_"),
                "{line}"
            );
        }
    }

    /// Spec 8, test 3 (macOS; Linux has its namespace): only the broker answers.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_call_reaches_no_address_but_its_broker() {
        use super::super::broker::{Origin, Policy, new_token};
        let origins = vec![Origin::parse("https://api.github.com").unwrap()];
        let broker = Broker::start(Policy::new(new_token().unwrap(), origins, Vec::new())).unwrap();
        let permissions = Permissions {
            network: vec!["https://api.github.com".into()],
            ..reads_project()
        };
        let Some(setup) = call_setup("call-network", permissions, Some(&broker)) else {
            return;
        };
        let call = setup
            .isolation
            .for_call(&setup.home, &setup.a, &[])
            .unwrap();
        let reach = async |host: &str, port: &str| {
            run(&call, &["/usr/bin/nc", "-z", "-w", "3", host, port])
                .await
                .status
                .success()
        };
        assert!(!reach("1.1.1.1", "443").await);
        assert!(reach("127.0.0.1", &broker.port().to_string()).await);
    }

    /// Spec 8, test 3 on Linux: a call runs in a network of its own, where only its
    /// broker answers; where this Linux cannot make one, the plugin does not start.
    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn a_linux_call_reaches_only_its_broker() {
        use super::super::broker::{Origin, Policy, new_token};
        // Listed, but on the machine's loopback: not the namespace's.
        let server = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = server.local_addr().unwrap().port();
        tokio::spawn(async move { while server.accept().await.is_ok() {} });
        let origin = format!("https://127.0.0.1:{port}");
        let mut policy = Policy::new(
            new_token().unwrap(),
            vec![Origin::parse(&origin).unwrap()],
            Vec::new(),
        );
        policy.public_only = false;
        let broker = Broker::start(policy).unwrap();
        let permissions = Permissions {
            network: vec![origin],
            ..reads_project()
        };
        if !platform::netns::available() {
            let paths = crate::testing::temp_paths("isolation-no-netns");
            let folder = paths.rules_root().join("plugins/p");
            let data = paths.config_root().join("plugin-data/p");
            std::fs::create_dir_all(&folder).unwrap();
            std::fs::create_dir_all(&data).unwrap();
            let refused = Isolation::for_contract(
                &paths,
                &folder,
                &data,
                &permissions,
                Vec::new(),
                Some(&broker),
            )
            .unwrap_err();
            assert!(
                format!("{refused:#}")
                    .starts_with("this Linux does not let a program have a network of its own ("),
                "{refused:#}"
            );
            eprintln!("no user namespaces here; refused: {refused:#}");
            return;
        }
        let Some(setup) = call_setup("call-netns", permissions, Some(&broker)) else {
            return;
        };
        let call = setup
            .isolation
            .for_call(&setup.home, &setup.a, &[])
            .unwrap();
        let bash = async |script: String| {
            let output = run(&call, &["bash", "-c", &script]).await;
            let printed = String::from_utf8_lossy(&output.stdout).into_owned();
            (output.status.success(), printed)
        };
        assert!(!bash("exec 3<>/dev/tcp/1.1.1.1/443".into()).await.0);
        assert!(!bash(format!("exec 3<>/dev/tcp/127.0.0.1/{port}")).await.0);
        let connect = |target: &str| {
            format!(
                "case $HTTPS_PROXY in *@127.0.0.1:3128) ;; *) exit 9;; esac; \
                 auth=$(printf 'dusk:%s' \"$DUSKCODE_PLUGIN_PROXY_TOKEN\" | base64 -w0) && \
                 exec 3<>/dev/tcp/127.0.0.1/3128 && \
                 printf 'CONNECT {target} HTTP/1.1\\r\\nProxy-Authorization: Basic %s\\r\\n\\r\\n' \
                 \"$auth\" >&3 && read -r line <&3 && echo \"$line\""
            )
        };
        let (ok, answer) = bash(connect(&format!("127.0.0.1:{port}"))).await;
        assert!(ok && answer.starts_with("HTTP/1.1 200"), "{answer}");
        let (_, answer) = bash(connect("other.example:443")).await;
        assert!(answer.starts_with("HTTP/1.1 403"), "{answer}");
        // No Unix socket on disk (the session bus, an agent), no pair, no vsock, though
        // the core's own listener is reachable unconfined.
        static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
        let id = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let socket = PathBuf::from(format!("/tmp/dusk-call-{}-{id}.sock", std::process::id()));
        let _ = std::fs::remove_file(&socket);
        let listener = std::os::unix::net::UnixListener::bind(&socket).unwrap();
        let unix = "socket(my $s, AF_UNIX, SOCK_STREAM, 0) or exit 3; \
                    connect($s, pack_sockaddr_un($ARGV[0])) or exit 4";
        let perl = |script: &str| {
            [
                "perl",
                "-MSocket",
                "-e",
                script,
                socket.display().to_string().as_str(),
            ]
            .map(str::to_owned)
            .to_vec()
        };
        let line = perl(unix);
        let unconfined = std::process::Command::new(&line[0])
            .args(&line[1..])
            .status()
            .unwrap();
        assert!(unconfined.success(), "{unconfined}");
        let confined = async |line: Vec<String>| {
            let line: Vec<&str> = line.iter().map(String::as_str).collect();
            run(&call, &line).await.status.code()
        };
        assert_eq!(confined(perl(unix)).await, Some(3));
        let pair = "socketpair(my $a, my $b, AF_UNIX, SOCK_STREAM, 0) or exit 3";
        assert_eq!(confined(perl(pair)).await, Some(3));
        // vsock proves something only where the kernel has it.
        let vsock = "socket(my $s, 40, SOCK_STREAM, 0) or exit 3";
        let line = perl(vsock);
        let unconfined = std::process::Command::new(&line[0])
            .args(&line[1..])
            .status()
            .unwrap();
        if unconfined.success() {
            assert_eq!(confined(perl(vsock)).await, Some(3));
        } else {
            eprintln!("no vsock in this kernel; its refusal not checked");
        }
        drop(listener);
        let _ = std::fs::remove_file(&socket);
        // A socket the core holds without close-on-exec does not reach the program.
        let (inherited, other) = std::os::unix::net::UnixStream::pair().unwrap();
        let fd = std::os::fd::AsRawFd::as_raw_fd(&inherited);
        // SAFETY: clears the flag of a descriptor this test owns.
        unsafe { nix::libc::fcntl(fd, nix::libc::F_SETFD, 0) };
        let write = format!("echo leaked >&{fd}");
        let unconfined = std::process::Command::new("bash")
            .args(["-c", &write])
            .status()
            .unwrap();
        assert!(unconfined.success(), "{unconfined}");
        assert!(!run(&call, &["bash", "-c", &write]).await.status.success());
        let mut seen = [0u8; 16];
        let read = std::io::Read::read(&mut &other, &mut seen).unwrap();
        assert_eq!(&seen[..read], b"leaked\n");
    }

    /// A temporary chat's folder sits in the temporary folders every plugin may use; a
    /// call still cannot read it. Linux grants the temporary folders whole, so only
    /// macOS can close it.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_call_cannot_read_a_temporary_chat() {
        let Some(setup) = call_setup("call-temporary", reads_project(), None) else {
            return;
        };
        let chat = std::env::temp_dir().join(format!(
            "duskcode-temp-isolation-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&chat).unwrap();
        std::fs::write(chat.join("note"), "temporary").unwrap();
        let call = setup.isolation.for_call(&setup.home, &setup.a, &[]);
        let read = run(
            &call.unwrap(),
            &["/bin/cat", &chat.join("note").display().to_string()],
        )
        .await;
        // A read grant of the temporary folder would reopen it: refused.
        let reads_temp = Permissions {
            read: vec![std::env::temp_dir().display().to_string()],
            ..reads_project()
        };
        let folder = setup.paths.rules_root().join("plugins/p");
        let data = setup.paths.config_root().join("plugin-data/p");
        let wide = Isolation::new(&setup.paths, &folder, &data, &reads_temp, Vec::new(), None);
        let refused = wide.unwrap().for_call(&setup.home, &setup.a, &[]).is_err();
        let _ = std::fs::remove_dir_all(&chat);
        assert!(!read.status.success(), "a temporary chat was readable");
        assert!(refused, "a read grant reopened the temporary chats");
    }

    /// `sandbox-exec` could not start the program: the sandbox refused it.
    #[cfg(target_os = "macos")]
    fn refused_at_exec(output: std::process::Output, line: &[&str]) {
        let said = String::from_utf8_lossy(&output.stderr);
        assert_eq!(output.status.code(), Some(71), "{line:?}: {said}");
        assert!(said.contains("Operation not permitted"), "{line:?}: {said}");
    }

    // On macOS /usr/bin/git is a launcher for the selected developer tools.
    // Resolve Git before sandboxing so the tests exercise Git directly.
    fn fixture_git() -> String {
        #[cfg(target_os = "macos")]
        {
            let output = std::process::Command::new("/usr/bin/xcrun")
                .args(["--find", "git"])
                .output()
                .unwrap();
            assert!(output.status.success(), "{output:?}");
            String::from_utf8(output.stdout).unwrap().trim().to_owned()
        }
        #[cfg(not(target_os = "macos"))]
        "/usr/bin/git".to_owned()
    }

    /// launchd, LaunchServices and Apple events run what they are handed outside the
    /// sandbox, so a call may not reach them; git and a gh-like program still run.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_call_cannot_hand_work_to_the_system() {
        let git = fixture_git();
        let permissions = Permissions {
            exec: vec![git.clone()],
            ..reads_project()
        };
        let Some(setup) = call_setup("call-hand-off", permissions, None) else {
            return;
        };
        let call = setup
            .isolation
            .for_call(&setup.home, &setup.a, &[])
            .unwrap();
        // Refused at exec, before launchd or LaunchServices see anything.
        let submit = [
            "/bin/launchctl",
            "submit",
            "-l",
            "dusk.test.never",
            "--",
            "/usr/bin/true",
        ];
        for line in [&submit[..], &["/usr/bin/open", "-g", "-n", "/usr/bin/true"]] {
            refused_at_exec(run(&call, line).await, line);
        }
        let script = "tell application \"Finder\" to get name";
        let line = ["/usr/bin/osascript", "-e", script];
        refused_at_exec(run(&call, &line).await, &line);
        // A gh stand-in in the plugin's folder that runs git in the project.
        let folder = setup.paths.rules_root().join("plugins/p");
        let gh = folder.join("gh");
        std::fs::write(&gh, "#!/bin/sh\n\"$1\" --version && pwd\n").unwrap();
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let output = run(&call, &[&gh.display().to_string(), &git]).await;
        let printed = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success(), "{output:?}");
        assert!(printed.starts_with("git version"), "{printed}");
    }

    /// A worktree's git dir lies in its main repository: a call reads it, and nothing
    /// else of that repository.
    #[tokio::test]
    async fn a_call_reads_its_worktrees_git_dir() {
        let Some(setup) = call_setup("call-worktree", reads_project(), None) else {
            return;
        };
        let main = setup.home.join("main");
        let worktree = setup.home.join("wt");
        std::fs::create_dir_all(&main).unwrap();
        std::fs::write(main.join("secret"), "main").unwrap();
        git(&main, &["init", "-q"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let target = worktree.display().to_string();
        git(&main, &["worktree", "add", "-q", "--detach", &target]);
        let worktree = worktree.canonicalize().unwrap();
        let call = setup
            .isolation
            .for_call(&setup.home, &worktree, &[])
            .unwrap();
        let git = fixture_git();
        let head = [git.as_str(), "-C", &target, "rev-parse", "HEAD"];
        let output = run(&call, &head).await;
        assert!(output.status.success(), "{output:?}");
        let secret = main.join("secret").display().to_string();
        assert!(!run(&call, &["/bin/cat", &secret]).await.status.success());
        // Saved as a project of its own, the main repository gives its git dir, as the
        // worktree's link holds both ways, and nothing else.
        let others = std::slice::from_ref(&main);
        let call = setup
            .isolation
            .for_call(&setup.home, &worktree, others)
            .unwrap();
        let output = run(&call, &head).await;
        assert!(output.status.success(), "{output:?}");
        assert!(!run(&call, &["/bin/cat", &secret]).await.status.success());
        // A forged `.git` naming that worktree's git dir has no link back: closed.
        let forged = setup.home.join("forged");
        std::fs::create_dir_all(&forged).unwrap();
        let git_dir = main.join(".git/worktrees/wt").canonicalize().unwrap();
        std::fs::write(
            forged.join(".git"),
            format!("gitdir: {}\n", git_dir.display()),
        )
        .unwrap();
        let call = setup
            .isolation
            .for_call(&setup.home, &forged, others)
            .unwrap();
        let forged_head = [
            git.as_str(),
            "-C",
            &forged.display().to_string(),
            "rev-parse",
            "HEAD",
        ]
        .map(str::to_owned);
        let forged_head: Vec<&str> = forged_head.iter().map(String::as_str).collect();
        assert!(!run(&call, &forged_head).await.status.success());
        let main_head = main.join(".git/HEAD").display().to_string();
        assert!(!run(&call, &["/bin/cat", &main_head]).await.status.success());
    }

    /// `git` in `dir`, with a name for commits; it must succeed.
    fn git(dir: &Path, args: &[&str]) {
        let status = std::process::Command::new(fixture_git())
            .arg("-C")
            .arg(dir)
            .args(["-c", "user.name=t", "-c", "user.email=t@t"])
            .args(["-c", "protocol.file.allow=always"])
            .args(args)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .unwrap();
        assert!(status.success(), "git {args:?}");
    }

    /// A repository's objects show all its files: an enclosing repository's git dir is
    /// reopened only when it holds no other saved project and is not home's, and a
    /// git dir named by a planted `.git` file never is.
    #[tokio::test]
    async fn a_call_reopens_an_enclosing_git_dir_only_alone() {
        let Some(setup) = call_setup("call-enclosing", reads_project(), None) else {
            return;
        };
        let home = &setup.home;
        let mono = home.join("mono");
        for (folder, text) in [("a", "mono a"), ("b", "mono b")] {
            std::fs::create_dir_all(mono.join(folder)).unwrap();
            std::fs::write(mono.join(folder).join("x"), text).unwrap();
        }
        git(&mono, &["init", "-q"]);
        git(&mono, &["add", "-A"]);
        git(&mono, &["commit", "-q", "-m", "x"]);
        let (mono, a, b) = (
            mono.canonicalize().unwrap(),
            mono.join("a").canonicalize().unwrap(),
            mono.join("b").canonicalize().unwrap(),
        );
        let git_program = fixture_git();
        let in_a = |args: &[&str]| -> Vec<String> {
            [git_program.as_str(), "-C", &a.display().to_string()]
                .iter()
                .chain(args)
                .map(|word| (*word).to_owned())
                .collect()
        };
        let succeeds = async |call: &Isolation, line: Vec<String>| {
            // Keep Git discovery inside the fixture, which can live in a checkout.
            call.command(&line)
                .unwrap()
                .env("GIT_CEILING_DIRECTORIES", &setup.root)
                .output()
                .await
                .unwrap()
                .status
                .success()
        };
        let mono_git = format!("{:?}", mono.join(".git"));
        let mono_head = mono.join(".git/HEAD").display().to_string();
        // With B saved too, A's call cannot read B through git's objects.
        let call = setup
            .isolation
            .for_call(home, &a, std::slice::from_ref(&b))
            .unwrap();
        assert!(!format!("{call:?}").contains(&mono_git), "{call:?}");
        assert!(call.confinement.listed.is_empty(), "{call:?}");
        assert!(!succeeds(&call, in_a(&["show", "HEAD:b/x"])).await);
        // Require Git to read the commit object, even if it can resolve a ref.
        let verify_head = ["rev-parse", "--verify", "HEAD^{commit}"];
        assert!(!succeeds(&call, in_a(&verify_head)).await);
        assert!(!run(&call, &["/bin/cat", &mono_head]).await.status.success());
        // Alone in its repository, A gets git.
        let call = setup.isolation.for_call(home, &a, &[]).unwrap();
        assert!(format!("{call:?}").contains(&mono_git), "{call:?}");
        assert!(succeeds(&call, in_a(&["show", "HEAD:a/x"])).await);
        assert!(succeeds(&call, in_a(&verify_head)).await);
        let b_file = b.join("x").display().to_string();
        assert!(!run(&call, &["/bin/cat", &b_file]).await.status.success());
        // The top's names are listed, not the folders under it (Landlock lists none).
        let (mono_list, b_list) = (mono.display().to_string(), b.display().to_string());
        #[cfg(target_os = "macos")]
        assert!(run(&call, &["/bin/ls", &mono_list]).await.status.success());
        #[cfg(not(target_os = "macos"))]
        let _ = mono_list;
        assert!(!run(&call, &["/bin/ls", &b_list]).await.status.success());
        // A planted `.git` file naming that repository is no link: closed.
        let planted = home.join("planted");
        std::fs::create_dir_all(&planted).unwrap();
        let named = format!("gitdir: {}\n", mono.join(".git").display());
        std::fs::write(planted.join(".git"), named).unwrap();
        let call = setup.isolation.for_call(home, &planted, &[]).unwrap();
        assert!(!format!("{call:?}").contains(&mono_git), "{call:?}");
        assert!(call.confinement.listed.is_empty(), "{call:?}");
        assert!(!run(&call, &["/bin/cat", &mono_head]).await.status.success());
        // A submodule whose git dir names it as its work tree gets that git dir only.
        let library = setup.root.join("library");
        std::fs::create_dir_all(&library).unwrap();
        git(&library, &["init", "-q"]);
        git(&library, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let library = library.display().to_string();
        git(&mono, &["submodule", "add", "-q", &library, "sub"]);
        let sub = mono.join("sub").canonicalize().unwrap();
        let call = setup.isolation.for_call(home, &sub, &[]).unwrap();
        let sub_head = [
            git_program.as_str(),
            "-C",
            &sub.display().to_string(),
            "rev-parse",
            "HEAD",
        ]
        .map(str::to_owned)
        .to_vec();
        assert!(succeeds(&call, sub_head).await, "{call:?}");
        assert!(!run(&call, &["/bin/cat", &mono_head]).await.status.success());
        // A dotfiles repository at home is never reopened, though A is alone in it.
        git(home, &["init", "-q"]);
        let lone = home.join("lone");
        std::fs::create_dir_all(&lone).unwrap();
        let call = setup.isolation.for_call(home, &lone, &[]).unwrap();
        let home_git = format!("{:?}", home.join(".git"));
        assert!(!format!("{call:?}").contains(&home_git), "{call:?}");
        assert!(call.confinement.listed.is_empty(), "{call:?}");
        let home_head = home.join(".git/HEAD").display().to_string();
        assert!(!run(&call, &["/bin/cat", &home_head]).await.status.success());
    }
}
