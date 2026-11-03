//! Raw log files under `logs/` beside the config: `core.log` for the core and the
//! libraries it uses, and one named by the client, such as `tui.log`, for its own events. Each
//! line is plain text: time, level, source and message. `DUSKCODE_LOG` sets what is
//! written the way `RUST_LOG` does, `info` when unset. A file over `MAX_BYTES` at start
//! is kept as `.1` and a new one begun. Secrets never reach the logs: the config and
//! every error that could quote one are redacted before they are logged.
//!
//! With `logging.traces` in the config, the same events and their spans also go to an
//! OpenTelemetry collector. With `logging.enabled` off, nothing is written or sent; the
//! switch silences a running process at once.

use std::{
    collections::HashMap,
    fs::{self, File, OpenOptions},
    path::Path,
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
};

use anyhow::{Context, Result, anyhow};
use opentelemetry::trace::TracerProvider;
use opentelemetry_otlp::{SpanExporter, WithExportConfig, WithHttpConfig};
use opentelemetry_sdk::{Resource, trace::SdkTracerProvider};
use tracing_subscriber::{
    EnvFilter, Layer, filter::filter_fn, fmt, layer::SubscriberExt, util::SubscriberInitExt,
};

use crate::{
    config::{Config, Traces},
    init::paths::Paths,
};

const MAX_BYTES: u64 = 10 * 1024 * 1024;
const LEVEL_VARIABLE: &str = "DUSKCODE_LOG";
const SERVICE: &str = "duskcode";

static ENABLED: AtomicBool = AtomicBool::new(true);

/// Silences or resumes every log and trace of this process. One that started with
/// logging off has nothing to resume until it starts again.
pub fn set_enabled(enabled: bool) {
    ENABLED.store(enabled, Ordering::Relaxed);
}

fn enabled() -> bool {
    ENABLED.load(Ordering::Relaxed)
}

/// Sends the spans still waiting when the process ends.
pub struct Telemetry {
    traces: Option<SdkTracerProvider>,
}

impl Telemetry {
    pub fn shutdown(self) {
        if let Some(traces) = self.traces {
            // The exporter's blocking HTTP client may not be dropped on the async runtime.
            let _ = std::thread::spawn(move || traces.shutdown()).join();
        }
    }
}

/// Starts writing the logs of this process, as the config file has them. Events whose
/// target is `client`, the client's crate name, go to `<file>.log`.
pub fn init(paths: &Paths, client: &'static str, file: &str) -> Result<Telemetry> {
    let logging = Config::load_from(paths.config_file())
        .map(|config| config.logging().clone())
        .unwrap_or_default();
    set_enabled(logging.enabled);
    if !logging.enabled {
        return Ok(Telemetry { traces: None });
    }
    let directory = paths.logs_dir();
    create_private_dir(&directory)?;
    let core = open(&directory.join("core.log"))?;
    let own = open(&directory.join(format!("{file}.log")))?;
    let is_client =
        move |target: &str| target == client || target.starts_with(&format!("{client}::"));
    let level = EnvFilter::try_from_env(LEVEL_VARIABLE)
        .unwrap_or_else(|_| EnvFilter::new("info"))
        // rmcp writes OAuth codes and tokens at debug.
        .add_directive(
            "rmcp::transport::auth=info"
                .parse()
                .expect("a valid directive"),
        );
    let traces = logging.traces.as_ref().map(tracer_provider).transpose()?;
    let exported = traces.as_ref().map(|provider| {
        tracing_opentelemetry::layer()
            .with_tracer(provider.tracer(SERVICE))
            .with_filter(filter_fn(|_| enabled()))
    });
    tracing_subscriber::registry()
        .with(level)
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(Mutex::new(core))
                .with_filter(filter_fn(move |meta| {
                    enabled() && !is_client(meta.target())
                })),
        )
        .with(
            fmt::layer()
                .with_ansi(false)
                .with_writer(Mutex::new(own))
                .with_filter(filter_fn(move |meta| enabled() && is_client(meta.target()))),
        )
        .with(exported)
        .try_init()
        .context("logs are already set up")?;
    Ok(Telemetry { traces })
}

/// Batches spans for the collector on a thread of their own.
fn tracer_provider(traces: &Traces) -> Result<SdkTracerProvider> {
    let endpoint = format!("{}/v1/traces", traces.endpoint.trim_end_matches('/'));
    let headers: HashMap<String, String> = traces.headers.clone().into_iter().collect();
    // The exporter's blocking HTTP client may not be built on the async runtime.
    std::thread::spawn(move || {
        let exporter = SpanExporter::builder()
            .with_http()
            .with_endpoint(endpoint)
            .with_headers(headers)
            .build()
            .map_err(|_| anyhow!("cannot set up the trace exporter"))?;
        Ok(SdkTracerProvider::builder()
            .with_batch_exporter(exporter)
            .with_resource(Resource::builder().with_service_name(SERVICE).build())
            .build())
    })
    .join()
    .map_err(|_| anyhow!("cannot set up the trace exporter"))?
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
