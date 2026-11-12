//! Requests about a thread's jobs, sources, plan and changes.

use super::Server;
use anyhow::Result;
use dusk_protocol::client::{Response, WorkRequest};

impl Server {
    pub(super) async fn work(&self, request: WorkRequest) -> Result<Response> {
        let runtime = &self.runtime;
        Ok(match request {
            WorkRequest::Jobs { thread_id } => Response::Jobs(runtime.jobs(&thread_id).await?),
            WorkRequest::JobOutput { thread_id, job_id } => {
                Response::Text(runtime.job_output(&thread_id, &job_id).await?)
            }
            WorkRequest::StopJob { thread_id, job_id } => {
                Response::Text(runtime.stop_job(&thread_id, &job_id).await?)
            }
            WorkRequest::Plan { thread_id } => Response::Plan(runtime.plan(&thread_id).await?),
            WorkRequest::Diff { thread_id, path } => {
                Response::Diff(runtime.chat_diff(&thread_id, path.as_deref()).await?)
            }
            WorkRequest::ChangedFiles { thread_id } => {
                Response::Changes(runtime.changed_files(&thread_id).await?)
            }
            WorkRequest::SavePlan { thread_id, text } => {
                runtime.save_plan(&thread_id, &text).await?;
                Response::Done
            }
            WorkRequest::PinSource {
                thread_id,
                id,
                pinned,
            } => {
                runtime.pin_source(&thread_id, &id, pinned).await?;
                Response::Done
            }
            WorkRequest::RemoveSource { thread_id, id } => {
                runtime.remove_source(&thread_id, &id).await?;
                Response::Done
            }
            WorkRequest::UploadPicture {
                thread_id,
                name,
                bytes,
            } => Response::Text(runtime.add_picture(&thread_id, &name, &bytes).await?),
        })
    }
}
