//! Raw log files under `logs/` beside the config: `core.log` for the core and the
//! libraries it uses, and one named by the client, such as `tui.log`, for its own events. Each
//! line is plain text: time, level, source and message. `DUSKCODE_LOG` sets what is
//! written the way `RUST_LOG` does, `info` when unset. A file over `MAX_BYTES` at start
//! is kept as `.1` and a new one begun. Secrets never reach the logs: the config and
//! every error that could quote one are redacted before they are logged.

use std::{
    fs::{self, File, OpenOptions},
    path::Path,
    sync::Mutex,
};

use anyhow::{Context, Result};
use tracing_subscriber::{
    EnvFilter, Layer, filter::filter_fn, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

use crate::paths::Paths;

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const LEVEL_VARIABLE: &str = "DUSKCODE_LOG";

/// Starts writing the logs of this process. Events whose target is `client`, the
/// client's crate name, go to `<file>.log`.
pub fn init(paths: &Paths, client: &'static str, file: &str) -> Result<()> {
    let directory = paths.logs_dir();
    create_private_dir(&directory)?;
    let core = open(&directory.join("core.log"))?;
    let own = open(&directory.join(format!("{file}.log")))?;
    let is_client =
        move |target: &str| target == client || target.starts_with(&format!("{client}::"));
    let level = EnvFilter::try_from_env(LEVEL_VARIABLE).unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::registry()
        .with(level)
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(Mutex::new(core))
                .with_filter(filter_fn(move |meta| !is_client(meta.target()))),
        )
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(Mutex::new(own))
                .with_filter(filter_fn(move |meta| is_client(meta.target()))),
        )
        .try_init()
        .context("logs are already set up")
}

fn create_private_dir(directory: &Path) -> Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(directory)
        .with_context(|| format!("cannot create {}", directory.display()))
}

/// The log file to append to, private to the user, the old one set aside when large.
fn open(path: &Path) -> Result<File> {
    if fs::metadata(path).is_ok_and(|metadata| metadata.len() > MAX_BYTES) {
        fs::rename(path, path.with_extension("log.1"))
            .with_context(|| format!("cannot set aside {}", path.display()))?;
    }
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    options
        .open(path)
        .with_context(|| format!("cannot open {}", path.display()))
}
