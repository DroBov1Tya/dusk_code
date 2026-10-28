use std::env;

use anyhow::Result;
use dusk_core::{
    api::CORE_VERSION,
    paths::Paths,
    runtime::{Runtime, ThreadHandle},
};
use tracing::info;

use crate::{args::AppArgs, chat, commands::Settings, constants::THREAD_SOURCE};

/// Starts the core, chats in one thread until input ends, then shuts the core down.
pub async fn run(args: AppArgs) -> Result<()> {
    let runtime = Runtime::start(Paths::from_home()?, args.state_db).await?;
    info!("core {CORE_VERSION} started");
    let result = match (args.list_threads, args.thread) {
        (true, _) => list_threads(&runtime).await,
        (false, thread) => chat_in_thread(&runtime, thread.as_deref()).await,
    };
    runtime.shutdown().await;
    result
}

async fn list_threads(runtime: &Runtime) -> Result<()> {
    for thread in runtime.list_threads().await? {
        let title = thread.title.as_deref().unwrap_or("(no messages)");
        println!("{}  {}", thread.id, chat::sanitize(title));
    }
    Ok(())
}

/// A new thread, or the one with `thread_id` after its history.
async fn chat_in_thread(runtime: &Runtime, thread_id: Option<&str>) -> Result<()> {
    let cwd = env::current_dir()?;
    let thread = match thread_id {
        Some(thread_id) => {
            chat::print_history(&runtime.thread_messages(thread_id).await?);
            runtime.resume_thread(thread_id, &cwd).await?
        }
        None => runtime.new_thread(THREAD_SOURCE, &cwd).await?,
    };
    let ThreadHandle {
        thread_id,
        input,
        output,
    } = thread;
    eprintln!("thread {thread_id}; continue it later with --thread {thread_id}");
    let settings = Settings {
        config: runtime.config().clone(),
        tools: runtime.tool_names(),
        skills_dir: runtime.paths().skills_dir(),
    };
    chat::run(input, output, settings).await
}
