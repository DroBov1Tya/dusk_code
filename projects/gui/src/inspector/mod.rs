//! The right column, the engineering side, as modules stacked one under another like
//! the TUI's sidebar: where the chat works, its context, what it wrote, the project's
//! git and files, and the chat's todo list, subagents and background jobs. Each module
//! folds on its header.

mod activity;
pub mod agents;
mod context;
mod environment;
mod files;
mod git;
mod glyph;
mod jobs;
pub mod mascot;
mod screenshots;
mod todo;

use gpui_kit::{
    component::{ActiveTheme, IconName},
    *,
};

use crate::{
    settings::SettingsPage,
    widgets,
    workspace::{Center, Right, Workspace},
};

pub const WIDTH: f32 = 340.;

const MODULES: &[(Right, &str)] = &[
    (Right::Arbiter, "Arbiter"),
    (Right::Context, "Context"),
    (Right::Activity, "Activity"),
    (Right::Screenshots, "Screenshots"),
    (Right::Git, "Changes"),
    (Right::Files, "Files"),
    (Right::Todo, "Todo"),
    (Right::Agents, "Agents"),
    (Right::Jobs, "Jobs"),
];

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border, muted) = (theme.sidebar, theme.sidebar_border, theme.muted_foreground);
    let mut column = div().flex().flex_col().pb_6();
    let shown: Vec<_> = MODULES
        .iter()
        .filter(|(module, _)| ws.has_content(*module))
        .collect();
    for (index, (module, title)) in shown.into_iter().enumerate() {
        let module = *module;
        let open = !ws.folded.contains(&module);
        let header = div()
            .id(("module", index))
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .h(px(36.))
            .px_4()
            .cursor_pointer()
            .child(
                div()
                    .flex_1()
                    .text_sm()
                    .font_weight(FontWeight::MEDIUM)
                    .child(*title),
            )
            .child(widgets::icon(
                if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                muted,
            ))
            .on_click(cx.listener(move |ws, _, _, cx| {
                if !ws.folded.remove(&module) {
                    ws.folded.insert(module);
                } else {
                    ws.refresh_module(module, cx);
                }
                cx.notify();
            }));
        let mut section = div()
            .flex()
            .flex_col()
            .when_border(index > 0, border)
            .child(header);
        if open {
            let body = match module {
                Right::Arbiter => arbiter_summary(ws, cx),
                Right::Context => context::render(ws, cx),
                Right::Activity => activity::render(ws, cx),
                Right::Screenshots => screenshots::render(ws, cx),
                Right::Git => git::render(ws, cx),
                Right::Files => files::render(ws, cx),
                Right::Todo => todo::render(ws, cx),
                Right::Agents => agents::render(ws, cx),
                Right::Jobs => jobs::render(ws, cx),
            };
            section = section.child(div().pb_3().child(body));
        }
        column = column.child(section);
    }
    div()
        .w(px(WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(panel)
        .border_l_1()
        .border_color(border)
        .child(
            div()
                .id("inspector-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(column),
        )
        // Where the chat works stays in sight, under everything that scrolls.
        .child(
            div()
                .flex_none()
                .border_t_1()
                .border_color(border)
                .pt_2()
                .pb_2()
                .child(environment::render(ws, cx))
                .child(
                    div()
                        .id("version")
                        .px_4()
                        .pt_1()
                        .flex()
                        .justify_end()
                        .text_xs()
                        .text_color(muted)
                        .cursor_pointer()
                        .hover(|version| version.text_color(cx.theme().foreground))
                        .child(format!("duskcode {}", env!("CARGO_PKG_VERSION")))
                        .on_click(cx.listener(|ws, _, _, cx| {
                            ws.center = Center::Settings;
                            ws.settings_page = SettingsPage::Updates;
                            ws.load_page(SettingsPage::Updates, cx);
                            cx.notify();
                        })),
                ),
        )
}

/// The arbiter's tally, and the way into its window.
fn arbiter_summary(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    use crate::chat::transcript::ExchangeKind;
    let muted = cx.theme().muted_foreground;
    let (mut asked, mut allowed, mut denied) = (0, 0, 0);
    if let Some(chat) = ws.shown_chat() {
        for exchange in &chat.exchanges {
            match exchange.kind {
                ExchangeKind::Approval => asked += 1,
                ExchangeKind::Allowed => allowed += 1,
                ExchangeKind::Denied => denied += 1,
                _ => {}
            }
        }
    }
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_4()
        .text_sm()
        .child(div().flex_1().text_color(muted).child(format!(
            "{asked} asked · {allowed} allowed · {denied} denied"
        )))
        .child(
            gpui_kit::component::button::Button::new("arbiter-open")
                .label("Open")
                .on_click(cx.listener(|ws, _, _, cx| {
                    if let Some(thread_id) = ws.shown.clone() {
                        ws.modal = Some(crate::modal::Modal::Arbiter { thread_id });
                    }
                    cx.notify();
                })),
        )
}

/// A hairline above every module but the first.
trait WhenBorder {
    fn when_border(self, on: bool, color: Hsla) -> Self;
}

impl WhenBorder for Div {
    fn when_border(self, on: bool, color: Hsla) -> Self {
        if on {
            self.border_t_1().border_color(color)
        } else {
            self
        }
    }
}

impl Workspace {
    /// A module shows only once it has something to show.
    fn has_content(&self, module: Right) -> bool {
        let chat = self.shown_chat();
        match module {
            Right::Arbiter => {
                self.status.approvals == dusk_protocol::config::ApprovalPolicy::Arbiter
                    || chat.is_some_and(|chat| !chat.exchanges.is_empty())
            }
            Right::Context => self.context.is_some(),
            Right::Activity => chat.is_some_and(|chat| !chat.activity().is_empty()),
            Right::Screenshots => chat.is_some_and(|chat| !chat.screenshots.is_empty()),
            Right::Git => self
                .git
                .as_ref()
                .is_some_and(|git| git.branch.is_some() || !git.branches.is_empty()),
            Right::Files => self.project.is_some(),
            Right::Todo => chat.is_some_and(|chat| !chat.tasks.is_empty()),
            Right::Agents => chat.is_some_and(|chat| !chat.agents.is_empty()),
            Right::Jobs => !self.jobs.is_empty(),
        }
    }

    /// Asks the core again for what the open modules show.
    pub fn refresh_inspector(&mut self, cx: &mut Context<Self>) {
        for (module, _) in MODULES {
            if !self.folded.contains(module) {
                self.refresh_module(*module, cx);
            }
        }
    }

    fn refresh_module(&mut self, module: Right, cx: &mut Context<Self>) {
        match module {
            Right::Git => self.load_git(cx),
            Right::Files => self.load_folder(String::new(), cx),
            Right::Context => self.load_context(cx),
            Right::Jobs => self.load_jobs(cx),
            Right::Arbiter | Right::Activity | Right::Screenshots | Right::Todo | Right::Agents => {
            }
        }
    }
}
