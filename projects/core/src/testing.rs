//! Shared test fixtures.

use std::{
    path::PathBuf,
    sync::{
        OnceLock,
        atomic::{AtomicUsize, Ordering},
    },
};

use crate::{
    config::Config,
    live_config::LiveConfig,
    paths::Paths,
    state::State,
    tools::{ToolContext, ToolScope},
};

/// A fresh directory under the system temp dir, unique per process and call.
pub(crate) fn temp_dir(name: &str) -> PathBuf {
    static NEXT: AtomicUsize = AtomicUsize::new(0);
    let id = NEXT.fetch_add(1, Ordering::Relaxed);
    std::env::temp_dir().join(format!("dusk_{name}_{}_{id}", std::process::id()))
}

pub(crate) fn temp_paths(name: &str) -> Paths {
    let root = temp_dir(name);
    Paths::new(root.join("config"), root.join("workspace"))
}

pub(crate) async fn temp_state(name: &str) -> State {
    State::open(temp_dir(name).join("state.db")).await.unwrap()
}

/// A config with one provider in a private directory, as `Config::load_from` requires.
pub(crate) fn temp_live_config(name: &str) -> LiveConfig {
    temp_live_config_with(name, "")
}

/// The same, with more settings appended as YAML.
pub(crate) fn temp_live_config_with(name: &str, settings: &str) -> LiveConfig {
    let directory = temp_dir(name);
    std::fs::create_dir_all(&directory).unwrap();
    let path = directory.join("config.yml");
    std::fs::write(&path, format!("{TEST_CONFIG}{settings}")).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700)).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    }
    LiveConfig::load(path).unwrap()
}

/// A context with a valid config where every tool and skill is on.
pub(crate) fn tool_context(scope: &ToolScope) -> ToolContext<'_> {
    static CONFIG: OnceLock<Config> = OnceLock::new();
    let config = CONFIG.get_or_init(|| Config::parse(TEST_CONFIG.as_bytes()).unwrap());
    ToolContext { scope, config }
}

const TEST_CONFIG: &str = "providers:\n  main:\n    kind: openai_compatible\n    base_url: 'https://host/v1'\n    api_key: 'sk-test'\n";
