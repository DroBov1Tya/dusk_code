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
        EventField, KeyCode, ScrollEventUnit,
    },
    event_source::{CGEventSource, CGEventSourceStateID},
    geometry::{CGPoint, CGRect},
    window::{
        copy_window_info, kCGNullWindowID, kCGWindowAlpha, kCGWindowBounds, kCGWindowLayer,
        kCGWindowListExcludeDesktopElements, kCGWindowListOptionOnScreenOnly, kCGWindowName,
        kCGWindowNumber, kCGWindowOwnerName, kCGWindowOwnerPID,
    },
};

use super::{Keys, Point, Rect, Window, click::PointerMode, cursor::Cursor};

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

/// A window as the system lists it, with what tells menus and overlays apart.
struct Listed {
    window: Window,
    pid: i64,
    layer: i64,
}

/// Includes menus and panels, but ignores our mouse-transparent cursor helper.
fn front_window_at(at: Point) -> Option<Window> {
    let overlay_pid = Cursor::shared().pid().map(i64::from);
    visible_windows()
        .into_iter()
        .find(|listed| Some(listed.pid) != overlay_pid && covers(listed.window.bounds, at))
        .map(|listed| listed.window)
}

/// Resolve the topmost visual window and require its owner and geometry to be unique.
/// AX does not expose a public CGWindowID, so ambiguous mappings are unsafe to press.
pub(super) fn independent_window(at: Point) -> Result<(u32, i32, Rect)> {
    let overlay = Cursor::shared().pid().map(i64::from);
    let windows = visible_windows();
    let front = windows
        .iter()
        .find(|entry| Some(entry.pid) != overlay && covers(entry.window.bounds, at))
        .context("no visible window at the click point")?;
    let bounds = front.window.bounds;
    ensure!(
        windows
            .iter()
            .filter(|entry| entry.pid == front.pid && entry.window.bounds == bounds)
            .count()
            == 1,
        "the target window cannot be identified unambiguously; no action was sent"
    );
    Ok((
        front.window.id,
        i32::try_from(front.pid).context("invalid window owner")?,
        bounds,
    ))
}

fn covers(bounds: Rect, at: Point) -> bool {
    (bounds.x..bounds.x + bounds.width).contains(&at.x)
        && (bounds.y..bounds.y + bounds.height).contains(&at.y)
}

fn rect(bounds: CGRect) -> Rect {
    Rect {
        x: bounds.origin.x,
        y: bounds.origin.y,
        width: bounds.size.width,
        height: bounds.size.height,
    }
}

fn cg(at: Point) -> CGPoint {
    CGPoint::new(at.x, at.y)
}

fn visible_windows() -> Vec<Listed> {
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
            if field(unsafe { kCGWindowAlpha })?
                .downcast::<CFNumber>()?
                .to_f64()?
                == 0.0
            {
                return None;
            }
            let bounds = field(unsafe { kCGWindowBounds })?.downcast::<CFDictionary>()?;
            Some(Listed {
                window: Window {
                    pid: i32::try_from(number(unsafe { kCGWindowOwnerPID })?).ok()?,
                    id: u32::try_from(number(unsafe { kCGWindowNumber })?).ok()?,
                    app: text(unsafe { kCGWindowOwnerName }).unwrap_or_default(),
                    title: text(unsafe { kCGWindowName }).unwrap_or_default(),
                    bounds: rect(CGRect::from_dict_representation(&bounds)?),
                },
                pid: number(unsafe { kCGWindowOwnerPID })?,
                layer: number(unsafe { kCGWindowLayer })?,
            })
        })
        .collect()
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
    require_accessibility()?;
    CGEventSource::new(CGEventSourceStateID::HIDSystemState)
        .map_err(|_| anyhow::anyhow!("cannot create an input event source"))
}

pub fn require_accessibility() -> Result<()> {
    if !accessibility_trusted() {
        bail!(
            "the terminal duskcode runs in needs Accessibility permission to click and \
             type: macOS has asked and listed it under System Settings > Privacy & \
             Security > Accessibility; switch it on there and restart duskcode"
        );
    }
    Ok(())
}

/// Whether this process may post input events. The first refusal of a process also asks
/// the user; later ones only check, so the system's dialog does not come back each time.
fn accessibility_trusted() -> bool {
    static ASKED: std::sync::Mutex<bool> = std::sync::Mutex::new(false);
    // Serialize checks and prompts so concurrent calls cannot show duplicate dialogs.
    let mut asked = ASKED
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    let trusted = accessibility_trusted_with_prompt(false);
    if !should_prompt(&mut asked, trusted) {
        return trusted;
    }
    if accessibility_trusted_with_prompt(true) {
        // The user may have granted access between the two checks.
        *asked = false;
        return true;
    }
    false
}

/// Ask once per continuous untrusted period; a subsequent grant resets the state.
fn should_prompt(asked: &mut bool, trusted: bool) -> bool {
    if trusted {
        *asked = false;
        return false;
    }
    if *asked {
        return false;
    }
    *asked = true;
    true
}

#[cfg(test)]
mod permission_tests {
    use super::should_prompt;

    #[test]
    fn prompts_once_per_untrusted_period_including_after_revocation() {
        let mut asked = false;
        assert!(!should_prompt(&mut asked, true));
        assert!(should_prompt(&mut asked, false));
        assert!(!should_prompt(&mut asked, false));
        assert!(!should_prompt(&mut asked, true));
        assert!(should_prompt(&mut asked, false));
        assert!(!should_prompt(&mut asked, false));
    }

    #[test]
    fn initially_untrusted_requests_permission_once() {
        let mut asked = false;
        assert!(should_prompt(&mut asked, false));
        assert!(!should_prompt(&mut asked, false));
    }
}

fn accessibility_trusted_with_prompt(ask: bool) -> bool {
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

fn type_units(units: &[u16]) -> Result<()> {
    for down in [true, false] {
        let event = CGEvent::new_keyboard_event(source()?, 0, down)
            .map_err(|_| anyhow::anyhow!("cannot create a key event"))?;
        event.set_string_from_utf16_unchecked(units);
        post(Ok(event))?;
    }
    Ok(())
}

fn code(name: &str) -> Option<CGKeyCode> {
    const LETTERS: [CGKeyCode; 26] = [
        KeyCode::ANSI_A,
        KeyCode::ANSI_B,
        KeyCode::ANSI_C,
        KeyCode::ANSI_D,
        KeyCode::ANSI_E,
        KeyCode::ANSI_F,
        KeyCode::ANSI_G,
        KeyCode::ANSI_H,
        KeyCode::ANSI_I,
        KeyCode::ANSI_J,
        KeyCode::ANSI_K,
        KeyCode::ANSI_L,
        KeyCode::ANSI_M,
        KeyCode::ANSI_N,
        KeyCode::ANSI_O,
        KeyCode::ANSI_P,
        KeyCode::ANSI_Q,
        KeyCode::ANSI_R,
        KeyCode::ANSI_S,
        KeyCode::ANSI_T,
        KeyCode::ANSI_U,
        KeyCode::ANSI_V,
        KeyCode::ANSI_W,
        KeyCode::ANSI_X,
        KeyCode::ANSI_Y,
        KeyCode::ANSI_Z,
    ];
    const DIGITS: [CGKeyCode; 10] = [
        KeyCode::ANSI_0,
        KeyCode::ANSI_1,
        KeyCode::ANSI_2,
        KeyCode::ANSI_3,
        KeyCode::ANSI_4,
        KeyCode::ANSI_5,
        KeyCode::ANSI_6,
        KeyCode::ANSI_7,
        KeyCode::ANSI_8,
        KeyCode::ANSI_9,
    ];
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Some(LETTERS[(c as u8 - b'a') as usize]);
        }
        if c.is_ascii_digit() {
            return Some(DIGITS[(c as u8 - b'0') as usize]);
        }
    }
    Some(match name {
        "enter" => KeyCode::RETURN,
        "tab" => KeyCode::TAB,
        "space" => KeyCode::SPACE,
        // The key a Mac labels delete is backspace.
        "delete" | "backspace" => KeyCode::DELETE,
        "forwarddelete" => KeyCode::FORWARD_DELETE,
        "esc" => KeyCode::ESCAPE,
        "left" => KeyCode::LEFT_ARROW,
        "right" => KeyCode::RIGHT_ARROW,
        "up" => KeyCode::UP_ARROW,
        "down" => KeyCode::DOWN_ARROW,
        "home" => KeyCode::HOME,
        "end" => KeyCode::END,
        "pageup" => KeyCode::PAGE_UP,
        "pagedown" => KeyCode::PAGE_DOWN,
        "f1" => KeyCode::F1,
        "f2" => KeyCode::F2,
        "f3" => KeyCode::F3,
        "f4" => KeyCode::F4,
        "f5" => KeyCode::F5,
        "f6" => KeyCode::F6,
        "f7" => KeyCode::F7,
        "f8" => KeyCode::F8,
        "f9" => KeyCode::F9,
        "f10" => KeyCode::F10,
        "f11" => KeyCode::F11,
        "f12" => KeyCode::F12,
        _ => return None,
    })
}

/// The desktop these tools drive.
pub struct Desktop;

impl Desktop {
    /// The main display in points, which input events use.
    pub fn display(&self) -> Result<Rect> {
        Ok(rect(CGDisplay::main().bounds()))
    }

    /// Shows the user where the agent acts now, by dusk's own cursor.
    pub fn mark(&self, at: Point) {
        Cursor::shared().show(at);
    }

    /// Marks a completed Accessibility press without posting any pointer events.
    pub fn mark_click(&self, at: Point) {
        Cursor::shared().click(at);
    }

    /// Keeps dusk's cursor up while the agent types, without moving it.
    pub fn touch(&self) {
        Cursor::shared().touch();
    }

    /// The ordinary windows on screen, frontmost first; menus, docks and overlays left out.
    pub fn windows(&self) -> Result<Vec<Window>> {
        Ok(visible_windows()
            .into_iter()
            .filter(|listed| listed.layer == 0)
            .map(|listed| listed.window)
            .collect())
    }

    /// Refuses a click into a window screenshot once that window moved, changed size or is
    /// covered at `at`.
    pub fn ensure_window_at(&self, at: Point, id: u32, bounds: Rect) -> Result<()> {
        let front = front_window_at(at)
            .context("the captured window is no longer visible here; take a new screenshot")?;
        ensure!(
            front.id == id && front.bounds == bounds,
            "the captured window moved, resized or is covered here; take a new screenshot"
        );
        Ok(())
    }

    /// Captures `rect` of the screen, in points, as a PNG at `file`.
    pub fn capture(&self, rect: Rect, file: &Path) -> Result<()> {
        let region = format!("{},{},{},{}", rect.x, rect.y, rect.width, rect.height);
        screencapture(&["-R", &region], file)
    }

    /// Captures one window as it is, even under others, without its shadow.
    pub fn capture_window(&self, window: &Window, file: &Path) -> Result<()> {
        screencapture(&["-a", "-o", "-l", &window.id.to_string()], file)
    }

    pub fn click(
        &self,
        at: Point,
        right: bool,
        count: i64,
        mode: PointerMode,
        expected_window: Option<(u32, Rect)>,
    ) -> Result<()> {
        mode.validate_click(right, count)?;
        if mode == PointerMode::Independent {
            super::accessibility::press(at, expected_window)?;
            self.mark_click(at);
            return Ok(());
        }
        let at = cg(at);
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

    pub fn type_text(&self, text: &str) -> Result<()> {
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

    pub fn press(&self, keys: &Keys) -> Result<()> {
        let key = code(&keys.key).context("unknown key")?;
        let mut flags = CGEventFlags::CGEventFlagNull;
        for (on, flag) in [
            (keys.cmd, CGEventFlags::CGEventFlagCommand),
            (keys.shift, CGEventFlags::CGEventFlagShift),
            (keys.ctrl, CGEventFlags::CGEventFlagControl),
            (keys.alt, CGEventFlags::CGEventFlagAlternate),
        ] {
            if on {
                flags |= flag;
            }
        }
        for down in [true, false] {
            let event = CGEvent::new_keyboard_event(source()?, key, down)
                .map_err(|_| anyhow::anyhow!("cannot create a key event"))?;
            event.set_flags(flags);
            post(Ok(event))?;
        }
        Ok(())
    }

    /// `down` and `right` in pixels; positive scrolls the content down and right.
    pub fn scroll(&self, at: Point, down: i32, right: i32) -> Result<()> {
        post(CGEvent::new_mouse_event(
            source()?,
            CGEventType::MouseMoved,
            cg(at),
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

    #[cfg(test)]
    pub fn knows_key(&self, name: &str) -> bool {
        code(name).is_some()
    }
}
