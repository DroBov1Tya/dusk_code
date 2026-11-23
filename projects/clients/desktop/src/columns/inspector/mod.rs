//! The right column, the engineering side, as modules stacked one under another like
//! the TUI's sidebar: where the chat works, its context, what it wrote, the project's
//! git and files, and the chat's todo list, subagents and background jobs. Each module
//! folds on its header and is dragged to another place by its grip (see `order`).

pub mod checks;
pub mod context;
mod environment;
mod files;
pub mod git;
pub mod jobs;
pub mod mascot;
pub mod order;
pub mod overview;
pub mod plan;
pub mod pulls;
mod screenshots;
mod sources;
mod todo;
pub mod touched;

use gpui_kit::{
    component::{ActiveTheme, Icon, IconName},
    *,
};

use crate::{
    pages::settings::SettingsPage,
    ui::{Type, widgets, widgets::Focusable as _},
    workspace::{Center, Right, Workspace},
};

pub const WIDTH: f32 = 300.;
/// How long a screen takes to slide over the list, or the list back.
const SLIDE: std::time::Duration = std::time::Duration::from_millis(180);

/// A heavy module opens as a screen of the whole column; a light one unfolds in place.
pub fn heavy(module: Right) -> bool {
    matches!(
        module,
        Right::Overview | Right::Team | Right::Git | Right::Files | Right::Plan
    )
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border) = (theme.sidebar, theme.sidebar_border);
    // A screen whose module has nothing to show for this chat gives way to the list.
    let screen = ws.right_screen.filter(|module| ws.has_content(*module));
    let shown = match screen {
        Some(module) => screen_view(ws, module, window, cx).into_any_element(),
        None => list(ws, window, cx).into_any_element(),
    };
    div()
        .w(px(WIDTH))
        .h_full()
        .flex()
        .flex_col()
        .bg(panel)
        .border_l_1()
        .border_color(border)
        .overflow_hidden()
        .child(shown)
        // Where the chat works waits folded under everything that scrolls: a bar across
        // the column's width unfolds it.
        .child(footer(ws, cx))
}

/// The column's list: a row a module with its icon, name and one line of how it stands.
/// A heavy module's row opens its screen; a light one's unfolds its body under it.
fn list(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> AnyElement {
    let muted = cx.theme().muted_foreground;
    let mut column =
        div().flex().flex_col().pt_2().pb_6().on_drop(
            cx.listener(|ws, dragged: &order::Dragged, _, cx| ws.drop_module(dragged, cx)),
        );
    let shown: Vec<Right> = ws
        .right_order
        .iter()
        .copied()
        .filter(|module| ws.has_content(*module))
        .collect();
    for module in shown {
        let title = title(module);
        let screen = heavy(module);
        let open = !screen && !ws.folded.contains(&module);
        let group = SharedString::from(format!("module-head-{}", module.key()));
        let line = summary(ws, module);
        let header = div()
            // Named by the module, not its place, so the focus follows it when it moves.
            .id(group.clone())
            .group(group.clone())
            .flex()
            .flex_row()
            .items_center()
            .gap_3()
            .mx_2()
            .px_2()
            .py(px(10.))
            .rounded_lg()
            .cursor_pointer()
            .hover(|header| header.bg(cx.theme().list_hover))
            .focusable_row(cx)
            .child(
                module_icon(module)
                    .size(px(crate::ui::size::ICON))
                    .text_color(muted),
            )
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .flex_col()
                    .child(div().body().font_weight(FontWeight::MEDIUM).child(title))
                    .children(
                        line.map(|line| div().meta().text_color(muted).truncate().child(line)),
                    ),
            )
            .child(order::grip(module, title, group, cx))
            .child(widgets::icon(
                if open {
                    IconName::ChevronDown
                } else {
                    IconName::ChevronRight
                },
                muted,
            ))
            .on_key_down(cx.listener(move |ws, event: &KeyDownEvent, _, cx| {
                let keys = &event.keystroke;
                let alt_only = keys.modifiers.alt
                    && !keys.modifiers.control
                    && !keys.modifiers.platform
                    && !keys.modifiers.shift;
                if alt_only && matches!(keys.key.as_str(), "up" | "down") {
                    cx.stop_propagation();
                    ws.step_module(module, keys.key == "up", cx);
                }
            }))
            .on_click(cx.listener(move |ws, _, _, cx| {
                if screen {
                    return ws.open_right(module, cx);
                }
                if !ws.folded.remove(&module) {
                    ws.folded.insert(module);
                } else {
                    ws.refresh_module(module, cx);
                }
                let kept: Vec<String> = ws.folded.iter().map(|m| m.key().to_owned()).collect();
                crate::ui::theme::change(cx, |prefs| prefs.right_folded = Some(kept));
                cx.notify();
            }));
        let mut section = div().relative().flex().flex_col().child(header);
        if open {
            let body = match module {
                Right::Arbiter => arbiter_summary(ws, cx),
                Right::Screenshots => screenshots::render(ws, cx),
                Right::Touched => touched::render(ws, cx),
                Right::Todo => todo::render(ws, cx),
                Right::Jobs => jobs::render(ws, cx),
                Right::Sources => sources::render(ws, cx),
                // Heavy modules open as screens, never here.
                Right::Overview | Right::Team | Right::Git | Right::Files | Right::Plan => div(),
            };
            section = section.child(widgets::unfold(
                SharedString::from(format!("module-{}", module.key())),
                div().pt_1().pb_3().child(body),
            ));
        }
        let section = section.children(order::indicator(ws, module, cx));
        let section = order::target(section, module, cx);
        column = column.child(order::settle(ws, module, section, window, cx));
    }
    let body = div()
        .id("inspector-body")
        .flex_1()
        .min_h_0()
        .overflow_y_scroll()
        .track_scroll(&ws.right_scroll)
        .child(column);
    // Back from a screen, the list comes in from the left as the screen leaves.
    let back = ws.right_moves;
    match back {
        0 => body.into_any_element(),
        _ => div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(body)
            .with_animation(
                ("right-list", back),
                Animation::new(SLIDE).with_easing(crate::ui::motion::ease_out),
                |list, t| {
                    list.relative()
                        .left(px(-0.28 * WIDTH * (1. - t)))
                        .opacity(t)
                },
            )
            .into_any_element(),
    }
}

/// A heavy module as the column's screen: Back and its name on top, its body under them.
fn screen_view(
    ws: &mut Workspace,
    module: Right,
    window: &mut Window,
    cx: &mut Context<Workspace>,
) -> AnyElement {
    let theme = cx.theme();
    let (muted, hover) = (theme.muted_foreground, theme.list_hover);
    let back = div()
        .id("right-back")
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .pl_1()
        .pr_2()
        .py_1()
        .rounded_md()
        .small()
        .text_color(muted)
        .cursor_pointer()
        .hover(move |back| back.bg(hover))
        .child(widgets::icon(IconName::ChevronLeft, muted))
        .child("Back")
        .on_click(cx.listener(|ws, _, _, cx| ws.close_right(cx)));
    // The Team and the Overview have a page of their own as well.
    let page: Option<fn(&mut Workspace, &mut Context<Workspace>)> = match module {
        Right::Team => Some(|ws, cx| ws.open_team(cx)),
        Right::Overview => Some(|ws, cx| ws.open_overview(cx)),
        _ => None,
    };
    let page = page.map(|open| {
        div()
            .id("right-page")
            .ml_auto()
            .meta()
            .text_color(muted)
            .cursor_pointer()
            .hover(|link| link.text_color(cx.theme().foreground))
            .child("Open \u{203A}")
            .on_click(cx.listener(move |ws, _, _, cx| open(ws, cx)))
    });
    let top = div()
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .px_3()
        .pt_3()
        .pb_2()
        .child(back)
        .child(
            div()
                .body()
                .font_weight(FontWeight::MEDIUM)
                .child(title(module)),
        )
        .children(page);
    let body = match module {
        Right::Overview => div().flex().flex_col().children(overview::sections::all(
            ws,
            overview::Form::Narrow,
            cx,
        )),
        Right::Team => crate::pages::team::module::render(ws, cx),
        Right::Git => git::render(ws, cx),
        Right::Files => files::render(ws, cx),
        Right::Plan => plan::render(ws, window, cx),
        _ => div(),
    };
    div()
        .flex_1()
        .min_h_0()
        .flex()
        .flex_col()
        .child(top)
        .child(
            div()
                .id(SharedString::from(format!("right-screen-{}", module.key())))
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .pt_1()
                .pb_6()
                .child(body),
        )
        .with_animation(
            ("right-screen", ws.right_moves),
            Animation::new(SLIDE).with_easing(crate::ui::motion::ease_out),
            |screen, t| screen.relative().left(px(WIDTH * (1. - t))),
        )
        .into_any_element()
}

fn title(module: Right) -> &'static str {
    match module {
        Right::Arbiter => "Arbiter",
        Right::Overview => "Overview",
        Right::Team => "Team",
        Right::Screenshots => "Screenshots",
        Right::Git => "Changes",
        Right::Files => "Files",
        Right::Touched => "Touched",
        Right::Plan => "Plan",
        Right::Todo => "Todo",
        Right::Jobs => "Jobs",
        Right::Sources => "Sources",
    }
}

fn module_icon(module: Right) -> Icon {
    match module {
        Right::Arbiter => IconName::Eye.into(),
        Right::Overview => IconName::LayoutDashboard.into(),
        Right::Team => IconName::Bot.into(),
        Right::Screenshots => IconName::Frame.into(),
        Right::Git => Icon::default().path(crate::ui::icons::GIT_BRANCH),
        Right::Files => IconName::Folder.into(),
        Right::Touched => IconName::FileText.into(),
        Right::Plan => IconName::Map.into(),
        Right::Todo => IconName::CircleCheck.into(),
        Right::Jobs => IconName::SquareTerminal.into(),
        Right::Sources => IconName::File.into(),
    }
}

/// One line under a module's name in the list: its count or how it stands.
fn summary(ws: &Workspace, module: Right) -> Option<String> {
    let chat = ws.shown_chat();
    match module {
        Right::Arbiter => {
            let count = chat.map_or(0, |chat| chat.exchanges.len());
            (count > 0).then(|| dusk_protocol::thread::plural(count as u32, "exchange"))
        }
        Right::Overview => ws
            .context
            .as_ref()
            .map(|report| format!("{}% of context", report.total * 100 / report.limit.max(1))),
        Right::Screenshots => chat.map(|chat| {
            dusk_protocol::thread::plural(chat.screenshots().count() as u32, "picture")
        }),
        Right::Git => ws.git.as_ref().map(|git| {
            let branch = git.branch.as_deref().unwrap_or("detached");
            match git.changes.len() {
                0 => branch.to_owned(),
                changes => format!("{branch} \u{00B7} {changes}"),
            }
        }),
        Right::Files => None,
        // The pinned plan's first line, as its title.
        Right::Plan => (ws.plan.chat == ws.shown)
            .then_some(ws.plan.text.as_deref())
            .flatten()
            .and_then(|text| text.lines().map(str::trim).find(|line| !line.is_empty()))
            .map(|line| line.trim_start_matches('#').trim().to_owned()),
        Right::Team => crate::pages::team::module::summary(ws),
        Right::Touched => Some(dusk_protocol::thread::plural(
            ws.touched.len() as u32,
            "file",
        )),
        Right::Todo => chat.map(|chat| {
            let items = chat.tasks.iter().flat_map(|task| &task.items);
            let (done, all) = items.fold((0, 0), |(done, all), item| {
                (done + usize::from(item.done), all + 1)
            });
            // A plan of tasks without steps counts its tasks.
            let (done, all) = match all {
                0 => (
                    chat.tasks.iter().filter(|task| task.done).count(),
                    chat.tasks.len(),
                ),
                _ => (done, all),
            };
            format!("{done} of {all} done")
        }),
        Right::Jobs => {
            let running = ws.jobs.iter().filter(|job| job.running).count();
            (running > 0).then(|| format!("{running} running"))
        }
        Right::Sources => {
            let pinned = ws.sources.iter().filter(|source| source.pinned).count();
            Some(match pinned {
                0 => ws.sources.len().to_string(),
                pinned => format!("{} \u{00B7} {pinned} every time", ws.sources.len()),
            })
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
        .small()
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

impl Workspace {
    /// A module shows only once it has something to show.
    pub fn has_content(&self, module: Right) -> bool {
        let chat = self.shown_chat();
        match module {
            Right::Arbiter => {
                self.status.approvals == dusk_protocol::config::ApprovalPolicy::Arbiter
                    || chat.is_some_and(|chat| !chat.exchanges.is_empty())
            }
            Right::Overview => chat.is_some(),
            Right::Team => crate::pages::team::module::has_content(self),
            Right::Screenshots => chat.is_some_and(|chat| chat.screenshots().next().is_some()),
            Right::Git => self
                .git
                .as_ref()
                .is_some_and(|git| git.branch.is_some() || !git.branches.is_empty()),
            Right::Files => self.project.is_some(),
            Right::Touched => !self.touched.is_empty(),
            Right::Plan => plan::has_content(self),
            Right::Todo => chat.is_some_and(|chat| !chat.tasks.is_empty()),
            Right::Jobs => !self.jobs.is_empty(),
            Right::Sources => !self.sources.is_empty(),
        }
    }

    /// Opens a heavy module as the column's screen, over the list.
    pub fn open_right(&mut self, module: Right, cx: &mut Context<Self>) {
        self.right_screen = Some(module);
        self.right_moves += 1;
        self.refresh_module(module, cx);
        cx.notify();
    }

    /// Back from the column's screen to the list, where it was scrolled.
    pub fn close_right(&mut self, cx: &mut Context<Self>) {
        if self.right_screen.take().is_some() {
            self.right_moves += 1;
            cx.notify();
        }
    }

    /// Whether a module's body is drawn now: a heavy one as the open screen, a light one
    /// unfolded in the list.
    pub fn module_shown(&self, module: Right) -> bool {
        match heavy(module) {
            true => self.right_screen == Some(module),
            false => !self.folded.contains(&module),
        }
    }

    /// Asks the core again for what the open modules show.
    pub fn refresh_inspector(&mut self, cx: &mut Context<Self>) {
        for module in Right::ALL {
            if self.module_shown(module) {
                self.refresh_module(module, cx);
            }
        }
        // The repository decides whether the Changes module shows at all, and the footer
        // names it, so it is read even while its screen is closed.
        if !self.module_shown(Right::Git) {
            self.load_git(cx);
        }
        // Likewise the jobs: the module shows only when the chat has some.
        if !self.module_shown(Right::Jobs) {
            self.load_jobs(cx);
        }
        if !self.module_shown(Right::Sources) {
            self.load_sources(cx);
        }
        // The Overview opened as a page follows the shown chat with its screen closed;
        // the list's line of it needs the context too.
        if !self.module_shown(Right::Overview) {
            match self.center == Center::Overview {
                true => self.refresh_module(Right::Overview, cx),
                false => self.load_context(cx),
            }
        }
    }

    pub fn refresh_module(&mut self, module: Right, cx: &mut Context<Self>) {
        match module {
            Right::Git => self.load_git(cx),
            Right::Files => self.load_folder(String::new(), cx),
            Right::Overview => {
                self.load_context(cx);
                self.load_overview(cx);
            }
            Right::Jobs => self.load_jobs(cx),
            Right::Sources => self.load_sources(cx),
            Right::Plan => self.load_plan(cx),
            Right::Touched => self.load_touched(cx),
            Right::Arbiter | Right::Screenshots | Right::Todo | Right::Team => {}
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
                    .meta()
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
