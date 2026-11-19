//! The checks of a pull request, in a window over its row in Changes: one row per state
//! with its count, which unfolds to a line per check with its name and workflow, and how
//! long it has run or how long ago it ended; a line opens the check's page. The core
//! orders the checks, the ones that need a look first, and caps them.

use std::collections::HashMap;

use dusk_protocol::project::{Check, CheckStage, PullStatus};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, icons, widgets},
    workspace::Workspace,
};

/// The groups in the order they show, each with its name.
const GROUPS: [(CheckStage, &str); 5] = [
    (CheckStage::Failed, "Failed"),
    (CheckStage::Running, "In progress"),
    (CheckStage::Queued, "Queued"),
    (CheckStage::Passed, "Passed"),
    (CheckStage::Skipped, "Skipped"),
];

/// The window over Changes: "CI monitoring" with a way to the checks' page, then a row
/// per state that has checks, scrolling inside when the unfolded list is long. It draws
/// from the shown status, so it follows the checks while it stays open. `folds` is which
/// groups the person unfolded or folded, kept by the workspace.
pub fn window(
    pull: &PullStatus,
    folds: &HashMap<(u64, &'static str), bool>,
    ws: WeakEntity<Workspace>,
    cx: &App,
) -> Div {
    let page = format!("{}/checks", pull.url);
    let mut groups = div().flex().flex_col();
    for (stage, name) in GROUPS {
        let unfolded = folds
            .get(&(pull.number, name))
            .copied()
            .unwrap_or(stage == CheckStage::Failed);
        groups = groups.children(group(pull, stage, name, unfolded, ws.clone(), cx));
    }
    div()
        .w(px(264.))
        .flex()
        .flex_col()
        .gap_1()
        .p_1()
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
                    Button::new(("pull-checks-open", pull.number as usize))
                        .ghost()
                        .xsmall()
                        .icon(IconName::ExternalLink)
                        .tooltip("Open the checks in the browser")
                        .on_click(move |_, _, cx| open(&page, cx)),
                ),
        )
        .child(
            div()
                .id(("pull-checks-list", pull.number as usize))
                .max_h(px(320.))
                .overflow_y_scroll()
                .child(groups),
        )
}

/// A state's row with its count, and its checks under it when unfolded; nothing when no
/// check is in that state.
fn group(
    pull: &PullStatus,
    stage: CheckStage,
    name: &'static str,
    unfolded: bool,
    ws: WeakEntity<Workspace>,
    cx: &App,
) -> Option<Div> {
    let checks = &pull.checks;
    let listed: Vec<(usize, &Check)> = checks
        .list
        .iter()
        .enumerate()
        .filter(|(_, check)| check.stage == stage)
        .collect();
    // The core counts failed, running and passed past the list's cap; queued and skipped
    // only as listed, so a capped list may show fewer of them than there are.
    let count = match stage {
        CheckStage::Failed => checks.failed as usize,
        CheckStage::Running => checks.running as usize,
        CheckStage::Passed => checks.passed as usize,
        CheckStage::Queued | CheckStage::Skipped => listed.len(),
    };
    if count == 0 {
        return None;
    }
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let number = pull.number;
    let id = SharedString::from(format!("pull-checks-group-{number}-{name}"));
    let header = div()
        .id(id.clone())
        .role(Role::Button)
        .aria_label(format!("{name}: {count}"))
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .px_1()
        .h(px(24.))
        .rounded_md()
        .cursor_pointer()
        .hover(|row| row.bg(cx.theme().list_hover))
        .child(stage_icon(stage, id, cx))
        .child(div().flex_1().min_w_0().truncate().small().child(name))
        .child(
            div()
                .flex_none()
                .meta()
                .font_features(crate::pages::chat::working::tabular())
                .text_color(muted)
                .child(count.to_string()),
        )
        .child(widgets::icon(
            Icon::new(if unfolded {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            }),
            muted,
        ))
        .on_click(move |_, _, cx| {
            // A closed window has no workspace left to fold anything in.
            let _ = ws.update(cx, |ws, cx| {
                ws.checks_open.insert((number, name), !unfolded);
                cx.notify();
            });
        });
    let mut column = div().flex().flex_col().child(header);
    if unfolded {
        let now = now();
        let mut lines = div().flex().flex_col().pl_3();
        for (index, check) in &listed {
            lines = lines.child(line(number, *index, check, now, cx));
        }
        let more = count.saturating_sub(listed.len());
        if more > 0 {
            let page = format!("{}/checks", pull.url);
            lines = lines.child(
                div()
                    .id(SharedString::from(format!(
                        "pull-checks-more-{number}-{name}"
                    )))
                    .px_1()
                    .h(px(22.))
                    .flex()
                    .items_center()
                    .rounded_md()
                    .cursor_pointer()
                    .hover(|line| line.bg(cx.theme().list_hover))
                    .meta()
                    .text_color(muted)
                    .child(format!("+{more} more"))
                    .on_click(move |_, _, cx| open(&page, cx)),
            );
        }
        column = column.child(lines);
    }
    Some(column)
}

fn line(number: u64, index: usize, check: &Check, now: i64, cx: &App) -> Stateful<Div> {
    let muted = cx.theme().muted_foreground;
    let url = check.url.clone();
    let id = SharedString::from(format!("pull-check-{number}-{index}"));
    div()
        .id(id.clone())
        .flex()
        .flex_row()
        .items_center()
        .gap_1p5()
        .px_1()
        .h(px(22.))
        .rounded_md()
        .when(!url.is_empty(), |line| {
            line.cursor_pointer()
                .hover(|line| line.bg(cx.theme().list_hover))
        })
        .child(stage_icon(check.stage, id, cx))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .small()
                .child(check.name.clone()),
        )
        .when(!check.workflow.is_empty(), |line| {
            line.child(
                div()
                    .flex_none()
                    .max_w(px(72.))
                    .truncate()
                    .meta()
                    .text_color(muted)
                    .child(check.workflow.clone()),
            )
        })
        .children(
            timing(check, now).map(|said| div().flex_none().meta().text_color(muted).child(said)),
        )
        .on_click(move |_, _, cx| open(&url, cx))
}

/// Passed green, failed red, running yellow and turning, queued and skipped muted.
fn stage_icon(stage: CheckStage, id: SharedString, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (icon, color) = match stage {
        CheckStage::Passed => (Icon::new(IconName::CircleCheck), theme.success),
        CheckStage::Failed => (Icon::new(IconName::CircleX), theme.danger),
        CheckStage::Running => (Icon::new(IconName::LoaderCircle), theme.warning),
        CheckStage::Queued => (
            Icon::default().path(icons::CHECK_QUEUED),
            theme.muted_foreground,
        ),
        CheckStage::Skipped => (
            Icon::default().path(icons::CHECK_SKIPPED),
            theme.muted_foreground,
        ),
    };
    let icon = widgets::icon(icon, color);
    if stage == CheckStage::Running {
        icon.with_animation(
            id,
            Animation::new(std::time::Duration::from_millis(1200)).repeat(),
            |icon, delta| icon.rotate(percentage(delta)),
        )
        .into_any_element()
    } else {
        icon.into_any_element()
    }
}

/// How long a running check has run, or how long ago one ended; nothing unknown.
fn timing(check: &Check, now: i64) -> Option<String> {
    match (check.started, check.finished) {
        (_, Some(finished)) => Some(format!("{} ago", ago(now - finished))),
        (Some(started), None) if matches!(check.stage, CheckStage::Running) => Some(
            dusk_protocol::thread::duration_words((now - started).saturating_mul(1000)),
        ),
        _ => None,
    }
}

/// `seconds` in its largest whole unit: "now", "4m", "4h", "3d".
pub fn ago(seconds: i64) -> String {
    match seconds.max(0) {
        0..60 => "now".to_owned(),
        s @ 60..3600 => format!("{}m", s / 60),
        s @ 3600..86400 => format!("{}h", s / 3600),
        s => format!("{}d", s / 86400),
    }
}

/// The time now, in the unix seconds the core gives.
pub fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |since| since.as_secs() as i64)
}

/// The core gives https links only; anything else is not opened.
fn open(url: &str, cx: &mut App) {
    if url.starts_with("https://") {
        cx.open_url(url);
    }
}
