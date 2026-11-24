//! The composer's panel: a small tab on the box's upper edge, and the band it opens, glued
//! on top of the box. The band shows where the chat works on the left and the plan's
//! limits on the right. Open or closed is the window's, never the core's.

use crate::ui::elevation::{Elevation, Level};
use crate::ui::press::Press;
use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, Sizable},
    *,
};

use crate::{
    pages::chat::ring,
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The tab's width and height; its lower pixel covers the edge it stands on.
pub const TAB: (f32, f32) = (36., 14.);
const RING: f32 = 16.;

/// The band's color, the theme's: a shade beside the box's, so the two read as one piece.
pub fn surface(cx: &App) -> Hsla {
    crate::ui::theme::band(cx)
}

/// How long the tab takes to rise out of the edge and sink back into it.
const TAB_SLIDE: std::time::Duration = std::time::Duration::from_millis(160);

/// The tab over the upper edge of whatever stands on top, the band or the box, in the
/// band's color: it is the band's handle. `border` is the edge's color. A later sibling
/// of both, so it paints over their edge. It is in sight only while the pointer is over
/// the box, the panel is open or the keyboard is on it; out of sight it is still there,
/// sunk below the edge, so Tab reaches it and it shows itself.
pub fn tab(
    ws: &Workspace,
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
    let shown = open || focused || ws.dock.box_hovered;
    let seen = ws.dock.tab_seen;
    // A new name each way, so the slide plays when the state turns and only then. Only
    // the handle moves, inside a clip of fixed size, so the stack's layout (and with it
    // the hover) never changes under the pointer.
    let slide = div().relative().child(handle).with_animation(
        if shown {
            "composer-tab-in"
        } else {
            "composer-tab-out"
        },
        Animation::new(TAB_SLIDE).with_easing(ease_in_out),
        move |slide, delta| {
            let sunk = match (shown, seen) {
                (true, _) => 1. - delta,
                (false, true) => delta,
                // Never in sight yet: nothing to sink as the chat opens.
                (false, false) => 1.,
            };
            // Sunk all the way it is wholly below the clip: not a pixel is left on the edge.
            slide.top(px(TAB.1 * sunk))
        },
    );
    Some(
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(TAB.1))
            .overflow_hidden()
            .flex()
            .justify_center()
            .child(slide),
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
    let band = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_1()
        .rounded_t_2xl()
        .elevation(Level::Raised, cx)
        .border_b_0()
        .border_color(border)
        .bg(surface(cx))
        .child(folder(ws, cx))
        .child(div().flex_1())
        .children(ring::limits(ws, RING, cx));
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
        .press(raised, cx)
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
