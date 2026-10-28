use tracing_subscriber::FmtSubscriber;

/// Logs go to stderr so stdout carries only the streamed answer.
pub fn init_logger() {
    FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .with_writer(std::io::stderr)
        .init();
}
