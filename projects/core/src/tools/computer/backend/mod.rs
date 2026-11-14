//! The desktop the computer tools drive: `macos` posts input as the HID system would and
//! captures through the system, with `accessibility` for presses that leave the pointer
//! alone and `native_window` for one pinned window; `linux` runs the X11 or Wayland
//! programs the desktop has. Each is built only on its system; `Desktop` is the one here.

#[cfg(target_os = "macos")]
mod accessibility;
#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "macos")]
mod native_window;

#[cfg(target_os = "linux")]
pub use linux::Desktop;
#[cfg(target_os = "macos")]
pub use macos::Desktop;
#[cfg(target_os = "macos")]
pub use native_window::NativeWindow;
