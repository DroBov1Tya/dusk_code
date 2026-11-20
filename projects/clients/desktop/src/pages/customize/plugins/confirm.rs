//! The yes before the store changes this machine: what a plugin may do before it is
//! installed, and what is lost before one is removed.

use dusk_protocol::{
    catalog::DiffKind,
    plugin::{Client, ConnectorState, Kind, Listing, Tier},
};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable,
        button::{Button, ButtonVariants},
    },
    *,
};

use super::workshop::{Reviewed, connectors};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

#[derive(Clone)]
pub enum Confirm {
    /// The listing as the user saw it; its version is the one installed.
    Install(Box<Listing>),
    /// The name, and what it is, which says what goes.
    Remove(String, Kind),
}

impl Confirm {
    pub fn title(&self) -> String {
        match self {
            Self::Install(listing) => format!("{} {}", verb(listing), listing.name),
            Self::Remove(name, _) => format!("Remove {name}"),
        }
    }
}

fn group(title: &'static str, cx: &App) -> Div {
    div()
        .pt_1()
        .group_name()
        .text_color(cx.theme().muted_foreground)
        .child(title)
}

/// "Update" when the source has a newer version than the one here, "Add" for a
/// connector, "Install" otherwise.
pub fn verb(listing: &Listing) -> &'static str {
    match (&listing.installed, &listing.update) {
        (Some(_), Some(_)) => "Update",
        _ if listing.kind == Kind::Connector => "Add",
        _ => "Install",
    }
}

pub fn render(
    confirm: &Confirm,
    review: Option<&Reviewed>,
    focus: &FocusHandle,
    cx: &mut Context<Workspace>,
) -> Div {
    let muted = cx.theme().muted_foreground;
    let (body, yes) = match confirm {
        Confirm::Install(listing) if super::workshop::reviewed_first(listing) => {
            (unverified(listing, review, cx), verb(listing))
        }
        Confirm::Install(listing) => (permissions(listing, cx), verb(listing)),
        Confirm::Remove(_, kind) => (
            div().small().text_color(muted).child(match kind {
                Kind::Connector => {
                    "Its server leaves your connectors, unless you changed it since. It can be added again from the Workshop."
                }
                _ => {
                    "Its folder is deleted from this machine. It can be installed again from the Workshop."
                }
            }),
            "Remove",
        ),
    };
    // An Unverified install waits for the files it names to be shown.
    let waiting = matches!(confirm, Confirm::Install(listing) if super::workshop::reviewed_first(listing))
        && !matches!(review, Some(Reviewed::Read(review)) if review.refused.is_none());
    let button = Button::new("store-confirm").label(yes).disabled(waiting);
    let button = match confirm {
        Confirm::Install(_) => button.primary(),
        // Solid when the theme sets `button.danger.*`, the kit's pale tint otherwise.
        Confirm::Remove(..) => button.danger(),
    };
    div().flex().flex_col().gap_4().p_5().child(body).child(
        div()
            .flex()
            .flex_row()
            .justify_end()
            .gap_2()
            .child(
                Button::new("store-cancel")
                    .ghost()
                    .track_focus(focus)
                    .label("Cancel")
                    .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
            )
            .child(button.on_click(cx.listener(|ws, _, _, cx| ws.store_confirmed(cx)))),
    )
}

/// What the install adds, in three groups: what runs on this machine or sends data
/// elsewhere, what else it may reach, and what it adds for the agent. The lines are the
/// listing's own; the core refuses the install if they changed since they were shown.
fn permissions(listing: &Listing, cx: &App) -> Div {
    let theme = cx.theme();
    let (muted, warning) = (theme.muted_foreground, theme.warning);
    let mono = theme.mono_font_family.clone();
    let mut body = div().flex().flex_col().gap_2();
    let runs: Vec<(&str, String)> = listing
        .programs
        .iter()
        .map(|line| (connectors::RUNS_HERE, line.clone()))
        .chain(listing.connectors.iter().map(|connector| {
            (
                connectors::can(&connector.runs),
                format!(
                    "{}: {}",
                    connector.name,
                    connectors::target(&connector.runs)
                ),
            )
        }))
        .collect();
    let reach = match listing.kind {
        Kind::Plugin | Kind::Connector => listing.reach(),
        _ => &listing.permissions[..],
    };
    if runs.is_empty() && reach.is_empty() && listing.skills.is_empty() {
        body = body.child(
            div()
                .body()
                .child("It asks for nothing beyond its own folder."),
        );
    } else {
        body = body.child(
            div()
                .body()
                .child(format!("{} {} adds:", listing.name, listing.version)),
        );
    }
    if !runs.is_empty() {
        let remote = listing.programs.is_empty()
            && listing
                .connectors
                .iter()
                .all(|connector| matches!(connector.runs, dusk_protocol::plugin::Runs::Url(_)));
        let title = match remote {
            true => "Connects to",
            false => "Runs on your machine",
        };
        body = body.child(group(title, cx));
        for (can, line) in runs {
            body = body.child(
                div()
                    .flex()
                    .flex_col()
                    .pl_3()
                    .child(div().small().child(format!("\u{2022} {can}")))
                    .child(
                        div()
                            .pl_3()
                            .small()
                            .font_family(mono.clone())
                            .text_color(muted)
                            .child(super::workshop::clip(&line, 200)),
                    ),
            );
        }
        for connector in &listing.connectors {
            if connector.state == ConnectorState::Yours {
                body = body.child(div().meta().text_color(muted).pl_3().child(format!(
                    "Your connector {} stays; the plugin's of that name is not used.",
                    connector.name
                )));
            }
        }
    }
    if !reach.is_empty() {
        body = body.child(group("May reach", cx));
        for line in reach {
            body = body.child(div().small().pl_3().child(format!("\u{2022} {line}")));
        }
    }
    if !listing.skills.is_empty() {
        body = body.child(group("Adds for the agent", cx)).child(
            div()
                .small()
                .pl_3()
                .child(widgets::count(listing.skills.len(), "skill"))
                .child(
                    div()
                        .text_color(muted)
                        .child(super::workshop::clip(&listing.skills.join(", "), 300)),
                ),
        );
    }
    if let Some(server) = &listing.server {
        if !server.secrets.is_empty() {
            body = body.child(div().small().text_color(warning).child(format!(
                "It is added switched off. Put your {} in its settings under Connectors, then switch it on.",
                server.secrets.join(", ")
            )));
        }
        if let Some(docs) = &server.docs {
            body = body.child(div().meta().text_color(muted).child(format!(
                "The service's page about it: {}",
                super::workshop::clip(docs, 200)
            )));
        }
    } else if listing.kind != Kind::Plugin {
        body =
            body.child(div().meta().text_color(muted).child(
                "Its skills and agents are text every thread may read; they run no programs.",
            ));
        if listing.tier == Tier::Community {
            body = body.child(
                div()
                    .small()
                    .text_color(warning)
                    .child("Instructions the agent will follow. Nobody checked them."),
            );
        }
    }
    let source = match listing.source.as_str() {
        "" => "duskcode",
        source => source,
    };
    body = body.child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_1()
            .meta()
            .text_color(muted)
            // The row's gap spaces its parts, so no brackets around the mark.
            .child(format!("From {source} ·"))
            .child(super::workshop::tier::mark(listing.tier, cx)),
    );
    if listing.clash {
        body = body.child(div().small().text_color(warning).child(
            "Another source offers the same name. A mirror may copy a name to pass for another source's listing; check that this is the one you mean.",
        ));
    }
    if !listing.clients.is_empty() && !listing.clients.contains(&Client::Gui) {
        body = body.child(
            div()
                .meta()
                .text_color(muted)
                .child("Its panels do not show in this app."),
        );
    }
    body
}

/// The yes for a listing nobody vouches for: where it comes from as far as anyone can
/// tell, what it may make the agent do, and every file it writes, with what an update
/// changes in the copy here.
fn unverified(listing: &Listing, review: Option<&Reviewed>, cx: &App) -> Div {
    let theme = cx.theme();
    let (muted, warning, danger) = (theme.muted_foreground, theme.warning, theme.danger);
    let mono = theme.mono_font_family.clone();
    let mut body = div().flex().flex_col().gap_2();
    let from = match &listing.from {
        Some(host) => format!("{} ({host})", listing.source),
        None => listing.source.clone(),
    };
    body = body.child(div().body().text_color(warning).child(format!(
        "Nobody vouches for {}. It comes from {from}, a source you added without a key: nobody signed its list and nobody checked these files.",
        listing.name
    )));
    if let Some(upstream) = &listing.upstream {
        body = body.child(
            div()
                .small()
                .text_color(muted)
                .child(format!("Its files: {upstream}.")),
        );
    }
    if !listing.author.is_empty() {
        body = body.child(div().small().text_color(muted).child(format!(
            "The source says it is by {}; nothing checks that.",
            super::workshop::clip(&listing.author, 64)
        )));
    }
    let review = match review {
        None | Some(Reviewed::Reading) => {
            return body.child(widgets::muted("Reading its files...", cx).px_0());
        }
        Some(Reviewed::Failed(why)) => {
            return body.child(div().small().text_color(danger).child(why.clone()));
        }
        Some(Reviewed::Read(review)) => review,
    };
    if let Some(why) = &review.refused {
        return body.child(div().small().text_color(danger).child(why.clone()));
    }
    if review.agent {
        let tools = match &review.agent_tools {
            None => "every tool".to_owned(),
            Some(tools) if tools.is_empty() => "no tools".to_owned(),
            Some(tools) => tools.join(", "),
        };
        body = body.child(div().small().text_color(warning).child(format!(
            "It is an agent: its AGENT.md becomes a chat's instructions, with {tools}."
        )));
    } else {
        body = body.child(
            div()
                .small()
                .child("Its text becomes instructions the agent may follow."),
        );
    }
    if !review.scripts.is_empty() {
        body = body.child(div().small().text_color(warning).child(format!(
            "It carries {} the agent may run, under the permission mode you chose: {}.",
            widgets::count(review.scripts.len(), "script"),
            super::workshop::clip(&review.scripts.join(", "), 300)
        )));
    }
    let lists = [
        ("Added", &review.added),
        ("Changed", &review.changed),
        ("Removed", &review.removed),
    ];
    let update = !review.changed.is_empty() || !review.removed.is_empty();
    for (title, paths) in lists {
        if paths.is_empty() || (!update && title != "Added") {
            continue;
        }
        let title = match update {
            true => title,
            false => "Files",
        };
        body = body.child(div().pt_1().group_name().text_color(muted).child(title));
        for path in paths.iter().take(40) {
            body = body.child(
                div()
                    .pl_3()
                    .small()
                    .font_family(mono.clone())
                    .child(super::workshop::clip(path, 120)),
            );
        }
        if paths.len() > 40 {
            body = body.child(
                div()
                    .pl_3()
                    .meta()
                    .text_color(muted)
                    .child(format!("and {} more", paths.len() - 40)),
            );
        }
    }
    if let Some(path) = &review.diff_path {
        body = body.child(
            div()
                .pt_1()
                .group_name()
                .text_color(muted)
                .child(format!("What changes in {path}")),
        );
        for line in review
            .diff
            .iter()
            .filter(|line| line.kind != DiffKind::Same)
            .take(60)
        {
            let (mark, color) = match line.kind {
                DiffKind::Added => ("+", theme.success),
                _ => ("-", danger),
            };
            body = body.child(
                div()
                    .pl_3()
                    .small()
                    .font_family(mono.clone())
                    .text_color(color)
                    .child(format!("{mark} {}", super::workshop::clip(&line.text, 160))),
            );
        }
    }
    body.child(
        div()
            .meta()
            .text_color(muted)
            .child("Installing it is your call: what it asks of the agent is yours to accept."),
    )
}
