//! Models and providers grouped by what they produce.

pub mod voice;

use gpui_kit::{
    component::tab::{Tab as Item, TabBar},
    *,
};

use crate::workspace::Workspace;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Tab {
    Llm,
    Voice,
    Image,
    Video,
}

const TABS: [(Tab, &str); 4] = [
    (Tab::Llm, "LLM"),
    (Tab::Voice, "Voice"),
    (Tab::Image, "Image"),
    (Tab::Video, "Video"),
];

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let index = TABS
        .iter()
        .position(|(tab, _)| *tab == ws.ai_tab)
        .unwrap_or_default();
    let tabs = TabBar::new("ai-tabs")
        .segmented()
        .w(px(256.))
        .max_w_full()
        .selected_index(index)
        .children(TABS.map(|(_, label)| Item::new().label(label)))
        .on_click(cx.listener(|ws, index: &usize, _, cx| {
            if let Some((tab, _)) = TABS.get(*index)
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
        Tab::Image => unavailable("Image generation", cx),
        Tab::Video => unavailable("Video generation", cx),
    };
    div().flex().flex_col().gap_5().child(tabs).child(body)
}

fn unavailable(label: &str, cx: &App) -> Div {
    super::list(
        [super::line(
            label,
            "No built-in provider is available in this version.",
            cx,
        )],
        cx,
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
