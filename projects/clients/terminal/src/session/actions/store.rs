//! The Workshop's calls to the plugin store. Each says what it is doing in the window
//! before the core answers, since the store is read over the network.

use anyhow::Result;
use dusk_core::client::Client;
use dusk_protocol::plugin::Listing;
use ratatui::DefaultTerminal;

use crate::app::{App, Panel, Step, Workshop};
use crate::ui;

pub async fn load(client: &Client, app: &mut App, terminal: &mut DefaultTerminal) -> Result<()> {
    if let Some(view) = workshop(app) {
        view.step = None;
        view.listings = None;
    }
    terminal.draw(|frame| ui::draw(frame, app))?;
    read(client, app).await;
    Ok(())
}

pub async fn install(
    client: &Client,
    app: &mut App,
    terminal: &mut DefaultTerminal,
    listing: &Listing,
) -> Result<()> {
    let (name, version) = (&listing.name, &listing.version);
    working(app, format!("installing {name} {version}"));
    terminal.draw(|frame| ui::draw(frame, app))?;
    let step = match client.plugins().install(listing).await {
        Ok(()) => Step::Done(format!("{name} {version} installed and switched on")),
        Err(error) => Step::Failed(format!("{name} not installed: {error:#}")),
    };
    finish(client, app, step).await;
    Ok(())
}

pub async fn remove(
    client: &Client,
    app: &mut App,
    terminal: &mut DefaultTerminal,
    name: String,
) -> Result<()> {
    working(app, format!("removing {name}"));
    terminal.draw(|frame| ui::draw(frame, app))?;
    let step = match client.plugins().remove(&name).await {
        Ok(()) => Step::Done(format!("{name} removed")),
        Err(error) => Step::Failed(format!("{name} not removed: {error:#}")),
    };
    finish(client, app, step).await;
    Ok(())
}

fn workshop(app: &mut App) -> Option<&mut Workshop> {
    match &mut app.panel {
        Some(Panel::Workshop(view)) => Some(view),
        _ => None,
    }
}

fn working(app: &mut App, what: String) {
    if let Some(view) = workshop(app) {
        view.confirm = None;
        view.step = Some(Step::Working(what));
    }
}

/// Reads the list again so its states are current, then says how the step went; a failed
/// step outranks a failed read.
async fn finish(client: &Client, app: &mut App, step: Step) {
    read(client, app).await;
    if let Some(view) = workshop(app)
        && (matches!(step, Step::Failed(_)) || view.step.is_none())
    {
        view.step = Some(step);
    }
}

async fn read(client: &Client, app: &mut App) {
    let result = client.plugins().store().await;
    let Some(view) = workshop(app) else {
        return;
    };
    match result {
        Ok(listings) => {
            view.selected = view.selected.min(listings.len().saturating_sub(1));
            view.listings = Some(listings);
            view.step = None;
        }
        Err(error) => view.step = Some(Step::Failed(format!("cannot read the store: {error:#}"))),
    }
}
