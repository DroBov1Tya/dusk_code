//! Every check of a pull request, unfolded under its row in Changes: one line each with
//! its state, its name and workflow, and how long it has run or how long ago it ended;
//! a line opens the check's page. The core orders them, the ones that need a look
//! first, and caps them; the fold is the window's own.

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

/// The chevron that unfolds the checks; none without checks to list.
pub fn toggle(ws: &Workspace, pull: &PullStatus, cx: &mut Context<Workspace>) -> Option<Button> {
    if pull.checks.list.is_empty() {
        return None;
    }
    let number = pull.number;
    let open = ws.open_checks.contains(&number);
    Some(
        Button::new(("pull-checks-fold", number as usize))
            .ghost()
            .xsmall()
            .icon(if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            })
            .tooltip(if open {
                "Hide the checks"
            } else {
                "Show every check"
            })
            .on_click(cx.listener(move |ws, _, _, cx| {
                // The row under the chevron would open the pull request.
                cx.stop_propagation();
                if !ws.open_checks.remove(&number) {
                    ws.open_checks.insert(number);
                }
                cx.notify();
            })),
    )
}

/// The lines of the checks, and one for those the core did not list.
pub fn list(pull: &PullStatus, cx: &App) -> Div {
    let now = now();
    let muted = cx.theme().muted_foreground;
    let mut column = div().flex().flex_col().ml(px(36.)).mr_2().pb_1();
    for (index, check) in pull.checks.list.iter().enumerate() {
        column = column.child(line(pull.number, index, check, now, cx));
    }
    let more = (pull.checks.total as usize).saturating_sub(pull.checks.list.len());
    if more > 0 {
        let page = format!("{}/checks", pull.url);
        column = column.child(
            div()
                .id(("pull-checks-more", pull.number as usize))
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
    column
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
