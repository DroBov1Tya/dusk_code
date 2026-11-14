//! The iOS Simulator beside the work, on macOS: the setup until Xcode, a runtime and a
//! device are there, then the booted device's screen, refreshed a few times a second.
//! The core runs simctl; this draws what it answers. Clicks and keys go to the device in
//! Simulator.app, which one button opens.

use std::{sync::Arc, time::Duration};

use dusk_protocol::system::{SimDevice, Simulator};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub const WIDTH: f32 = 380.;
/// The pause between two screens; simctl itself takes about as long for one.
const FRAME: Duration = Duration::from_millis(120);
/// How often the setup is looked at again while a runtime downloads.
const RECHECK: Duration = Duration::from_secs(4);
const APP_STORE_XCODE: &str = "macappstore://apps.apple.com/app/id497799835";

#[derive(Default)]
pub struct Panel {
    pub open: bool,
    found: Option<Simulator>,
    error: Option<String>,
    /// The device whose screen is shown, by UDID.
    chosen: Option<String>,
    /// What is being done to it, said in place of the buttons.
    busy: Option<&'static str>,
    screen: Option<Arc<RenderImage>>,
    /// Screens replaced since the last draw; their textures are freed there.
    retired: Vec<Arc<RenderImage>>,
    /// Bumped on every open and close, so a loop of an earlier opening stops.
    run: u64,
    pulling: bool,
}

impl Panel {
    fn chosen_device(&self) -> Option<&SimDevice> {
        let chosen = self.chosen.as_ref()?;
        let found = self.found.as_ref()?;
        found.devices.iter().find(|device| &device.udid == chosen)
    }

    fn show(&mut self, screen: Option<Arc<RenderImage>>) {
        if let Some(old) = std::mem::replace(&mut self.screen, screen) {
            self.retired.push(old);
        }
    }
}

impl Workspace {
    pub fn toggle_simulator(&mut self, cx: &mut Context<Self>) {
        let panel = &mut self.simulator;
        panel.open = !panel.open;
        panel.run += 1;
        panel.pulling = false;
        if panel.open {
            self.look_simulator(cx);
        } else {
            panel.show(None);
        }
        cx.notify();
    }

    fn look_simulator(&mut self, cx: &mut Context<Self>) {
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
                        let still_there = panel
                            .chosen
                            .as_ref()
                            .is_some_and(|udid| found.devices.iter().any(|d| &d.udid == udid));
                        if !still_there {
                            panel.chosen = found
                                .devices
                                .iter()
                                .find(|device| device.booted)
                                .map(|device| device.udid.clone());
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
        let asked = udid.clone();
        self.core.call(
            cx,
            move |client| async move {
                let jpeg = client.simulator().screen(asked).await?;
                let picture = decode(&jpeg)?;
                tokio::time::sleep(FRAME).await;
                Ok(picture)
            },
            move |ws, result, cx| {
                let panel = &mut ws.simulator;
                let current = panel.run == run && panel.chosen.as_ref() == Some(&udid);
                if !current {
                    return;
                }
                match result {
                    Ok(picture) => {
                        panel.show(Some(picture));
                        ws.next_screen(udid, run, cx);
                    }
                    Err(error) => {
                        panel.pulling = false;
                        panel.error = Some(format!("{error:#}"));
                    }
                }
                cx.notify();
            },
        );
    }

    fn choose_device(&mut self, udid: String, cx: &mut Context<Self>) {
        let panel = &mut self.simulator;
        panel.chosen = Some(udid);
        panel.run += 1;
        panel.pulling = false;
        panel.error = None;
        panel.show(None);
        self.pull_screen(cx);
        cx.notify();
    }

    /// Boots or shuts down the chosen device, then looks again.
    fn power(&mut self, boot: bool, cx: &mut Context<Self>) {
        let Some(udid) = self.simulator.chosen.clone() else {
            return;
        };
        let panel = &mut self.simulator;
        panel.busy = Some(if boot { "Booting" } else { "Shutting down" });
        panel.error = None;
        panel.run += 1;
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

    fn install_runtime(&mut self, cx: &mut Context<Self>) {
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

    fn open_simulator_app(&mut self, cx: &mut Context<Self>) {
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

/// A JPEG as the picture the window draws, its pixels in the BGRA order it wants.
fn decode(jpeg: &[u8]) -> anyhow::Result<Arc<RenderImage>> {
    let mut pixels =
        image::load_from_memory_with_format(jpeg, image::ImageFormat::Jpeg)?.into_rgba8();
    for pixel in pixels.pixels_mut() {
        pixel.0.swap(0, 2);
    }
    let frame = image::Frame::new(pixels);
    Ok(Arc::new(RenderImage::new(vec![frame])))
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Option<Div> {
    if !ws.simulator.open {
        return None;
    }
    for old in ws.simulator.retired.drain(..) {
        let _ = window.drop_image(old);
    }
    let theme = cx.theme();
    let (panel_bg, border, muted) = (theme.sidebar, theme.sidebar_border, theme.muted_foreground);
    let head = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(40.))
        .pl_4()
        .pr_2()
        .child(
            div()
                .flex_1()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child("iOS Simulator"),
        )
        .child(
            Button::new("simulator-close")
                .ghost()
                .small()
                .icon(IconName::Close)
                .tooltip("Close the simulator; the device keeps running")
                .on_click(cx.listener(|ws, _, _, cx| ws.toggle_simulator(cx))),
        );
    let panel = &ws.simulator;
    let body = match &panel.found {
        None if panel.error.is_none() => {
            crate::ui::loading("Looking for Xcode", cx).into_any_element()
        }
        None => div().into_any_element(),
        Some(found) if found.xcode && !found.runtimes.is_empty() && !found.devices.is_empty() => {
            device_view(panel, found, cx).into_any_element()
        }
        Some(found) => setup(found, cx).into_any_element(),
    };
    Some(
        div()
            .w(px(WIDTH))
            .h_full()
            .flex_none()
            .flex()
            .flex_col()
            .bg(panel_bg)
            .border_l_1()
            .border_color(border)
            .child(head)
            .child(
                div()
                    .id("simulator-body")
                    .flex_1()
                    .min_h_0()
                    .overflow_y_scroll()
                    .px_4()
                    .pb_4()
                    .flex()
                    .flex_col()
                    .gap_3()
                    .child(body)
                    .when_some(panel.error.clone(), |body, error| {
                        body.child(div().small().text_color(muted).child(error))
                    }),
            ),
    )
}

/// The three steps before a device can run, each done or with the way to do it.
fn setup(found: &Simulator, cx: &mut Context<Workspace>) -> Div {
    let xcode = step(1, "Xcode", found.xcode, cx).when(!found.xcode, |step| {
        step.child(hint(
            "Install Xcode from the App Store, then open it once.",
            cx,
        ))
        .child(
            Button::new("simulator-xcode")
                .small()
                .label("Open the App Store")
                .on_click(|_, _, cx| cx.open_url(APP_STORE_XCODE)),
        )
    });
    let has_runtime = !found.runtimes.is_empty();
    let runtime = step(2, "iOS runtime", has_runtime, cx)
        .when(has_runtime, |step| {
            step.child(hint(&found.runtimes.join(", "), cx))
        })
        .when(found.xcode && !has_runtime, |step| {
            let step = step.child(hint(
                "The simulator needs the iOS platform, a download of several gigabytes.",
                cx,
            ));
            match (found.installing, &found.install_failed) {
                (true, _) => step.child(crate::ui::loading("Downloading iOS", cx)),
                (false, failed) => step
                    .when_some(failed.clone(), |step, failed| {
                        step.child(hint(&format!("The last download failed: {failed}"), cx))
                    })
                    .child(
                        Button::new("simulator-runtime")
                            .primary()
                            .small()
                            .label("Download iOS")
                            .on_click(cx.listener(|ws, _, _, cx| ws.install_runtime(cx))),
                    ),
            }
        });
    let device = step(3, "A device", !found.devices.is_empty(), cx).when(
        has_runtime && found.devices.is_empty(),
        |step| {
            step.child(hint(
                "Add an iPhone in Xcode, under Window, Devices and Simulators.",
                cx,
            ))
        },
    );
    div()
        .flex()
        .flex_col()
        .gap_4()
        .pt_2()
        .child(xcode)
        .child(runtime)
        .child(device)
}

fn step(number: usize, title: &'static str, done: bool, cx: &App) -> Div {
    let theme = cx.theme();
    let mark = if done {
        widgets::icon(IconName::CircleCheck, theme.success).into_any_element()
    } else {
        div()
            .size(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .border_1()
            .border_color(theme.muted_foreground)
            .meta()
            .text_color(theme.muted_foreground)
            .child(number.to_string())
            .into_any_element()
    };
    // Buttons keep their own width rather than the column's.
    div().flex().flex_col().items_start().gap_2().child(
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap_2()
            .child(mark)
            .child(div().body().font_weight(FontWeight::MEDIUM).child(title)),
    )
}

fn hint(text: &str, cx: &App) -> Div {
    div()
        .small()
        .text_color(cx.theme().muted_foreground)
        .child(text.to_owned())
}

/// The devices to choose from, and the chosen one's screen or the way to boot it.
fn device_view(panel: &Panel, found: &Simulator, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (surface, muted) = (theme.muted, theme.muted_foreground);
    let mut list = div().flex().flex_col();
    for (index, device) in found.devices.iter().enumerate() {
        let chosen = panel.chosen.as_ref() == Some(&device.udid);
        let udid = device.udid.clone();
        list = list.child(
            widgets::row(("simulator-device", index), chosen, cx)
                .h(px(32.))
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .small()
                        .child(device.name.clone()),
                )
                .child(div().meta().text_color(muted).child(if device.booted {
                    format!("{} \u{00B7} on", device.runtime)
                } else {
                    device.runtime.clone()
                }))
                .on_click(cx.listener(move |ws, _, _, cx| ws.choose_device(udid.clone(), cx))),
        );
    }
    let mut view = div().flex().flex_col().gap_3().child(list);
    let Some(device) = panel.chosen_device() else {
        return view.child(hint("Choose a device.", cx));
    };
    if let Some(busy) = panel.busy {
        return view.child(crate::ui::loading(busy, cx));
    }
    if !device.booted {
        return view.child(
            div().flex().child(
                Button::new("simulator-boot")
                    .primary()
                    .small()
                    .label(format!("Boot {}", device.name))
                    .on_click(cx.listener(|ws, _, _, cx| ws.power(true, cx))),
            ),
        );
    }
    let screen = match &panel.screen {
        Some(picture) => img(ImageSource::Render(Arc::clone(picture)))
            .w_full()
            .object_fit(ObjectFit::Contain)
            .into_any_element(),
        None => crate::ui::loading("Waiting for the screen", cx).into_any_element(),
    };
    view = view
        .child(
            div()
                .w_full()
                .rounded_xl()
                .overflow_hidden()
                .bg(surface)
                .child(screen),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    Button::new("simulator-app")
                        .small()
                        .label("Open in Simulator")
                        .tooltip("Clicks and keys reach the device there")
                        .on_click(cx.listener(|ws, _, _, cx| ws.open_simulator_app(cx))),
                )
                .child(
                    Button::new("simulator-shutdown")
                        .ghost()
                        .small()
                        .label("Shut down")
                        .on_click(cx.listener(|ws, _, _, cx| ws.power(false, cx))),
                ),
        );
    view
}
