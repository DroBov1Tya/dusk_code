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

/// The tab over the upper edge of whatever stands on top, the band or the box: `under`
/// is that one's color and `border` its edge's. A later sibling of both, so it paints
/// over their edge.
pub fn tab(
    ws: &Workspace,
    under: Hsla,
    border: Hsla,
    window: &Window,
    cx: &mut Context<Workspace>,
) -> Option<Div> {
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
        .bg(under)
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
    Some(
        div()
            .absolute()
            .top(px(1. - TAB.1))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .child(handle),
    )
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
/// click shows it in the file manager.
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
    let tip = SharedString::from(format!(
        "{}: {}",
        crate::columns::inspector::environment::reveal_label(),
        dir.display()
    ));
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
        .on_click(cx.listener(move |_, _, _, cx| cx.reveal_path(&dir)))
}

/// The panel's rings, left to right. The provider's rate limit windows (5-hour, weekly)
/// join here as meters of their own once the core reports them one by one.
pub fn meters(ws: &Workspace) -> Vec<Meter> {
    let mut meters = vec![context(ws)];
    // Until then, the tightest of the reported windows stands for them all.
    if !ws.status.chatgpt_model
        && let Some(limits) = ws.status.limits.as_ref()
        && let Some(used) = limits.used()
    {
        meters.push(Meter {
            label: "Rate limits".into(),
            used,
            detail: limits.text(),
        });
    }
    meters
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
        cx.notify();
    }
}
