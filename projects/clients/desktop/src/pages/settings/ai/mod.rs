//! Models and providers grouped by what they produce.

pub mod voice;

use gpui_kit::{
    component::{
        ActiveTheme, Icon,
        tab::{Tab as Item, TabBar},
    },
    *,
};

use crate::{
    ui::{Type, icons},
    workspace::Workspace,
};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Llm,
    Voice,
    Image,
    Video,
}

const TABS: [(Tab, &str, &str); 4] = [
    (Tab::Llm, "LLM", "icons/cpu.svg"),
    (Tab::Voice, "Voice", "icons/mic.svg"),
    (Tab::Image, "Image", icons::IMAGE),
    (Tab::Video, "Video", icons::VIDEO),
];

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let index = TABS
        .iter()
        .position(|(tab, ..)| *tab == ws.ai_tab)
        .unwrap_or_default();
    let tabs = TabBar::new("ai-tabs")
        .outline()
        .w_full()
        .selected_index(index)
        .children(
            TABS.map(|(_, label, icon)| Item::new().label(label).prefix(glyph(icon).size(px(16.)))),
        )
        .on_click(cx.listener(|ws, index: &usize, _, cx| {
            if let Some((tab, ..)) = TABS.get(*index)
                && ws.ai_tab != *tab
            {
                ws.select_ai_tab(*tab, cx);
            }
        }));
    let body = match ws.ai_tab {
        Tab::Llm => super::behavior::llm(ws, cx),
        Tab::Voice => {
            ws.voice_form.settle(window, cx);
            voice::render(ws, cx)
        }
        Tab::Image => unavailable(
            icons::IMAGE,
            "Image generation",
            "Built-in image generation is unavailable in this version. You can still attach images to a chat.",
            cx,
        ),
        Tab::Video => unavailable(
            icons::VIDEO,
            "Video generation",
            "Built-in video generation is unavailable in this version.",
            cx,
        ),
    };
    let padding = if window.viewport_size().width < px(1200.) {
        px(16.)
    } else {
        px(32.)
    };
    let column = || div().w_full().max_w(px(880.)).mx_auto().px(padding);
    div()
        .flex_1()
        .min_w_0()
        .h_full()
        .flex()
        .flex_col()
        .child(
            column()
                .pt_6()
                .flex_none()
                .child(div().title().pb_4().child("AI"))
                .child(tabs),
        )
        .child(
            div()
                .id(("ai-settings-scroll", index))
                .flex_1()
                .min_h_0()
                .border_t_1()
                .border_color(cx.theme().border)
                .overflow_y_scroll()
                .child(column().pt_5().pb_8().child(body)),
        )
}

pub(super) fn section(title: &str, hint: &str, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_0p5()
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
        .child(
            div()
                .small()
                .text_color(cx.theme().muted_foreground)
                .child(hint.to_owned()),
        )
}

fn glyph(path: &'static str) -> Icon {
    Icon::default().path(path)
}

fn unavailable(icon: &'static str, title: &str, hint: &str, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_2()
        .rounded_xl()
        .border_1()
        .border_color(theme.border)
        .px_5()
        .py_8()
        .child(glyph(icon).size(px(24.)).text_color(theme.muted_foreground))
        .child(
            div()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child(title.to_owned()),
        )
        .child(
            div()
                .max_w(px(340.))
                .small()
                .text_center()
                .text_color(theme.muted_foreground)
                .child(hint.to_owned()),
        )
}

impl Workspace {
    pub fn select_ai_tab(&mut self, tab: Tab, cx: &mut Context<Self>) {
        self.ai_tab = tab;
        self.load_ai_tab(cx);
        cx.notify();
    }

    pub fn load_ai_tab(&mut self, cx: &mut Context<Self>) {
        match self.ai_tab {
            Tab::Llm => self.load_models(cx),
            Tab::Voice => {
                if self.voice.is_none() {
                    self.load_voice(cx);
                }
                self.load_voice_models(cx);
            }
            Tab::Image | Tab::Video => {}
        }
    }
}
