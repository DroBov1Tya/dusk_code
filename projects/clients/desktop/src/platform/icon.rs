//! The app icon. A bare binary has none on macOS (only a bundle does), so the Dock
//! gets it at start from the picture built into the binary. On Linux the window's
//! app ID ties it to the installed `duskcode.desktop` and its icon.

/// The window's app ID; the installer names the desktop entry after it.
pub const APP_ID: &str = "duskcode";

#[cfg(target_os = "macos")]
pub fn set_dock_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    static ICON: &[u8] = include_bytes!("../../assets/icon.png");
    let Some(main_thread) = MainThreadMarker::new() else {
        tracing::warn!("the dock icon is set off the main thread; skipped");
        return;
    };
    let data = NSData::with_bytes(ICON);
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        tracing::warn!("the built-in icon is not a picture AppKit reads");
        return;
    };
    let app = NSApplication::sharedApplication(main_thread);
    // SAFETY: called on the main thread with a valid, non-nil image.
    unsafe { app.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub fn set_dock_icon() {}
