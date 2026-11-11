//! The mode menu over the bar, like the model's: each mode with the first sentence of
//! its prompt, a check on the one in use. Tab still walks them in order.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{ui::Type, widgets, workspace::Workspace};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, accent) = (theme.muted_foreground, theme.primary);
    let mut list = div()
        .id("mode-menu")
        .absolute()
        .occlude()
        .bottom(px(32.))
        .left_0()
        .w(px(360.))
        .overflow_hidden()
        .p_1()
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .bg(theme.popover)
        .shadow_lg();
    let modes = ws
        .settings
        .as_ref()
        .map(|settings| settings.modes.clone())
        .unwrap_or_default();
    if modes.is_empty() {
        list = list.child(widgets::muted("Loading the modes...", cx));
    }
    for mode in modes {
        let current = mode.name == ws.status.mode;
        let name = mode.name.clone();
        list = list.child(
            div()
                .id(SharedString::from(format!("mode-{}", mode.name)))
                .flex()
                .flex_row()
                .gap_2()
                .px_2()
                .py_2()
                .rounded_md()
                .cursor_pointer()
                .hover(|row| row.bg(theme.accent))
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .gap_0p5()
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .child(title(&mode.name)),
                        )
                        .child(
                            div()
                                .meta()
                                .text_color(muted)
                                .line_clamp(2)
                                .child(mode.summary.clone()),
                        ),
                )
                .when(current, |row| {
                    row.child(widgets::icon(IconName::Check, accent))
                })
                .on_click(cx.listener(move |ws, _, _, cx| ws.choose_mode(&name, cx))),
        );
    }
    list.on_mouse_down_out(cx.listener(|ws, _, _, cx| {
        ws.picker = None;
        ws.picker_closed = Some(std::time::Instant::now());
        cx.notify();
    }))
}

/// `PLAN` as `Plan`.
fn title(name: &str) -> String {
    let lower = name.to_lowercase();
    let mut chars = lower.chars();
    chars
        .next()
        .map(|first| first.to_uppercase().chain(chars).collect())
        .unwrap_or_default()
}

impl Workspace {
    fn choose_mode(&mut self, name: &str, cx: &mut Context<Self>) {
        self.picker = None;
        let name = name.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.settings().mode(&name).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_settings(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
        cx.notify();
    }
}
