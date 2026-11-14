//! The text boxes of open views. A box needs state that outlives one draw, so each input
//! widget gets one, kept by its part and id; a new view that brings a different value
//! puts it in the box, and Enter sends what is typed.

use dusk_protocol::plugin::{UiEvent, Widget};
use gpui_kit::{
    component::input::{InputEvent, InputState},
    *,
};

use super::{Key, Slot};
use crate::workspace::Workspace;

pub struct Field {
    pub state: Entity<InputState>,
    /// The value the view gave last, to tell when a new view changes it.
    given: String,
    _enter: Subscription,
}

/// Gives every input of the open views its box, and the value a new view brought.
pub fn settle(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    let mut wanted = Vec::new();
    for (key, root) in &ws.plugins.views {
        super::view::each(root, &mut |widget| {
            if let Widget::Input {
                id,
                placeholder,
                value,
            } = widget
            {
                wanted.push((
                    (key.clone(), id.clone()),
                    placeholder.clone(),
                    value.clone(),
                ));
            }
        });
    }
    let live: std::collections::HashSet<&Slot> = wanted.iter().map(|(slot, ..)| slot).collect();
    ws.plugins.fields.retain(|slot, _| live.contains(slot));
    for (slot, placeholder, value) in wanted {
        match ws.plugins.fields.get_mut(&slot) {
            Some(field) if field.given != value => {
                field.given.clone_from(&value);
                field
                    .state
                    .update(cx, |state, cx| state.set_value(value, window, cx));
            }
            Some(_) => {}
            None => {
                let field = new_field(slot.0.clone(), &slot.1, placeholder, value, window, cx);
                ws.plugins.fields.insert(slot, field);
            }
        }
    }
}

fn new_field(
    key: Key,
    id: &str,
    placeholder: String,
    value: String,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> Field {
    let state = cx.new(|cx| {
        let mut state = InputState::new(window, cx).placeholder(placeholder);
        state.set_value(value.clone(), window, cx);
        state
    });
    let id = id.to_owned();
    let enter = cx.subscribe_in(
        &state,
        window,
        move |ws, state, event: &InputEvent, _, cx| {
            if let InputEvent::PressEnter { .. } = event {
                let value = state.read(cx).value().to_string();
                ws.plugin_event(&key, &id, UiEvent::Input { value }, cx);
            }
        },
    );
    Field {
        state,
        given: value,
        _enter: enter,
    }
}
