//! The screen as the model sees it: the last screenshot fixes the picture's coordinates,
//! and every click or scroll is mapped from them back to the display's points.

use std::io;

use anyhow::{Context, Result, bail, ensure};
use image::{DynamicImage, ImageFormat, codecs::jpeg::JpegEncoder};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{Keys, Point, Rect, Window, desktop};
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
    scale: f64,
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
    view: Mutex<Option<View>>,
    artifacts: ArtifactStore,
}

impl Screen {
    pub fn new(artifacts: ArtifactStore) -> Self {
        Self {
            view: Mutex::new(None),
            artifacts,
        }
    }

    /// Shows the user where the agent acts now, in display points.
    pub fn mark(&self, at: Point) {
        desktop::mark(at);
    }

    /// The display, a region of the last screenshot or one window, as a picture for
    /// the model, with what to tell it about the picture's size.
    pub fn capture(&self, target: Target) -> Result<(ImageArtifact, String)> {
        let mut window = None;
        let mut captured_window = None;
        let rect = match &target {
            Target::Display => desktop::display()?,
            Target::Window(name) => {
                let found = find_window(name)?;
                let bounds = found.bounds;
                captured_window = Some((found.id, bounds));
                window = Some(found);
                bounds
            }
            &Target::Region(region) => {
                let from = self.point(region.x, region.y)?;
                // The far corner must lie in the picture too.
                self.point(region.x + region.width, region.y + region.height)?;
                let view = self.view()?;
                captured_window = view.window;
                Rect {
                    x: from.x,
                    y: from.y,
                    width: region.width * view.scale,
                    height: region.height * view.scale,
                }
            }
        };
        ensure!(
            rect.width >= 1.0 && rect.height >= 1.0,
            "the region is empty"
        );
        let file = std::env::temp_dir().join(format!("duskcode-{}.png", Uuid::new_v4()));
        let taken = match &window {
            Some(window) => desktop::capture_window(window, &file),
            None => desktop::capture(rect, &file),
        };
        let captured =
            taken.and_then(|()| std::fs::read(&file).context("cannot read the screenshot"));
        let _ = std::fs::remove_file(&file);
        let image = image::load_from_memory(&captured?).context("invalid screenshot")?;
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
            scale: rect.width / width,
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
        let text = format!(
            "Screenshot {} of {what}, {width}x{height}. click, scroll and screenshot regions \
             take x and y in this picture, from its top left corner.",
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
    pub fn independent_point(&self, x: f64, y: f64) -> Result<Point> {
        let view = self.view()?;
        let at = view.point(x, y)?;
        ensure!(
            x < view.width && y < view.height,
            "click coordinates must be inside the screenshot, not its far edge"
        );
        if let Some((id, bounds)) = view.window {
            desktop::ensure_window_at(at, id, bounds)?;
        }
        Ok(at)
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

impl View {
    fn point(self, x: f64, y: f64) -> Result<Point> {
        ensure!(
            (0.0..=self.width).contains(&x) && (0.0..=self.height).contains(&y),
            "x and y must lie in the last screenshot, {}x{}",
            self.width,
            self.height
        );
        Ok(Point {
            x: self.origin.x + x * self.scale,
            y: self.origin.y + y * self.scale,
        })
    }
}

/// The largest window of the app named `name`, else the largest whose app or title holds
/// it, so a toolbar strip never wins over the window itself; a miss names the windows
/// there are.
fn find_window(name: &str) -> Result<Window> {
    let wanted = name.trim().to_lowercase();
    ensure!(!wanted.is_empty(), "name the app or window to capture");
    let windows = desktop::windows()?;
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
            assert!(desktop::knows_key(name), "{name}");
        }
    }

    #[test]
    fn clicks_map_from_the_picture_to_points_and_stay_inside_it() {
        let screen = Screen::new(ArtifactStore::new(std::env::temp_dir()));
        assert!(screen.point(1.0, 1.0).is_err(), "no screenshot yet");
        *screen.view.lock() = Some(View {
            origin: Point { x: 100.0, y: 50.0 },
            scale: 2.0,
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

    #[test]
    fn window_crops_preserve_global_negative_display_coordinates() {
        let view = View {
            origin: Point {
                x: -1600.0,
                y: -900.0,
            },
            scale: 1.5,
            width: 800.0,
            height: 500.0,
            window: None,
        };
        let point = view.point(100.0, 200.0).unwrap();
        assert_eq!((point.x, point.y), (-1450.0, -600.0));
    }
}
