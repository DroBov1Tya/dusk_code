//! Dictation: the microphone's words go into the message box while it is on. The
//! `microphone` reads sound on a thread of its own, `pieces` cuts it at pauses into WAV for
//! the speech server, and this module sends the pieces one at a time and puts what comes
//! back in the box.

use crate::workspace::Workspace;
use gpui_kit::*;
use parking_lot::Mutex;
use std::{
    collections::VecDeque,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

mod microphone;
mod pieces;

use microphone::{Heard, listen};
use pieces::take_piece;

/// Pieces waiting for a server that is slower than the speech: past this, the dictation ends.
const MAX_QUEUE: usize = 8;

/// After the stop, a microphone that sends nothing for this long is taken as ended.
const STALLED: Duration = Duration::from_secs(2);

/// What the window knows of a dictation in progress.
pub struct Dictation {
    stop: Arc<AtomicBool>,
    heard: Arc<Mutex<Heard>>,
    /// Pieces cut and not yet sent.
    queue: VecDeque<Vec<u8>>,
    /// A piece is with the server.
    busy: bool,
    /// The microphone gave nothing yet; its failure to open is waited for.
    started: std::time::Instant,
}

impl Drop for Dictation {
    /// A dictation that goes out of the window's hands, as with the window itself, must not
    /// leave the microphone on.
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

impl Workspace {
    pub fn dictating(&self) -> bool {
        self.dictation.is_some()
    }

    /// Starts or ends the dictation.
    pub fn toggle_dictation(&mut self, cx: &mut Context<Self>) {
        if let Some(dictation) = &self.dictation {
            // The thread ends on its next sample; what it heard is cut and sent first.
            dictation.stop.store(true, Ordering::Relaxed);
            return;
        }
        // The microphone is opened only when a server is there to take what it hears.
        self.when_voice_ready(
            crate::pages::connectors::voice::Speech::Recognition,
            cx,
            Box::new(|ws, cx| ws.start_dictation(cx)),
        );
    }

    fn start_dictation(&mut self, cx: &mut Context<Self>) {
        if self.dictation.is_some() {
            return;
        }
        let stop = Arc::new(AtomicBool::new(false));
        let heard = Arc::new(Mutex::new(Heard::default()));
        let (thread_stop, thread_heard) = (Arc::clone(&stop), Arc::clone(&heard));
        if let Err(error) = std::thread::Builder::new()
            .name("dictation".into())
            .spawn(move || listen(thread_stop, thread_heard))
        {
            self.fail(anyhow::anyhow!("cannot start the microphone: {error}"), cx);
            return;
        }
        self.dictation = Some(Dictation {
            stop,
            heard,
            queue: VecDeque::new(),
            busy: false,
            started: std::time::Instant::now(),
        });
        self.watch_dictation(cx);
        cx.notify();
    }

    /// Looks at the microphone four times a second: cuts pieces, sends them, notices the end.
    fn watch_dictation(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |ws, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(250))
                    .await;
                let go = ws.update(cx, |ws, cx| ws.dictation_step(cx));
                if !matches!(go, Ok(true)) {
                    break;
                }
            }
        })
        .detach();
    }

    /// One look; false when the dictation is over.
    fn dictation_step(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(dictation) = self.dictation.as_mut() else {
            return false;
        };
        let stopping = dictation.stop.load(Ordering::Relaxed);
        let (piece, ended, mut error) = {
            let mut heard = dictation.heard.lock();
            // A microphone that stalls after the stop never sends the sample that ends its
            // loop; it is taken as ended.
            let quiet_for = heard
                .last
                .map_or(dictation.started.elapsed(), |at| at.elapsed());
            if stopping && !heard.ended && quiet_for > STALLED {
                heard.ended = true;
            }
            let ending = stopping || heard.ended;
            let piece = take_piece(&mut heard, ending);
            (piece, heard.ended, heard.error.take())
        };
        if let Some(piece) = piece {
            dictation.queue.push_back(piece);
        }
        if dictation.queue.len() > MAX_QUEUE {
            dictation.queue.clear();
            dictation.stop.store(true, Ordering::Relaxed);
            error.get_or_insert_with(|| "the speech server is too slow for dictation".into());
        }
        let finished = ended && dictation.queue.is_empty() && !dictation.busy;
        if let Some(error) = error {
            self.dictation = None;
            self.fail(anyhow::anyhow!(error), cx);
            cx.notify();
            return false;
        }
        if finished {
            self.dictation = None;
            cx.notify();
            return false;
        }
        self.send_next_piece(cx);
        cx.notify();
        true
    }

    fn send_next_piece(&mut self, cx: &mut Context<Self>) {
        let Some(dictation) = self.dictation.as_mut() else {
            return;
        };
        if dictation.busy {
            return;
        }
        let Some(piece) = dictation.queue.pop_front() else {
            return;
        };
        dictation.busy = true;
        self.core.call(
            cx,
            move |client| async move { client.transcribe(piece).await },
            |ws, result, cx| {
                if let Some(dictation) = ws.dictation.as_mut() {
                    dictation.busy = false;
                }
                match result {
                    Ok(text) if !text.is_empty() => {
                        let said = ws.composer.read(cx).value().to_string();
                        let joined = match said.trim_end() {
                            "" => text,
                            before => format!("{before} {text}"),
                        };
                        ws.fill_composer = Some(joined);
                    }
                    Ok(_) => {}
                    // A server that fails once is likely to fail again: the dictation ends.
                    Err(error) => {
                        if let Some(dictation) = ws.dictation.as_mut() {
                            dictation.stop.store(true, Ordering::Relaxed);
                            dictation.queue.clear();
                        }
                        ws.fail(error, cx);
                    }
                }
                cx.notify();
            },
        );
    }
}
