use std::env;

use anyhow::Result;
use dusk_core::{api::CORE_VERSION, paths::Paths, runtime::Runtime};
use tracing::info;

use crate::{args::AppArgs, chat, commands::Settings, constants::THREAD_SOURCE};

/// Starts the core, chats in one new thread until input ends, then shuts the core down.
pub async fn run(args: AppArgs) -> Result<()> {
    let runtime = Runtime::start(Paths::from_home()?, args.state_db).await?;
    info!("core {CORE_VERSION} started");
    let result = chat_in_new_thread(&runtime).await;
    runtime.shutdown().await;
    result
}

async fn chat_in_new_thread(runtime: &Runtime) -> Result<()> {
    let thread = runtime
        .new_thread(THREAD_SOURCE, &env::current_dir()?)
        .await?;
    let settings = Settings {
        config: runtime.config().clone(),
        tools: runtime.tool_names(),
        skills_dir: runtime.paths().skills_dir(),
    };
    chat::run(thread.input, thread.output, settings).await
}
