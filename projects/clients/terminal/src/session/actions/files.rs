//! Actions on the files the app edits in place: @me, snippets, and the skill, agent and
//! MCP server files of the settings editor.

use dusk_core::client::Client;
use dusk_protocol::client::Command;

use crate::app::{App, FileEditor, Panel, PasteEditor};
use crate::session::{answers::answer, thread::OpenThread};

pub async fn open_me(client: &Client, app: &mut App) {
    match client.catalog().profile().await {
        Ok(profile) => app.panel = Some(Panel::Paste(PasteEditor::me(profile.about))),
        Err(error) => app.error(format!("cannot read the profile: {error:#}")),
    }
}

pub async fn save_me(client: &Client, app: &mut App, text: String) {
    let saved = async {
        let mut profile = client.catalog().profile().await?;
        profile.about = text;
        client.catalog().save_profile(profile).await
    }
    .await;
    match saved {
        Ok(()) => app.flash = Some("@me saved".to_owned()),
        Err(error) => app.error(format!("cannot save the profile: {error:#}")),
    }
}

pub async fn open_snippet(client: &Client, app: &mut App, name: String) {
    match client.catalog().snippets().await {
        Ok(snippets) => {
            let text = snippets
                .into_iter()
                .find(|snippet| snippet.name == name)
                .map(|snippet| snippet.text)
                .unwrap_or_default();
            app.panel = Some(Panel::Paste(PasteEditor::snippet(name, text)));
        }
        Err(error) => app.error(format!("cannot read the snippets: {error:#}")),
    }
}

pub async fn save_snippet(client: &Client, app: &mut App, name: String, text: String) {
    match client.catalog().save_snippet(&name, &text).await {
        Ok(()) => {
            app.flash = Some(match text.trim().is_empty() {
                true => format!("@{name} removed"),
                false => format!("@{name} saved"),
            })
        }
        Err(error) => app.error(format!("cannot save the snippet: {error:#}")),
    }
}

pub async fn open_skill(client: &Client, app: &mut App, name: String) {
    match client.catalog().skill(&name).await {
        Ok(skill) => open_editor(app, FileEditor::skill(skill), false),
        Err(error) => app.flash = Some(format!("cannot open the skill: {error:#}")),
    }
}

pub async fn save_skill(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    name: String,
    text: String,
) {
    match client.catalog().save_skill(&name, &text).await {
        Ok(skill) => {
            open_editor(app, FileEditor::skill(skill), true);
            app.flash = Some(format!("skill {name} saved"));
            answer(client, app, open, Command::Settings).await;
        }
        Err(error) => editor_error(app, format!("not saved: {error:#}")),
    }
}

pub async fn open_agent_file(client: &Client, app: &mut App, name: String) {
    match client.catalog().agent(&name).await {
        Ok(agent) => open_editor(app, FileEditor::agent(agent), false),
        Err(error) => app.flash = Some(format!("cannot open the agent: {error:#}")),
    }
}

pub async fn save_agent_file(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    name: String,
    text: String,
) {
    match client.catalog().save_agent(&name, &text).await {
        Ok(agent) => {
            open_editor(app, FileEditor::agent(agent), true);
            app.flash = Some(format!("agent {name} saved"));
            answer(client, app, open, Command::Settings).await;
        }
        Err(error) => editor_error(app, format!("not saved: {error:#}")),
    }
}

pub async fn open_mcp(client: &Client, app: &mut App, name: Option<String>) {
    let text = match name.as_deref() {
        Some(name) => client.catalog().mcp_server_text(name).await.map(Some),
        None => Ok(None),
    };
    match text {
        Ok(text) => {
            let servers = client.catalog().mcp_servers().await.unwrap_or_default();
            let server = servers
                .iter()
                .find(|server| Some(&server.name) == name.as_ref());
            open_editor(app, FileEditor::mcp(server, text), false);
        }
        Err(error) => app.flash = Some(format!("cannot open the server: {error:#}")),
    }
}

pub async fn save_mcp(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    old: Option<String>,
    text: String,
) {
    match client
        .catalog()
        .save_mcp_server(old.as_deref(), &text)
        .await
    {
        Ok(saved) => {
            let servers = client.catalog().mcp_servers().await.unwrap_or_default();
            match &saved {
                Some(name) => {
                    let server = servers.iter().find(|server| server.name == *name);
                    let text = client.catalog().mcp_server_text(name).await.ok();
                    open_editor(app, FileEditor::mcp(server, text), true);
                    app.flash = Some(format!("MCP server {name} saved, starting"));
                }
                None => {
                    if let Some(Panel::Settings(view)) = &mut app.panel {
                        view.editor = None;
                    }
                    app.flash = Some("MCP server removed".into());
                }
            }
            answer(client, app, open, Command::Settings).await;
        }
        Err(error) => editor_error(app, format!("not saved: {error:#}")),
    }
}

/// Shows `editor` in the settings. After a save the file was read again and may differ,
/// so the cursor and scroll are kept, on a char boundary.
fn open_editor(app: &mut App, mut editor: FileEditor, keep_place: bool) {
    let Some(Panel::Settings(view)) = &mut app.panel else {
        return;
    };
    if keep_place && let Some(old) = &view.editor {
        let mut cursor = old.text.cursor.min(editor.text.text.len());
        while !editor.text.text.is_char_boundary(cursor) {
            cursor -= 1;
        }
        editor.text.cursor = cursor;
        editor.scroll = old.scroll;
    }
    view.editor = Some(editor);
}

fn editor_error(app: &mut App, error: String) {
    if let Some(Panel::Settings(view)) = &mut app.panel
        && let Some(editor) = &mut view.editor
    {
        editor.error = Some(error);
    }
}
