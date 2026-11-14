//! A part's view: a tree of the host's own widgets, which every client draws its way.
//! A plugin never draws itself, so it cannot imitate the app or flood the window; the
//! host checks a view's size before it draws it.

use serde::{Deserialize, Serialize};

/// The most widgets a view may hold.
pub const MAX_NODES: usize = 500;
/// The deepest a view may nest.
pub const MAX_DEPTH: usize = 8;
/// The longest text a widget may carry.
pub const MAX_TEXT: usize = 4000;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Tone {
    #[default]
    Normal,
    Muted,
    Good,
    Danger,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Title,
    #[default]
    Body,
    Small,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ListItem {
    pub id: String,
    pub title: String,
    #[serde(default)]
    pub detail: String,
}

/// A widget. Ids name the widget in the events it sends back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Widget {
    Column {
        children: Vec<Widget>,
    },
    Row {
        children: Vec<Widget>,
    },
    Text {
        text: String,
        #[serde(default)]
        tone: Tone,
        #[serde(default)]
        size: Size,
    },
    Button {
        id: String,
        label: String,
        #[serde(default)]
        primary: bool,
        #[serde(default)]
        disabled: bool,
    },
    /// A one-line text field; `Input` comes back when it is submitted.
    Input {
        id: String,
        #[serde(default)]
        placeholder: String,
        #[serde(default)]
        value: String,
    },
    /// A list to choose from; a menu when `menu` is set.
    List {
        id: String,
        items: Vec<ListItem>,
        #[serde(default)]
        selected: Option<String>,
        #[serde(default)]
        menu: bool,
    },
    /// A live picture the plugin sends frames for, such as a device's screen. It takes
    /// taps and swipes in its own `width` by `height` units; `phone` draws a phone's
    /// body around it.
    Screen {
        id: String,
        width: u32,
        height: u32,
        #[serde(default)]
        phone: bool,
    },
    /// Work under way.
    Progress {
        #[serde(default)]
        text: String,
    },
    Divider,
}

/// What a widget sends back; places on a screen are in its own whole units.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum UiEvent {
    Click,
    Input { value: String },
    Select { item: String },
    Tap { x: u32, y: u32 },
    Swipe { from: (u32, u32), to: (u32, u32) },
}

/// Whether a view is small enough to draw: few widgets, shallow, short texts and finite
/// sizes. The reason when it is not.
pub fn check_view(root: &Widget) -> Result<(), String> {
    let mut nodes = 0;
    walk(root, 1, &mut nodes)
}

fn walk(widget: &Widget, depth: usize, nodes: &mut usize) -> Result<(), String> {
    *nodes += 1;
    if *nodes > MAX_NODES {
        return Err(format!("a view holds more than {MAX_NODES} widgets"));
    }
    if depth > MAX_DEPTH {
        return Err(format!("a view nests deeper than {MAX_DEPTH}"));
    }
    let long = |text: &str| text.chars().count() > MAX_TEXT;
    match widget {
        Widget::Column { children } | Widget::Row { children } => {
            for child in children {
                walk(child, depth + 1, nodes)?;
            }
            Ok(())
        }
        Widget::Text { text, .. } | Widget::Progress { text } if long(text) => {
            Err(format!("a text is longer than {MAX_TEXT} characters"))
        }
        Widget::Button { label, .. } if long(label) => Err("a label is too long".into()),
        Widget::List { items, .. } if items.len() > MAX_NODES => {
            Err(format!("a list holds more than {MAX_NODES} items"))
        }
        Widget::Screen { width, height, .. }
            if !(1..=10_000).contains(width) || !(1..=10_000).contains(height) =>
        {
            Err("a screen has no sensible size".into())
        }
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_view_too_large_or_deep_is_refused() {
        let text = |text: &str| Widget::Text {
            text: text.into(),
            tone: Tone::Normal,
            size: Size::Body,
        };
        assert!(check_view(&text("hello")).is_ok());
        assert!(check_view(&text(&"x".repeat(MAX_TEXT + 1))).is_err());
        let wide = Widget::Column {
            children: (0..MAX_NODES).map(|_| text("a")).collect(),
        };
        assert!(check_view(&wide).is_err());
        let mut deep = text("leaf");
        for _ in 0..MAX_DEPTH {
            deep = Widget::Column {
                children: vec![deep],
            };
        }
        assert!(check_view(&deep).is_err());
        let screen = Widget::Screen {
            id: "s".into(),
            width: 0,
            height: 10,
            phone: false,
        };
        assert!(check_view(&screen).is_err());
    }

    #[test]
    fn a_view_reads_from_the_json_a_plugin_writes() {
        let json = r#"{"kind":"column","children":[
            {"kind":"text","text":"Boot a device","tone":"muted"},
            {"kind":"button","id":"boot","label":"Boot","primary":true},
            {"kind":"screen","id":"screen","width":402,"height":874,"phone":true}]}"#;
        let view: Widget = serde_json::from_str(json).unwrap();
        assert!(check_view(&view).is_ok());
        assert!(serde_json::from_str::<Widget>(r#"{"kind":"html","text":"<b>"}"#).is_err());
    }
}
