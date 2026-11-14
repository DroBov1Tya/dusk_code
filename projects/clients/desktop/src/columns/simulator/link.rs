//! What the panel asks the core: the setup, the loop of screens while a booted device
//! is shown, and every touch and button.

use std::{sync::Arc, time::Duration};

use dusk_protocol::system::{SimButton, SimScreen};
use gpui_kit::*;

use super::{Screen, WIDTH};
use crate::workspace::Workspace;

/// A failed screen is asked again after this, at most `RETRIES` times in a row: while a
/// device boots or its stream starts, a picture can fail for a moment.
const RETRY: Duration = Duration::from_millis(500);
const RETRIES: u32 = 6;
/// How often the setup is looked at again while a runtime downloads.
const RECHECK: Duration = Duration::from_secs(4);
/// The widest a screen is kept, in pixels: the panel at twice its points.
const SHOWN_WIDTH: u32 = 2 * WIDTH as u32;

impl Workspace {
    pub fn toggle_simulator(&mut self, cx: &mut Context<Self>) {
        let panel = &mut self.simulator;
        panel.open = !panel.open;
        panel.picking = false;
        panel.run += 1;
        panel.seq = 0;
        panel.pulling = false;
        if panel.open {
            self.look_simulator(cx);
        } else {
            panel.show(None);
        }
        cx.notify();
    }

    pub(super) fn look_simulator(&mut self, cx: &mut Context<Self>) {
        let run = self.simulator.run;
        self.core.call(
            cx,
            |client| async move { client.simulator().look().await },
            move |ws, result, cx| {
                let panel = &mut ws.simulator;
                if panel.run != run {
                    return;
                }
                match result {
                    Ok(found) => {
                        let installing = found.installing;
                        let known = panel
                            .chosen
                            .as_ref()
                            .is_some_and(|udid| found.devices.iter().any(|d| &d.udid == udid));
                        if !known {
                            // A booted device first, else the first iPhone.
                            let first = found.devices.iter().find(|d| d.booted).or_else(|| {
                                found.devices.iter().find(|d| d.name.starts_with("iPhone"))
                            });
                            panel.chosen = first.map(|device| device.udid.clone());
                        }
                        panel.found = Some(found);
                        panel.error = None;
                        if installing {
                            ws.look_again(run, cx);
                        }
                        ws.pull_screen(cx);
                    }
                    Err(error) => panel.error = Some(format!("{error:#}")),
                }
                cx.notify();
            },
        );
    }

    fn look_again(&mut self, run: u64, cx: &mut Context<Self>) {
        cx.spawn(async move |ws, cx| {
            cx.background_executor().timer(RECHECK).await;
            let _ = ws.update(cx, |ws, cx| {
                if ws.simulator.run == run {
                    ws.look_simulator(cx);
                }
            });
        })
        .detach();
    }

    /// Starts the loop of screens when the chosen device is booted and none runs.
    fn pull_screen(&mut self, cx: &mut Context<Self>) {
        let panel = &mut self.simulator;
        let Some(udid) = panel
            .chosen_device()
            .filter(|device| device.booted)
            .map(|device| device.udid.clone())
        else {
            return;
        };
        if panel.pulling || !panel.open {
            return;
        }
        panel.pulling = true;
        let run = panel.run;
        self.next_screen(udid, run, cx);
    }

    fn next_screen(&mut self, udid: String, run: u64, cx: &mut Context<Self>) {
        let (asked, after, failed) = (udid.clone(), self.simulator.seq, self.simulator.failed);
        self.core.call(
            cx,
            move |client| async move {
                if failed > 0 {
                    tokio::time::sleep(RETRY).await;
                }
                let screen = client.simulator().screen(asked, after).await?;
                decode(screen)
            },
            move |ws, result, cx| {
                let panel = &mut ws.simulator;
                if panel.run != run || panel.chosen.as_ref() != Some(&udid) {
                    return;
                }
                match result {
                    Ok(screen) => {
                        panel.failed = 0;
                        if screen.seq != panel.seq {
                            panel.seq = screen.seq;
                            panel.show(Some(screen));
                            cx.notify();
                        }
                        ws.next_screen(udid, run, cx);
                    }
                    Err(_) if panel.failed < RETRIES => {
                        panel.failed += 1;
                        ws.next_screen(udid, run, cx);
                    }
                    Err(error) => {
                        panel.pulling = false;
                        panel.failed = 0;
                        panel.error = Some(format!("{error:#}"));
                        cx.notify();
                    }
                }
            },
        );
    }

    pub fn toggle_device_menu(&mut self, cx: &mut Context<Self>) {
        self.simulator.picking = !self.simulator.picking;
        cx.notify();
    }

    pub(super) fn choose_device(&mut self, udid: String, cx: &mut Context<Self>) {
        let panel = &mut self.simulator;
        panel.chosen = Some(udid);
        panel.picking = false;
        panel.run += 1;
        panel.seq = 0;
        panel.pulling = false;
        panel.error = None;
        panel.show(None);
        self.pull_screen(cx);
        cx.notify();
    }

    /// Boots or shuts down the chosen device, then looks again.
    pub(super) fn power(&mut self, boot: bool, cx: &mut Context<Self>) {
        let Some(udid) = self.simulator.chosen.clone() else {
            return;
        };
        let panel = &mut self.simulator;
        panel.busy = Some(if boot { "Booting" } else { "Shutting down" });
        panel.error = None;
        panel.run += 1;
        panel.seq = 0;
        panel.pulling = false;
        panel.show(None);
        cx.notify();
        self.core.call(
            cx,
            move |client| async move {
                let simulator = client.simulator();
                match boot {
                    true => simulator.boot(udid).await,
                    false => simulator.shutdown(udid).await,
                }
            },
            |ws, result, cx| {
                ws.simulator.busy = None;
                if let Err(error) = result {
                    ws.simulator.error = Some(format!("{error:#}"));
                }
                ws.look_simulator(cx);
                cx.notify();
            },
        );
    }

    pub(super) fn sim_tap(&mut self, x: u32, y: u32, cx: &mut Context<Self>) {
        self.on_device(cx, move |client, udid| async move {
            client.simulator().tap(udid, x, y).await
        });
    }

    pub(super) fn sim_swipe(&mut self, from: (u32, u32), to: (u32, u32), cx: &mut Context<Self>) {
        self.on_device(cx, move |client, udid| async move {
            client.simulator().swipe(udid, from, to).await
        });
    }

    pub(super) fn sim_button(&mut self, button: SimButton, cx: &mut Context<Self>) {
        self.on_device(cx, move |client, udid| async move {
            client.simulator().button(udid, button).await
        });
    }

    /// Something done to the chosen device; only a failure is said.
    fn on_device<F, Fut>(&mut self, cx: &mut Context<Self>, act: F)
    where
        F: FnOnce(Arc<dusk_core::client::Client>, String) -> Fut + Send + 'static,
        Fut: Future<Output = anyhow::Result<()>> + Send + 'static,
    {
        let Some(udid) = self.simulator.chosen.clone() else {
            return;
        };
        self.core.call(
            cx,
            move |client| act(client, udid),
            |ws, result, cx| {
                ws.simulator.error = result.err().map(|error| format!("{error:#}"));
                cx.notify();
            },
        );
    }

    pub(super) fn install_runtime(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.simulator().install_runtime().await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.simulator.error = Some(format!("{error:#}"));
                }
                ws.look_simulator(cx);
            },
        );
    }

    pub(super) fn open_simulator_app(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.simulator().open_app().await },
            |ws, result, cx| {
                if let Err(error) = result {
                    ws.simulator.error = Some(format!("{error:#}"));
                    cx.notify();
                }
            },
        );
    }
}

/// A JPEG as the picture the window draws: no wider than the panel needs, its pixels in
/// the BGRA order the window wants.
fn decode(screen: SimScreen) -> anyhow::Result<Screen> {
    let mut full = image::load_from_memory_with_format(&screen.jpeg, image::ImageFormat::Jpeg)?;
    if full.width() > SHOWN_WIDTH {
        full = full.resize(SHOWN_WIDTH, u32::MAX, image::imageops::FilterType::Triangle);
    }
    let mut pixels = full.into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    let frame = image::Frame::new(pixels);
    Ok(Screen {
        picture: Arc::new(RenderImage::new(vec![frame])),
        width: screen.width,
        height: screen.height,
        seq: screen.seq,
    })
}
