//! The yes before the store changes this machine: what a plugin may do before it is
//! installed, and what is lost before one is removed.

use dusk_protocol::plugin::{Client, Kind, Listing};
use gpui_kit::{
    component::{
        ActiveTheme,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{ui::Type, workspace::Workspace};

#[derive(Clone)]
pub enum Confirm {
    /// The listing as the user saw it; its version is the one installed.
    Install(Box<Listing>),
    Remove(String),
}

impl Confirm {
    pub fn title(&self) -> String {
        match self {
            Self::Install(listing) => format!("{} {}", verb(listing), listing.name),
            Self::Remove(name) => format!("Remove {name}"),
        }
    }
}

/// "Update" when the source has a newer version than the one here, "Install" otherwise.
pub fn verb(listing: &Listing) -> &'static str {
    match (&listing.installed, &listing.update) {
        (Some(_), Some(_)) => "Update",
        _ => "Install",
    }
}

pub fn render(confirm: &Confirm, focus: &FocusHandle, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let (body, yes) = match confirm {
        Confirm::Install(listing) => (permissions(listing, cx), verb(listing)),
        Confirm::Remove(_) => (
            div().small().text_color(muted).child(
                "Its folder is deleted from this machine. It can be installed again from the Workshop.",
            ),
            "Remove",
        ),
    };
    let button = Button::new("store-confirm").label(yes);
    let button = match confirm {
        Confirm::Install(_) => button.primary(),
        // Solid when the theme sets `button.danger.*`, the kit's pale tint otherwise.
        Confirm::Remove(_) => button.danger(),
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

/// What the plugin may do, one line each, as the store lists it.
fn permissions(listing: &Listing, cx: &App) -> Div {
    let (muted, warning) = (cx.theme().muted_foreground, cx.theme().warning);
    let mut body = div().flex().flex_col().gap_2();
    body = if listing.permissions.is_empty() {
        body.child(
            div()
                .body()
                .child("It asks for nothing beyond its own folder."),
        )
    } else {
        body.child(
            div()
                .body()
                .child(format!("{} {} may:", listing.name, listing.version)),
        )
    };
    for line in &listing.permissions {
        body = body.child(div().small().pl_3().child(format!("• {line}")));
    }
    if listing.kind != Kind::Plugin {
        body =
            body.child(div().meta().text_color(muted).child(
                "Its skills and agents are text every thread may read; they run no programs.",
            ));
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
