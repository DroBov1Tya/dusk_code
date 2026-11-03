//! The system side: permissions, the display's size, a capture, and input events
//! posted as the HID system would.

use std::{path::Path, process::Command, thread, time::Duration};

use anyhow::{Context, Result, bail, ensure};
use core_foundation::{
    base::{CFType, TCFType},
    dictionary::{CFDictionary, CFDictionaryRef},
    number::CFNumber,
    string::{CFString, CFStringRef},
};
use core_graphics::{
    display::CGDisplay,
    event::{
        CGEvent, CGEventFlags, CGEventTapLocation, CGEventType, CGKeyCode, CGMouseButton,
        EventField, ScrollEventUnit,
    },
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::{CGPoint, CGRect},
    window::{
        copy_window_info, kCGNullWindowID, kCGWindowBounds, kCGWindowLayer,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowName,
        kCGWindowNumber, kCGWindowOwnerName,
    },
};

/// Between the events of one action, so apps see them as a person's.
const PAUSE: Duration = Duration::from_millis(30);
/// Characters a keyboard event carries at most.
const MAX_TYPED_UNITS: usize = 20;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    /// Returns a `Boolean`, an unsigned char. With the prompt option it also puts the
    /// app in the Accessibility list and asks the user once, as only an app that asked
    /// is listed there.
    fn AXIsProcessTrustedWithOptions(options: CFDictionaryRef) -> u8;
    static kAXTrustedCheckOptionPrompt: CFStringRef;
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

/// An app's window on screen, front to back as the system lists them.
pub struct Window {
    pub id: u32,
    pub app: String,
    pub title: String,
    /// In display points.
    pub bounds: CGRect,
}

/// The ordinary windows on screen, frontmost first; menus, docks and overlays left out.
pub fn windows() -> Vec<Window> {
    let options = kCGWindowListOptionOnScreenOnly | kCGWindowListExcludeDesktopElements;
    let Some(list) = copy_window_info(options, kCGNullWindowID) else {
        return Vec::new();
    };
    list.iter()
        .filter_map(|entry| {
            // SAFETY: every entry of the window list is a dictionary the list owns.
            let info: CFDictionary<CFString, CFType> =
                unsafe { CFDictionary::wrap_under_get_rule(*entry as CFDictionaryRef) };
            // SAFETY: the keys are constant strings the framework exports.
            let field = |key: CFStringRef| info.find(unsafe { CFString::wrap_under_get_rule(key) });
            let number = |key| field(key)?.downcast::<CFNumber>()?.to_i64();
            let text = |key| {
                field(key)
                    .and_then(|value| value.downcast::<CFString>())
                    .map(|value| value.to_string())
            };
            (number(unsafe { kCGWindowLayer })? == 0).then_some(())?;
            let bounds = field(unsafe { kCGWindowBounds })?.downcast::<CFDictionary>()?;
            Some(Window {
                id: u32::try_from(number(unsafe { kCGWindowNumber })?).ok()?,
                app: text(unsafe { kCGWindowOwnerName }).unwrap_or_default(),
                title: text(unsafe { kCGWindowName }).unwrap_or_default(),
                bounds: CGRect::from_dict_representation(&bounds)?,
            })
        })
        .collect()
}

/// Captures `rect` of the screen, in points, as a PNG at `file`.
pub fn capture(rect: CGRect, file: &Path) -> Result<()> {
    let region = format!(
        "{},{},{},{}",
        rect.origin.x, rect.origin.y, rect.size.width, rect.size.height
    );
    screencapture(&["-R", &region], file)
}

/// Captures one window as it is, even under others, without its shadow.
pub fn capture_window(id: u32, file: &Path) -> Result<()> {
    screencapture(&["-o", "-l", &id.to_string()], file)
}

fn screencapture(target: &[&str], file: &Path) -> Result<()> {
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
    let status = Command::new("screencapture")
        .args(["-x", "-t", "png"])
        .args(target)
        .arg(file)
        .status()
        .context("cannot run screencapture")?;
    ensure!(status.success(), "screencapture failed");
    Ok(())
}

fn source() -> Result<CGEventSource> {
    if !accessibility_trusted() {
        bail!(
            "the terminal duskcode runs in needs Accessibility permission to click and \
             type: macOS has asked and listed it under System Settings > Privacy & \
             Security > Accessibility; switch it on there and restart duskcode"
        );
    }
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("cannot create an input event source"))
}

/// Whether this process may post input events. The first refusal of a process also asks
/// the user; later ones only check, so the system's dialog does not come back each time.
fn accessibility_trusted() -> bool {
    static ASKED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
    let ask = !ASKED.swap(true, std::sync::atomic::Ordering::SeqCst);
    // SAFETY: the key is a constant of the framework; the dictionary lives across the call.
    let key = unsafe { CFString::wrap_under_get_rule(kAXTrustedCheckOptionPrompt) };
    let options = CFDictionary::from_CFType_pairs(&[(
        key.as_CFType(),
        core_foundation::boolean::CFBoolean::from(ask).as_CFType(),
    )]);
    // SAFETY: reads the permission; the options dictionary is valid for the call.
    unsafe { AXIsProcessTrustedWithOptions(options.as_concrete_TypeRef()) != 0 }
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
    // Cut between characters, so no surrogate pair is split.
    let mut chunk: Vec<u16> = Vec::new();
    for c in text.chars() {
        let mut units = [0; 2];
        let units = c.encode_utf16(&mut units);
        if chunk.len() + units.len() > MAX_TYPED_UNITS {
            type_units(&std::mem::take(&mut chunk))?;
        }
        chunk.extend_from_slice(units);
    }
    if !chunk.is_empty() {
        type_units(&chunk)?;
    }
    Ok(())
}

fn type_units(units: &[u16]) -> Result<()> {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(source()?, 0, down)
            .map_err(|_| anyhow::anyhow!("cannot create a key event"))?;
        event.set_string_from_utf16_unchecked(units);
        post(Ok(event))?;
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
