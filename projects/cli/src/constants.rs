use std::time::Duration;

pub const APP_NAME: &str = "dusk_code";
/// Identity bound to turns this process executes.
pub const AGENT_ID: &str = "dusk_code-agent";
/// Recorded as the thread origin.
pub const THREAD_SOURCE: &str = "cli";
/// How often the core checks config.yml for hand edits.
pub const CONFIG_RELOAD_PERIOD: Duration = Duration::from_secs(5);
