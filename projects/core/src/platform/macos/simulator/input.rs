//! Touches, typing and buttons, through AXe; and what is on the screen as the
//! accessibility tree, which tells the agent what it can tap without a picture.

use anyhow::{Result, ensure};
use dusk_protocol::{client::MAX_SIM_TEXT, system::SimButton};

use super::run::axe;

/// No device's screen is this many points across.
const MAX_POINT: u32 = 4096;
/// The tree of a busy screen, cut so it does not flood the model.
const MAX_TREE: usize = 24 * 1024;

pub async fn tap(udid: &str, x: u32, y: u32) -> Result<()> {
    let (x, y) = (point(x)?, point(y)?);
    axe(&["tap", "-x", &x, "-y", &y, "--udid", udid]).await?;
    Ok(())
}

pub async fn swipe(udid: &str, from: (u32, u32), to: (u32, u32)) -> Result<()> {
    let (start_x, start_y) = (point(from.0)?, point(from.1)?);
    let (end_x, end_y) = (point(to.0)?, point(to.1)?);
    let args = [
        "swipe",
        "--start-x",
        &start_x,
        "--start-y",
        &start_y,
        "--end-x",
        &end_x,
        "--end-y",
        &end_y,
        "--udid",
        udid,
    ];
    axe(&args).await?;
    Ok(())
}

pub async fn type_text(udid: &str, text: &str) -> Result<()> {
    ensure!(!text.is_empty(), "nothing to type");
    ensure!(
        text.chars().count() <= MAX_SIM_TEXT,
        "at most {MAX_SIM_TEXT} characters at once"
    );
    // After `--`, text that starts with a dash is typed rather than read as a flag.
    axe(&["type", "--udid", udid, "--", text]).await?;
    Ok(())
}

pub async fn button(udid: &str, button: SimButton) -> Result<()> {
    axe(&["button", button.name(), "--udid", udid]).await?;
    Ok(())
}

/// The screen's elements, with their labels and frames in points.
pub async fn tree(udid: &str) -> Result<String> {
    let tree = axe(&["describe-ui", "--udid", udid]).await?;
    let mut tree = String::from_utf8_lossy(&tree).into_owned();
    if tree.len() > MAX_TREE {
        let cut = (0..=MAX_TREE)
            .rev()
            .find(|&at| tree.is_char_boundary(at))
            .unwrap_or(0);
        tree.truncate(cut);
        tree.push_str("\n[cut: the tree goes on]");
    }
    Ok(tree)
}

fn point(value: u32) -> Result<String> {
    ensure!(value <= MAX_POINT, "a point off the screen");
    Ok(value.to_string())
}
