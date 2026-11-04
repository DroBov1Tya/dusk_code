//! The Linux desktop, driven by the programs it has. On X11 `xdotool` moves the pointer,
//! types and lists windows, and `maim`, `scrot` or ImageMagick's `import` takes the
//! picture. On Wayland `ydotool` types and clicks, and `grim`, `gnome-screenshot` or
//! `spectacle` takes the picture; Wayland shows other apps no windows, so only the
//! display and its regions can be captured there. Display points are its pixels.
//!
//! There is no Accessibility press like on macOS: every click moves the pointer.

use std::{
    io::Write,
    path::Path,
    process::{Command, Stdio},
};

use anyhow::{Context, Result, bail, ensure};

use super::{Keys, Point, Rect, Window, tools::PointerMode};

/// Windows looked at at most, so a crowded desktop stays quick to list.
const MAX_WINDOWS: usize = 64;
/// Smaller ones are tool tips, docks and the like.
const MIN_WINDOW_EDGE: f64 = 50.0;
/// Pixels one notch of the wheel scrolls, about.
const PIXELS_A_NOTCH: i32 = 40;
/// Between clicks of a double click, in ms.
const CLICK_DELAY_MS: &str = "80";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Session {
    X11,
    Wayland,
}

/// Wayland when its display is set, even with X11 there too: XWayland reaches only the
/// apps that run on it.
fn session() -> Session {
    match std::env::var_os("WAYLAND_DISPLAY") {
        Some(display) if !display.is_empty() => Session::Wayland,
        _ => Session::X11,
    }
}

/// The main display. On X11 it comes from the X server; on Wayland from a picture of it.
pub fn display() -> Result<Rect> {
    match session() {
        Session::X11 => {
            let output = run("xdotool", &["getdisplaygeometry"])?;
            let mut numbers = output.split_whitespace().map(str::parse::<f64>);
            match (numbers.next(), numbers.next()) {
                (Some(Ok(width)), Some(Ok(height))) => Ok(Rect {
                    x: 0.0,
                    y: 0.0,
                    width,
                    height,
                }),
                _ => bail!("xdotool gave no display size"),
            }
        }
        Session::Wayland => {
            let file = temp_file();
            let taken = capture_display(&file);
            let size = taken.and_then(|()| {
                image::image_dimensions(&file).context("cannot read the screenshot")
            });
            let _ = std::fs::remove_file(&file);
            let (width, height) = size?;
            Ok(Rect {
                x: 0.0,
                y: 0.0,
                width: f64::from(width),
                height: f64::from(height),
            })
        }
    }
}

/// No cursor of dusk's own is drawn on Linux; the pointer itself moves.
pub fn mark(_at: Point) {}

pub fn touch() {}

/// The windows on screen that are large enough to be an app's.
pub fn windows() -> Result<Vec<Window>> {
    ensure!(
        session() == Session::X11,
        "Wayland does not list other apps' windows; capture the display or a region of it"
    );
    let ids = run("xdotool", &["search", "--onlyvisible", "--name", "."])?;
    let mut windows = Vec::new();
    for id in ids.lines().filter_map(|id| id.trim().parse::<u32>().ok()) {
        if windows.len() == MAX_WINDOWS {
            break;
        }
        let id_text = id.to_string();
        // A window can close while it is looked at; it is then left out.
        let Ok(geometry) = run("xdotool", &["getwindowgeometry", "--shell", &id_text]) else {
            continue;
        };
        let Some(bounds) = parse_geometry(&geometry) else {
            continue;
        };
        if bounds.width < MIN_WINDOW_EDGE || bounds.height < MIN_WINDOW_EDGE {
            continue;
        }
        let title = run("xdotool", &["getwindowname", &id_text]).unwrap_or_default();
        let app = run("xdotool", &["getwindowclassname", &id_text]).unwrap_or_default();
        windows.push(Window {
            id,
            app: app.trim().to_owned(),
            title: title.trim().to_owned(),
            bounds,
        });
    }
    Ok(windows)
}

/// `X=…`, `Y=…`, `WIDTH=…` and `HEIGHT=…` lines, as `getwindowgeometry --shell` prints.
fn parse_geometry(text: &str) -> Option<Rect> {
    let value = |name: &str| {
        text.lines()
            .find_map(|line| line.strip_prefix(name)?.strip_prefix('='))?
            .trim()
            .parse::<f64>()
            .ok()
    };
    Some(Rect {
        x: value("X")?,
        y: value("Y")?,
        width: value("WIDTH")?,
        height: value("HEIGHT")?,
    })
}

/// Only a press without the pointer checks the window under it, and Linux has none.
pub fn ensure_window_at(_at: Point, _id: u32, _bounds: Rect) -> Result<()> {
    bail!("a click without the pointer is not possible on Linux")
}

/// Captures `rect` of the display as a PNG at `file`: the whole display, then cut.
pub fn capture(rect: Rect, file: &Path) -> Result<()> {
    capture_display(file)?;
    let picture = image::open(file).context("cannot read the screenshot")?;
    let x = rect.x.max(0.0) as u32;
    let y = rect.y.max(0.0) as u32;
    ensure!(
        x < picture.width() && y < picture.height(),
        "the region is off the display"
    );
    let width = (rect.width as u32).min(picture.width() - x);
    let height = (rect.height as u32).min(picture.height() - y);
    picture
        .crop_imm(x, y, width, height)
        .save_with_format(file, image::ImageFormat::Png)
        .context("cannot save the screenshot")
}

/// A window as it shows on the display, covered parts included.
pub fn capture_window(window: &Window, file: &Path) -> Result<()> {
    capture(window.bounds, file)
}

fn capture_display(file: &Path) -> Result<()> {
    let path = file.to_str().context("the screenshot path is not UTF-8")?;
    let tools: &[(&str, &[&str])] = match session() {
        Session::X11 => &[
            ("maim", &[]),
            ("scrot", &[]),
            ("import", &["-window", "root"]),
        ],
        Session::Wayland => &[
            ("grim", &[]),
            ("gnome-screenshot", &["-f"]),
            ("spectacle", &["-b", "-n", "-f", "-o"]),
        ],
    };
    let Some((program, arguments)) = tools.iter().find(|(program, _)| installed(program)) else {
        let names: Vec<&str> = tools.iter().map(|(program, _)| *program).collect();
        bail!(
            "taking a screenshot needs one of {} installed",
            names.join(", ")
        );
    };
    let mut all = arguments.to_vec();
    all.push(path);
    run(program, &all)?;
    ensure!(file.exists(), "{program} saved no screenshot");
    Ok(())
}

pub fn click(at: Point, right: bool, count: i64, mode: PointerMode) -> Result<()> {
    mode.validate_click(right, count)?;
    ensure!(
        mode == PointerMode::System,
        "on Linux a click always moves the pointer: use mode system"
    );
    let count = count.to_string();
    match session() {
        Session::X11 => {
            let (x, y) = pixels(at);
            let button = if right { "3" } else { "1" };
            run(
                "xdotool",
                &[
                    "mousemove",
                    &x,
                    &y,
                    "click",
                    "--repeat",
                    &count,
                    "--delay",
                    CLICK_DELAY_MS,
                    button,
                ],
            )?;
        }
        Session::Wayland => {
            move_to(at)?;
            // Down and up of the left or right button.
            let button = if right { "0xC1" } else { "0xC0" };
            run(
                "ydotool",
                &[
                    "click",
                    "--repeat",
                    &count,
                    "--next-delay",
                    CLICK_DELAY_MS,
                    button,
                ],
            )?;
        }
    }
    Ok(())
}

pub fn type_text(text: &str) -> Result<()> {
    match session() {
        Session::X11 => run("xdotool", &["type", "--delay", "12", "--", text]).map(drop),
        // From stdin, so text that starts with a dash is not read as an option.
        Session::Wayland => run_with_input("ydotool", &["type", "--file", "-"], text),
    }
}

pub fn press(keys: &Keys) -> Result<()> {
    match session() {
        Session::X11 => {
            let mut combo: Vec<&str> = [
                (keys.ctrl, "ctrl"),
                (keys.shift, "shift"),
                (keys.alt, "alt"),
                (keys.cmd, "super"),
            ]
            .into_iter()
            .filter_map(|(on, name)| on.then_some(name))
            .collect();
            let key = keysym(&keys.key).context("unknown key")?;
            combo.push(&key);
            run("xdotool", &["key", "--", &combo.join("+")]).map(drop)
        }
        Session::Wayland => {
            let mut held: Vec<u16> = [
                (keys.ctrl, 29),
                (keys.shift, 42),
                (keys.alt, 56),
                (keys.cmd, 125),
            ]
            .into_iter()
            .filter_map(|(on, code)| on.then_some(code))
            .collect();
            held.push(evdev(&keys.key).context("unknown key")?);
            // Down in order, then up the other way round.
            let events: Vec<String> = held
                .iter()
                .map(|code| format!("{code}:1"))
                .chain(held.iter().rev().map(|code| format!("{code}:0")))
                .collect();
            let mut arguments = vec!["key"];
            arguments.extend(events.iter().map(String::as_str));
            run("ydotool", &arguments).map(drop)
        }
    }
}

/// `down` and `right` in pixels; positive scrolls the content down and right. The
/// wheel goes by notches, so small amounts make at least one.
pub fn scroll(at: Point, down: i32, right: i32) -> Result<()> {
    let notches = |pixels: i32| match pixels {
        0 => 0,
        _ => (pixels.abs() / PIXELS_A_NOTCH).max(1),
    };
    match session() {
        Session::X11 => {
            let (x, y) = pixels(at);
            run("xdotool", &["mousemove", &x, &y])?;
            // Buttons 4 and 5 are the wheel up and down, 6 and 7 left and right.
            for (pixels, forward, back) in [(down, "5", "4"), (right, "7", "6")] {
                let count = notches(pixels);
                if count > 0 {
                    let button = if pixels > 0 { forward } else { back };
                    let repeat = count.to_string();
                    run("xdotool", &["click", "--repeat", &repeat, button])?;
                }
            }
        }
        Session::Wayland => {
            move_to(at)?;
            // The wheel turns up for positive steps, as REL_WHEEL counts.
            let wheel = |pixels: i32| (-pixels.signum() * notches(pixels)).to_string();
            let (x, y) = (wheel(-right), wheel(down));
            run("ydotool", &["mousemove", "--wheel", "-x", &x, "-y", &y])?;
        }
    }
    Ok(())
}

fn move_to(at: Point) -> Result<()> {
    let (x, y) = pixels(at);
    run("ydotool", &["mousemove", "--absolute", "-x", &x, "-y", &y]).map(drop)
}

fn pixels(at: Point) -> (String, String) {
    (
        (at.x.round() as i64).to_string(),
        (at.y.round() as i64).to_string(),
    )
}

/// The X keysym of a key name the parser lets through.
fn keysym(name: &str) -> Option<String> {
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next())
        && c.is_ascii_alphanumeric()
    {
        return Some(name.to_owned());
    }
    let named = match name {
        "enter" => "Return",
        "tab" => "Tab",
        "space" => "space",
        "backspace" => "BackSpace",
        "delete" | "forwarddelete" => "Delete",
        "esc" => "Escape",
        "left" => "Left",
        "right" => "Right",
        "up" => "Up",
        "down" => "Down",
        "home" => "Home",
        "end" => "End",
        "pageup" => "Prior",
        "pagedown" => "Next",
        _ => {
            let number: u8 = name.strip_prefix('f')?.parse().ok()?;
            return (1..=12).contains(&number).then(|| format!("F{number}"));
        }
    };
    Some(named.to_owned())
}

/// The Linux input code of a key name the parser lets through, for ydotool.
fn evdev(name: &str) -> Option<u16> {
    const LETTERS: [u16; 26] = [
        30, 48, 46, 32, 18, 33, 34, 35, 23, 36, 37, 38, 50, 49, 24, 25, 16, 19, 31, 20, 22, 47, 17,
        45, 21, 44,
    ];
    let mut chars = name.chars();
    if let (Some(c), None) = (chars.next(), chars.next()) {
        if c.is_ascii_lowercase() {
            return Some(LETTERS[(c as u8 - b'a') as usize]);
        }
        if c == '0' {
            return Some(11);
        }
        if c.is_ascii_digit() {
            return Some(u16::from(c as u8 - b'1') + 2);
        }
    }
    Some(match name {
        "enter" => 28,
        "tab" => 15,
        "space" => 57,
        "backspace" => 14,
        "delete" | "forwarddelete" => 111,
        "esc" => 1,
        "left" => 105,
        "right" => 106,
        "up" => 103,
        "down" => 108,
        "home" => 102,
        "end" => 107,
        "pageup" => 104,
        "pagedown" => 109,
        "f11" => 87,
        "f12" => 88,
        _ => {
            let number: u16 = name.strip_prefix('f')?.parse().ok()?;
            return (1..=10).contains(&number).then_some(58 + number);
        }
    })
}

#[cfg(test)]
pub fn knows_key(name: &str) -> bool {
    keysym(name).is_some() && evdev(name).is_some()
}

fn temp_file() -> std::path::PathBuf {
    std::env::temp_dir().join(format!("duskcode-{}.png", uuid::Uuid::new_v4()))
}

/// Whether `program` is on the PATH.
fn installed(program: &str) -> bool {
    std::env::var_os("PATH").is_some_and(|path| {
        std::env::split_paths(&path).any(|folder| folder.join(program).is_file())
    })
}

/// Runs `program` and returns what it printed; a program that is not there or fails
/// says so, with the start of what it wrote to stderr.
fn run(program: &str, arguments: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(arguments)
        .stdin(Stdio::null())
        .output()
        .map_err(|error| missing(program, error))?;
    checked(program, output)
}

fn run_with_input(program: &str, arguments: &[&str], input: &str) -> Result<()> {
    let mut child = Command::new(program)
        .args(arguments)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|error| missing(program, error))?;
    child
        .stdin
        .take()
        .context("no stdin")?
        .write_all(input.as_bytes())
        .with_context(|| format!("cannot write to {program}"))?;
    checked(program, child.wait_with_output()?).map(drop)
}

fn missing(program: &str, error: std::io::Error) -> anyhow::Error {
    if error.kind() != std::io::ErrorKind::NotFound {
        anyhow::anyhow!("cannot run {program}: {error}")
    } else if program == "ydotool" {
        anyhow::anyhow!("computer use on Wayland needs ydotool installed, with ydotoold running")
    } else {
        anyhow::anyhow!("computer use here needs {program} installed")
    }
}

fn checked(program: &str, output: std::process::Output) -> Result<String> {
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        let reason = stderr.lines().next().unwrap_or("no reason given");
        bail!("{program} failed: {}", reason.trim());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn window_geometry_reads_as_xdotool_prints_it() {
        let text = "WINDOW=4194307\nX=10\nY=-20\nWIDTH=800\nHEIGHT=600\nSCREEN=0\n";
        let bounds = parse_geometry(text).unwrap();
        assert_eq!(
            (bounds.x, bounds.y, bounds.width, bounds.height),
            (10.0, -20.0, 800.0, 600.0)
        );
        assert!(parse_geometry("X=1\nY=2\n").is_none());
    }
}
