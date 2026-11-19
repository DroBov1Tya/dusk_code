//! The core a run talks to, on a tokio runtime of its own, so a run can kill it the way a
//! crash would: the runtime is dropped, every task of the core stops at the point it had
//! reached and no shutdown code runs, then a new core starts on the same folders. What
//! the old process held in memory is gone; what it wrote stays, as after a real kill.

use std::time::Duration;

use anyhow::{Context, Result};
use dusk_core::{
    app::{self, Paths},
    client::Client,
};
use dusk_protocol::client::Hello;
use tokio::runtime::Runtime;

const SOURCE: &str = "evals";
/// How long a kill waits for the core's threads to let go of what they hold, such as the
/// lock of an open thread. Work in a blocking thread past it is left running.
const LET_GO: Duration = Duration::from_secs(10);

pub struct Core {
    pub client: Client,
    runtime: Option<Runtime>,
    paths: Paths,
    mode: Option<String>,
}

impl Core {
    /// Starts the core over `paths`, in `mode` when given, and waits until it knows its
    /// model.
    pub async fn start(paths: Paths, mode: Option<String>) -> Result<Self> {
        let runtime = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .thread_name("dusk-core")
            .build()
            .context("cannot make a runtime for the core")?;
        let hello = Hello::new(SOURCE);
        // Its start is not `Send`, so it runs on a thread of its own, in the core's
        // runtime, where what it spawns lives.
        let (handle, start_paths) = (runtime.handle().clone(), paths.clone());
        let started = tokio::task::spawn_blocking(move || {
            handle.block_on(app::start(start_paths, None, hello))
        })
        .await;
        let client = match started {
            Ok(Ok(client)) => client,
            failed => {
                runtime.shutdown_background();
                return Err(match failed {
                    Ok(Err(error)) => error,
                    _ => anyhow::anyhow!("the core's start panicked"),
                });
            }
        };
        let mut core = Self {
            client,
            runtime: Some(runtime),
            paths,
            mode,
        };
        if let Some(mode) = &core.mode {
            core.client.settings().mode(mode).await?;
        }
        // The core looks for a model in the background when the config names none.
        while core.client.status.borrow_and_update().discovering_model {
            if core.client.status.changed().await.is_err() {
                break;
            }
        }
        Ok(core)
    }

    /// Kills the core and starts a new one on the same folders. Open threads are not
    /// opened again: the caller resumes the ones it follows, as a person would.
    pub async fn restart(&mut self) -> Result<()> {
        if let Some(runtime) = self.runtime.take() {
            // Dropping a runtime blocks, which an async context must not do.
            tokio::task::spawn_blocking(move || runtime.shutdown_timeout(LET_GO))
                .await
                .context("cannot kill the core")?;
        }
        *self = Self::start(self.paths.clone(), self.mode.clone()).await?;
        Ok(())
    }

    /// Closes every thread and stops the core.
    pub async fn stop(mut self) {
        self.client.shutdown().await;
        if let Some(runtime) = self.runtime.take() {
            let _ = tokio::task::spawn_blocking(move || runtime.shutdown_timeout(LET_GO)).await;
        }
    }
}

impl Drop for Core {
    /// A core left behind by an error: dropped without waiting, which a runtime allows
    /// only this way inside another one.
    fn drop(&mut self) {
        if let Some(runtime) = self.runtime.take() {
            runtime.shutdown_background();
        }
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::sandbox::TempHome;

    /// A kill must let go of what a crash lets go of. An open thread holds a lock the
    /// system frees when its process ends; a kill that did not free it would leave the
    /// thread unopenable, and a run that resumes it would test nothing.
    #[tokio::test(flavor = "multi_thread")]
    async fn a_killed_core_starts_again_and_its_thread_opens_again() {
        let home = TempHome::new("dusk-evals-kill").unwrap();
        let project = home.0.join("project");
        fs::create_dir(&project).unwrap();
        let paths = || Paths::new(home.0.join("config"), home.0.join("workspace"));
        let mut core = Core::start(paths(), None).await.unwrap();
        let thread_id = core.client.threads().start(project.clone()).await.unwrap();

        // While the first core lives, a second one cannot open its thread.
        let second = Core::start(paths(), None).await.unwrap();
        let refused = second
            .client
            .threads()
            .resume(&thread_id, project.clone())
            .await;
        assert!(refused.is_err(), "two cores opened one thread");
        second.stop().await;

        core.restart().await.unwrap();
        let resumed = core
            .client
            .threads()
            .resume(&thread_id, project.clone())
            .await;
        assert_eq!(resumed.unwrap(), thread_id);
        let listed = core.client.threads().list().await.unwrap();
        assert!(listed.iter().any(|thread| thread.id == thread_id));
        core.stop().await;
    }
}
