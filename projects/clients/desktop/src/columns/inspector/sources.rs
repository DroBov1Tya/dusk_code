//! The shown chat's sources: the files and long pastes attached to it. Each goes to the
//! model once, with the turn it came in; a pinned one goes with every request until it is
//! unpinned. The pin and the removal are the core's; this only shows them.

use dusk_protocol::client::{Answer, Command};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    if ws.sources.is_empty() {
        return div().child(widgets::muted("Nothing is attached to this chat.", cx));
    }
    let theme = cx.theme();
    let (accent, muted, surface) = (theme.primary, theme.muted_foreground, theme.muted);
    let mut column = div().flex().flex_col().gap_1().p_2();
    for source in &ws.sources {
        let (id, pinned) = (source.source_id.clone(), source.pinned);
        let remove = id.clone();
        let state = if pinned {
            "sent with every request"
        } else if source.delivered {
            "sent once; the model reads it again by its path"
        } else {
            "goes with the next request"
        };
        column = column.child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_2()
                .p_2()
                .rounded_md()
                .bg(surface)
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .flex()
                        .flex_col()
                        .child(div().small().truncate().child(source.name.clone()))
                        .child(
                            div()
                                .meta()
                                .text_color(if pinned { accent } else { muted })
                                .child(format!(
                                    "{} \u{00B7} {state}",
                                    widgets::short(source.bytes)
                                )),
                        ),
                )
                .child(
                    Button::new(SharedString::from(format!("pin-{id}")))
                        .small()
                        .label(if pinned { "Every time" } else { "Once" })
                        .tooltip(if pinned {
                            "Send it once again only"
                        } else {
                            "Send it with every request"
                        })
                        .map(|button| {
                            if pinned {
                                button.primary()
                            } else {
                                button.ghost()
                            }
                        })
                        .on_click(cx.listener(move |ws, _, _, cx| ws.pin_source(&id, !pinned, cx))),
                )
                .child(
                    Button::new(SharedString::from(format!("drop-{remove}")))
                        .ghost()
                        .small()
                        .icon(IconName::Close)
                        .tooltip("Take it out of the chat")
                        .on_click(cx.listener(move |ws, _, _, cx| ws.remove_source(&remove, cx))),
                ),
        );
    }
    column
}

impl Workspace {
    pub fn load_sources(&mut self, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| async move {
                let command = Command::Sources {
                    thread_id: Some(thread_id.clone()),
                    add: None,
                };
                anyhow::Ok((thread_id, client.command(command).await?))
            },
            |ws, result, cx| match result {
                // An answer for a chat left meanwhile is not this chat's.
                Ok((thread_id, Answer::Sources(sources))) => {
                    if ws.shown.as_deref() == Some(thread_id.as_str()) {
                        ws.sources = sources;
                        cx.notify();
                    }
                }
                Ok(_) => {}
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn pin_source(&mut self, id: &str, pinned: bool, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let id = id.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.work().pin_source(&thread_id, &id, pinned).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_sources(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn remove_source(&mut self, id: &str, cx: &mut Context<Self>) {
        let Some(thread_id) = self.shown.clone() else {
            return;
        };
        let id = id.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.work().remove_source(&thread_id, &id).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_sources(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
