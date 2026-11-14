//! The yes before the store changes this machine: what a plugin may do before it is
//! installed, and what is lost before one is removed.

use dusk_protocol::plugin::{Client, Listing};
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
    Install(Listing),
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

/// "Update" when another version of it is installed, "Install" otherwise.
pub fn verb(listing: &Listing) -> &'static str {
    match &listing.installed {
        Some(version) if *version != listing.version => "Update",
        _ => "Install",
    }
}

pub fn render(confirm: &Confirm, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let (body, yes) = match confirm {
        Confirm::Install(listing) => (permissions(listing, muted), verb(listing)),
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
                    .label("Cancel")
                    .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
            )
            .child(button.on_click(cx.listener(|ws, _, _, cx| ws.store_confirmed(cx)))),
    )
}

/// What the plugin may do, one line each, as the store lists it.
fn permissions(listing: &Listing, muted: Hsla) -> Div {
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
