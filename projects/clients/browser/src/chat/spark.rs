//! The spark of a thinking turn: six rays that ease from shape to shape, drawn as one
//! SVG path whose outline is worked out thirty times a second. It stands on its first
//! shape when the system asks for less motion or the page is hidden.

use leptos::prelude::*;
use std::f32::consts::{FRAC_PI_2, PI, TAU};

/// The shapes it passes through, each as how far its rays reach, how thick they are at
/// the middle and at the tip, in shares of half the square: a pointed star, petals, a
/// stout star, a dot, a thin asterisk.
const SHAPES: [[f32; 3]; 5] = [
    [0.84, 0.30, 0.05],
    [0.90, 0.09, 0.30],
    [0.66, 0.22, 0.22],
    [0.16, 0.36, 0.36],
    [0.94, 0.11, 0.11],
];
/// The share of each shape's time it stands before easing into the next.
const HOLD: f32 = 0.42;
/// One pass through all the shapes, in milliseconds.
const CYCLE_MS: f64 = 2600.;
const FRAME_MS: u64 = 33;

fn ease_in_out(t: f32) -> f32 {
    let t = t.clamp(0., 1.);
    if t < 0.5 {
        4. * t * t * t
    } else {
        1. - (-2. * t + 2.).powi(3) / 2.
    }
}

/// The path at `phase` of the loop, in a square of 16 around (8, 8). It turns a sixth
/// between shapes, which six rays hide once it stands, so the loop has no seam.
pub fn path(phase: f32) -> String {
    let through = phase.clamp(0., 1.) * SHAPES.len() as f32;
    let index = (through as usize).min(SHAPES.len() - 1);
    let moved = ease_in_out((through - index as f32 - HOLD) / (1. - HOLD));
    let (from, to) = (SHAPES[index], SHAPES[(index + 1) % SHAPES.len()]);
    let part = |at: usize| (from[at] + (to[at] - from[at]) * moved) * 7.;
    let (reach, middle, tip) = (part(0), part(1), part(2));
    let turned = (index as f32 + moved) * TAU / 6.;
    let long = (reach - tip / 2.).max(0.);
    let mut d = String::new();
    for ray in 0..6 {
        let (sin, cos) = (turned - FRAC_PI_2 + ray as f32 * TAU / 6.).sin_cos();
        let mut place = |command: char, along: f32, across: f32| {
            let (x, y) = (
                8. + along * cos - across * sin,
                8. + along * sin + across * cos,
            );
            d.push_str(&format!("{command}{x:.2} {y:.2}"));
        };
        place('M', 0., -middle / 2.);
        // The tip is round: half a turn in a few steps.
        for step in 0..=6 {
            let around = -FRAC_PI_2 + PI * step as f32 / 6.;
            place('L', long + tip / 2. * around.cos(), tip / 2. * around.sin());
        }
        place('L', 0., middle / 2.);
        d.push('Z');
    }
    // The middle, a disc as thick as the rays there.
    let r = middle / 2.;
    d.push_str(&format!(
        "M{:.2} 8a{r:.2} {r:.2} 0 1 0 {:.2} 0a{r:.2} {r:.2} 0 1 0 {:.2} 0Z",
        8. - r,
        2. * r,
        -2. * r
    ));
    d
}

/// Whether the page may move: it is in sight and the system did not ask for less motion.
fn may_move() -> bool {
    let Some(window) = web_sys::window() else {
        return false;
    };
    if window.document().is_some_and(|document| document.hidden()) {
        return false;
    }
    // `matchMedia` is reached by name, as its type is not among this crate's features.
    let asks_less = js_sys::Reflect::get(&window, &"matchMedia".into())
        .ok()
        .and_then(|f| {
            use wasm_bindgen::JsCast;
            f.dyn_into::<js_sys::Function>().ok()
        })
        .and_then(|f| {
            f.call1(&window, &"(prefers-reduced-motion: reduce)".into())
                .ok()
        })
        .and_then(|list| js_sys::Reflect::get(&list, &"matches".into()).ok())
        .and_then(|matches| matches.as_bool());
    asks_less != Some(true)
}

#[component]
pub fn Spark(
    /// Nothing came from the model for a long while: it turns toward the warning colour.
    stalled: bool,
) -> impl IntoView {
    let d = RwSignal::new(path(0.));
    let handle = set_interval_with_handle(
        move || {
            if may_move() {
                let phase = (js_sys::Date::now() % CYCLE_MS) / CYCLE_MS;
                d.set(path(phase as f32));
            }
        },
        std::time::Duration::from_millis(FRAME_MS),
    )
    .ok();
    on_cleanup(move || {
        if let Some(handle) = handle {
            handle.clear();
        }
    });
    view! {
        <svg class="act-spark" class:stalled=stalled viewBox="0 0 16 16" aria-hidden="true">
            <path d=move || d.get()></path>
        </svg>
    }
}

#[cfg(test)]
mod tests {
    use super::path;

    #[test]
    fn the_spark_is_one_closed_outline_per_ray_and_the_middle() {
        for phase in [0., 0.13, 0.5, 0.87, 1.] {
            let d = path(phase);
            assert_eq!(d.matches('Z').count(), 7, "{phase}");
            assert!(!d.contains("NaN") && !d.contains("inf"), "{d}");
        }
    }
}
