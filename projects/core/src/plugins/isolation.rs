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
    keychain: bool,
    /// Its variables, its settings among them: never logged.
    env: Vec<(String, String)>,
    confinement: Confinement,
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
        #[cfg(target_os = "linux")]
        ensure!(
            network == Network::Closed,
            "Linux cannot keep a program to the core's broker, so a plugin that connects \
             does not start here"
        );
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
            env.extend(broker.env());
        }
        Ok(Self {
            folder,
            config_root: config_root.clone(),
            data: data.clone(),
            cwd: None,
            project: permissions.project,
            keychain: permissions.keychain,
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
            .current_dir(self.cwd.as_ref().unwrap_or(&self.folder));
        Ok(command)
    }

    /// The confinement of one call of a contract plugin about the project in `project`:
    /// home and every folder of `others` (the other saved projects) hidden, `project`
    /// reopened read-only and the program run in it, git's own config readable, and the
    /// login keychain open when the plugin asked for it. Hiding home alone would leave
    /// projects outside it open, so each other project is hidden by name.
    ///
    /// Build it anew for every call: the folders of temporary chats, taken when it is
    /// built, are hidden too, and the program cannot hand work
    /// to launchd, LaunchServices or Apple events (macOS), which run it unconfined.
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
        let home = home
            .canonicalize()
            .context("cannot resolve the home folder")?;
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
        let safe = |path: &Path| {
            !path.starts_with(&self.config_root)
                && !self.config_root.starts_with(path)
                && !home.starts_with(path)
                && !overlaps(path)
        };
        let mut reopened = vec![project.clone()];
        // A worktree's or a sub-folder's git dir lies outside the project; one that
        // fails the project's own checks is left closed, and git fails.
        for git in git_dirs(&project) {
            if !git.starts_with(&project)
                && !project.starts_with(&git)
                && safe(&git)
                && !reopened.contains(&git)
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
        // Seatbelt closes the keychain files with home; the services follow `keychain`.
        #[cfg(target_os = "macos")]
        if self.keychain
            && let Ok(path) = home.join("Library/Keychains").canonicalize()
            && safe(&path)
        {
            reopened.push(path);
        }
        confinement.hidden.extend(hidden);
        confinement.reopened.extend(reopened);
        confinement.keychain = self.keychain;
        confinement.hand_off = false;
        call.cwd = Some(project);
        Ok(call)
    }

    /// Tells one confinement from another, to restart what runs under a changed one.
    /// Kept in memory only: it holds the settings.
    pub fn fingerprint(&self) -> String {
        format!("{:?} {:?}", self.confinement, self.env)
    }
}

/// The git dir and common git dir of the repository `project` is in, resolved, read by
/// the core outside any sandbox: no shell, no prompt, within git's usual time for a
/// read. Empty when it is no repository or git fails.
fn git_dirs(project: &Path) -> Vec<PathBuf> {
    use std::{io::Read, process::Stdio, time::Instant};
    const LIMIT: std::time::Duration = std::time::Duration::from_secs(10);
    let mut command = std::process::Command::new("git");
    if let Some(path) = platform::login_path::get() {
        command.env("PATH", path);
    }
    for key in [
        "GIT_DIR",
        "GIT_COMMON_DIR",
        "GIT_WORK_TREE",
        "GIT_CEILING_DIRECTORIES",
    ] {
        command.env_remove(key);
    }
    command
        .arg("-C")
        .arg(project)
        .args(["rev-parse", "--absolute-git-dir", "--git-common-dir"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .env("GIT_OPTIONAL_LOCKS", "0")
        .env("LC_ALL", "C")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    let Ok(mut child) = command.spawn() else {
        return Vec::new();
    };
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
    if !finished || !matches!(read, Some(Ok(_))) {
        return Vec::new();
    }
    // The common dir may be printed relative to the project.
    printed
        .lines()
        .filter_map(|line| project.join(line).canonicalize().ok())
        .collect()
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
            Isolation::new(&paths, &folder, &data, &permissions, Vec::new(), broker).unwrap();
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

    /// Seatbelt closes `~/Library/Keychains` with home: a plugin that asked for the
    /// keychain gets it back, one that did not stays without.
    #[tokio::test]
    async fn a_keychain_plugin_reopens_the_keychain_files() {
        for keychain in [true, false] {
            let permissions = Permissions {
                keychain,
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
            assert!(shown.contains(&format!("keychain: {keychain}")), "{shown}");
            let listed = run(&call, &["/bin/ls", &keychains.display().to_string()]).await;
            #[cfg(target_os = "macos")]
            {
                assert_eq!(
                    shown.contains(&format!("{keychains:?}")),
                    keychain,
                    "{shown}"
                );
                assert_eq!(listed.status.success(), keychain);
            }
            #[cfg(not(target_os = "macos"))]
            assert!(!listed.status.success());
        }
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

    /// launchd, LaunchServices and Apple events run what they are handed outside the
    /// sandbox, so a call may not reach them; git and a gh-like program still run.
    #[cfg(target_os = "macos")]
    #[tokio::test]
    async fn a_call_cannot_hand_work_to_the_system() {
        let Some(setup) = call_setup("call-hand-off", reads_project(), None) else {
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
        std::fs::write(&gh, "#!/bin/sh\n/usr/bin/git --version && pwd\n").unwrap();
        std::fs::set_permissions(&gh, std::os::unix::fs::PermissionsExt::from_mode(0o755)).unwrap();
        let output = run(&call, &[&gh.display().to_string()]).await;
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
        let git = |dir: &Path, args: &[&str]| {
            let status = std::process::Command::new("git")
                .arg("-C")
                .arg(dir)
                .args(["-c", "user.name=t", "-c", "user.email=t@t"])
                .args(args)
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .unwrap();
            assert!(status.success(), "git {args:?}");
        };
        git(&main, &["init", "-q"]);
        git(&main, &["commit", "-q", "--allow-empty", "-m", "x"]);
        let target = worktree.display().to_string();
        git(&main, &["worktree", "add", "-q", "--detach", &target]);
        let worktree = worktree.canonicalize().unwrap();
        let call = setup
            .isolation
            .for_call(&setup.home, &worktree, &[])
            .unwrap();
        let head = ["/usr/bin/git", "-C", &target, "rev-parse", "HEAD"];
        let output = run(&call, &head).await;
        assert!(output.status.success(), "{output:?}");
        let secret = main.join("secret").display().to_string();
        assert!(!run(&call, &["/bin/cat", &secret]).await.status.success());
        // Saved as a project of its own, the main repository stays closed: git fails.
        let call = setup
            .isolation
            .for_call(&setup.home, &worktree, std::slice::from_ref(&main))
            .unwrap();
        assert!(!run(&call, &head).await.status.success());
    }
}
