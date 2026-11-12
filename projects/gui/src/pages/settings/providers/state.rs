//! What the providers page remembers: the form, what each check found, which provider is being
//! connected anew.

use std::collections::HashMap;

use dusk_protocol::config::ProviderKind;
use gpui_kit::{component::input::InputState, *};

use super::check::Check;

pub struct State {
    pub(super) kind: ProviderKind,
    /// The list of kinds is unfolded under its button.
    pub(super) kinds_open: bool,
    pub(super) name: Entity<InputState>,
    pub(super) url: Entity<InputState>,
    pub(super) key: Entity<InputState>,
    /// What the last check of the form found.
    pub(super) form_check: Option<Check>,
    /// What the last check of each saved provider found, by name.
    pub(super) checks: HashMap<String, Check>,
    /// The provider whose key is being entered anew.
    pub(super) reconnecting: Option<String>,
    pub(super) reconnect_key: Entity<InputState>,
}

impl State {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        let field = |placeholder: &'static str, masked: bool, window: &mut Window, cx: &mut App| {
            cx.new(|cx| {
                InputState::new(window, cx)
                    .placeholder(placeholder)
                    .masked(masked)
            })
        };
        Self {
            kind: ProviderKind::default(),
            kinds_open: false,
            name: field("my-server", false, window, cx),
            url: field("https://host/v1", false, window, cx),
            key: field("Paste the key; empty for a local server", true, window, cx),
            form_check: None,
            checks: HashMap::new(),
            reconnecting: None,
            reconnect_key: field(
                "New key; empty connects again with the saved one",
                true,
                window,
                cx,
            ),
        }
    }
}
