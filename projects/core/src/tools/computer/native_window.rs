//! A retained AXWindow lives on one thread until its scoped session ends. A CG ID
//! alone is not an identity: it can be reused after a window or process closes.

use std::sync::mpsc::{self, Sender};

use anyhow::{Context, Result, ensure};
use core_foundation::{
    array::CFArray,
    base::{CFType, TCFType},
    string::CFString,
};

use super::{Desktop, Point, Rect, Window, WindowSnapshot, accessibility::*};

const MAX_WINDOWS: usize = 128;

#[derive(Clone)]
pub(super) struct NativeWindow {
    pub token: String,
    pub label: String,
    requests: Sender<Request>,
}

enum Request {
    Inspect(Sender<Result<Window>>),
    Press(Point, WindowSnapshot, Sender<Result<()>>),
}

impl NativeWindow {
    pub fn select(id: u32, pid: i32) -> Result<Self> {
        ensure!(
            id != 0 && pid > 0,
            "select a window_id and pid from window_session list"
        );
        let (requests, incoming) = mpsc::channel();
        let (ready, result) = mpsc::channel();
        std::thread::Builder::new()
            .name("dusk-window".into())
            .spawn(move || {
                let pinned = match Pinned::new(id, pid) {
                    Ok(pinned) => pinned,
                    Err(error) => {
                        let _ = ready.send(Err(error));
                        return;
                    }
                };
                let _ = ready.send(pinned.inspect());
                while let Ok(request) = incoming.recv() {
                    match request {
                        Request::Inspect(reply) => {
                            let _ = reply.send(pinned.inspect());
                        }
                        Request::Press(at, window, reply) => {
                            let _ = reply.send(pinned.press(at, window));
                        }
                    }
                }
            })
            .context("cannot start the window session")?;
        let window = result.recv().context("window selection stopped")??;
        Ok(Self {
            token: uuid::Uuid::new_v4().to_string(),
            label: format!(
                "{:?}: {:?} (window {}, pid {})",
                window.app, window.title, id, pid
            ),
            requests,
        })
    }

    pub fn inspect(&self) -> Result<Window> {
        let (reply, result) = mpsc::channel();
        self.requests
            .send(Request::Inspect(reply))
            .context("window session stopped")?;
        result.recv().context("window session stopped")?
    }

    pub fn press(&self, at: Point, window: WindowSnapshot) -> Result<()> {
        let (reply, result) = mpsc::channel();
        self.requests
            .send(Request::Press(at, window, reply))
            .context("window session stopped")?;
        result.recv().context("window session stopped")?
    }
}

struct Pinned {
    id: u32,
    pid: i32,
    app: CFType,
    window: CFType,
}

impl Pinned {
    fn new(id: u32, pid: i32) -> Result<Self> {
        super::macos::require_accessibility()?;
        // SAFETY: retained AX objects are created, used and released on this thread.
        let app = owned(unsafe { AXUIElementCreateApplication(pid) })
            .context("application unavailable")?;
        timeout(&app)?;
        let current = current_window(id, pid)?;
        let window = matching_window(&app, current.bounds)?;
        timeout(&window)?;
        Ok(Self {
            id,
            pid,
            app,
            window,
        })
    }

    fn inspect(&self) -> Result<Window> {
        let current = current_window(self.id, self.pid)?;
        let matched = matching_window(&self.app, current.bounds)?;
        ensure!(
            matched == self.window && window_rect(self.window.as_CFTypeRef())? == current.bounds,
            "the selected window closed, changed identity or is ambiguous; select it again"
        );
        Ok(current)
    }

    fn press(&self, at: Point, expected: WindowSnapshot) -> Result<()> {
        ensure!(
            [at.x, at.y]
                .into_iter()
                .all(|v| v.is_finite() && v.abs() <= f64::from(f32::MAX)),
            "invalid window coordinates"
        );
        ensure!(
            WindowSnapshot::from(&self.inspect()?) == expected,
            "the selected window changed; take a new screenshot"
        );
        let bounds = expected.bounds;
        ensure!(
            (bounds.x..bounds.x + bounds.width).contains(&at.x)
                && (bounds.y..bounds.y + bounds.height).contains(&at.y),
            "point is outside the selected window"
        );
        // App-scoped hit testing can see a covered window, but it may instead hit a
        // sibling window in this same process. Never walk ancestors or redirect it.
        let (code, hit) = hit_test(self.app.as_CFTypeRef(), at);
        check(code, false).context(
            "macOS Accessibility hit-testing in the selected app failed; no action was sent",
        )?;
        let hit = hit.context("no accessible control at this point")?;
        timeout(&hit)?;
        let mut pid = 0;
        // SAFETY: the hit element stays retained and pid is writable.
        check(
            unsafe { AXUIElementGetPid(hit.as_CFTypeRef(), &mut pid) },
            false,
        )?;
        ensure!(
            pid == self.pid && attribute(hit.as_CFTypeRef(), "AXWindow")? == self.window,
            "the accessible control is outside the selected window; no action was sent"
        );
        ensure!(
            WindowSnapshot::from(&self.inspect()?) == expected,
            "the selected window changed; no action was sent"
        );
        // The final identity check and the remote action cannot be atomic. The
        // retained control prevents rerouting to a newly reused window ID.
        let action = CFString::new("AXPress");
        // SAFETY: both arguments stay retained through the request.
        check(
            unsafe { AXUIElementPerformAction(hit.as_CFTypeRef(), action.as_concrete_TypeRef()) },
            true,
        )
    }
}

fn timeout(element: &CFType) -> Result<()> {
    // SAFETY: callers pass a retained AXUIElement.
    check(
        unsafe { AXUIElementSetMessagingTimeout(element.as_CFTypeRef(), 2.0) },
        false,
    )
}

fn current_window(id: u32, pid: i32) -> Result<Window> {
    let windows = Desktop.windows()?;
    let found = windows
        .iter()
        .find(|w| w.id == id && w.pid == pid)
        .context("the selected window is closed, minimized, hidden or unavailable")?;
    ensure!(
        windows
            .iter()
            .filter(|w| w.pid == pid && w.bounds == found.bounds)
            .count()
            == 1,
        "the selected window has ambiguous geometry; no action was sent"
    );
    Ok(found.clone())
}

fn matching_window(app: &CFType, bounds: Rect) -> Result<CFType> {
    let list = attribute(app.as_CFTypeRef(), "AXWindows")?;
    let windows = list
        .downcast::<CFArray>()
        .context("window list unavailable")?;
    ensure!(
        windows.len() <= MAX_WINDOWS as isize,
        "too many windows to identify the target safely"
    );
    let mut matching = Vec::new();
    for window in windows.iter() {
        // SAFETY: the array retains each AXWindow during this read.
        let window = unsafe { CFType::wrap_under_get_rule(*window as _) };
        timeout(&window)?;
        if window_rect(window.as_CFTypeRef())? == bounds {
            matching.push(window);
        }
    }
    ensure!(
        matching.len() == 1,
        "the selected CG and AX windows cannot be matched uniquely"
    );
    Ok(matching.remove(0))
}
