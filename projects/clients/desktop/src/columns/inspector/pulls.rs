//! The pull requests under the Changes module: a state icon, the title, the repository
//! with the number and the lines, and a dot for the checks. The core reads them, keeps
//! them fresh while the project is shown, and says when they change; a row opens its page.

use dusk_protocol::project::{CheckState, Checks, PullState, PullStatus, RepositoryStatus};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
        hover_card::HoverCard,
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, icons, widgets},
    workspace::Workspace,
};

/// One row per pull request, and the quiet reason when GitHub could not be asked.
pub fn rows(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let Some(status) = &ws.repository else {
        return div();
    };
    let mut column = div().flex().flex_col();
    for pull in &status.pulls {
        column = column.child(row(status, pull, cx));
    }
    if let Some(reason) = &status.unavailable {
        column = column.child(
            div()
                .px_4()
                .py_1()
                .meta()
                .text_color(cx.theme().muted_foreground.opacity(0.8))
                .child(reason.clone()),
        );
    }
    column
}

fn row(status: &RepositoryStatus, pull: &PullStatus, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, ok, bad) = (theme.muted_foreground, theme.success, theme.danger);
    let url = pull.url.clone();
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
                .child(div().text_sm().truncate().child(pull.title.clone()))
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
                        ),
                ),
        )
        .when_some(checks_dot(pull, cx), |row, dot| row.child(dot))
        .on_click(move |_, _, cx| {
            // The core gives https links only; anything else is not opened.
            if url.starts_with("https://") || url.starts_with("http://") {
                cx.open_url(&url);
            }
        })
}

/// A dot for the checks: green passed, yellow and breathing while one runs, red failed;
/// nothing without checks. Under the pointer it opens a card that counts them by state.
fn checks_dot(pull: &PullStatus, cx: &App) -> Option<AnyElement> {
    let state = pull.checks.state?;
    let color = check_color(state, cx);
    let dot = div().size_2().rounded_full().bg(color);
    let dot = if state == CheckState::Pending {
        dot.with_animation(
            SharedString::from(format!("pull-running-{}", pull.number)),
            Animation::new(std::time::Duration::from_millis(1400)).repeat(),
            |dot, phase| dot.opacity(0.35 + 0.65 * (phase * std::f32::consts::TAU).cos().abs()),
        )
        .into_any_element()
    } else {
        dot.into_any_element()
    };
    let (checks, url, said) = (
        pull.checks.clone(),
        pull.url.clone(),
        checks_said(&pull.checks),
    );
    let number = pull.number as usize;
    Some(
        HoverCard::new(("pull-checks-card", number))
            .anchor(Anchor::TopRight)
            .open_delay(std::time::Duration::from_millis(150))
            .trigger(
                // Wider than the dot, so the pointer finds it; a press on it is the card's,
                // not the row's, which would open the pull request.
                div()
                    .id(("pull-checks", number))
                    .role(Role::Button)
                    .aria_label(said)
                    .flex_none()
                    .size_5()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .child(dot)
                    .on_click(|_, _, cx| cx.stop_propagation()),
            )
            .content(move |_, _, cx| checks_card(number, &checks, &url, cx))
            // The card pads its own rows; the popover's wide padding made a small list large.
            .p_1()
            .into_any_element(),
    )
}

/// "CI monitoring": a way to the checks' page, then a row for each state that has any.
fn checks_card(number: usize, checks: &Checks, url: &str, cx: &App) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let page = format!("{url}/checks");
    let rows = [
        (
            IconName::CircleCheck,
            "Passed",
            checks.passed,
            theme.success,
        ),
        (
            IconName::LoaderCircle,
            "Running",
            checks.running,
            theme.warning,
        ),
        (IconName::CircleX, "Failed", checks.failed, theme.danger),
    ];
    div()
        .w(px(208.))
        .flex()
        .flex_col()
        .gap_1()
        .p_2()
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .pl_1()
                .child(
                    div()
                        .flex_1()
                        .small()
                        .font_weight(FontWeight::MEDIUM)
                        .child("CI monitoring"),
                )
                .child(
                    Button::new(("pull-checks-open", number))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ExternalLink)
                        .tooltip("Open the checks in the browser")
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            // The core gives https links only; anything else is not opened.
                            if page.starts_with("https://") {
                                cx.open_url(&page);
                            }
                        }),
                ),
        )
        .children(rows.into_iter().filter(|(_, _, count, _)| *count > 0).map(
            |(icon, word, count, color)| {
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap_2()
                    .h(px(24.))
                    .px_1()
                    .small()
                    .child(widgets::icon(icon, color))
                    .child(div().flex_1().child(word))
                    .child(div().text_color(muted).child(count.to_string()))
            },
        ))
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

pub fn state_name(state: PullState) -> &'static str {
    match state {
        PullState::Open => "open",
        PullState::Draft => "draft",
        PullState::Merged => "merged",
        PullState::Closed => "closed",
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
                cx.notify();
            },
        );
    }

    /// The pull requests of each project's branch, for the icons beside its chats.
    pub fn load_pull_summaries(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.projects().pull_summaries().await },
            |ws, result, cx| {
                if let Ok(summaries) = result {
                    ws.pulls = summaries
                        .into_iter()
                        .map(|summary| (summary.project_id.clone(), summary))
                        .collect();
                    cx.notify();
                }
            },
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
        }
        self.load_pull_summaries(cx);
    }
}
