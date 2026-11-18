//! A list of checks the model reports, each with the state it is in. The card says in its
//! head that it is "from the agent": it is a snapshot the model wrote, not what a forge or
//! a test run reports. It has no button.

use crate::ui::elevation::{Elevation, Level};
use dusk_protocol::thread::{Check, CheckState, Checks};
use gpui_kit::{component::ActiveTheme, *};

use super::glyph::{self, Tones};
use crate::{pages::chat::working::tabular, ui::Type};

pub fn render(checks: &Checks, cx: &App) -> AnyElement {
    let theme = cx.theme();
    let (muted, hairline) = (theme.muted_foreground, theme.border);
    let tones = Tones {
        muted,
        accent: theme.primary,
        success: theme.success,
        danger: theme.danger,
        track: theme.foreground.opacity(0.14),
    };
    div()
        .flex()
        .flex_col()
        .rounded(px(12.))
        .elevation(Level::Raised, cx)
        .overflow_hidden()
        .child(
            div()
                .h(px(36.))
                .px_4()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .border_b_1()
                .border_color(hairline)
                .child(
                    div()
                        .small()
                        .font_weight(FontWeight::MEDIUM)
                        .child("Checks"),
                )
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .meta()
                        .text_color(muted)
                        .child(checks.summary()),
                )
                .child(div().flex_1())
                .child(
                    div()
                        .flex_none()
                        .meta()
                        .text_color(muted)
                        .child("from the agent"),
                ),
        )
        .children(checks.items().iter().map(|check| row(check, tones)))
        .into_any_element()
}

fn row(check: &Check, tones: Tones) -> Div {
    div()
        .min_h(px(if check.detail.is_some() { 48. } else { 32. }))
        .px_4()
        .py_1()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .child(glyph::state(check.state, tones))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .small()
                        .child(check.name.to_string()),
                )
                .children(check.detail.as_ref().map(|detail| {
                    div()
                        .min_w_0()
                        .truncate()
                        .meta()
                        .text_color(tones.muted)
                        .child(detail.to_string())
                })),
        )
        .child(
            div()
                .flex_none()
                .meta()
                .text_color(tones.muted)
                .font_features(tabular())
                .child(tally(check)),
        )
}

/// What stands at the right of a row: the counts when the model gave them, else the state
/// where the glyph alone says too little. A tick or a cross needs no word beside it.
fn tally(check: &Check) -> String {
    match check.counts {
        Some(counts) if counts.failed() > 0 => format!(
            "{} / {} \u{00B7} {} failed",
            counts.passed(),
            counts.total(),
            counts.failed()
        ),
        Some(counts) => format!("{} / {}", counts.passed(), counts.total()),
        None => match check.state {
            CheckState::Passed | CheckState::Failed => String::new(),
            other => other.name().to_owned(),
        },
    }
}

#[cfg(test)]
mod tests {
    use dusk_protocol::thread::{Block, Check};

    use super::tally;

    fn check(json: serde_json::Value) -> Check {
        let block: Block =
            serde_json::from_value(serde_json::json!({ "checks": { "items": [json] } })).unwrap();
        match block {
            Block::Checks(checks) => checks.items()[0].clone(),
            _ => unreachable!("checks were asked for"),
        }
    }

    #[test]
    fn the_right_of_a_row_is_the_counts_else_the_state_that_a_glyph_says_too_little_of() {
        let named = |state: &str, counts: serde_json::Value| {
            let mut item = serde_json::json!({ "name": "t", "state": state });
            if !counts.is_null() {
                item["counts"] = counts;
            }
            tally(&check(item))
        };
        let counts = |passed: u32, failed: u32, total: u32| serde_json::json!({ "passed": passed, "failed": failed, "total": total });
        assert_eq!(named("passed", counts(214, 0, 214)), "214 / 214");
        assert_eq!(
            named("failed", counts(11, 1, 12)),
            "11 / 12 \u{b7} 1 failed"
        );
        assert_eq!(named("running", serde_json::Value::Null), "running");
        assert_eq!(named("skipped", serde_json::Value::Null), "skipped");
        assert_eq!(named("passed", serde_json::Value::Null), "");
    }
}
