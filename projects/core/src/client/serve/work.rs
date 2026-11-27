//! Requests about a thread's jobs, sources, plan, changes and Overview.

use super::{Server, publish::publish};
use anyhow::Result;
use dusk_protocol::client::{Event, Response, WorkRequest};
use dusk_protocol::thread::ThreadEvent;

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
            WorkRequest::FileChanges { thread_id } => {
                Response::FileChanges(runtime.file_changes(&thread_id).await?)
            }
            WorkRequest::SavePlan { thread_id, text } => {
                runtime.save_plan(&thread_id, &text).await?;
                Response::Done
            }
            WorkRequest::Plans { thread_id } => Response::Plans(runtime.plans(&thread_id).await?),
            WorkRequest::RemoveOldPlans { thread_id } => {
                Response::Number(runtime.remove_old_plans(&thread_id).await?)
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
            WorkRequest::SourcePicture { thread_id, name } => {
                Response::Picture(runtime.source_picture(&thread_id, &name).await?)
            }
            WorkRequest::Card {
                thread_id,
                card_id,
                part,
            } => match runtime.card(&thread_id, &card_id, &part).await? {
                Some(bytes) => Response::Picture(bytes),
                None => Response::Error("no such card".to_owned()),
            },
            WorkRequest::PlanTasks {
                thread_id,
                done_before,
                limit,
            } => Response::PlanPage(
                runtime
                    .plan_page(&thread_id, done_before.as_deref(), limit)
                    .await?,
            ),
            WorkRequest::Team { thread_id } => Response::Team(runtime.team(&thread_id).await?),
            WorkRequest::Overview { thread_id } => {
                Response::Overview(runtime.overview(&thread_id).await?)
            }
            WorkRequest::Events {
                thread_id,
                before,
                limit,
            } => Response::Events(runtime.events(&thread_id, before, limit).await?),
            WorkRequest::ConfirmTask { thread_id, task_id } => {
                let (chat, task) = runtime.confirm_task(&thread_id, &task_id).await?;
                self.announce(&chat, ThreadEvent::TaskChanged(task));
                Response::Done
            }
            WorkRequest::RetryTask { thread_id, task_id } => {
                let (chat, task) = runtime.retry_task(&thread_id, &task_id).await?;
                self.announce(&chat, ThreadEvent::TaskChanged(task));
                Response::Done
            }
            WorkRequest::StopTeam { thread_id } => {
                let (team, returned) = runtime.stop_team(&thread_id).await?;
                let chat = team.thread_id.clone();
                for task in returned {
                    self.announce(&chat, ThreadEvent::TaskChanged(task));
                }
                self.announce(&chat, ThreadEvent::Team(team));
                Response::Done
            }
            WorkRequest::SetTeamAtOnce { thread_id, at_once } => {
                let team = runtime.set_team_at_once(&thread_id, at_once).await?;
                self.announce(&team.thread_id.clone(), ThreadEvent::Team(team));
                Response::Done
            }
        })
    }

    /// Tells every client that may read the chat of a change to its plan or team.
    fn announce(&self, chat: &str, event: ThreadEvent) {
        publish(
            &self.subscribers,
            Event::Thread {
                thread_id: chat.to_owned(),
                event: Box::new(event),
            },
        );
    }
}
