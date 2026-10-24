use tracing_subscriber::FmtSubscriber;

pub fn init_logger() {
    FmtSubscriber::builder()
        .with_max_level(tracing::Level::INFO)
        .init();
}
