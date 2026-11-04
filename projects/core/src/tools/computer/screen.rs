//! The screen as the model sees it: the last screenshot fixes the picture's coordinates,
//! and every click or scroll is mapped from them back to the display's points.

use std::io;

use anyhow::{Context, Result, bail, ensure};
use core_graphics::{
    event::{CGEventFlags, CGKeyCode, KeyCode},
    geometry::{CGPoint, CGRect, CGSize},
};
use image::{DynamicImage, ImageFormat, codecs::jpeg::JpegEncoder};
use parking_lot::Mutex;
use uuid::Uuid;

use super::{cursor::Cursor, macos};
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
    origin: CGPoint,
    scale: f64,
    width: f64,
    height: f64,
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
    pub fn mark(&self, at: CGPoint) {
        Cursor::shared().show(at);
    }

    /// The display, a region of the last screenshot or one window, as a picture for
    /// the model, with what to tell it about the picture's size.
    pub fn capture(&self, target: Target) -> Result<(ImageArtifact, String)> {
        let mut window = None;
        let rect = match &target {
            Target::Display => macos::display(),
            Target::Window(name) => {
                let found = find_window(name)?;
                let bounds = found.bounds;
                window = Some(found);
                bounds
            }
            &Target::Region(region) => {
                let from = self.point(region.x, region.y)?;
                // The far corner must lie in the picture too.
                self.point(region.x + region.width, region.y + region.height)?;
                let view = self.view()?;
                CGRect::new(
                    &from,
                    &CGSize::new(region.width * view.scale, region.height * view.scale),
                )
            }
        };
        ensure!(
            rect.size.width >= 1.0 && rect.size.height >= 1.0,
            "the region is empty"
        );
        let file = std::env::temp_dir().join(format!("duskcode-{}.png", Uuid::new_v4()));
        let taken = match &window {
            Some(window) => macos::capture_window(window.id, &file),
            None => macos::capture(rect, &file),
        };
        let captured =
            taken.and_then(|()| std::fs::read(&file).context("cannot read the screenshot"));
        let _ = std::fs::remove_file(&file);
        let image = image::load_from_memory(&captured?).context("invalid screenshot")?;
        // A Retina capture has two pixels a point; no picture is larger than the points.
        let fit = (MAX_EDGE / rect.size.width.max(rect.size.height)).min(1.0);
        let width = (rect.size.width * fit).round().max(1.0);
        let height = (rect.size.height * fit).round().max(1.0);
        let picture = image.resize_exact(
            width as u32,
            height as u32,
            image::imageops::FilterType::Triangle,
        );
        let artifact = self.save(&picture)?;
        *self.view.lock() = Some(View {
            origin: rect.origin,
            scale: rect.size.width / width,
            width,
            height,
        });
        let what = match (&target, &window) {
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
    pub fn point(&self, x: f64, y: f64) -> Result<CGPoint> {
        let view = self.view()?;
        ensure!(
            (0.0..=view.width).contains(&x) && (0.0..=view.height).contains(&y),
            "x and y must lie in the last screenshot, {}x{}",
            view.width,
            view.height
        );
        Ok(CGPoint::new(
            view.origin.x + x * view.scale,
            view.origin.y + y * view.scale,
        ))
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

/// The largest window of the app named `name`, else the largest whose app or title holds
/// it, so a toolbar strip never wins over the window itself; a miss names the windows
/// there are.
fn find_window(name: &str) -> Result<macos::Window> {
    let wanted = name.trim().to_lowercase();
    ensure!(!wanted.is_empty(), "name the app or window to capture");
    let windows = macos::windows();
    let area = |window: &macos::Window| window.bounds.size.width * window.bounds.size.height;
    let largest = |matches: &dyn Fn(&macos::Window) -> bool| {
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

/// `cmd+shift+t`, `enter` or `f5` as a key with its modifiers.
pub fn keys(combo: &str) -> Result<(CGKeyCode, CGEventFlags)> {
    let mut flags = CGEventFlags::CGEventFlagNull;
    let mut key = None;
    for part in combo
        .split('+')
        .map(|part| part.trim().to_ascii_lowercase())
    {
        match part.as_str() {
            "cmd" | "command" | "meta" | "super" => flags |= CGEventFlags::CGEventFlagCommand,
            "shift" => flags |= CGEventFlags::CGEventFlagShift,
            "ctrl" | "control" => flags |= CGEventFlags::CGEventFlagControl,
            "alt" | "option" | "opt" => flags |= CGEventFlags::CGEventFlagAlternate,
            name => {
                ensure!(key.is_none(), "one key at a time, with modifiers: {combo}");
                key = Some(code(name).with_context(|| format!("unknown key {name:?}"))?);
            }
        }
    }
    match key {
        Some(key) => Ok((key, flags)),
        None => bail!("name a key, not only modifiers: {combo}"),
    }
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
        "enter" | "return" => KeyCode::RETURN,
        "tab" => KeyCode::TAB,
        "space" => KeyCode::SPACE,
        "delete" | "backspace" => KeyCode::DELETE,
        "forwarddelete" => KeyCode::FORWARD_DELETE,
        "esc" | "escape" => KeyCode::ESCAPE,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn combos_name_one_key_and_its_modifiers() {
        let (key, flags) = keys("cmd+shift+t").unwrap();
        assert_eq!(key, KeyCode::ANSI_T);
        assert!(flags.contains(CGEventFlags::CGEventFlagCommand));
        assert!(flags.contains(CGEventFlags::CGEventFlagShift));
        assert_eq!(keys("Enter").unwrap().0, KeyCode::RETURN);
        assert_eq!(keys("5").unwrap().0, KeyCode::ANSI_5);
        for wrong in ["cmd", "a+b", "cmd+nope", ""] {
            assert!(keys(wrong).is_err(), "{wrong}");
        }
    }

    #[test]
    fn clicks_map_from_the_picture_to_points_and_stay_inside_it() {
        let screen = Screen::new(ArtifactStore::new(std::env::temp_dir()));
        assert!(screen.point(1.0, 1.0).is_err(), "no screenshot yet");
        *screen.view.lock() = Some(View {
            origin: CGPoint::new(100.0, 50.0),
            scale: 2.0,
            width: 800.0,
            height: 500.0,
        });
        let point = screen.point(10.0, 20.0).unwrap();
        assert_eq!((point.x, point.y), (120.0, 90.0));
        assert!(screen.point(801.0, 0.0).is_err());
        assert!(screen.point(-1.0, 0.0).is_err());
    }
}
