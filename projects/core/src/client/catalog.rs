//! Skills, agents, MCP servers, snippets, the profile and the secrets and speech settings.

use super::{Client, unexpected};
use anyhow::Result;
use dusk_protocol::config::{Profile, Snippet};
use dusk_protocol::{
    catalog::NodeText,
    client::{CatalogRequest, Request, Response},
    config::McpInfo,
};

/// The files the user edits through the settings, and the MCP servers.
pub struct Catalog<'a>(pub(super) &'a Client);

impl Catalog<'_> {
    pub async fn skill(&self, name: &str) -> Result<NodeText> {
        let name = name.to_owned();
        self.node(CatalogRequest::Skill { name }).await
    }

    pub async fn save_skill(&self, name: &str, text: &str) -> Result<NodeText> {
        let (name, text) = (name.to_owned(), text.to_owned());
        self.node(CatalogRequest::SaveSkill { name, text }).await
    }

    pub async fn agent(&self, name: &str) -> Result<NodeText> {
        let name = name.to_owned();
        self.node(CatalogRequest::Agent { name }).await
    }

    pub async fn save_agent(&self, name: &str, text: &str) -> Result<NodeText> {
        let (name, text) = (name.to_owned(), text.to_owned());
        self.node(CatalogRequest::SaveAgent { name, text }).await
    }

    pub async fn mcp_servers(&self) -> Result<Vec<McpInfo>> {
        match self.call(CatalogRequest::McpServers).await? {
            Response::Mcp(servers) => Ok(servers),
            other => Err(unexpected(other)),
        }
    }

    pub async fn mcp_server_text(&self, name: &str) -> Result<String> {
        let name = name.to_owned();
        let request = Request::Catalog(CatalogRequest::McpServerText { name });
        self.0.text(request).await
    }

    /// Returns the name it was saved under, or none when the text removed it.
    pub async fn save_mcp_server(&self, old: Option<&str>, text: &str) -> Result<Option<String>> {
        let (old, text) = (old.map(str::to_owned), text.to_owned());
        match self
            .call(CatalogRequest::SaveMcpServer { old, text })
            .await?
        {
            Response::Name(name) => Ok(name),
            other => Err(unexpected(other)),
        }
    }

    pub async fn restart_mcp(&self, name: &str) -> Result<()> {
        let name = name.to_owned();
        self.0
            .done(Request::Catalog(CatalogRequest::RestartMcp { name }))
            .await
    }

    /// Returns the page that signs in.
    pub async fn sign_in_mcp(&self, name: &str) -> Result<String> {
        let name = name.to_owned();
        let request = Request::Catalog(CatalogRequest::SignInMcp { name });
        self.0.text(request).await
    }

    async fn call(&self, request: CatalogRequest) -> Result<Response> {
        self.0.call(Request::Catalog(request)).await
    }

    async fn node(&self, request: CatalogRequest) -> Result<NodeText> {
        self.0.node(Request::Catalog(request)).await
    }

    pub async fn snippets(&self) -> Result<Vec<Snippet>> {
        match self
            .0
            .call(Request::Catalog(CatalogRequest::Snippets))
            .await?
        {
            Response::Snippets(snippets) => Ok(snippets),
            other => Err(unexpected(other)),
        }
    }

    /// Empty text removes the snippet.
    pub async fn save_snippet(&self, name: &str, text: &str) -> Result<()> {
        let (name, text) = (name.to_owned(), text.to_owned());
        self.0
            .done(Request::Catalog(CatalogRequest::SaveSnippet { name, text }))
            .await
    }

    /// The speech servers that are set up.
    pub async fn voice(&self) -> Result<dusk_protocol::config::VoiceSettings> {
        match self.0.call(Request::Catalog(CatalogRequest::Voice)).await? {
            Response::Voice(voice) => Ok(voice),
            other => Err(unexpected(other)),
        }
    }

    /// Which secrets are set, never their values.
    pub async fn secrets(&self) -> Result<dusk_protocol::config::SecretsInfo> {
        match self
            .0
            .call(Request::Catalog(CatalogRequest::Secrets))
            .await?
        {
            Response::Secrets(info) => Ok(info),
            other => Err(unexpected(other)),
        }
    }

    pub async fn profile(&self) -> Result<Profile> {
        match self
            .0
            .call(Request::Catalog(CatalogRequest::Profile))
            .await?
        {
            Response::Profile(profile) => Ok(profile),
            other => Err(unexpected(other)),
        }
    }

    pub async fn save_profile(&self, profile: Profile) -> Result<()> {
        self.0
            .done(Request::Catalog(CatalogRequest::SaveProfile(profile)))
            .await
    }
}
