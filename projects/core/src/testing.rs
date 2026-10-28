//! Shared test fixtures.

use std::{
    path::PathBuf,
    sync::atomic::{AtomicUsize, Ordering},
};

use crate::{paths::Paths, state::State};

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
