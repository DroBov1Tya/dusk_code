//! A sound as a card: its tile, title, play button, bar and time.

use super::player::sound_path;
use crate::pages::chat::transcript::Clip;
use crate::{ui::Type, workspace::Workspace};
use gpui_kit::{
    component::{
        ActiveTheme, IconName,
        button::{Button, ButtonRounded, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};
use std::{cell::Cell, rc::Rc, time::Duration};

pub(super) fn kind_label(mime: &str) -> &'static str {
    match mime {
        "audio/mpeg" => "MP3 audio",
        "audio/wav" => "WAV audio",
        "audio/ogg" => "OGG audio",
        "audio/flac" => "FLAC audio",
        "audio/mp4" => "M4A audio",
        _ => "Audio",
    }
}

/// `0:06`, or `1:02:03` past an hour.
pub(super) fn clock(time: Duration) -> String {
    let seconds = time.as_secs();
    match seconds {
        0..3600 => format!("{}:{:02}", seconds / 60, seconds % 60),
        _ => format!(
            "{}:{:02}:{:02}",
            seconds / 3600,
            seconds / 60 % 60,
            seconds % 60
        ),
    }
}

/// How far into a bar `x` lies, from 0 to 1.
pub(super) fn fraction(x: Pixels, bar: Bounds<Pixels>) -> f32 {
    if bar.size.width <= px(0.) {
        return 0.;
    }
    ((x - bar.origin.x) / bar.size.width).clamp(0., 1.)
}

pub fn card(ws: &Workspace, key: &str, clip: &Clip, cx: &mut Context<Workspace>) -> AnyElement {
    let theme = cx.theme();
    let (muted, accent, border, surface, raised, foreground) = (
        theme.muted_foreground,
        theme.primary,
        theme.border,
        theme.muted,
        theme.accent,
        theme.foreground,
    );
    let now = ws.audio.now.as_ref().filter(|now| now.id == clip.id);
    let playing = now.is_some_and(|now| !now.player.is_paused());
    let position = now.map_or(Duration::ZERO, |now| now.player.get_pos());
    let length = clip.duration.or(now.and_then(|now| now.length));
    let share = length
        .filter(|length| !length.is_zero())
        .map_or(0., |length| {
            (position.as_secs_f32() / length.as_secs_f32()).clamp(0., 1.)
        });
    let times = match length {
        Some(length) => format!("{} / {}", clock(position), clock(length)),
        None => clock(position),
    };
    let title = clip
        .title
        .clone()
        .unwrap_or_else(|| kind_label(&clip.mime).to_owned());
    let (play_id, seek_id, menu_id) = (clip.id.clone(), clip.id.clone(), clip.id.clone());
    let bar_bounds = Rc::new(Cell::new(Bounds::<Pixels>::default()));
    let measured = Rc::clone(&bar_bounds);
    let seekable = length.is_some();

    // The tile: five bars of a sound wave.
    let mut wave = div().flex().flex_row().items_center().gap(px(3.));
    for height in [10., 18., 26., 16., 8.] {
        wave = wave.child(div().w(px(3.)).h(px(height)).rounded_full().bg(accent));
    }
    let tile = div()
        .size(px(48.))
        .flex_none()
        .flex()
        .items_center()
        .justify_center()
        .rounded_lg()
        .bg(raised)
        .child(wave);

    let head = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .p_3()
        .child(tile)
        .child(
            div()
                .flex()
                .flex_col()
                .flex_1()
                .min_w_0()
                .gap_0p5()
                .child(
                    div()
                        .body()
                        .font_weight(FontWeight::MEDIUM)
                        .truncate()
                        .child(title),
                )
                .child(
                    div()
                        .small()
                        .text_color(muted)
                        .child(kind_label(&clip.mime)),
                ),
        )
        .child(
            Button::new(SharedString::from(format!("play-{key}")))
                .outline()
                .rounded(ButtonRounded::Size(px(999.)))
                .icon(if playing {
                    IconName::Pause
                } else {
                    IconName::Play
                })
                .tooltip(if playing { "Pause" } else { "Play" })
                .on_click(cx.listener(move |ws, _, _, cx| ws.toggle_clip(&play_id, cx))),
        )
        .child(
            Button::new(SharedString::from(format!("more-{key}")))
                .ghost()
                .rounded(ButtonRounded::Size(px(999.)))
                .icon(IconName::Ellipsis)
                .tooltip("More")
                .on_click(cx.listener(move |ws, event: &ClickEvent, _, cx| {
                    if let Some(path) = sound_path(&ws.status.artifacts_dir, &menu_id) {
                        ws.audio.menu = Some((path, event.position()));
                        cx.notify();
                    }
                })),
        );

    let track = div()
        .id(SharedString::from(format!("bar-{key}")))
        .relative()
        .flex_1()
        .h(px(18.))
        .flex()
        .items_center()
        .when(seekable, |bar| bar.cursor_pointer())
        .child(
            div()
                .relative()
                .w_full()
                .h(px(4.))
                .rounded_full()
                .bg(border)
                .child(
                    div()
                        .absolute()
                        .left_0()
                        .top_0()
                        .h_full()
                        .w(relative(share))
                        .rounded_full()
                        .bg(accent),
                )
                // The knob sits where the sound is.
                .child(
                    div()
                        .absolute()
                        .top(px(-5.))
                        .left(relative(share))
                        .ml(px(-7.))
                        .size(px(14.))
                        .rounded_full()
                        .bg(accent),
                ),
        )
        // Measures the bar for the click: the canvas has its size and nothing else.
        .child(
            canvas(move |bounds, _, _| measured.set(bounds), |_, _, _, _| {})
                .absolute()
                .size_full(),
        )
        .on_mouse_down(
            MouseButton::Left,
            cx.listener(move |ws, event: &MouseDownEvent, _, cx| {
                ws.seek_clip(&seek_id, fraction(event.position.x, bar_bounds.get()), cx)
            }),
        );

    div()
        .w_full()
        .max_w(px(440.))
        .flex()
        .flex_col()
        .rounded_xl()
        .border_1()
        .border_color(border)
        .bg(surface)
        .child(head)
        .child(div().h(px(1.)).bg(border))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap_3()
                .px_4()
                .py_2()
                .child(track)
                .child(
                    div()
                        .meta()
                        .flex_none()
                        .text_color(foreground.opacity(0.7))
                        .child(times),
                ),
        )
        .into_any_element()
}
