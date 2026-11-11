//! Playing the sound: the device, the sound that plays, jumping and the redraw loop.

use crate::workspace::Workspace;
use gpui_kit::*;
use rodio::{Decoder, DeviceSinkBuilder, MixerDeviceSink, Player, Source};
use std::{fs::File, path::PathBuf, time::Duration};

/// What the window knows of playing: the sound device, the sound that plays, and the open menu.
#[derive(Default)]
pub struct Audio {
    pub(super) sink: Option<MixerDeviceSink>,
    pub(super) now: Option<Now>,
    /// A redraw loop is running.
    pub(super) ticking: bool,
    /// The sound a click on `...` opened the menu of, and where.
    pub menu: Option<(PathBuf, Point<Pixels>)>,
}

pub(super) struct Now {
    pub(super) id: String,
    pub(super) player: Player,
    pub(super) length: Option<Duration>,
}

/// A card's sound: where it lies, for an ID of hex digits and dashes only, so it cannot name
/// a path outside the artifact folder.
pub(super) fn sound_path(folder: &std::path::Path, id: &str) -> Option<PathBuf> {
    let valid = !id.is_empty() && id.chars().all(|c| c.is_ascii_hexdigit() || c == '-');
    valid.then(|| folder.join(id).join("audio"))
}

impl Workspace {
    /// Reads an answer aloud: the core makes the sound, and it plays at once.
    pub fn speak_answer(&mut self, text: &str, cx: &mut Context<Self>) {
        if self.synthesizing {
            return;
        }
        let text = text.to_owned();
        self.when_voice_ready(
            crate::pages::connectors::voice::Speech::Synthesis,
            cx,
            Box::new(move |ws, cx| ws.read_aloud(text, cx)),
        );
    }

    fn read_aloud(&mut self, text: String, cx: &mut Context<Self>) {
        if self.synthesizing {
            return;
        }
        self.synthesizing = true;
        self.core.call(
            cx,
            move |client| async move { client.speak(&text).await },
            |ws, result, cx| {
                ws.synthesizing = false;
                match result {
                    Ok(card) => ws.toggle_clip(&card.id, cx),
                    Err(error) => ws.fail(error, cx),
                }
                cx.notify();
            },
        );
        cx.notify();
    }

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
