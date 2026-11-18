//! The meters of the composer's panel: a ring in four equal quarters, filled as what it
//! measures fills and colored by how full it is, so its state reads at a glance.

use std::f32::consts::{FRAC_PI_2, TAU};

use gpui_kit::{component::ActiveTheme, *};

/// One ring: what it measures, the share of it used, and the numbers its tooltip says.
pub struct Meter {
    pub label: SharedString,
    pub used: f32,
    pub detail: String,
}

/// A meter as a small ring, its numbers on hover. Nearly full, it breathes: time to act,
/// said before the limit says it.
pub fn meter(id: usize, meter: &Meter, size: f32, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let used = meter.used.clamp(0., 1.);
    let fill = if used > 0.9 {
        theme.danger
    } else if used > 0.7 {
        theme.warning
    } else {
        theme.primary
    };
    let tip = SharedString::from(format!(
        "{}: {:.0}% \u{00B7} {}",
        meter.label,
        used * 100.,
        meter.detail
    ));
    let breathes = used > 0.85;
    div()
        .id(("meter", id))
        .flex_none()
        .child(render(size, used, fill, theme.border).with_animation(
            if breathes {
                "meter-breath"
            } else {
                "meter-still"
            },
            Animation::new(std::time::Duration::from_millis(2600)).repeat(),
            move |ring, phase| {
                let breath = (phase * TAU).sin() * 0.5 + 0.5;
                ring.opacity(if breathes { 0.5 + breath * 0.5 } else { 1. })
            },
        ))
        .tooltip(move |window, cx| {
            gpui_kit::component::tooltip::Tooltip::new(tip.clone()).build(window, cx)
        })
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
