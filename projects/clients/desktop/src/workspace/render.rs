//! The window drawn: the header, the rail, the side columns sliding on springs, the page
//! in the middle and what opens over it.

use gpui_kit::{base::spring, component::ActiveTheme, prelude::FluentBuilder, *};

use super::{Center, Workspace};
use crate::{
    columns::{inspector, rail, sidebar, topbar},
    modal,
    pages::{artifacts, chat, connectors, review, schedules, settings},
};

impl Workspace {
    /// The side panel belongs to the work: pages about the app itself have the whole width.
    pub fn side_panel_allowed(&self) -> bool {
        matches!(self.center, Center::Chat | Center::Review | Center::File)
    }

    fn side_panel_shown(&self) -> bool {
        self.side_panel_allowed() && !self.right_hidden && !self.quiet
    }

    /// The middle column's width once the side columns have settled.
    pub fn chat_width(&self, window: &Window) -> f32 {
        let sidebar = if self.left_hidden || self.quiet {
            0.
        } else {
            sidebar::WIDTH
        };
        let inspector = if self.side_panel_shown() {
            inspector::WIDTH
        } else {
            0.
        };
        window.viewport_size().width.as_f32() - rail::width(self) - sidebar - inspector
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        self.record_place();
        crate::plugins::settle(self, window, cx);
        let theme = cx.theme();
        let (background, foreground) = (theme.background, theme.foreground);
        let (frame, theme_border) = (rail::color(cx), theme.border);
        let center = match self.center {
            Center::Chat => chat::render(self, window, cx).into_any_element(),
            Center::Artifacts => artifacts::render(self, window, cx).into_any_element(),
            Center::Connectors => connectors::render(self, window, cx).into_any_element(),
            Center::Settings => settings::render(self, window, cx).into_any_element(),
            Center::Review => review::render(self, cx).into_any_element(),
            Center::File => crate::pages::file::render(self, window, cx).into_any_element(),
            Center::Schedules => schedules::render(self, window, cx).into_any_element(),
        };
        let terminal = crate::columns::terminal::render(self, window, cx);
        let simulator = crate::columns::simulator::render(self, window, cx);
        let right_dock = crate::plugins::right_dock(self, cx);
        let bottom_dock = crate::plugins::bottom_dock(self, window, cx);
        let plugins_menu = crate::plugins::menu(self, cx);
        let dragging = self.terminal.dragging() || self.plugins.bottom_drag.is_some();
        let modal = modal::render(self, window, cx);
        let chat_menu = modal::chat_menu(self, window, cx);
        let artifact_menu = artifacts::menu(self, window, cx);
        let audio_menu = chat::audio::menu(self, window, cx);
        // The side columns slide rather than jump: their widths follow a spring, and a
        // column is drawn at its full width inside a clipped frame, so its rows do not
        // reflow while it moves.
        let motion = cx.theme().motion_tokens().spring_move;
        let left = spring(
            "sidebar-width",
            px(if self.left_hidden || self.quiet {
                0.
            } else {
                sidebar::WIDTH
            }),
            motion,
            window,
            cx,
        );
        let right = spring(
            "inspector-width",
            px(if self.side_panel_shown() {
                inspector::WIDTH
            } else {
                0.
            }),
            motion,
            window,
            cx,
        );
        let sidebar = (left > px(0.5)).then(|| {
            div()
                .w(left)
                .h_full()
                .flex_none()
                .overflow_hidden()
                .child(sidebar::render(self, window, cx))
        });
        let inspector = (right > px(0.5)).then(|| {
            div()
                .w(right)
                .h_full()
                .flex_none()
                .overflow_hidden()
                .flex()
                .justify_end()
                .child(inspector::render(self, window, cx))
        });
        // The header runs the window's width; under it the rail on the frame, the
        // sidebar as a card with one rounded corner, and the work beside it.
        let nav_width = px(rail::width(self)) + left;
        let sidebar = sidebar.map(|column| {
            column
                .rounded_tl_xl()
                .border_t_1()
                .border_l_1()
                .border_color(theme_border)
        });
        div()
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(frame)
            .text_color(foreground)
            .child(topbar::render(self, nav_width, window, cx))
            .child(
                div()
                    .flex_1()
                    .min_h_0()
                    .flex()
                    .flex_row()
                    .children((!self.rail_hidden).then(|| rail::render(self, cx)))
                    .children(sidebar)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .h_full()
                            .flex()
                            .flex_row()
                            .border_t_1()
                            .border_color(theme_border)
                            .bg(background)
                            // The page, and under it the terminal when it is open.
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .h_full()
                                    .flex()
                                    .flex_col()
                                    .child(div().flex_1().min_h_0().child(center))
                                    .children(terminal)
                                    .children(bottom_dock),
                            )
                            .children(inspector)
                            .children(simulator)
                            .children(right_dock),
                    ),
            )
            .children(crate::columns::flyout::render(self, window, cx))
            .children(modal)
            .children(chat_menu)
            .children(artifact_menu)
            .children(audio_menu)
            .children(plugins_menu)
            .children(super::toast::render(self, cx))
            .children(crate::columns::fog::render(self, window, cx))
            // While the terminal's or the bottom dock's edge is held, the whole window
            // follows the pointer.
            .when(dragging, |root| {
                root.on_mouse_move(cx.listener(|ws, event: &MouseMoveEvent, window, cx| {
                    ws.drag_terminal(event, cx);
                    ws.drag_plugin_dock(event, window, cx);
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|ws, _, _, cx| {
                        ws.plugins.bottom_drag = None;
                        ws.drop_terminal_edge(cx);
                    }),
                )
            })
            .capture_any_mouse_down(cx.listener(|ws, _, _, cx| ws.attended(cx)))
            .capture_key_down(cx.listener(|ws, _: &KeyDownEvent, _, cx| ws.attended(cx)))
            .track_focus(&self.focus)
            .key_context("Workspace")
            .on_action(cx.listener(Self::on_new_chat))
            .on_action(cx.listener(Self::on_focus_next))
            .on_action(cx.listener(Self::on_focus_prev))
            .on_action(cx.listener(Self::on_find_chat))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_side_panel))
            .on_action(cx.listener(Self::on_toggle_terminal))
            .on_action(cx.listener(Self::on_save_file))
            .on_action(cx.listener(Self::copy_code))
            .on_action(cx.listener(Self::run_code))
            .capture_key_down(cx.listener(Self::menu_keys))
    }
}
