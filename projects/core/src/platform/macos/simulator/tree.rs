//! The screen's accessibility tree, as AXe prints it, made short for the model: one
//! line for each element that has a label or a value and is on the screen, with the
//! point at its middle to tap. A home screen goes from about 400 KB of JSON to two.

use anyhow::{Context, Result};
use serde::Deserialize;

/// Past this many lines a screen is cut; the rest is said to go on.
const MAX_LINES: usize = 300;
/// A label or value longer than this is cut.
const MAX_TEXT: usize = 120;

#[derive(Deserialize)]
struct Element {
    #[serde(rename = "type", default)]
    kind: Option<String>,
    #[serde(rename = "AXLabel", default)]
    label: Option<String>,
    #[serde(rename = "AXValue", default)]
    value: Option<String>,
    frame: Option<Frame>,
    #[serde(default)]
    children: Vec<Element>,
}

#[derive(Deserialize)]
struct Frame {
    x: f64,
    y: f64,
    width: f64,
    height: f64,
}

pub fn compact(json: &[u8]) -> Result<String> {
    let roots: Vec<Element> = serde_json::from_slice(json).context("AXe gave no tree")?;
    let mut lines = Vec::new();
    let mut stack: Vec<&Element> = roots.iter().rev().collect();
    while let Some(element) = stack.pop() {
        if let Some(line) = line(element) {
            if lines.len() == MAX_LINES {
                lines.push("[cut: the screen has more]".to_owned());
                break;
            }
            lines.push(line);
        }
        stack.extend(element.children.iter().rev());
    }
    if lines.is_empty() {
        return Ok("nothing with a label is on the screen".into());
    }
    Ok(lines.join("\n"))
}

fn line(element: &Element) -> Option<String> {
    let frame = element.frame.as_ref()?;
    if frame.width <= 0. || frame.height <= 0. {
        return None;
    }
    let label = text(element.label.as_deref());
    let value = text(element.value.as_deref());
    if label.is_empty() && value.is_empty() {
        return None;
    }
    let kind = element.kind.as_deref().unwrap_or("Element");
    let (x, y) = (frame.x + frame.width / 2., frame.y + frame.height / 2.);
    let value = match value.is_empty() {
        true => String::new(),
        false => format!(" = {value:?}"),
    };
    Some(format!("{kind} {label:?}{value} at {x:.0},{y:.0}"))
}

fn text(raw: Option<&str>) -> String {
    let text = raw.unwrap_or_default().trim();
    text.chars()
        .filter(|c| !c.is_control())
        .take(MAX_TEXT)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labelled_elements_on_the_screen_become_lines_to_tap() {
        let json = br#"[{
          "type": "Application", "AXLabel": " ", "frame": {"x": 0, "y": 0, "width": 402, "height": 874},
          "children": [
            {"type": "Button", "AXLabel": "Safari", "frame": {"x": 120, "y": 770, "width": 74, "height": 70}},
            {"type": "Button", "AXLabel": "Hidden", "frame": {"x": 0, "y": 0, "width": 0, "height": 0}},
            {"type": "Group", "frame": {"x": 0, "y": 0, "width": 10, "height": 10},
             "children": [{"type": "StaticText", "AXLabel": "a\u0007b", "AXValue": "On", "frame": {"x": 10, "y": 20, "width": 20, "height": 10}}]}
          ]}]"#;
        let tree = compact(json).unwrap();
        assert_eq!(
            tree,
            "Button \"Safari\" at 157,805\nStaticText \"ab\" = \"On\" at 20,25"
        );
    }

    #[test]
    fn what_is_not_a_tree_is_refused() {
        assert!(compact(b"not json").is_err());
        assert!(compact(br#"{"type": "Button"}"#).is_err());
        assert_eq!(
            compact(b"[]").unwrap(),
            "nothing with a label is on the screen"
        );
    }
}
