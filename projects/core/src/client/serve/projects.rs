//! Requests about projects, their git state and files.

use super::Server;
use crate::project;
use anyhow::Result;
use dusk_protocol::client::{ProjectRequest, Response};
use std::path::PathBuf;

impl Server {
    pub(super) async fn project(&self, request: ProjectRequest) -> Result<Response> {
        let state = self.runtime.state();
        Ok(match request {
            ProjectRequest::List => Response::Projects(state.list_projects().await?),
            ProjectRequest::Save {
                id,
                name,
                description,
                dir,
            } => Response::Project(project::save(state, id, &name, &description, &dir).await?),
            ProjectRequest::Env { id } => Response::Names(project::env::names(state, &id).await?),
            ProjectRequest::SetEnv { id, name, value } => {
                Response::Names(project::env::set(state, &id, &name, &value.0).await?)
            }
            ProjectRequest::RemoveEnv { id, name } => {
                Response::Names(project::env::remove(state, &id, &name).await?)
            }
            ProjectRequest::Remove { id } => {
                state.remove_project(&id).await?;
                Response::Done
            }
            ProjectRequest::Git { id } => {
                Response::Git(project::git(&project::find(state, &id).await?).await?)
            }
            ProjectRequest::Diff { id, path } => Response::Diff(
                project::diff(&project::find(state, &id).await?.dir, path.as_deref()).await?,
            ),
            ProjectRequest::SwitchBranch { id, branch } => {
                project::switch_branch(&project::find(state, &id).await?, &branch).await?;
                Response::Done
            }
            ProjectRequest::Files { id, path } => {
                Response::Files(project::files(&project::find(state, &id).await?, &path).await?)
            }
            ProjectRequest::Outputs => Response::Outputs(
                project::outputs(self.output_roots(), self.runtime.paths().artifacts_dir()).await?,
            ),
            ProjectRequest::DeleteOutput { path } => {
                project::delete_output(&path, &self.output_roots()).await?;
                Response::Done
            }
            ProjectRequest::ReadOutput { path } => {
                let mut roots = self.output_roots();
                roots.extend(state.list_projects().await?.into_iter().map(|p| p.dir));
                Response::Text(project::read_output(&path, &roots).await?)
            }
        })
    }

    pub(super) fn output_roots(&self) -> Vec<PathBuf> {
        let paths = self.runtime.paths();
        vec![paths.workspace_root().to_path_buf(), paths.artifacts_dir()]
    }
}
