//! The pull requests under the Changes module: a state icon, the title, the repository
//! with the number and the lines, and the checks as a pill of a dot and a count. The core
//! reads them, keeps them fresh while the project is shown, and says when they change; a
//! row opens its page, its pill opens the checks.

use std::collections::HashMap;

use dusk_protocol::project::{CheckState, Checks, Forge, PullState, PullStatus, RepositoryStatus};
use gpui_kit::{
    component::{ActiveTheme, Icon, Selectable, popover::Popover, tooltip::Tooltip},
    prelude::FluentBuilder,
    *,
};

use crate::ui::looping::Loop;
use crate::{
    ui::{Type, icons, widgets},
    workspace::Workspace,
};

/// One row per pull request, and the quiet line when the forge could not be asked:
/// how to sign in, or that a plugin serves the host.
pub fn rows(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(status) = &ws.repository else {
        return div();
    };
    let mut column = div().flex().flex_col();
    let muted = cx.theme().muted_foreground.opacity(0.8);
    let note = |text: String| div().px_4().py_1().meta().text_color(muted).child(text);
    match &status.forge {
        Forge::Served(served) => {
            for pull in &served.pulls {
                column = column.child(row(status, pull, &ws.checks_open, cx));
            }
            if let Some(reason) = served.unavailable.as_ref().or(status.unavailable.as_ref()) {
                column = column.child(note(reason.clone()));
            }
        }
        Forge::SignIn(sign_in) => {
            column = column.child(sign_in_line(&sign_in.host, &sign_in.hint, cx));
        }
        // Phase 3 adds the offer's button; until then the line is all there is.
        Forge::Unserved { host, .. } => {
            column = column.child(note(format!("Pull requests on {host} come from a plugin.")));
        }
        Forge::None => {
            if let Some(reason) = &status.unavailable {
                column = column.child(note(reason.clone()));
            }
        }
    }
    column
}

/// `host: ` and the plugin's hint as a command to copy; it is never run from here.
fn sign_in_line(host: &str, hint: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let mono = crate::ui::theme::prefs(cx)
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    let group = SharedString::from("forge-sign-in");
    div()
        .group(group.clone())
        .px_4()
        .py_1()
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .meta()
        .text_color(theme.muted_foreground.opacity(0.8))
        .child(format!("{host}: "))
        .child(
            div()
                .min_w_0()
                .truncate()
                .px_1p5()
                .rounded_md()
                .bg(theme.muted)
                .font_family(mono)
                .child(hint.to_owned()),
        )
        .child(widgets::copy(
            SharedString::from("forge-sign-in-copy"),
            &group,
            hint,
            cx,
        ))
}

fn row(
    status: &RepositoryStatus,
    pull: &PullStatus,
    open: &HashMap<(u64, &'static str), bool>,
    cx: &mut Context<Workspace>,
) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, ok, bad) = (theme.muted_foreground, theme.success, theme.danger);
    let url = pull.url.clone();
    let pill = checks(pull, open, cx);
    widgets::row(("pull", pull.number as usize), false, cx)
        .h_auto()
        .py_1p5()
        .mx_2()
        .items_start()
        .child(div().pt_0p5().child(widgets::icon(
            state_icon(pull.state),
            state_color(pull.state, cx),
        )))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(div().small().truncate().child(pull.title.clone()))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .gap_1p5()
                        .meta()
                        .text_color(muted)
                        .child(format!("{} #{}", status.repository, pull.number))
                        .child(
                            div()
                                .text_color(ok)
                                .child(format!("+{}", grouped(pull.added))),
                        )
                        .child(
                            div()
                                .text_color(bad)
                                .child(format!("\u{2212}{}", grouped(pull.removed))),
                        )
                        .when_some(pill, |line, pill| line.child(div().flex_1()).child(pill)),
                ),
        )
        .on_click(move |_, _, cx| {
            // The core gives https links only; anything else is not opened.
            if url.starts_with("https://") || url.starts_with("http://") {
                cx.open_url(&url);
            }
        })
}

/// The checks at the right of the row's second line: a pill with a dot (green passed,
/// yellow and breathing while one runs, red failed) and "passed/total"; nothing without
/// checks. A press opens the checks grouped by state over the column.
fn checks(
    pull: &PullStatus,
    open: &HashMap<(u64, &'static str), bool>,
    cx: &Context<Workspace>,
) -> Option<AnyElement> {
    let state = pull.checks.state?;
    let theme = cx.theme();
    let dot = div().size_1p5().rounded_full().bg(check_color(state, cx));
    let dot = if state == CheckState::Pending {
        dot.looping(std::time::Duration::from_millis(1400), |dot, phase| {
            dot.opacity(0.35 + 0.65 * (phase * std::f32::consts::TAU).cos().abs())
        })
        .into_any_element()
    } else {
        dot.into_any_element()
    };
    let tip = SharedString::from(match state {
        CheckState::Pending => "CI: checks in progress",
        CheckState::Passing => "CI: checks passed",
        CheckState::Failing => "CI: checks failed",
    });
    let number = pull.number as usize;
    let hover = theme.list_active;
    let trigger = div()
        .id(("pull-checks", number))
        .role(Role::Button)
        .aria_label(checks_said(&pull.checks))
        .flex_none()
        .flex()
        .flex_row()
        .items_center()
        .gap_1()
        .px_1p5()
        .rounded_full()
        .bg(theme.muted)
        .cursor_pointer()
        .hover(move |pill| pill.bg(hover))
        .meta()
        .font_features(crate::pages::chat::working::tabular())
        .text_color(theme.muted_foreground)
        .child(dot)
        .child(format!("{}/{}", pull.checks.passed, pull.checks.total))
        .tooltip(move |window, cx| Tooltip::new(tip.clone()).build(window, cx))
        // The press is the window's, not the row's, which would open the pull request.
        .on_click(|_, _, cx| cx.stop_propagation());
    let shown = pull.clone();
    let open = open.clone();
    let ws = cx.entity().downgrade();
    Some(
        Popover::new(("pull-checks-window", number))
            .anchor(Anchor::TopRight)
            .trigger(Trigger(trigger))
            .content(move |_, _, cx| super::checks::window(&shown, &open, ws.clone(), cx))
            // The window pads its own rows; the popover's wide padding made a small list large.
            .p_1()
            .into_any_element(),
    )
}

/// A plain element as a popover's trigger; it draws no open state of its own.
struct Trigger(Stateful<Div>);

impl Selectable for Trigger {
    fn selected(self, _: bool) -> Self {
        self
    }

    fn is_selected(&self) -> bool {
        false
    }
}

impl IntoElement for Trigger {
    type Element = Stateful<Div>;

    fn into_element(self) -> Self::Element {
        self.0
    }
}

fn checks_said(checks: &Checks) -> String {
    let mut parts = vec![format!("{} passed", checks.passed)];
    if checks.running > 0 {
        parts.push(format!("{} running", checks.running));
    }
    if checks.failed > 0 {
        parts.push(format!("{} failed", checks.failed));
    }
    format!(
        "{}: {}",
        widgets::count(checks.total as usize, "check"),
        parts.join(", ")
    )
}

pub fn state_icon(state: PullState) -> Icon {
    Icon::default().path(match state {
        PullState::Merged => icons::GIT_MERGE,
        PullState::Closed => icons::GIT_PULL_CLOSED,
        PullState::Draft => icons::GIT_PULL_DRAFT,
        PullState::Open => icons::GIT_PULL,
    })
}

pub fn state_color(state: PullState, cx: &App) -> Hsla {
    let theme = cx.theme();
    match state {
        PullState::Open => theme.success,
        PullState::Merged => theme.magenta,
        PullState::Closed => theme.danger,
        PullState::Draft => theme.muted_foreground,
    }
}

pub fn check_color(state: CheckState, cx: &App) -> Hsla {
    let theme = cx.theme();
    match state {
        CheckState::Passing => theme.success,
        CheckState::Pending => theme.warning,
        CheckState::Failing => theme.danger,
    }
}

/// 1473 as "1,473".
pub fn grouped(n: u64) -> String {
    let digits = n.to_string();
    let mut out = String::with_capacity(digits.len() + digits.len() / 3);
    for (index, digit) in digits.chars().enumerate() {
        if index > 0 && (digits.len() - index).is_multiple_of(3) {
            out.push(',');
        }
        out.push(digit);
    }
    out
}

impl Workspace {
    /// Reads the shown project's repository status; the core then watches it for this
    /// window. A failure stays quiet: the pull requests are simply not shown.
    pub fn load_repository(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.project.clone() else {
            return;
        };
        let asked = id.clone();
        self.core.call(
            cx,
            move |client| async move { client.projects().repository_status(&asked).await },
            move |ws, result, cx| {
                if ws.project.as_deref() != Some(id.as_str()) {
                    return;
                }
                ws.repository = result.ok();
                crate::pages::chat::pull_link::show(ws, cx);
                cx.notify();
            },
        );
    }

    /// Asks for the pull requests of each project's branch. Nothing shows them any more,
    /// but the asking keeps the core reading every project's repository, not only the
    /// shown one, and each chat's pull requests (`Thread::pulls`) are kept fresh by
    /// those reads.
    pub fn load_pull_summaries(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.projects().pull_summaries().await },
            |_, _, _| {},
        );
    }

    /// The core read a project's repository again and it changed.
    pub fn repository_changed(
        &mut self,
        project_id: &str,
        status: RepositoryStatus,
        cx: &mut Context<Self>,
    ) {
        if self.project.as_deref() == Some(project_id) {
            self.repository = Some(status);
            crate::pages::chat::pull_link::show(self, cx);
        }
        self.load_pull_summaries(cx);
    }
}
