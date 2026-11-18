//! The composer's panel: a small tab on the box's upper edge, and the band it opens, glued
//! on top of the box. The band shows where the chat works on the left and its meters on
//! the right. Open or closed is the window's, never the core's.

use gpui_kit::{
    component::{ActiveTheme, Colorize, Icon, IconName, Sizable},
    *,
};

use crate::{
    pages::chat::ring::{self, Meter},
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The tab's width and height; its lower pixel covers the edge it stands on.
pub const TAB: (f32, f32) = (36., 14.);
const RING: f32 = 16.;

/// The band's color: a shade under the box's, so the two read as one piece.
pub fn surface(cx: &App) -> Hsla {
    cx.theme().muted.darken(0.06)
}

/// How long the tab takes to come and go.
const TAB_FADE: std::time::Duration = std::time::Duration::from_millis(140);

/// The tab over the upper edge of whatever stands on top, the band or the box, in the
/// band's color: it is the band's handle. `border` is the edge's color. A later sibling
/// of both, so it paints over their edge. It is in sight only while the pointer is over
/// the chat, the panel is open or the keyboard is on it; out of sight it is still there,
/// so Tab reaches it and it shows itself.
pub fn tab(
    ws: &Workspace,
    border: Hsla,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Option<AnimationElement<Div>> {
    ws.shown.as_ref()?;
    let open = ws.dock.panel_open;
    let theme = cx.theme();
    let (muted, ring) = (theme.muted_foreground, theme.ring);
    let focused = ws.dock.panel_focus.is_focused(window);
    let handle = div()
        .id("composer-tab")
        .track_focus(&ws.dock.panel_focus)
        .role(Role::Button)
        .aria_label(if open {
            "Close the panel"
        } else {
            "Open the panel"
        })
        .w(px(TAB.0))
        .h(px(TAB.1))
        .flex()
        .items_center()
        .justify_center()
        .rounded_t_md()
        .border_1()
        .border_b_0()
        .border_color(if focused { ring } else { border })
        .bg(surface(cx))
        .cursor_pointer()
        .child(
            Icon::new(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronUp
            })
            .xsmall()
            .text_color(muted),
        )
        .on_key_down(cx.listener(|ws, event: &KeyDownEvent, _, cx| {
            if matches!(event.keystroke.key.as_str(), "enter" | "space") {
                cx.stop_propagation();
                ws.toggle_panel(cx);
            }
        }))
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_panel(cx)));
    let shown = open || focused || ws.dock.chat_hovered;
    let seen = ws.dock.tab_seen;
    let tab = div()
        .absolute()
        .top(px(1. - TAB.1))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(handle);
    // A new name each way, so the fade plays when the state turns and only then.
    Some(tab.with_animation(
        if shown {
            "composer-tab-in"
        } else {
            "composer-tab-out"
        },
        Animation::new(TAB_FADE),
        move |tab, delta| {
            tab.opacity(match (shown, seen) {
                (true, _) => delta,
                (false, true) => 1. - delta,
                // Never in sight yet: nothing to fade away as the chat opens.
                (false, false) => 0.,
            })
        },
    ))
}

/// The band on top of the box, as wide as it, sharing its edge: the project's folder on
/// the left, the meters on the right. `border` is the box's edge color.
pub fn panel(
    ws: &Workspace,
    border: Hsla,
    cx: &mut Context<Workspace>,
) -> Option<AnimationElement<Div>> {
    if !ws.dock.panel_open {
        return None;
    }
    ws.shown.as_ref()?;
    let meters = meters(ws);
    let band = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded_t_2xl()
        .border_1()
        .border_b_0()
        .border_color(border)
        .bg(surface(cx))
        .child(folder(ws, cx))
        .child(div().flex_1())
        .children(
            meters
                .iter()
                .enumerate()
                .map(|(index, meter)| ring::meter(index, meter, RING, cx)),
        );
    Some(widgets::unfold("composer-panel-open", band))
}

/// The chat's project folder, or the workspace root for a chat without a project; a
/// click opens it in the file manager.
fn folder(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, raised) = (theme.muted_foreground, theme.list_hover);
    let dir = ws
        .selected_project()
        .map_or_else(|| ws.home.clone(), |project| project.dir.clone());
    let name = dir.file_name().map_or_else(
        || dir.display().to_string(),
        |name| name.to_string_lossy().into_owned(),
    );
    let tip = SharedString::from(format!("Open {}", dir.display()));
    div()
        .id("composer-folder")
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .min_w_0()
        .px_1()
        .rounded_md()
        .cursor_pointer()
        .hover(move |link| link.bg(raised))
        .child(Icon::new(IconName::Folder).xsmall().text_color(muted))
        .child(
            div()
                .min_w_0()
                .truncate()
                .meta()
                .text_color(muted)
                .child(name),
        )
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
        .on_click(cx.listener(move |_, _, _, cx| cx.open_with_system(&dir)))
}

/// The panel's rings, left to right: the context, then each window the provider reported,
/// the plan's first and then the per-minute requests and tokens. A new meter joins here.
pub fn meters(ws: &Workspace) -> Vec<Meter> {
    let mut meters = vec![context(ws)];
    let Some(limits) = ws.status.limits.as_ref() else {
        return meters;
    };
    meters.extend(limits.plan.iter().map(|window| Meter {
        label: plan_label(window.minutes).into(),
        used: f32::from(window.used_percent) / 100.,
        detail: window.resets_in.map_or_else(
            || "last reported by the provider".to_owned(),
            |seconds| format!("resets in {}", words(seconds)),
        ),
    }));
    for (label, limit) in [("Requests", &limits.requests), ("Tokens", &limits.tokens)] {
        let Some(limit) = limit.as_ref().filter(|limit| limit.limit > 0) else {
            continue;
        };
        let mut detail = format!(
            "{} of {} left this minute",
            widgets::short(limit.remaining),
            widgets::short(limit.limit)
        );
        if let Some(seconds) = limit.resets_in {
            detail.push_str(&format!(", resets in {}", words(seconds)));
        }
        meters.push(Meter {
            label: label.into(),
            used: 1. - limit.remaining as f32 / limit.limit as f32,
            detail,
        });
    }
    meters
}

/// A plan window's name from its length.
fn plan_label(minutes: Option<u64>) -> String {
    match minutes {
        Some(300) => "5 hours".into(),
        Some(10_080) => "Week".into(),
        Some(minutes) if minutes.is_multiple_of(1440) => format!("{} d", minutes / 1440),
        Some(minutes) if minutes.is_multiple_of(60) => format!("{} h", minutes / 60),
        Some(minutes) => format!("{minutes} min"),
        None => "Plan".into(),
    }
}

fn words(seconds: u64) -> String {
    let ms = i64::try_from(seconds).map_or(i64::MAX, |s| s.saturating_mul(1000));
    dusk_protocol::thread::duration_words(ms)
}

/// How full the context is; the counts and the cost in its detail.
fn context(ws: &Workspace) -> Meter {
    let (used, limit) = ws
        .shown_chat()
        .and_then(|chat| chat.tokens)
        .unwrap_or((0, ws.status.input_limit));
    let share = if limit == 0 {
        0.
    } else {
        (used as f32 / limit as f32).min(1.)
    };
    let mut detail = format!(
        "{} of {} tokens",
        widgets::short(used),
        widgets::short(limit)
    );
    if let Some(cost) = ws.shown_chat().and_then(|chat| chat.cost.clone()) {
        detail.push_str(&format!(" \u{00B7} {cost}"));
    }
    Meter {
        label: "Context".into(),
        used: share,
        detail,
    }
}

impl Workspace {
    pub fn toggle_panel(&mut self, cx: &mut Context<Self>) {
        if self.shown.is_none() {
            return;
        }
        self.dock.panel_open = !self.dock.panel_open;
        self.dock.tab_seen = true;
        cx.notify();
    }
}
