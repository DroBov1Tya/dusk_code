//! A session the agent proposed: a quiet card with its title, where it would run, the
//! brief's first lines (the rest on a click), and Start session or Dismiss while it
//! waits. Start makes a new chat with the brief and shows it; nothing starts otherwise.

use dusk_protocol::thread::{Proposal, ProposalState};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    pages::chat::transcript::Transcript,
    ui::{Type, size, widgets},
    workspace::Workspace,
};

/// Lines of the brief shown before it is opened.
const BRIEF_LINES: usize = 3;

pub fn card(
    ws: &Workspace,
    shown: &str,
    call_id: &str,
    proposal: &Proposal,
    state: &ProposalState,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let theme = cx.theme();
    let (muted, foreground) = (theme.muted_foreground, theme.foreground);
    let key = format!("proposal-{shown}-{call_id}");
    let open_key = (shown.to_owned(), call_id.to_owned());
    let open = ws.briefs_open.contains(&open_key);
    let lines: Vec<&str> = proposal.brief.lines().collect();
    let folded = lines.len() > BRIEF_LINES && !open;
    let brief = match folded {
        true => format!("{}\u{2026}", lines[..BRIEF_LINES].join("\n")),
        false => proposal.brief.clone(),
    };
    let mut about = Vec::new();
    about.extend(proposal.mode.as_ref().map(|mode| format!("mode {mode}")));
    about.extend(
        proposal
            .branch
            .as_ref()
            .map(|branch| format!("branch {branch}")),
    );
    about.extend(proposal.project.as_ref().map(|dir| format!("in {dir}")));
    let mut body = widgets::card(cx)
        .gap(px(size::GAP_S))
        .w_full()
        .child(div().meta().text_color(muted).child("Proposed session"))
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child(proposal.title.clone()),
        )
        .when(!about.is_empty(), |card| {
            card.child(div().meta().text_color(muted).child(about.join(" \u{b7} ")))
        })
        .child(
            div()
                .id(SharedString::from(format!("{key}-brief")))
                .small()
                .text_color(foreground.opacity(0.85))
                .whitespace_normal()
                .child(brief)
                .when(lines.len() > BRIEF_LINES, |text| {
                    text.cursor_pointer()
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            if !ws.briefs_open.remove(&open_key) {
                                ws.briefs_open.insert(open_key.clone());
                            }
                            ws.measure_again = true;
                            cx.notify();
                        }))
                }),
        );
    let actions = div().flex().flex_row().items_center().gap(px(size::GAP_S));
    body = body.child(match state {
        ProposalState::Pending => {
            let (start, dismiss) = (
                (shown.to_owned(), call_id.to_owned()),
                (shown.to_owned(), call_id.to_owned()),
            );
            actions
                .child(
                    Button::new(SharedString::from(format!("{key}-start")))
                        .primary()
                        .label("Start session")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.start_proposal(&start.0, &start.1, cx)
                        })),
                )
                .child(
                    Button::new(SharedString::from(format!("{key}-dismiss")))
                        .ghost()
                        .label("Dismiss")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            ws.dismiss_proposal(&dismiss.0, &dismiss.1, cx)
                        })),
                )
        }
        ProposalState::Started { thread_id } => {
            let thread_id = thread_id.clone();
            actions
                .child(
                    div()
                        .meta()
                        .text_color(muted)
                        .child("Started as a new chat"),
                )
                .child(
                    Button::new(SharedString::from(format!("{key}-open")))
                        .ghost()
                        .small()
                        .label("Open")
                        .on_click(cx.listener(move |ws, _, _, cx| ws.show(thread_id.clone(), cx))),
                )
        }
        ProposalState::Dismissed => {
            actions.child(div().meta().text_color(muted).child("Dismissed"))
        }
    });
    body.into_any_element()
}

impl Workspace {
    /// Starts the proposed session and shows it, as a fork is shown.
    pub fn start_proposal(&mut self, thread_id: &str, call_id: &str, cx: &mut Context<Self>) {
        let (from, call_id) = (thread_id.to_owned(), call_id.to_owned());
        self.core.call(
            cx,
            move |client| async move {
                let threads = client.threads();
                let thread_id = threads.start_proposal(&from, &call_id).await?;
                let history = threads.messages(&thread_id).await?;
                anyhow::Ok((thread_id, history))
            },
            |ws, result, cx| match result {
                Ok((thread_id, history)) => {
                    ws.chats
                        .insert(thread_id.clone(), Transcript::from_history(&history));
                    ws.reload_threads(cx);
                    ws.show(thread_id, cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    pub fn dismiss_proposal(&mut self, thread_id: &str, call_id: &str, cx: &mut Context<Self>) {
        let (thread_id, call_id) = (thread_id.to_owned(), call_id.to_owned());
        self.core.call(
            cx,
            move |client| async move {
                client
                    .threads()
                    .dismiss_proposal(&thread_id, &call_id)
                    .await
            },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.fail(error, cx);
                }
            },
        );
    }
}
