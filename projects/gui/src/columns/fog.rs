//! Mist at the foot of the window when nobody has touched it for a while: it rises
//! slowly while the window is left alone, drifts, and clears in a moment at the first
//! click or key. It is drawn over everything and takes no clicks. Appearance can switch
//! it off.

use std::time::{Duration, Instant};

use gpui_kit::{component::ActiveTheme, *};

use crate::workspace::Workspace;

/// How long the window is left alone before the mist starts.
const AFTER: Duration = Duration::from_secs(4 * 60);
/// How long it takes to rise fully from there.
const RISE: Duration = Duration::from_secs(6 * 60);
/// How long it takes to clear.
const CLEAR: Duration = Duration::from_millis(700);
/// The drift's cycle; every wisp goes round a whole number of times in it.
const DRIFT: Duration = Duration::from_secs(48);
/// How long the dawn takes: the mist lifting as a turn ends in a window left alone.
const DAWN: Duration = Duration::from_millis(2600);
/// How much of the window's height the mist reaches when full.
const REACH: f32 = 0.34;

/// How thick the mist stands now, 0 to 1, from how long the window was left alone.
fn risen(idle: Duration) -> f32 {
    (idle.saturating_sub(AFTER).as_secs_f32() / RISE.as_secs_f32()).min(1.)
}

impl Workspace {
    /// A click or a key: the window is attended again, and the mist that stood clears.
    pub fn attended(&mut self, cx: &mut Context<Self>) {
        let level = risen(self.last_input.elapsed());
        self.last_input = Instant::now();
        if level > 0. {
            self.fog_cleared = Some((Instant::now(), level));
            cx.notify();
        }
    }

    /// A turn ended while the mist stood: the work is done, and the day breaks. The mist
    /// lifts instead of clearing, with a warm light along the foot of the window.
    pub fn dawn(&mut self, cx: &mut Context<Self>) {
        let level = risen(self.last_input.elapsed());
        if level > 0. {
            self.last_input = Instant::now();
            self.fog_dawn = Some((Instant::now(), level));
            cx.notify();
        }
    }

    /// A window left alone draws nothing, so nothing would bring the mist up: this looks
    /// now and then, and asks for a frame once it is due. From there the drift keeps it.
    pub fn watch_idle(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor().timer(AFTER / 8).await;
                let alive = this.update(cx, |this, cx| {
                    if this.last_input.elapsed() >= AFTER {
                        cx.notify();
                    }
                });
                if alive.is_err() {
                    return;
                }
            }
        })
        .detach();
    }
}

pub fn render(ws: &Workspace, window: &Window, cx: &App) -> Option<AnyElement> {
    if !crate::ui::theme::prefs(cx).fog {
        return None;
    }
    let theme = cx.theme();
    // Light mist on a dark theme, a grey one on a light theme.
    let color = theme.muted_foreground;
    let height = window.viewport_size().height.as_f32() * REACH;
    if let Some((at, level)) = ws.fog_dawn.filter(|(at, _)| at.elapsed() < DAWN) {
        let id = SharedString::from(format!("fog-dawn-{at:?}"));
        let warm = hsla(0.07, 0.95, 0.64, 1.);
        return Some(
            div()
                .absolute()
                .inset_0()
                .with_animation(
                    id,
                    Animation::new(DAWN).with_easing(ease_in_out),
                    move |holder, delta| {
                        // The light swells and goes; the mist thins as it climbs.
                        let light = (delta * std::f32::consts::PI).sin();
                        holder
                            .child(
                                div()
                                    .absolute()
                                    .left_0()
                                    .right_0()
                                    .bottom_0()
                                    .h(px(height * (0.4 + delta * 0.5)))
                                    .bg(linear_gradient(
                                        0.,
                                        linear_color_stop(warm.opacity(0.30 * light), 0.),
                                        linear_color_stop(warm.opacity(0.), 1.),
                                    )),
                            )
                            .child(layer(
                                height * (1. + delta * 0.9),
                                level * (1. - delta),
                                delta * 0.05,
                                color,
                            ))
                    },
                )
                .into_any_element(),
        );
    }
    if let Some((at, level)) = ws.fog_cleared.filter(|(at, _)| at.elapsed() < CLEAR) {
        let id = SharedString::from(format!("fog-clear-{at:?}"));
        return Some(
            layer(height, level, 0., color)
                .with_animation(
                    id,
                    Animation::new(CLEAR).with_easing(ease_in_out),
                    |fog, delta| fog.opacity(1. - delta),
                )
                .into_any_element(),
        );
    }
    let level = risen(ws.last_input.elapsed());
    if level <= 0. {
        return None;
    }
    let since = ws.last_input;
    Some(
        div()
            .absolute()
            .inset_0()
            .with_animation(
                "fog-drift",
                Animation::new(DRIFT).repeat().with_max_fps(12.),
                move |holder, phase| {
                    // The level is read anew each frame, so the mist rises as it drifts.
                    holder.child(layer(height, risen(since.elapsed()), phase, color))
                },
            )
            .into_any_element(),
    )
}

/// The columns the mist is drawn in; enough of them that its top reads as one soft line.
const COLUMNS: usize = 64;

/// The mist at one moment: columns that each fade upward to nothing, their heights
/// following two slow waves that pass through each other, so its top rolls and nothing
/// in it has an edge.
fn layer(height: f32, level: f32, phase: f32, color: Hsla) -> Div {
    use std::f32::consts::TAU;
    let reach = height * (0.35 + 0.65 * level);
    let mut fog = div()
        .absolute()
        .left_0()
        .right_0()
        .bottom_0()
        .h(px(reach))
        .flex()
        .flex_row()
        .items_end();
    for column in 0..COLUMNS {
        let across = column as f32 / COLUMNS as f32;
        // One wave goes right once a cycle, the other left twice, at different lengths.
        let swell = ((across * 1.5 - phase) * TAU).sin() * 0.5
            + ((across * 3.5 + phase * 2.) * TAU).sin() * 0.3
            + 0.2;
        let rise = 0.72 + 0.28 * swell.clamp(-1., 1.);
        fog = fog.child(div().flex_1().h(px(reach * rise)).bg(linear_gradient(
            0.,
            linear_color_stop(color.opacity(0.42 * level), 0.),
            linear_color_stop(color.opacity(0.), 1.),
        )));
    }
    fog
}
