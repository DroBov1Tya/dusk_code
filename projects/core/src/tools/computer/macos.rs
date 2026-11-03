//! The system side: permissions, the display's size, a capture, and input events
//! posted as the HID system would.

use std::{path::Path, process::Command, thread, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use core_graphics::{
    display::CGDisplay,
    event::{
        CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton,
        EventField, ScrollEventUnit,
    },
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::{CGPoint, CGRect},
};

/// Between the events of one action, so apps see them as a person's.
const PAUSE: Duration = Duration::from_millis(30);
/// Characters a keyboard event carries at most.
const MAX_TYPED_UNITS: usize = 20;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXIsProcessTrusted() -> bool;
}

#[link(name = "CoreGraphics", kind = "framework")]
unsafe extern "C" {
    fn CGPreflightScreenCaptureAccess() -> bool;
    fn CGRequestScreenCaptureAccess() -> bool;
}

/// The main display in points, which input events use.
pub fn display() -> CGRect {
    CGDisplay::main().bounds()
}

/// Captures `rect` of the screen, in points, as a PNG at `file`.
pub fn capture(rect: CGRect, file: &Path) -> Result<()> {
    // Without the permission the capture shows only the wallpaper; asking opens the
    // system prompt the first time.
    // SAFETY: both functions take no arguments and only read or request a permission.
    if !unsafe { CGPreflightScreenCaptureAccess() } {
        unsafe { CGRequestScreenCaptureAccess() };
        bail!(
            "the terminal duskcode runs in needs Screen Recording permission: System \
             Settings > Privacy & Security > Screen Recording, then start it again"
        );
    }
    let region = format!(
        "{},{},{},{}",
        rect.origin.x, rect.origin.y, rect.size.width, rect.size.height
    );
    let status = Command::new("screencapture")
        .args(["-x", "-t", "png", "-R", &region])
        .arg(file)
        .status()
        .context("cannot run screencapture")?;
    ensure!(status.success(), "screencapture failed");
    Ok(())
}

fn source() -> Result<CGEventSource> {
    // SAFETY: takes no arguments; only reads whether this process may post events.
    if !unsafe { AXIsProcessTrusted() } {
        bail!(
            "the terminal duskcode runs in needs Accessibility permission to click and \
             type: System Settings > Privacy & Security > Accessibility"
        );
    }
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("cannot create an input event source"))
}

fn post(event: Result<CGEvent, ()>) -> Result<()> {
    event
        .map_err(|_| anyhow::anyhow!("cannot create an input event"))?
        .post(CGEventTapLocation::HID);
    thread::sleep(PAUSE);
    Ok(())
}

pub fn click(at: CGPoint, right: bool, count: i64) -> Result<()> {
    let (down, up, button) = match right {
        true => (
            CGEventType::RightMouseDown,
            CGEventType::RightMouseUp,
            CGMouseButton::Right,
        ),
        false => (
            CGEventType::LeftMouseDown,
            CGEventType::LeftMouseUp,
            CGMouseButton::Left,
        ),
    };
    post(CGEvent::new_mouse_event(
        source()?,
        CGEventType::MouseMoved,
        at,
        button,
    ))?;
    for click in 1..=count {
        for kind in [down, up] {
            let event = CGEvent::new_mouse_event(source()?, kind, at, button)
                .map_err(|_| anyhow::anyhow!("cannot create a click"))?;
            event.set_integer_value_field(EventField::MOUSE_EVENT_CLICK_STATE, click);
            post(Ok(event))?;
        }
    }
    Ok(())
}

pub fn type_text(text: &str) -> Result<()> {
    let units: Vec<u16> = text.encode_utf16().collect();
    for chunk in units.chunks(MAX_TYPED_UNITS) {
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(source()?, 0, down)
                .map_err(|_| anyhow::anyhow!("cannot create a key event"))?;
            event.set_string_from_utf16_unchecked(chunk);
            post(Ok(event))?;
        }
    }
    Ok(())
}

pub fn press(key: CGKeyCode, flags: CGEventFlags) -> Result<()> {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(source()?, key, down)
            .map_err(|_| anyhow::anyhow!("cannot create a key event"))?;
        event.set_flags(flags);
        post(Ok(event))?;
    }
    Ok(())
}

/// `down` and `right` in pixels; positive scrolls the content down and right.
pub fn scroll(at: CGPoint, down: i32, right: i32) -> Result<()> {
    post(CGEvent::new_mouse_event(
        source()?,
        CGEventType::MouseMoved,
        at,
        CGMouseButton::Left,
    ))?;
    post(CGEvent::new_scroll_event(
        source()?,
        ScrollEventUnit::PIXEL,
        2,
        -down,
        -right,
        0,
    ))
}
