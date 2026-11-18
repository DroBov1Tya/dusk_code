//! The rings of the message box: how full the context is, beside the box, and how much of
//! the plan's limits is used, in the panel over it. A ring is four equal quarters, filled
//! as what it measures fills and colored by how full it is, so its state reads at a glance.

use std::f32::consts::{FRAC_PI_2, TAU};

use dusk_protocol::{
    client::CHATGPT_USAGE_URL,
    model::{PlanWindow, RateLimit, RateLimits},
};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        hover_card::HoverCard,
    },
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// A ring's color by how full it is.
fn fullness(used: f32, cx: &App) -> Hsla {
    let theme = cx.theme();
    if used > 0.9 {
        theme.danger
    } else if used > 0.7 {
        theme.warning
    } else {
        theme.primary
    }
}

/// The empty part of a ring: the quiet text color, thinned. The hairline's own color left
/// an empty quarter all but unseen, and a ring with unseen quarters reads as a spinner.
fn track(cx: &App) -> Hsla {
    cx.theme().muted_foreground.opacity(0.3)
}

/// How full the context is: a ring the size of the marks beside the box, with the share
/// in its middle; the counts and the cost on hover. Nearly full, it breathes: time to
/// compact, said before the limit says it.
pub fn context(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let size = super::composer::SIDE_MARK;
    let muted = cx.theme().muted_foreground;
    let (used, limit) = ws
        .shown_chat()
        .and_then(|chat| chat.tokens)
        .unwrap_or((0, ws.status.input_limit));
    let share = if limit == 0 {
        0.
    } else {
        (used as f32 / limit as f32).min(1.)
    };
    let mut tip = format!(
        "Context: {} of {} tokens",
        widgets::short(used),
        widgets::short(limit)
    );
    if let Some(cost) = ws.shown_chat().and_then(|chat| chat.cost.clone()) {
        tip.push_str(&format!(" \u{00B7} {cost}"));
    }
    let tip = SharedString::from(tip);
    let breathes = share > 0.85;
    div()
        .id("context-ring")
        .relative()
        .size(px(size))
        .flex()
        .items_center()
        .justify_center()
        .child(
            div()
                .absolute()
                .inset_0()
                .child(render(size, share, fullness(share, cx), track(cx)))
                .with_animation(
                    if breathes {
                        "context-breath"
                    } else {
                        "context-still"
                    },
                    Animation::new(std::time::Duration::from_millis(2600)).repeat(),
                    move |ring, phase| {
                        let breath = (phase * TAU).sin() * 0.5 + 0.5;
                        ring.opacity(if breathes { 0.5 + breath * 0.5 } else { 1. })
                    },
                ),
        )
        .child(
            div()
                .text_size(px(9.))
                .font_weight(FontWeight::MEDIUM)
                .font_features(super::working::tabular())
                .text_color(muted)
                .child(format!("{:.0}", share * 100.)),
        )
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
}

/// The share the limits ring shows: the fullest of the plan's windows, so a week that is
/// nearly spent is not hidden behind five fresh hours. None without a plan window.
pub fn plan_used(limits: &RateLimits) -> Option<f32> {
    limits
        .plan
        .iter()
        .map(|window| window.used_percent.min(100))
        .max()
        .map(|percent| f32::from(percent) / 100.)
}

/// The plan's limits as one small ring, when the provider reported a window of the plan;
/// under the pointer it opens a card with every window. No ring otherwise: a limit per
/// minute is a row of the card, not a meter to watch.
pub fn limits(ws: &Workspace, size: f32, cx: &App) -> Option<AnyElement> {
    let limits = ws.status.limits.clone()?;
    let used = plan_used(&limits)?;
    let usage_page = ws.status.chatgpt_model;
    let said = SharedString::from(format!("Plan usage limits: {:.0}% used", used * 100.));
    Some(
        HoverCard::new("plan-limits-card")
            .anchor(Anchor::BottomRight)
            .open_delay(std::time::Duration::from_millis(150))
            .trigger(
                div()
                    .id("plan-limits")
                    .role(Role::Button)
                    .aria_label(said)
                    .flex_none()
                    .size_5()
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_md()
                    .child(render(size, used, fullness(used, cx), track(cx))),
            )
            .content(move |_, _, cx| limits_card(&limits, usage_page, cx))
            // The card pads its own rows, as the checks' card does.
            .p_1()
            .into_any_element(),
    )
}

/// "Plan usage limits": a row and a thin bar for each window of the plan, then the
/// limits per minute as plain rows. `usage_page` adds the way to the plan's own page.
fn limits_card(limits: &RateLimits, usage_page: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let mut card = div().w(px(264.)).flex().flex_col().gap_2().p_2().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .h(px(20.))
            .child(
                div()
                    .flex_1()
                    .small()
                    .font_weight(FontWeight::MEDIUM)
                    .child("Plan usage limits"),
            )
            .children(usage_page.then(|| {
                Button::new("plan-limits-open")
                    .ghost()
                    .xsmall()
                    .icon(IconName::ExternalLink)
                    .tooltip("Open ChatGPT usage settings")
                    .on_click(|_, _, cx| cx.open_url(CHATGPT_USAGE_URL))
            })),
    );
    for window in &limits.plan {
        card = card.child(plan_row(window, cx));
    }
    let minute = [("Requests", &limits.requests), ("Tokens", &limits.tokens)];
    for (label, limit) in minute {
        if let Some(limit) = limit.as_ref().filter(|limit| limit.limit > 0) {
            card = card.child(minute_row(label, limit, muted));
        }
    }
    card
}

/// One window: its name, when it resets and the share used, over a bar filled that far.
fn plan_row(window: &PlanWindow, cx: &App) -> Div {
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let percent = window.used_percent.min(100);
    let fill = match percent {
        90.. => theme.danger,
        75.. => theme.warning,
        _ => theme.primary,
    };
    div()
        .flex()
        .flex_col()
        .gap_1p5()
        .child(
            div()
                .flex()
                .flex_row()
                .items_baseline()
                .gap_2()
                .small()
                .child(div().flex_1().child(window_name(window.minutes)))
                .children(window.resets_in.map(|seconds| {
                    div()
                        .meta()
                        .text_color(muted)
                        .child(format!("Resets in {}", words(seconds)))
                }))
                .child(
                    div()
                        .w(px(36.))
                        .text_right()
                        .font_features(super::working::tabular())
                        .child(format!("{percent}%")),
                ),
        )
        .child(
            div().h(px(3.)).w_full().rounded_full().bg(track(cx)).child(
                div()
                    .h_full()
                    .w(relative(f32::from(percent) / 100.))
                    .rounded_full()
                    .bg(fill),
            ),
        )
}

fn minute_row(label: &str, limit: &RateLimit, muted: Hsla) -> Div {
    let mut left = format!(
        "{} of {} left this minute",
        widgets::short(limit.remaining),
        widgets::short(limit.limit)
    );
    if let Some(seconds) = limit.resets_in {
        left.push_str(&format!(", resets in {}", words(seconds)));
    }
    div()
        .flex()
        .flex_row()
        .items_baseline()
        .gap_2()
        .small()
        .child(div().flex_1().child(label.to_owned()))
        .child(div().meta().text_color(muted).child(left))
}

/// A plan window's name from its length; the provider gives no other.
fn window_name(minutes: Option<u64>) -> String {
    match minutes {
        Some(300) => "5-hour limit".into(),
        Some(10_080) => "Weekly".into(),
        Some(minutes) if minutes.is_multiple_of(1440) => format!("{}-day limit", minutes / 1440),
        Some(minutes) if minutes.is_multiple_of(60) => format!("{}-hour limit", minutes / 60),
        Some(minutes) => format!("{minutes}-minute limit"),
        None => "Plan limit".into(),
    }
}

fn words(seconds: u64) -> String {
    let ms = i64::try_from(seconds).map_or(i64::MAX, |s| s.saturating_mul(1000));
    dusk_protocol::thread::duration_words(ms)
}

const STEPS: usize = 24;
/// The gap between quarters, in radians.
const GAP: f32 = 0.22;

fn render(size: f32, share: f32, fill: Hsla, track: Hsla) -> Div {
    let share = share.clamp(0., 1.);
    div().size(px(size)).flex_none().child(
        canvas(
            |_, _, _| {},
            move |bounds, _, window, _| {
                let center = bounds.center();
                let radius = bounds.size.width.min(bounds.size.height) / 2. - px(2.);
                let arc = |from: f32, to: f32, color: Hsla, window: &mut Window| {
                    if to <= from {
                        return;
                    }
                    let mut path = PathBuilder::stroke(px(2.5));
                    for step in 0..=STEPS {
                        let angle = from + (to - from) * step as f32 / STEPS as f32 - FRAC_PI_2;
                        let at = point(
                            center.x + radius * angle.cos(),
                            center.y + radius * angle.sin(),
                        );
                        if step == 0 {
                            path.move_to(at);
                        } else {
                            path.line_to(at);
                        }
                    }
                    if let Ok(path) = path.build() {
                        window.paint_path(path, color);
                    }
                };
                let quarter = TAU / 4.;
                for part in 0..4 {
                    let from = part as f32 * quarter + GAP / 2.;
                    let to = from + quarter - GAP;
                    arc(from, to, track, window);
                    // How much of this quarter the context covers.
                    let covered = (share * 4. - part as f32).clamp(0., 1.);
                    arc(from, from + (to - from) * covered, fill, window);
                }
            },
        )
        .size_full(),
    )
}

#[cfg(test)]
mod tests {
    use super::{PlanWindow, RateLimits, plan_used};

    fn limits(plan: &[(u64, u8)]) -> RateLimits {
        RateLimits {
            requests: None,
            tokens: None,
            plan: plan
                .iter()
                .map(|&(minutes, used_percent)| PlanWindow {
                    minutes: Some(minutes),
                    used_percent,
                    resets_in: None,
                })
                .collect(),
        }
    }

    #[test]
    fn the_ring_shows_the_fuller_window_and_nothing_without_a_plan() {
        assert_eq!(plan_used(&limits(&[(300, 22), (10_080, 64)])), Some(0.64));
        assert_eq!(plan_used(&limits(&[(300, 88), (10_080, 40)])), Some(0.88));
        assert_eq!(plan_used(&limits(&[(300, 5)])), Some(0.05));
        assert_eq!(plan_used(&limits(&[])), None);
    }
}
