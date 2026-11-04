//! Semantic activation through macOS Accessibility, without posting mouse events.

use anyhow::{Context, Result, bail, ensure};
use core_foundation::{
    base::{CFType, CFTypeRef, TCFType},
    string::{CFString, CFStringRef},
};

use super::Point;

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
}

/// Press the exact accessible element at a global, top-left-relative display point.
/// No ancestor search, application activation, event injection or fallback: a failure
/// must not turn into a physical click, and an uncertain action must never be retried.
pub fn press(at: Point) -> Result<()> {
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
    check(result, false).context("cannot find the accessible control at this point")?;
    let element = element.context("there is no accessible control at this point")?;
    // SAFETY: the Copy result is a live AXUIElement, held until after the action.
    check(
        unsafe { AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), AX_TIMEOUT_SECONDS) },
        false,
    )?;
    let action = CFString::new("AXPress");
    // SAFETY: both the element and the action string outlive this call.
    check(
        unsafe { AXUIElementPerformAction(element.as_CFTypeRef(), action.as_concrete_TypeRef()) },
        true,
    )
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
                press(Point { x: value, y: 0.0 })
                    .unwrap_err()
                    .to_string()
                    .contains("coordinates")
            );
            assert!(
                press(Point { x: 0.0, y: value })
                    .unwrap_err()
                    .to_string()
                    .contains("coordinates")
            );
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
}
