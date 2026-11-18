//! The Workshop's calls to the store and the skill packs. Each says what it is doing in
//! the window before the core answers, since the store is read over the network.

use anyhow::Result;
use dusk_core::client::Client;
use dusk_protocol::{
    client::Switch,
    plugin::{Listing, UpdateReport},
};
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

/// Updates what asks for nothing more, then says what was updated and what was left.
pub async fn update_all(
    client: &Client,
    app: &mut App,
    terminal: &mut DefaultTerminal,
) -> Result<()> {
    working(app, "updating everything that asks for nothing more".into());
    terminal.draw(|frame| ui::draw(frame, app))?;
    let step = match client.plugins().update_all().await {
        Ok(report) => Step::Done(said(&report)),
        Err(error) => Step::Failed(format!("not updated: {error:#}")),
    };
    finish(client, app, step).await;
    Ok(())
}

fn said(report: &UpdateReport) -> String {
    let mut parts = Vec::new();
    match report.updated.is_empty() {
        true => parts.push("nothing was updated".to_owned()),
        false => parts.push(format!("updated {}", report.updated.join(", "))),
    }
    for (name, why) in &report.skipped {
        parts.push(format!("{name} left for you: {why}"));
    }
    parts.join("; ")
}

/// Switches a pack or one of its skills, then reads the packs again.
pub async fn switch(client: &Client, app: &mut App, switch: Switch, on: bool) {
    if let Err(error) = client.settings().switch(switch, on).await {
        app.error(format!("cannot switch it: {error:#}"));
        return;
    }
    packs(client, app).await;
}

async fn packs(client: &Client, app: &mut App) {
    let result = client.catalog().skill_packs().await;
    match result {
        Ok(packs) => {
            if let Some(view) = workshop(app) {
                view.packs = Some(packs);
            }
        }
        Err(error) => app.error(format!("cannot read the skill packs: {error:#}")),
    }
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
    let Some(view) = workshop(app) else {
        return;
    };
    let (order, category) = (view.order, view.category.clone());
    let all = category.is_none();
    let result = client.plugins().store(order, category).await;
    packs(client, app).await;
    let Some(view) = workshop(app) else {
        return;
    };
    match result {
        Ok(list) => {
            if all {
                let mut categories: Vec<String> = list
                    .listings
                    .iter()
                    .map(|listing| listing.category.clone())
                    .filter(|category| !category.is_empty())
                    .collect();
                categories.sort();
                categories.dedup();
                view.categories = categories;
            }
            let rows = list.listings.len();
            view.listings = Some(list.listings);
            view.failed = list.failed;
            view.selected = view.selected.min(rows.saturating_sub(1));
            view.step = None;
        }
        Err(error) => view.step = Some(Step::Failed(format!("cannot read the store: {error:#}"))),
    }
}
