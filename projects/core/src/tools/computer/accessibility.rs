//! Semantic activation through macOS Accessibility, without posting mouse events.

use anyhow::{Context, Result, bail, ensure};
use core_foundation::{
    array::CFArray,
    base::{CFType, CFTypeRef, TCFType},
    string::{CFString, CFStringRef},
};

use core_graphics::geometry::{CGPoint, CGSize};

use super::{Point, Rect};

// AXUIElement is an opaque Core Foundation object. Each Create/Copy result below is
// owned by a CFType and released on every exit path, on the blocking worker thread.
type AXUIElementRef = CFTypeRef;
const AX_TIMEOUT_SECONDS: f32 = 2.0;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn AXUIElementCreateSystemWide() -> AXUIElementRef;
    fn AXUIElementCopyElementAtPosition(
        application: AXUIElementRef,
        x: f32,
        y: f32,
        element: *mut AXUIElementRef,
    ) -> i32;
    fn AXUIElementSetMessagingTimeout(element: AXUIElementRef, seconds: f32) -> i32;
    fn AXUIElementPerformAction(element: AXUIElementRef, action: CFStringRef) -> i32;
    fn AXUIElementCreateApplication(pid: i32) -> AXUIElementRef;
    fn AXUIElementGetPid(element: AXUIElementRef, pid: *mut i32) -> i32;
    fn AXUIElementCopyAttributeValue(
        element: AXUIElementRef,
        attribute: CFStringRef,
        value: *mut CFTypeRef,
    ) -> i32;
    fn AXValueGetType(value: CFTypeRef) -> u32;
    fn AXValueGetValue(value: CFTypeRef, kind: u32, result: *mut std::ffi::c_void) -> bool;
    fn AXValueGetTypeID() -> usize;
}

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    fn CFEqual(a: CFTypeRef, b: CFTypeRef) -> u8;
}

/// Press the exact accessible element at a global, top-left-relative display point.
/// No ancestor search, application activation, event injection or fallback: a failure
/// must not turn into a physical click, and an uncertain action must never be retried.
pub fn press(at: Point, expected_window: Option<(u32, Rect)>) -> Result<()> {
    ensure!(
        [at.x, at.y]
            .into_iter()
            .all(|value| value.is_finite() && value.abs() <= f64::from(f32::MAX)),
        "the independent click coordinates are invalid"
    );
    super::macos::require_accessibility()?;
    // SAFETY: Create returns a retained object, or null on allocation failure.
    let system = owned(unsafe { AXUIElementCreateSystemWide() })
        .context("cannot create the system accessibility object")?;
    // The system-wide object's timeout applies to this process. All calls in this
    // module use the same bound; no AX objects cross threads or survive the action.
    // SAFETY: system is a live AXUIElement for the duration of this call.
    check(
        unsafe { AXUIElementSetMessagingTimeout(system.as_CFTypeRef(), AX_TIMEOUT_SECONDS) },
        false,
    )?;
    let mut hit = std::ptr::null();
    // AX uses global screen points with the origin at the main display's top left,
    // as do Screen's screenshot transforms, including negative secondary displays.
    // SAFETY: system and the output pointer are valid; coordinates were checked.
    let result = unsafe {
        AXUIElementCopyElementAtPosition(system.as_CFTypeRef(), at.x as f32, at.y as f32, &mut hit)
    };
    let element = owned(hit);
    check_hit_test(result)?;
    let element = element.context("there is no accessible control at this point")?;
    // SAFETY: the Copy result is a live AXUIElement, held until after the action.
    check(
        unsafe { AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), AX_TIMEOUT_SECONDS) },
        false,
    )?;
    verify_target(element.as_CFTypeRef(), at, expected_window)?;
    let action = CFString::new("AXPress");
    // SAFETY: both the element and the action string outlive this call.
    check(
        unsafe { AXUIElementPerformAction(element.as_CFTypeRef(), action.as_concrete_TypeRef()) },
        true,
    )
}

fn check_hit_test(code: i32) -> Result<()> {
    if code == -25200 {
        bail!(
            "macOS Accessibility hit-testing failed (kAXErrorFailure, -25200); \
             the cause is unknown and no action was sent. Take a new screenshot and \
             check the target window. Some apps or controls may not support Accessibility; \
             mode system uses the user's pointer and requires separate approval"
        );
    }
    check(code, false).context("macOS Accessibility hit-testing failed; no action was sent")
}

/// A visual window and the AX hit-test can disagree, especially for click-through
/// windows. Match the frontmost CG window to exactly one AX window, then require the
/// hit's AXWindow to be that object. Refuse incomplete or ambiguous metadata.
fn verify_target(
    element: AXUIElementRef,
    at: Point,
    expected_window: Option<(u32, Rect)>,
) -> Result<()> {
    let (window_id, pid, bounds) = super::macos::independent_window(at)?;
    if let Some(expected) = expected_window {
        ensure!(
            (window_id, bounds) == expected,
            "the captured window changed or is covered; no action was sent"
        );
    }
    let mut target_pid = 0;
    // SAFETY: element and output are valid.
    check(
        unsafe { AXUIElementGetPid(element, &mut target_pid) },
        false,
    )?;
    ensure!(
        target_pid == pid,
        "the accessible control belongs to a different window; no action was sent"
    );
    let target_window = attribute(element, "AXWindow")?;
    // SAFETY: Create returns a retained application element.
    let app = owned(unsafe { AXUIElementCreateApplication(pid) })
        .context("cannot inspect the target application")?;
    check(
        unsafe { AXUIElementSetMessagingTimeout(app.as_CFTypeRef(), AX_TIMEOUT_SECONDS) },
        false,
    )?;
    let list = attribute(app.as_CFTypeRef(), "AXWindows")?;
    let windows = list
        .downcast::<CFArray>()
        .context("cannot identify the target window; no action was sent")?;
    let mut matching = 0;
    let mut target_matches = false;
    for window in windows.iter() {
        let window = *window as CFTypeRef;
        // All listed windows must have inspectable geometry: otherwise uniqueness
        // cannot be established even if a different one happened to match.
        let window_bounds = window_rect(window)?;
        if window_bounds == bounds {
            matching += 1;
            // SAFETY: both objects remain retained by their owner for this comparison.
            target_matches |= unsafe { CFEqual(window, target_window.as_CFTypeRef()) != 0 };
        }
    }
    ensure_unique_match(matching, target_matches)?;
    // Recheck frontmost geometry after the AX queries, not only before them.
    ensure!(
        super::macos::independent_window(at)? == (window_id, pid, bounds),
        "the window changed during the click; no action was sent"
    );
    Ok(())
}

fn ensure_unique_match(matching: usize, target_matches: bool) -> Result<()> {
    ensure!(
        matching == 1 && target_matches,
        "the visible window and accessible control cannot be matched uniquely; no action was sent"
    );
    Ok(())
}

fn attribute(element: AXUIElementRef, name: &str) -> Result<CFType> {
    let name = CFString::new(name);
    let mut value = std::ptr::null();
    // SAFETY: element and output pointer are valid, name lives for the call.
    let result =
        unsafe { AXUIElementCopyAttributeValue(element, name.as_concrete_TypeRef(), &mut value) };
    let value = owned(value);
    check(result, false).context("cannot identify the target window; no action was sent")?;
    value.context("cannot identify the target window; no action was sent")
}

fn window_rect(window: AXUIElementRef) -> Result<Rect> {
    let position = attribute(window, "AXPosition")?;
    let size = attribute(window, "AXSize")?;
    ensure!(
        position.type_of() == unsafe { AXValueGetTypeID() }
            && size.type_of() == unsafe { AXValueGetTypeID() },
        "window geometry is unavailable; no action was sent"
    );
    // AXValueType: CGPoint = 1, CGSize = 2.
    let mut point = CGPoint::new(0.0, 0.0);
    let mut dimensions = CGSize::new(0.0, 0.0);
    ensure!(
        unsafe {
            AXValueGetType(position.as_CFTypeRef()) == 1
                && AXValueGetValue(position.as_CFTypeRef(), 1, &mut point as *mut _ as *mut _)
                && AXValueGetType(size.as_CFTypeRef()) == 2
                && AXValueGetValue(size.as_CFTypeRef(), 2, &mut dimensions as *mut _ as *mut _)
        },
        "window geometry is unavailable; no action was sent"
    );
    Ok(Rect {
        x: point.x,
        y: point.y,
        width: dimensions.width,
        height: dimensions.height,
    })
}

fn owned(reference: CFTypeRef) -> Option<CFType> {
    if reference.is_null() {
        return None;
    }
    // SAFETY: callers only pass retained AX Create/Copy results; CFType releases them.
    Some(unsafe { CFType::wrap_under_create_rule(reference) })
}

fn check(code: i32, action: bool) -> Result<()> {
    match code {
        0 => Ok(()),
        -25202 => bail!("the accessible control no longer exists; take a new screenshot"),
        -25204 if action => bail!(
            "AXPress did not confirm completion; the action may already have happened. \
             Check the result before doing anything else; do not retry automatically"
        ),
        -25204 => bail!(
            "the application did not complete the accessibility request; it may be busy or unavailable"
        ),
        -25206 | -25208 => bail!(
            "this element does not support independent activation (AXPress); no mouse event was sent"
        ),
        -25211 => bail!("Accessibility permission is unavailable; no mouse event was sent"),
        -25212 => bail!("there is no accessible control at this point"),
        _ => bail!("macOS Accessibility failed with error {code}; no mouse event was sent"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn invalid_coordinates_are_rejected_before_any_accessibility_access() {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, f64::MAX] {
            assert!(
                press(Point { x: value, y: 0.0 }, None)
                    .unwrap_err()
                    .to_string()
                    .contains("coordinates")
            );
            assert!(
                press(Point { x: 0.0, y: value }, None)
                    .unwrap_err()
                    .to_string()
                    .contains("coordinates")
            );
        }
    }

    #[test]
    fn window_identity_requires_one_matching_window_and_the_actual_hit() {
        assert!(ensure_unique_match(1, true).is_ok());
        for (count, hit) in [(0, false), (1, false), (2, true), (2, false)] {
            assert!(ensure_unique_match(count, hit).is_err());
        }
    }

    #[test]
    fn uncertain_actions_require_observation_and_do_not_claim_failure() {
        let action = check(-25204, true).unwrap_err().to_string();
        assert!(action.contains("may already have happened"));
        assert!(action.contains("do not retry automatically"));
        assert!(
            !check(-25204, false)
                .unwrap_err()
                .to_string()
                .contains("already")
        );
        assert!(
            check(-25206, true)
                .unwrap_err()
                .to_string()
                .contains("does not support")
        );
        assert!(
            check(-25211, false)
                .unwrap_err()
                .to_string()
                .contains("permission")
        );
    }

    #[test]
    fn generic_hit_test_failure_does_not_claim_the_control_is_missing() {
        let message = check_hit_test(-25200).unwrap_err().to_string();
        assert!(message.contains("kAXErrorFailure"));
        assert!(message.contains("cause is unknown"));
        assert!(message.contains("no action was sent"));
        assert!(message.contains("separate approval"));
        assert!(!message.contains("cannot find"));
    }
}
