//! The iOS Simulator beside the work, on macOS: the setup until Xcode, a runtime and a
//! device are there, then the device as a phone, its screen refreshed as fast as simctl
//! gives pictures. A click on the screen is a tap, a drag a swipe; buttons under it press
//! the device's own. The core runs simctl and AXe; this draws and asks.
//!
//! - `link`: what the panel asks the core, and the loop of screens;
//! - `picker`: the device's name at the top, a menu of the others;
//! - `setup`: the steps before a device can run;
//! - `phone`: the device's body and its screen, which takes touches;
//! - `toolbar`: the buttons under the phone.

mod link;
mod phone;
mod picker;
mod setup;
mod toolbar;

use std::{cell::Cell, rc::Rc, sync::Arc};

use dusk_protocol::system::{SimDevice, Simulator};
use gpui_kit::{
    component::{
        ActiveTheme, IconName,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub const WIDTH: f32 = 400.;
/// The panel's top, as tall as the window's header.
const HEAD: f32 = crate::ui::size::BAR;

#[derive(Default)]
pub struct Panel {
    pub open: bool,
    found: Option<Simulator>,
    error: Option<String>,
    /// The device shown, by UDID.
    chosen: Option<String>,
    /// The menu of devices is open.
    picking: bool,
    /// What is being done to the device, said in place of its screen.
    busy: Option<&'static str>,
    screen: Option<Screen>,
    /// Screens replaced since the last draw; their textures are freed there.
    retired: Vec<Arc<RenderImage>>,
    /// Bumped on every open, close and change of device, so an older loop stops.
    run: u64,
    pulling: bool,
    /// Where the screen was drawn last, to turn a click into points.
    drawn: Rc<Cell<Option<Bounds<Pixels>>>>,
    /// Where a press on the screen began, in points.
    pressed: Option<(u32, u32)>,
}

/// A picture of the device's screen and its size in points.
struct Screen {
    picture: Arc<RenderImage>,
    width: f32,
    height: f32,
}

impl Panel {
    fn chosen_device(&self) -> Option<&SimDevice> {
        let chosen = self.chosen.as_ref()?;
        let found = self.found.as_ref()?;
        found.devices.iter().find(|device| &device.udid == chosen)
    }

    fn input(&self) -> bool {
        self.found.as_ref().is_some_and(|found| found.input)
    }

    fn show(&mut self, screen: Option<Screen>) {
        if let Some(old) = std::mem::replace(&mut self.screen, screen) {
            self.retired.push(old.picture);
        }
    }
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
    let panel = &ws.simulator;
    let ready = panel.found.as_ref().is_some_and(|found| {
        found.xcode && !found.runtimes.is_empty() && !found.devices.is_empty()
    });
    let head = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .h(px(HEAD))
        .pl_3()
        .pr_2()
        .border_b_1()
        .border_color(border)
        .child(match ready {
            true => picker::button(panel, cx).into_any_element(),
            false => div()
                .pl_1()
                .body()
                .font_weight(FontWeight::SEMIBOLD)
                .child("iOS Simulator")
                .into_any_element(),
        })
        .child(div().flex_1())
        .child(
            Button::new("simulator-close")
                .ghost()
                .icon(IconName::Close)
                .tooltip("Close the panel; the device keeps running")
                .on_click(cx.listener(|ws, _, _, cx| ws.toggle_simulator(cx))),
        );
    let body = match &panel.found {
        None if panel.error.is_none() => {
            crate::ui::loading("Looking for Xcode", cx).into_any_element()
        }
        None => div().into_any_element(),
        Some(found) if ready => phone::render(panel, found, cx).into_any_element(),
        Some(found) => setup::render(found, cx).into_any_element(),
    };
    let menu = (ready && panel.picking).then(|| picker::menu(panel, cx));
    Some(
        div()
            .relative()
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
                    .p_4()
                    .flex()
                    .flex_col()
                    .gap_4()
                    .child(body)
                    .when_some(panel.error.clone(), |body, error| {
                        body.child(widgets::muted(error, cx).text_color(muted))
                    }),
            )
            .children(menu),
    )
}
