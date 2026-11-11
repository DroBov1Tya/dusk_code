//! The mode menu over the bar, like the model's: each mode with the first sentence of
//! its prompt, a check on the one in use. Tab still walks them in order.

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use super::model_bar::{picker_heading, picker_hint, picker_panel};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, accent) = (picker_hint(cx), theme.primary);
    let mut list = picker_panel(cx)
        .id("mode-menu")
        .absolute()
        .occlude()
        .bottom(px(36.))
        .left_0()
        .w(px(336.))
        .max_h(px(400.))
        .overflow_y_scroll()
        .child(picker_heading("Mode", cx));
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
            widgets::row(
                SharedString::from(format!("mode-{}", mode.name)),
                current,
                cx,
            )
            .h_auto()
            .items_start()
            .min_h(px(48.))
            .px_3()
            .py_2()
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
                            .body()
                            .font_weight(FontWeight::MEDIUM)
                            .child(title(&mode.name)),
                    )
                    .child(
                        div()
                            .small()
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
