//! Commands the core answers in a panel, and the update window.

use anyhow::Result;
use dusk_core::client::Client;
use dusk_protocol::{client::Command, release::InstallProgress};
use ratatui::DefaultTerminal;

use crate::app::{App, ModelList, Panel};
use crate::session::{answers::answer, run::InstallingUpdate, thread::OpenThread};
use crate::ui;

pub async fn command(
    client: &Client,
    app: &mut App,
    open: &mut Option<OpenThread>,
    terminal: &mut DefaultTerminal,
    command: Command,
) -> Result<()> {
    if let Command::Migrate { .. } = command {
        app.flash = Some("creating a private archive; backups are excluded".into());
        terminal.draw(|frame| ui::draw(frame, app))?;
    }
    // Checking providers takes a moment; the window says so meanwhile.
    if let Command::Update { .. } = command {
        let note = Some("asking GitHub for the newest version".into());
        app.panel = Some(Panel::Update {
            info: None,
            note,
            progress: None,
            button: None,
        });
        terminal.draw(|frame| ui::draw(frame, app))?;
    }
    if command == (Command::Model { name: None }) {
        let model = client.status.borrow().model.clone();
        app.panel = Some(Panel::Models(ModelList::checking(model)));
        terminal.draw(|frame| ui::draw(frame, app))?;
    }
    answer(client, app, open, command).await;
    Ok(())
}

pub fn install_update(
    client: &Client,
    app: &mut App,
    update: &mut Option<InstallingUpdate>,
    tag: String,
) {
    if update.is_none() {
        let result = Box::pin(client.install_update(&tag));
        *update = Some(InstallingUpdate { tag, result });
        if let Some(Panel::Update {
            progress,
            note,
            button,
            ..
        }) = &mut app.panel
        {
            *progress = Some(InstallProgress::default());
            *note = None;
            *button = None;
        }
    }
}

pub fn set_update_note(app: &mut App, text: String) {
    if let Some(Panel::Update { note, .. }) = &mut app.panel {
        *note = Some(text);
    }
}
