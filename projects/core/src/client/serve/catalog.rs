//! Requests about skills, agents, MCP servers, the profile, secrets and speech settings.

use super::Server;
use anyhow::Result;
use dusk_protocol::client::{CatalogRequest, Response};

impl Server {
    pub(super) async fn catalog(&self, request: CatalogRequest) -> Result<Response> {
        let runtime = &self.runtime;
        Ok(match request {
            CatalogRequest::Skill { name } => Response::Node(runtime.skill(&name)?),
            CatalogRequest::SaveSkill { name, text } => {
                Response::Node(runtime.save_skill(&name, &text)?)
            }
            CatalogRequest::Agent { name } => Response::Node(runtime.agent(&name)?),
            CatalogRequest::SaveAgent { name, text } => {
                Response::Node(runtime.save_agent(&name, &text).await?)
            }
            CatalogRequest::McpServers => Response::Mcp(runtime.mcp_servers()),
            CatalogRequest::Snippets => Response::Snippets(runtime.mentions().snippets()?),
            CatalogRequest::SaveSnippet { name, text } => {
                runtime.mentions().save_snippet(&name, &text)?;
                Response::Done
            }
            CatalogRequest::Profile => Response::Profile(runtime.mentions().profile()?),
            CatalogRequest::Secrets => Response::Secrets(runtime.secrets_info().await),
            CatalogRequest::Voice => Response::Voice(runtime.voice_settings()),
            CatalogRequest::SaveProfile(profile) => {
                runtime.mentions().save_profile(&profile)?;
                Response::Done
            }
            CatalogRequest::McpServerText { name } => {
                Response::Text(runtime.mcp_server_text(&name)?)
            }
            CatalogRequest::SaveMcpServer { old, text } => {
                Response::Name(runtime.save_mcp_server(old.as_deref(), &text)?)
            }
            CatalogRequest::RestartMcp { name } => {
                runtime.restart_mcp(&name);
                Response::Done
            }
            CatalogRequest::SignInMcp { name } => {
                Response::Text(runtime.sign_in_mcp(&name).await?.to_string())
            }
        })
    }
}
