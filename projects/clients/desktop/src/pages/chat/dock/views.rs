//! The views tab: a small handle on the composer's upper edge, out of sight until the
//! pointer comes near, the keyboard reaches it or the box is written in, and its menu of
//! the views about the chat. Only views that exist are listed.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

const WIDTH: f32 = 220.;
const HANDLE: (f32, f32) = (44., 12.);
/// The band above the composer's edge in which the pointer brings the handle up.
const NEAR: f32 = 24.;
/// From the composer's top to the box's upper edge, which the handle sits 6 px above.
const EDGE: f32 = 8.;

#[derive(Clone, Copy)]
enum View {
    Overview,
}

/// The views in their order: name, icon.
const VIEWS: [(View, &str, IconName); 1] =
    [(View::Overview, "Overview", IconName::LayoutDashboard)];

pub fn render(ws: &Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Option<Div> {
    // Every view is about a chat.
    ws.shown.as_ref()?;
    let theme = cx.theme();
    let (accent, border, raised, muted, ring) = (
        theme.primary,
        theme.border,
        theme.list_hover,
        theme.muted_foreground,
        theme.ring,
    );
    let open = ws.dock.views_open;
    let focused = ws.dock.views_focus.is_focused(window);
    let writing = gpui_kit::Focusable::focus_handle(ws.composer.read(cx), cx).is_focused(window);
    let rest = match (open || focused, writing) {
        (true, _) => 1.,
        (false, true) => 0.4,
        (false, false) => 0.,
    };
    let handle = div()
        .id("views-tab")
        .track_focus(&ws.dock.views_focus)
        .w(px(HANDLE.0))
        .h(px(HANDLE.1))
        .flex()
        .flex_row()
        .items_center()
        .justify_center()
        .gap(px(3.))
        .rounded_full()
        .border_1()
        .border_color(if focused { ring } else { border })
        .bg(if open { accent.opacity(0.15) } else { raised })
        .cursor_pointer()
        .opacity(rest)
        .group_hover("views-near", |style| style.opacity(1.))
        .children((0..3).map(|_| div().size(px(2.)).rounded_full().bg(muted)))
        .on_click(cx.listener(|ws, _, window, cx| ws.toggle_views(window, cx)));
    let menu = open.then(|| menu(ws, cx));
    Some(
        div().relative().w_full().h(px(0.)).child(
            div()
                .group("views-near")
                .absolute()
                .left_0()
                .right_0()
                .bottom(px(6. - EDGE))
                .h(px(NEAR))
                .flex()
                .flex_row()
                .justify_center()
                .items_end()
                .child(handle)
                .children(menu),
        ),
    )
}

/// The menu over the handle, opening upward.
fn menu(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, surface, muted) = (theme.border, theme.popover, theme.muted_foreground);
    let chosen = ws.dock.views_chosen;
    let mut list = div()
        .id("views-menu")
        .track_focus(&ws.dock.views_menu_focus)
        .w(px(WIDTH))
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(border)
        .bg(surface)
        .shadow_lg()
        .on_key_down(cx.listener(Workspace::views_keys))
        .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
            ws.dock.views_open = false;
            cx.notify();
        }));
    for (index, (_, name, icon)) in VIEWS.into_iter().enumerate() {
        list = list.child(
            widgets::menu_item(ElementId::Name(format!("view-{name}").into()), cx)
                .when(index == chosen, |row| row.bg(theme.accent))
                .child(widgets::icon(icon, muted))
                .child(div().flex_1().body().child(name))
                .on_click(cx.listener(move |ws, _, window, cx| ws.open_view(index, window, cx))),
        );
    }
    div()
        .absolute()
        .bottom(px(HANDLE.1 + 6.))
        .left_0()
        .right_0()
        .flex()
        .justify_center()
        .child(widgets::on_top(widgets::appear("views-menu-open", list)))
}

impl Workspace {
    /// Opens the views menu with the keyboard in it, or closes it.
    pub fn toggle_views(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.shown.is_none() {
            return;
        }
        self.dock.views_open = !self.dock.views_open;
        self.dock.views_chosen = 0;
        if self.dock.views_open {
            window.focus(&self.dock.views_menu_focus, cx);
        } else {
            self.focus_composer(window, cx);
        }
        cx.notify();
    }

    /// Escape closed the menu: the keyboard goes back to the box.
    pub fn close_views(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !std::mem::take(&mut self.dock.views_open) {
            return false;
        }
        self.focus_composer(window, cx);
        true
    }

    fn focus_composer(&self, window: &mut Window, cx: &mut Context<Self>) {
        let composer = gpui_kit::Focusable::focus_handle(self.composer.read(cx), cx);
        window.focus(&composer, cx);
    }

    fn open_view(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.dock.views_open = false;
        match VIEWS.get(index).map(|(view, ..)| *view) {
            Some(View::Overview) => self.open_overview(cx),
            None => {}
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Up and Down choose a view, Enter opens it; Escape is `close_top`'s.
    fn views_keys(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let last = VIEWS.len() - 1;
        let chosen = self.dock.views_chosen;
        match event.keystroke.key.as_str() {
            "up" => self.dock.views_chosen = chosen.saturating_sub(1),
            "down" => self.dock.views_chosen = (chosen + 1).min(last),
            "enter" => self.open_view(chosen, window, cx),
            _ => return,
        }
        cx.stop_propagation();
        cx.notify();
    }
}
