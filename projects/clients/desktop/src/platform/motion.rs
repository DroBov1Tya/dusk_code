//! Whether the person asked the system for less motion. gpui keeps a flag that makes
//! every animation draw its resting state, but nothing fills it from the system, so the
//! window does at start. It is read once: a change in the system's settings is seen at
//! the next start.

/// The system's "reduce motion" setting; `false` where there is no way to ask.
#[cfg(target_os = "macos")]
pub fn reduced() -> bool {
    objc2_app_kit::NSWorkspace::sharedWorkspace().accessibilityDisplayShouldReduceMotion()
}

#[cfg(not(target_os = "macos"))]
pub fn reduced() -> bool {
    false
}
