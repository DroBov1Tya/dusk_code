use std::time::Duration;

pub const APP_NAME: &str = "dusk_code";
/// Recorded as the thread origin.
pub const THREAD_SOURCE: &str = "cli";
/// How often the core checks config.yml for hand edits.
pub const CONFIG_RELOAD_PERIOD: Duration = Duration::from_secs(5);
