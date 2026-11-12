//! The right column, the engineering side, as modules stacked one under another like
//! the TUI's sidebar: where the chat works, its context, what it wrote, the project's
//! git and files, and the chat's todo list, subagents and background jobs. Each module
//! folds on its header.

mod activity;
mod context;
mod environment;
mod files;
pub mod git;
mod jobs;
pub mod mascot;
pub mod plan;
mod screenshots;
mod todo;
pub mod touched;

use gpui_kit::{
    component::{ActiveTheme, Icon, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{
    pages::settings::SettingsPage,
    ui::{Type, widgets},
    workspace::{Center, Right, Workspace},
};

pub const WIDTH: f32 = 300.;

const MODULES: &[(Right, &str)] = &[
    (Right::Arbiter, "Arbiter"),
    (Right::Statistics, "Statistics"),
    (Right::Screenshots, "Screenshots"),
    (Right::Git, "Changes"),
    (Right::Files, "Files"),
    (Right::Touched, "Touched"),
    (Right::Plan, "Plan"),
    (Right::Todo, "Todo"),
    (Right::Jobs, "Jobs"),
];

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
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
            .h(px(44.))
            .px_4()
            .cursor_pointer()
            // Every header starts with its icon, on the grid of the rows below.
            .child(
                Icon::new(module_icon(module))
                    .size(px(18.))
                    .text_color(muted),
            )
            .child(
                div()
                    .flex_1()
                    .body()
                    .font_weight(FontWeight::MEDIUM)
                    .child(*title),
            )
            // A folded module still says how it stands.
            .children(
                (!open)
                    .then(|| summary(ws, module))
                    .flatten()
                    .map(|text| div().meta().text_color(muted).child(text)),
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
                let kept: Vec<String> = ws.folded.iter().map(|m| m.key().to_owned()).collect();
                crate::ui::theme::change(cx, |prefs| prefs.right_folded = Some(kept));
                cx.notify();
            }));
        // The rule between modules stops short of the column's edges.
        let mut section = div()
            .flex()
            .flex_col()
            .when(index > 0, |section| {
                section.child(div().mx_4().h(px(1.)).bg(border))
            })
            .child(header);
        if open {
            let body = match module {
                Right::Arbiter => arbiter_summary(ws, cx),
                Right::Statistics => statistics(ws, cx),
                Right::Screenshots => screenshots::render(ws, cx),
                Right::Git => git::render(ws, cx),
                Right::Files => files::render(ws, cx),
                Right::Touched => touched::render(ws, cx),
                Right::Plan => plan::render(ws, window, cx),
                Right::Todo => todo::render(ws, cx),
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
        // Where the chat works waits folded under everything that scrolls: a bar across
        // the column's width unfolds it.
        .child(footer(ws, cx))
}

fn module_icon(module: Right) -> IconName {
    match module {
        Right::Arbiter => IconName::Eye,
        Right::Statistics => IconName::ChartPie,
        Right::Screenshots => IconName::Frame,
        Right::Git => IconName::Github,
        Right::Files => IconName::Folder,
        Right::Touched => IconName::FileText,
        Right::Plan => IconName::Map,
        Right::Todo => IconName::CircleCheck,
        Right::Jobs => IconName::SquareTerminal,
    }
}

/// A few words for a folded module's header: its count or its state.
fn summary(ws: &Workspace, module: Right) -> Option<String> {
    let chat = ws.shown_chat();
    match module {
        Right::Arbiter => {
            let count = chat.map_or(0, |chat| chat.exchanges.len());
            (count > 0).then(|| count.to_string())
        }
        Right::Statistics => ws
            .context
            .as_ref()
            .map(|report| format!("{}%", report.total * 100 / report.limit.max(1))),
        Right::Screenshots => chat.map(|chat| chat.screenshots.len().to_string()),
        Right::Git => ws.git.as_ref().map(|git| {
            let branch = git.branch.as_deref().unwrap_or("detached");
            match git.changes.len() {
                0 => branch.to_owned(),
                changes => format!("{branch} \u{00B7} {changes}"),
            }
        }),
        Right::Files | Right::Plan => None,
        Right::Touched => Some(ws.touched.len().to_string()),
        Right::Todo => chat.map(|chat| {
            let items = chat.tasks.iter().flat_map(|task| &task.items);
            let (done, all) = items.fold((0, 0), |(done, all), item| {
                (done + usize::from(item.done), all + 1)
            });
            format!("{done}/{all}")
        }),
        Right::Jobs => {
            let running = ws.jobs.iter().filter(|job| job.running).count();
            (running > 0).then(|| format!("{running} running"))
        }
    }
}

/// The arbiter's tally, and the way into its window.
fn arbiter_summary(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    use crate::pages::chat::transcript::ExchangeKind;
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

/// The context, then the activity, each under a small caption when it has anything.
fn statistics(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let mut column = div().flex().flex_col().gap_3();
    if ws.context.is_some() {
        column = column
            .child(widgets::caption("Context", cx))
            .child(context::render(ws, cx));
    }
    if ws
        .shown_chat()
        .is_some_and(|chat| !chat.activity().is_empty())
    {
        column = column
            .child(widgets::caption("Activity", cx))
            .child(activity::render(ws, cx));
    }
    column
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
            Right::Statistics => {
                self.context.is_some() || chat.is_some_and(|chat| !chat.activity().is_empty())
            }
            Right::Screenshots => chat.is_some_and(|chat| !chat.screenshots.is_empty()),
            Right::Git => self
                .git
                .as_ref()
                .is_some_and(|git| git.branch.is_some() || !git.branches.is_empty()),
            Right::Files => self.project.is_some(),
            Right::Touched => !self.touched.is_empty(),
            Right::Plan => plan::has_content(self),
            Right::Todo => chat.is_some_and(|chat| !chat.tasks.is_empty()),
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
        // The repository decides whether the Changes module shows at all, and the footer
        // names it, so it is read even while the module is folded.
        if self.folded.contains(&Right::Git) {
            self.load_git(cx);
        }
    }

    fn refresh_module(&mut self, module: Right, cx: &mut Context<Self>) {
        match module {
            Right::Git => self.load_git(cx),
            Right::Files => self.load_folder(String::new(), cx),
            Right::Statistics => self.load_context(cx),
            Right::Jobs => self.load_jobs(cx),
            Right::Plan => self.load_plan(cx),
            Right::Touched => self.load_touched(cx),
            Right::Arbiter | Right::Screenshots | Right::Todo => {}
        }
    }
}

/// The foot of the column: a bar with an arrow while folded; open, the chat's project,
/// folder, branch, model and mode, and the version.
fn footer(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (border, muted, hover) = (
        theme.sidebar_border,
        theme.muted_foreground,
        theme.list_hover,
    );
    let open = crate::ui::theme::prefs(cx).footer_open;
    let bar = div()
        .id("footer-toggle")
        .h(px(32.))
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .hover(move |bar| bar.bg(hover))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(if open {
                "Fold where the chat works"
            } else {
                "Show where the chat works"
            })
            .build(window, cx)
        })
        .child(widgets::icon(
            if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronUp
            },
            muted,
        ))
        .on_click(cx.listener(|_, _, _, cx| {
            crate::ui::theme::change(cx, |prefs| prefs.footer_open = !prefs.footer_open);
        }));
    let footer = div()
        .flex_none()
        .flex()
        .flex_col()
        .border_t_1()
        .border_color(border)
        .child(bar);
    if !open {
        return footer;
    }
    footer.child(widgets::appear(
        "footer-body",
        div()
            .flex()
            .flex_col()
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
    ))
}
