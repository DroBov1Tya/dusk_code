//! What the update brought: shown once, on the first start of a new version. The notes
//! are the changelog built into the program, so it needs no network.

use gpui_kit::{
    component::{ActiveTheme, button::Button, button::ButtonVariants},
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// An entry may start with where it applies, as in "GUI: the sky stays up".
const AREA_CHARS: usize = 28;

pub fn render(from: Option<&str>, cx: &mut Context<Workspace>) -> Div {
    let notes = dusk_core::app::notes_since(from);
    let theme = cx.theme();
    let (muted, accent, success, warning) = (
        theme.muted_foreground,
        theme.primary,
        theme.success,
        theme.warning,
    );
    let count = |number: usize, name: &'static str, color: Hsla| {
        div()
            .flex()
            .flex_col()
            .items_center()
            .min_w(px(84.))
            .child(
                div()
                    .text_size(rems(1.9))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if number == 0 { muted } else { color })
                    .child(number.to_string()),
            )
            .child(div().small().text_color(muted).child(name))
    };
    let updated = match from {
        Some(from) => format!("Updated from {from}"),
        None => "Updated".to_owned(),
    };
    let head = div()
        .flex()
        .flex_col()
        .items_center()
        .gap_1()
        .pt_3()
        .child(
            div()
                .text_size(rems(1.7))
                .font_weight(FontWeight::SEMIBOLD)
                .child(format!("duskcode {}", dusk_core::app::VERSION)),
        )
        .child(div().body().text_color(muted).child(updated))
        .child(
            div()
                .flex()
                .flex_row()
                .gap_4()
                .pt_4()
                .child(count(notes.added.len(), "New", accent))
                .child(count(notes.changed.len(), "Changed", warning))
                .child(count(notes.fixed.len(), "Fixed", success)),
        );
    let mut body = div().flex().flex_col().gap_4().p_5().child(head);
    let groups = [
        ("New", &notes.added, accent),
        ("Changed", &notes.changed, warning),
        ("Fixed", &notes.fixed, success),
    ];
    for (name, entries, color) in groups {
        if entries.is_empty() {
            continue;
        }
        let mut group = div()
            .flex()
            .flex_col()
            .gap_2()
            .child(widgets::caption(name, cx).px_0());
        for entry in entries {
            let (area, text) = split(entry);
            // The first sentence says what changed; the rest is detail, set quieter.
            let (lead, rest) = match text.split_once(". ") {
                Some((lead, rest)) => (format!("{lead}."), Some(rest.to_owned())),
                None => (text, None),
            };
            group = group.child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap_2p5()
                    .child(
                        div()
                            .mt(px(7.))
                            .size(px(6.))
                            .flex_none()
                            .rounded_full()
                            .bg(color.opacity(0.8)),
                    )
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .body()
                            .when_some(area, |line, area| {
                                line.child(
                                    div()
                                        .small()
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(muted)
                                        .child(area),
                                )
                            })
                            .child(lead)
                            .when_some(rest, |line, rest| {
                                line.child(div().small().text_color(muted).child(rest))
                            }),
                    ),
            );
        }
        body = body.child(group);
    }
    if notes.is_empty() {
        body = body.child(
            div()
                .flex()
                .justify_center()
                .child(widgets::muted("This build has no release notes yet.", cx)),
        );
    }
    body.child(
        div().flex().flex_row().justify_end().child(
            Button::new("whats-new-done")
                .primary()
                .label("Done")
                .on_click(cx.listener(|ws, _, _, cx| ws.close_modal(cx))),
        ),
    )
}

/// Where an entry applies, when it says so, and the entry without the code marks.
fn split(entry: &str) -> (Option<String>, String) {
    let plain = entry.replace('`', "");
    match plain.split_once(": ") {
        Some((area, text)) if area.len() <= AREA_CHARS && !area.contains('.') => {
            (Some(area.to_owned()), widgets::capitalize(text))
        }
        _ => (None, plain),
    }
}
