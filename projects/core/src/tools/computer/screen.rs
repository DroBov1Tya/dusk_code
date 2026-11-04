//! The screen as the model sees it: the last screenshot fixes the picture's coordinates,
//! and every click or scroll is mapped from them back to the display's points.

use std::io;

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageFormat, codecs::jpeg::JpegEncoder};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{Desktop, Keys, Point, Rect, Window};
use crate::state::artifacts::{ArtifactStore, ImageArtifact};

/// Windows named in a failed lookup at most.
const MAX_LISTED_WINDOWS: usize = 20;
/// The longest edge of a screenshot; models shrink larger pictures anyway.
const MAX_EDGE: f64 = 1568.0;
const THUMBNAIL_EDGE: u32 = 256;
const JPEG_QUALITY: u8 = 80;

/// How a picture's pixel maps to the display: `origin + pixel * scale` in points.
#[derive(Debug, Clone, Copy)]
struct View {
    origin: Point,
    scale_x: f64,
    scale_y: f64,
    width: f64,
    height: f64,
    /// A window capture (or its crop) must still address the same visible window.
    window: Option<(u32, Rect)>,
}

/// A part of the last screenshot, in its pixels.
#[derive(Debug, Clone, Copy)]
pub struct Region {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

/// What a screenshot shows.
pub enum Target {
    Display,
    /// A part of the last screenshot.
    Region(Region),
    /// The frontmost window whose app or title holds this text.
    Window(String),
}

pub struct Screen {
    operation: Mutex<()>,
    #[cfg(target_os = "macos")]
    session: Mutex<Option<super::native_window::NativeWindow>>,
    view: Mutex<Option<View>>,
    artifacts: ArtifactStore,
}

impl Screen {
    pub fn new(artifacts: ArtifactStore) -> Self {
        Self {
            operation: Mutex::new(()),
            #[cfg(target_os = "macos")]
            session: Mutex::new(None),
            view: Mutex::new(None),
            artifacts,
        }
    }

    pub fn scope(&self) -> Option<(String, String)> {
        #[cfg(target_os = "macos")]
        {
            self.session
                .lock()
                .as_ref()
                .map(|session| (session.token.clone(), session.label.clone()))
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    }

    fn ensure_scope(&self, expected: Option<&str>) -> Result<()> {
        ensure!(
            self.scope().as_ref().map(|(id, _)| id.as_str()) == expected,
            "the window session changed while waiting; repeat the request in its new scope"
        );
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn select(
        &self,
        session: super::native_window::NativeWindow,
        expected: Option<&str>,
    ) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .context("a screen action is still running")?;
        self.ensure_scope(expected)?;
        *self.session.lock() = Some(session);
        *self.view.lock() = None;
        Ok(())
    }

    #[cfg(target_os = "macos")]
    pub(super) fn release(&self, expected: &str) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .context("a screen action is still running")?;
        self.ensure_scope(Some(expected))?;
        *self.session.lock() = None;
        *self.view.lock() = None;
        Ok(())
    }

    pub fn require_unscoped(&self) -> Result<()> {
        ensure!(
            self.scope().is_none(),
            "shared keyboard, system pointer and scrolling are disabled in a window session; release it explicitly first"
        );
        Ok(())
    }

    pub fn unscoped<T>(&self, action: impl FnOnce() -> Result<T>) -> Result<T> {
        let _operation = self
            .operation
            .try_lock()
            .context("a screen action is still running")?;
        self.require_unscoped()?;
        action()
    }

    pub fn click(
        &self,
        x: f64,
        y: f64,
        right: bool,
        count: i64,
        mode: super::click::PointerMode,
        expected_scope: Option<&str>,
    ) -> Result<()> {
        let _operation = self
            .operation
            .try_lock()
            .context("a screen action is still running")?;
        self.ensure_scope(expected_scope)?;
        #[cfg(target_os = "macos")]
        let session = self.session.lock().clone();
        #[cfg(target_os = "macos")]
        if let Some(session) = session {
            ensure!(
                mode == super::click::PointerMode::Independent,
                "system pointer is disabled in a window session"
            );
            mode.validate_click(right, count)?;
            let view = self.view()?;
            let at = view.point(x, y)?;
            ensure!(
                x < view.width && y < view.height,
                "click coordinates must be inside the screenshot"
            );
            let (_, bounds) = view
                .window
                .context("take a screenshot of the selected window first")?;
            self.mark(at);
            return session.press(at, bounds);
        }
        let (at, expected_window) = match mode {
            super::click::PointerMode::Independent => self.independent_point(x, y)?,
            super::click::PointerMode::System => (self.point(x, y)?, None),
        };
        self.mark(at);
        Desktop.click(at, right, count, mode, expected_window)
    }

    /// Shows the user where the agent acts now, in display points.
    pub fn mark(&self, at: Point) {
        Desktop.mark(at);
    }

    /// The display, a region of the last screenshot or one window, as a picture for
    /// the model, with what to tell it about the picture's size.
    pub fn capture(
        &self,
        target: Target,
        expected_scope: Option<&str>,
    ) -> Result<(ImageArtifact, String)> {
        let _operation = self
            .operation
            .try_lock()
            .context("a screen action is still running")?;
        self.ensure_scope(expected_scope)?;
        let previous = if expected_scope.is_some() {
            // A failed scoped capture must not retain stale coordinates.
            self.view.lock().take()
        } else {
            *self.view.lock()
        };
        let mut window: Option<Window> = None;
        #[cfg(target_os = "macos")]
        let session = self.session.lock().clone();
        #[cfg(target_os = "macos")]
        if let Some(session) = &session {
            ensure!(
                !matches!(target, Target::Window(_)),
                "a window session is pinned; omit window or release it explicitly"
            );
            window = Some(session.inspect()?);
        }
        let mut captured_window = None;
        let rect = match &target {
            Target::Display => match &window {
                Some(window) => {
                    captured_window = Some((window.id, window.bounds));
                    window.bounds
                }
                None => Desktop.display()?,
            },
            Target::Window(name) => {
                let found = find_window(name)?;
                let bounds = found.bounds;
                captured_window = Some((found.id, bounds));
                window = Some(found);
                bounds
            }
            &Target::Region(region) => {
                let view = previous.context("take a screenshot before requesting a region")?;
                let from = view.point(region.x, region.y)?;
                view.point(region.x + region.width, region.y + region.height)?;
                captured_window = view.window;
                if let Some(found) = &window {
                    ensure!(
                        view.window == Some((found.id, found.bounds)),
                        "the captured window moved or resized; take a new screenshot"
                    );
                }
                Rect {
                    x: from.x,
                    y: from.y,
                    width: region.width * view.scale_x,
                    height: region.height * view.scale_y,
                }
            }
        };
        ensure!(
            rect.width >= 1.0 && rect.height >= 1.0,
            "the region is empty"
        );
        let file = std::env::temp_dir().join(format!("duskcode-{}.png", Uuid::new_v4()));
        let taken = match &window {
            Some(window) => Desktop.capture_window(window, &file),
            None => Desktop.capture(rect, &file),
        };
        let captured =
            taken.and_then(|()| std::fs::read(&file).context("cannot read the screenshot"));
        let _ = std::fs::remove_file(&file);
        let image = image::load_from_memory(&captured?).context("invalid screenshot")?;
        #[cfg(target_os = "macos")]
        let mut image = image;
        #[cfg(target_os = "macos")]
        if let (Some(session), Some(window)) = (&session, &window) {
            let current = session.inspect()?;
            ensure!(
                current.bounds == window.bounds,
                "the window moved during capture; take a new screenshot"
            );
            if matches!(target, Target::Region(_)) {
                image = crop_window(&image, window.bounds, rect)?;
            }
        }
        // A Retina capture has two pixels a point; no picture is larger than the points.
        let fit = (MAX_EDGE / rect.width.max(rect.height)).min(1.0);
        let width = (rect.width * fit).round().max(1.0);
        let height = (rect.height * fit).round().max(1.0);
        let picture = image.resize_exact(
            width as u32,
            height as u32,
            image::imageops::FilterType::Triangle,
        );
        let artifact = self.save(&picture)?;
        *self.view.lock() = Some(View {
            origin: Point {
                x: rect.x,
                y: rect.y,
            },
            scale_x: rect.width / width,
            scale_y: rect.height / height,
            width,
            height,
            window: captured_window,
        });
        let what = match (&target, &window) {
            (_, Some(window)) if window.app.is_empty() => {
                format!("the window \"{}\"", window.title)
            }
            (_, Some(window)) => format!("the {} window \"{}\"", window.app, window.title),
            (Target::Region(_), None) => "the region".into(),
            _ => "the main display".into(),
        };
        let tools = if expected_scope.is_some() {
            "click and screenshot regions"
        } else {
            "click, scroll and screenshot regions"
        };
        let text = format!(
            "Screenshot {} of {what}, {width}x{height}. {tools} take x and y in this picture, \
             from its top left corner.",
            artifact.id
        );
        Ok((artifact, text))
    }

    /// The display point under the pixel `x`, `y` of the last screenshot.
    pub fn point(&self, x: f64, y: f64) -> Result<Point> {
        let view = self.view()?;
        view.point(x, y)
    }

    /// Independent activation of a captured window must not hit a covering window,
    /// or use coordinates left behind when the captured window moved or resized.
    pub fn independent_point(&self, x: f64, y: f64) -> Result<(Point, Option<(u32, Rect)>)> {
        let view = self.view()?;
        let at = view.point(x, y)?;
        ensure!(
            x < view.width && y < view.height,
            "click coordinates must be inside the screenshot, not its far edge"
        );
        if let Some((id, bounds)) = view.window {
            Desktop.ensure_window_at(at, id, bounds)?;
        }
        Ok((at, view.window))
    }

    fn view(&self) -> Result<View> {
        (*self.view.lock()).context("take a screenshot first: coordinates come from it")
    }

    fn save(&self, picture: &DynamicImage) -> Result<ImageArtifact> {
        let mut jpeg = Vec::new();
        JpegEncoder::new_with_quality(&mut jpeg, JPEG_QUALITY)
            .encode_image(&picture.to_rgb8())
            .context("cannot encode the screenshot")?;
        let mut thumbnail = io::Cursor::new(Vec::new());
        picture
            .thumbnail(THUMBNAIL_EDGE, THUMBNAIL_EDGE)
            .write_to(&mut thumbnail, ImageFormat::Png)
            .context("cannot encode the screenshot thumbnail")?;
        self.artifacts.save(
            ImageArtifact {
                id: String::new(),
                title: Some("screenshot".into()),
                description: None,
                mime_type: "image/jpeg".into(),
                width: picture.width(),
                height: picture.height(),
                byte_len: 0,
                sha256: String::new(),
                for_model: true,
            },
            &jpeg,
            thumbnail.get_ref(),
        )
    }
}

/// Crop the captured window pixels, never the display behind/above it.
#[cfg(target_os = "macos")]
fn crop_window(image: &DynamicImage, bounds: Rect, crop: Rect) -> Result<DynamicImage> {
    ensure!(
        bounds.width > 0.0 && bounds.height > 0.0 && crop.width > 0.0 && crop.height > 0.0,
        "invalid window crop"
    );
    let sx = f64::from(image.width()) / bounds.width;
    let sy = f64::from(image.height()) / bounds.height;
    let x = ((crop.x - bounds.x) * sx).round();
    let y = ((crop.y - bounds.y) * sy).round();
    let right = ((crop.x + crop.width - bounds.x) * sx).round();
    let bottom = ((crop.y + crop.height - bounds.y) * sy).round();
    ensure!(
        x >= 0.0
            && y >= 0.0
            && right <= f64::from(image.width())
            && bottom <= f64::from(image.height())
            && right > x
            && bottom > y,
        "crop is outside the captured window"
    );
    Ok(image.crop_imm(x as u32, y as u32, (right - x) as u32, (bottom - y) as u32))
}

impl View {
    fn point(self, x: f64, y: f64) -> Result<Point> {
        ensure!(
            (0.0..=self.width).contains(&x) && (0.0..=self.height).contains(&y),
            "x and y must lie in the last screenshot, {}x{}",
            self.width,
            self.height
        );
        Ok(Point {
            x: self.origin.x + x * self.scale_x,
            y: self.origin.y + y * self.scale_y,
        })
    }
}

/// The largest window of the app named `name`, else the largest whose app or title holds
/// it, so a toolbar strip never wins over the window itself; a miss names the windows
/// there are.
fn find_window(name: &str) -> Result<Window> {
    let wanted = name.trim().to_lowercase();
    ensure!(!wanted.is_empty(), "name the app or window to capture");
    let windows = Desktop.windows()?;
    let area = |window: &Window| window.bounds.width * window.bounds.height;
    let largest = |matches: &dyn Fn(&Window) -> bool| {
        windows
            .iter()
            .enumerate()
            .filter(|(_, window)| matches(window))
            .max_by(|(_, a), (_, b)| area(a).total_cmp(&area(b)))
            .map(|(at, _)| at)
    };
    let found = largest(&|window| window.app.to_lowercase() == wanted).or_else(|| {
        largest(&|window| {
            window.app.to_lowercase().contains(&wanted)
                || window.title.to_lowercase().contains(&wanted)
        })
    });
    if let Some(at) = found {
        return windows
            .into_iter()
            .nth(at)
            .context("the window list changed");
    }
    let open: Vec<String> = windows
        .iter()
        .take(MAX_LISTED_WINDOWS)
        .map(|window| match window.title.is_empty() {
            true => window.app.clone(),
            false => format!("{}: {}", window.app, window.title),
        })
        .collect();
    bail!("no window matches {name:?}; open ones: {}", open.join("; "))
}

/// The keys besides letters and digits that every desktop presses by these names.
pub const KEY_NAMES: [&str; 27] = [
    "enter",
    "tab",
    "space",
    "backspace",
    "delete",
    "esc",
    "left",
    "right",
    "up",
    "down",
    "home",
    "end",
    "pageup",
    "pagedown",
    "f1",
    "f2",
    "f3",
    "f4",
    "f5",
    "f6",
    "f7",
    "f8",
    "f9",
    "f10",
    "f11",
    "f12",
    "forwarddelete",
];

/// `cmd+shift+t`, `enter` or `f5` as a key with its modifiers. The key comes out by its
/// one name: a lowercase letter, a digit or one of [`KEY_NAMES`].
pub fn keys(combo: &str) -> Result<Keys> {
    let mut keys = Keys {
        key: String::new(),
        cmd: false,
        shift: false,
        ctrl: false,
        alt: false,
    };
    for part in combo
        .split('+')
        .map(|part| part.trim().to_ascii_lowercase())
    {
        match part.as_str() {
            "cmd" | "command" | "meta" | "super" | "win" => keys.cmd = true,
            "shift" => keys.shift = true,
            "ctrl" | "control" => keys.ctrl = true,
            "alt" | "option" | "opt" => keys.alt = true,
            name => {
                ensure!(
                    keys.key.is_empty(),
                    "one key at a time, with modifiers: {combo}"
                );
                keys.key = key_name(name).with_context(|| format!("unknown key {name:?}"))?;
            }
        }
    }
    ensure!(
        !keys.key.is_empty(),
        "name a key, not only modifiers: {combo}"
    );
    Ok(keys)
}

fn key_name(name: &str) -> Option<String> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(name.to_owned());
    }
    let name = match name {
        "return" => "enter",
        "escape" => "esc",
        _ => name,
    };
    KEY_NAMES.contains(&name).then(|| name.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_name_one_key_and_its_modifiers() {
        let combo = keys("cmd+shift+T").unwrap();
        assert_eq!(combo.key, "t");
        assert!(combo.cmd && combo.shift && !combo.ctrl && !combo.alt);
        assert_eq!(keys("Return").unwrap().key, "enter");
        assert_eq!(keys("5").unwrap().key, "5");
        for wrong in ["cmd", "a+b", "cmd+nope", "", "é"] {
            assert!(keys(wrong).is_err(), "{wrong}");
        }
        // Every name the parser lets through, the desktop can press.
        for name in KEY_NAMES.iter().copied().chain(["a", "z", "0", "9"]) {
            assert!(Desktop.knows_key(name), "{name}");
        }
    }

    #[test]
    fn clicks_map_from_the_picture_to_points_and_stay_inside_it() {
        let screen = Screen::new(ArtifactStore::new(std::env::temp_dir()));
        assert!(screen.point(1.0, 1.0).is_err(), "no screenshot yet");
        *screen.view.lock() = Some(View {
            origin: Point { x: 100.0, y: 50.0 },
            scale_x: 2.0,
            scale_y: 2.0,
            width: 800.0,
            height: 500.0,
            window: None,
        });
        let point = screen.point(10.0, 20.0).unwrap();
        assert_eq!((point.x, point.y), (120.0, 90.0));
        assert!(screen.point(801.0, 0.0).is_err());
        assert!(screen.point(-1.0, 0.0).is_err());
        assert!(screen.independent_point(800.0, 0.0).is_err());
        assert!(screen.independent_point(0.0, 500.0).is_err());
        assert!(screen.independent_point(f64::NAN, 0.0).is_err());
        assert!(screen.independent_point(0.0, f64::INFINITY).is_err());
    }

    #[cfg(target_os = "macos")]
    #[test]
    fn crop_uses_only_window_pixels_with_distinct_axis_scales() {
        let bounds = Rect {
            x: -500.0,
            y: -100.0,
            width: 4.0,
            height: 4.0,
        };
        let source = DynamicImage::ImageRgb8(image::RgbImage::from_fn(8, 12, |x, y| {
            image::Rgb([x as u8, y as u8, 0])
        }));
        let region = Rect {
            x: -499.0,
            y: -98.0,
            width: 3.0,
            height: 2.0,
        };
        let cropped = crop_window(&source, bounds, region).unwrap().into_rgb8();
        assert_eq!(cropped.dimensions(), (6, 6));
        assert_eq!(cropped.get_pixel(0, 0).0, [2, 6, 0]);
        assert_eq!(cropped.get_pixel(5, 5).0, [7, 11, 0]);
        assert!(
            crop_window(
                &source,
                bounds,
                Rect {
                    width: 4.0,
                    ..region
                }
            )
            .is_err()
        );
    }

    #[test]
    fn window_crops_preserve_global_negative_display_coordinates() {
        let view = View {
            origin: Point {
                x: -1600.0,
                y: -900.0,
            },
            scale_x: 1.5,
            scale_y: 1.5,
            width: 800.0,
            height: 500.0,
            window: None,
        };
        let point = view.point(100.0, 200.0).unwrap();
        assert_eq!((point.x, point.y), (-1450.0, -600.0));
    }
}

#[cfg(all(test, target_os = "macos"))]
#[path = "window_tests.rs"]
mod window_tests;
