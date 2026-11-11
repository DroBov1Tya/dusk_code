//! The core's requests as typed calls, so a view says what it wants and not which
//! response variant carries it. Every error is the core's own text.

use crate::link::request;
use dusk_protocol::{
    catalog::NodeText,
    client::{CatalogRequest, Response},
    config::{McpInfo, Profile, SecretsInfo, Snippet, VoiceSettings},
};

use super::*;

pub async fn skill(name: &str) -> Res<NodeText> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::Skill { name }), Response::Node(n) => n)
}

pub async fn save_skill(name: &str, text: String) -> Res<NodeText> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::SaveSkill { name, text }), Response::Node(n) => n)
}

pub async fn agent(name: &str) -> Res<NodeText> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::Agent { name }), Response::Node(n) => n)
}

pub async fn save_agent(name: &str, text: String) -> Res<NodeText> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::SaveAgent { name, text }), Response::Node(n) => n)
}

pub async fn mcp_servers() -> Res<Vec<McpInfo>> {
    ask!(catalog(CatalogRequest::McpServers), Response::Mcp(m) => m)
}

pub async fn save_mcp_server(old: Option<String>, text: String) -> Res<Option<String>> {
    ask!(catalog(CatalogRequest::SaveMcpServer { old, text }), Response::Name(n) => n)
}

pub async fn restart_mcp(name: &str) -> Res<()> {
    done(catalog(CatalogRequest::RestartMcp {
        name: name.to_owned(),
    }))
    .await
}

pub async fn sign_in_mcp(name: &str) -> Res<String> {
    let name = name.to_owned();
    ask!(catalog(CatalogRequest::SignInMcp { name }), Response::Text(t) => t)
}

pub async fn snippets() -> Res<Vec<Snippet>> {
    ask!(catalog(CatalogRequest::Snippets), Response::Snippets(s) => s)
}

pub async fn save_snippet(name: String, text: String) -> Res<()> {
    done(catalog(CatalogRequest::SaveSnippet { name, text })).await
}

pub async fn profile() -> Res<Profile> {
    ask!(catalog(CatalogRequest::Profile), Response::Profile(p) => p)
}

pub async fn save_profile(profile: Profile) -> Res<()> {
    done(catalog(CatalogRequest::SaveProfile(profile))).await
}

pub async fn secrets() -> Res<SecretsInfo> {
    ask!(catalog(CatalogRequest::Secrets), Response::Secrets(s) => s)
}

pub async fn voice_settings() -> Res<VoiceSettings> {
    ask!(catalog(CatalogRequest::Voice), Response::Voice(v) => v)
}
