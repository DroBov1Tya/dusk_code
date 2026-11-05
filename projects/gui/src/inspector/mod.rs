//! The right column, the engineering side: the project's git and files, and the shown
//! chat's context, todo list, subagents and background jobs.

mod agents;
mod context;
mod files;
mod git;
mod glyph;
mod jobs;
mod todo;

use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    *,
};

use crate::{
    host, widgets,
    workspace::{Right, Workspace},
};

pub const WIDTH: f32 = 340.;

const TABS: &[(Right, &str)] = &[
    (Right::Git, "Git"),
    (Right::Files, "Files"),
    (Right::Context, "Context"),
    (Right::Todo, "Todo"),
    (Right::Agents, "Agents"),
    (Right::Jobs, "Jobs"),
];

pub fn render(ws: &mut Workspace, _: &mut Window, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (panel, border) = (theme.sidebar, theme.sidebar_border);
    let mut tabs = div().flex().flex_row().gap_0p5().px_1().py_2();
    for (index, (tab, label)) in TABS.iter().enumerate() {
        let tab = *tab;
        tabs = tabs.child(
            widgets::tab(("inspector-tab", index), label, ws.right == tab, cx).on_click(
                cx.listener(move |ws, _, _, cx| {
                    ws.right = tab;
                    ws.refresh_inspector(cx);
                    cx.notify();
                }),
            ),
        );
    }
    let body = match ws.right {
        Right::Git => git::render(ws, cx),
        Right::Files => files::render(ws, cx),
        Right::Context => context::render(ws, cx),
        Right::Todo => todo::render(ws, cx),
        Right::Agents => agents::render(ws, cx),
        Right::Jobs => jobs::render(ws, cx),
    };
    let dir = ws.selected_project().map(|project| project.dir.clone());
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
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .pr_2()
                .border_b_1()
                .border_color(border)
                .child(tabs)
                .child(
                    Button::new("terminal")
                        .ghost()
                        .small()
                        .icon(IconName::SquareTerminal)
                        .tooltip("Open a terminal in the project")
                        .on_click(cx.listener(move |ws, _, _, cx| match &dir {
                            Some(dir) => {
                                if let Err(error) = host::terminal::open(dir) {
                                    ws.fail(error, cx);
                                }
                            }
                            None => ws.fail(anyhow::anyhow!("pick a project first"), cx),
                        })),
                ),
        )
        .child(
            div()
                .id("inspector-body")
                .flex_1()
                .min_h_0()
                .overflow_y_scroll()
                .child(body),
        )
}

impl Workspace {
    /// Asks the core again for what the open tab shows.
    pub fn refresh_inspector(&mut self, cx: &mut Context<Self>) {
        match self.right {
            Right::Git => self.load_git(cx),
            Right::Files => self.load_folder(String::new(), cx),
            Right::Context => self.load_context(cx),
            Right::Jobs => self.load_jobs(cx),
            Right::Todo | Right::Agents => {}
        }
    }
}
