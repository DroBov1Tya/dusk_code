//! Sounds in the chat: a card with a play button, a bar that shows how far it has played
//! and takes a click to jump, the time, and a menu for the file. One sound plays at a time;
//! starting another stops the first. The sound is the file the core kept in the artifact
//! store, played from there.

use std::{cell::Cell, fs::File, path::PathBuf, rc::Rc, time::Duration};

use gpui_kit::{
    component::{
        ActiveTheme, IconName,
        button::{Button, ButtonRounded, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};

use super::transcript::Clip;
use crate::{ui::Type, widgets, workspace::Workspace};

/// What the window knows of playing: the sound device, the sound that plays, and the open menu.
#[derive(Default)]
pub struct Audio {
    sink: Option<MixerDeviceSink>,
    now: Option<Now>,
    /// A redraw loop is running.
    ticking: bool,
    /// The sound a click on `...` opened the menu of, and where.
    pub menu: Option<(PathBuf, Point<Pixels>)>,
}

struct Now {
    id: String,
    player: Player,
    length: Option<Duration>,
}

/// A card's sound: where it lies, for an ID of hex digits and dashes only, so it cannot name
/// a path outside the artifact folder.
fn sound_path(folder: &std::path::Path, id: &str) -> Option<PathBuf> {
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid.then(|| folder.join(id).join("audio"))
}

fn kind_label(mime: &str) -> &'static str {
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
fn clock(time: Duration) -> String {
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
fn fraction(x: Pixels, bar: Bounds<Pixels>) -> f32 {
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

/// What `...` on a card offers: the file in the system's file manager, and its path.
pub fn menu(ws: &Workspace, window: &Window, cx: &mut Context<Workspace>) -> Option<Stateful<Div>> {
    let (path, at) = ws.audio.menu.clone()?;
    const HEIGHT: f32 = 84.;
    let room = window.viewport_size().height - at.y;
    let top = if room < px(HEIGHT + 8.) {
        at.y - px(HEIGHT)
    } else {
        at.y
    };
    let theme = cx.theme();
    let muted = theme.muted_foreground;
    let (reveal, copy) = (path.clone(), path);
    Some(
        div()
            .id("audio-menu")
            .absolute()
            .left(at.x)
            .top(top)
            .occlude()
            .w(px(200.))
            .p_1()
            .flex()
            .flex_col()
            .rounded_lg()
            .border_1()
            .border_color(theme.border)
            .bg(theme.popover)
            .shadow_lg()
            .child(
                widgets::menu_item("audio-menu-reveal", cx)
                    .child(widgets::icon(IconName::FolderOpen, muted))
                    .child(if cfg!(target_os = "macos") {
                        "Show in Finder"
                    } else {
                        "Show in files"
                    })
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.audio.menu = None;
                        cx.reveal_path(&reveal);
                        cx.notify();
                    })),
            )
            .child(
                widgets::menu_item("audio-menu-copy", cx)
                    .child(widgets::icon(IconName::Copy, muted))
                    .child("Copy path")
                    .on_click(cx.listener(move |ws, _, _, cx| {
                        ws.audio.menu = None;
                        cx.write_to_clipboard(ClipboardItem::new_string(
                            copy.display().to_string(),
                        ));
                        cx.notify();
                    })),
            )
            .on_mouse_down_out(cx.listener(|ws, _, _, cx| {
                ws.audio.menu = None;
                cx.notify();
            })),
    )
}

impl Workspace {
    /// Plays the sound, pauses it while it plays, or goes on from a pause. A sound that ended,
    /// or another one, starts from the beginning.
    pub fn toggle_clip(&mut self, id: &str, cx: &mut Context<Self>) {
        if let Some(now) = self.audio.now.as_ref().filter(|now| now.id == id) {
            if now.player.is_paused() {
                now.player.play();
                self.tick_audio(cx);
            } else {
                now.player.pause();
            }
            cx.notify();
            return;
        }
        if let Err(error) = self.start_clip(id) {
            self.fail(error, cx);
            return;
        }
        self.tick_audio(cx);
        cx.notify();
    }

    fn start_clip(&mut self, id: &str) -> anyhow::Result<()> {
        use anyhow::Context as _;
        let path = sound_path(&self.status.artifacts_dir, id).context("not a sound of the chat")?;
        let file = File::open(&path).context("the sound is gone from the artifact folder")?;
        let source = Decoder::try_from(file).context("this sound cannot be decoded")?;
        let length = source.total_duration();
        if self.audio.sink.is_none() {
            let sink = DeviceSinkBuilder::open_default_sink()
                .map_err(|error| anyhow::anyhow!("no sound output: {error}"))?;
            self.audio.sink = Some(sink);
        }
        let sink = self.audio.sink.as_ref().context("no sound output")?;
        // The sound that played before stops with its player.
        self.audio.now = None;
        let player = Player::connect_new(sink.mixer());
        player.append(source);
        self.audio.now = Some(Now {
            id: id.to_owned(),
            player,
            length,
        });
        Ok(())
    }

    /// Jumps to a place of the sound that plays; a sound that does not play starts there.
    pub fn seek_clip(&mut self, id: &str, share: f32, cx: &mut Context<Self>) {
        if self.audio.now.as_ref().is_none_or(|now| now.id != id) {
            self.toggle_clip(id, cx);
        }
        let Some(now) = self.audio.now.as_ref().filter(|now| now.id == id) else {
            return;
        };
        let Some(length) = now.length.filter(|length| !length.is_zero()) else {
            return;
        };
        if let Err(error) = now.player.try_seek(length.mul_f32(share.clamp(0., 1.))) {
            tracing::warn!("cannot jump in the sound: {error}");
        }
        cx.notify();
    }

    /// Redraws about eight times a second while a sound plays, so the bar moves.
    fn tick_audio(&mut self, cx: &mut Context<Self>) {
        if self.audio.ticking {
            return;
        }
        self.audio.ticking = true;
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(120))
                    .await;
                let go = ws.update(cx, |ws, cx| {
                    let ended = match &ws.audio.now {
                        Some(now) if now.player.is_paused() => {
                            ws.audio.ticking = false;
                            return false;
                        }
                        Some(now) => now.player.empty(),
                        None => true,
                    };
                    if ended {
                        ws.audio.now = None;
                        ws.audio.ticking = false;
                    }
                    cx.notify();
                    !ended
                });
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }
}

#[cfg(test)]
mod tests {
    // Named imports: a glob would bring gpui's own `test` macro over the standard one.
    use super::{clock, sound_path};
    use std::time::Duration;

    #[test]
    fn a_sound_path_takes_only_an_id() {
        let folder = std::path::Path::new("/artifacts");
        assert!(sound_path(folder, "0192-ab").is_some());
        assert!(sound_path(folder, "../etc").is_none());
        assert!(sound_path(folder, "").is_none());
    }

    #[test]
    fn time_reads_as_minutes_and_seconds() {
        assert_eq!(clock(Duration::from_secs(6)), "0:06");
        assert_eq!(clock(Duration::from_secs(75)), "1:15");
        assert_eq!(clock(Duration::from_secs(3723)), "1:02:03");
    }
}
