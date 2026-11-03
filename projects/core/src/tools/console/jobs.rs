//! `jobs`: the model's view of its thread's background commands, which the runtime's
//! job manager keeps.

use anyhow::{Context, ensure};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::Value;

use crate::{
    BoxFuture,
    runtime::jobs::JobOwner,
    tools::{Tool, ToolContext, ToolKind, schema, schema::parse_arguments},
};

pub struct Jobs(pub JobOwner);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct Arguments {
    action: Action,
    /// Required except for list.
    job_id: Option<String>,
}
#[derive(Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Action {
    List,
    Status,
    Output,
    Cancel,
}

impl Tool for Jobs {
    fn name(&self) -> &'static str {
        "jobs"
    }
    fn kind(&self) -> ToolKind {
        ToolKind::Console
    }
    fn description(&self, _: &ToolContext<'_>) -> String {
        "List, inspect output/status, or cancel this thread's background commands. Results are delivered automatically; polling is optional. Output keeps at most 32 KiB per stream. Finished job history is bounded and older IDs expire.".into()
    }
    fn parameters(&self) -> Value {
        schema::of::<Arguments>()
    }
    fn call<'a>(&'a self, arguments: Value, _: ToolContext<'a>) -> BoxFuture<'a, String> {
        Box::pin(async move {
            let arguments: Arguments = parse_arguments(arguments)?;
            let JobOwner { manager, thread_id } = &self.0;
            if matches!(arguments.action, Action::List) {
                ensure!(arguments.job_id.is_none(), "list does not accept job_id");
                let jobs = manager.state.jobs(thread_id).await?;
                if jobs.is_empty() {
                    return Ok("no background jobs".into());
                }
                return Ok(jobs
                    .into_iter()
                    .map(|job| {
                        format!(
                            "{}: {} - {}",
                            job.job_id,
                            manager.status(&job),
                            job.command.chars().take(120).collect::<String>()
                        )
                    })
                    .collect::<Vec<_>>()
                    .join("\n"));
            }
            let id = arguments.job_id.context("job_id is required")?;
            if matches!(arguments.action, Action::Cancel) {
                return manager.cancel(thread_id, &id).await;
            }
            let job = manager.state.job(thread_id, &id).await?;
            if matches!(arguments.action, Action::Status) {
                return Ok(format!("{}: {}", job.job_id, manager.status(&job)));
            }
            let status = manager.status(&job);
            let output = manager.printed(thread_id, &id, job).await;
            Ok(format!("job {id}: {status}\n{output}"))
        })
    }
}
