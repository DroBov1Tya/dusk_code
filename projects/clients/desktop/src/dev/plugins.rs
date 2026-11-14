//! Plugin parts and views made up for the screenshot tool, so the docks, the menu and a
//! view can be photographed without a real plugin: a few parts that show every widget,
//! and many more that prove the menu stays usable.

use dusk_protocol::plugin::{ListItem, Opens, Part, Place, Size, Tone, Widget};
use gpui_kit::*;

use crate::{plugins::Key, workspace::Workspace};

const PHONE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><rect width="14" height="20" x="5" y="2" rx="2" ry="2" /><path d="M12 18h.01" /></svg>"#;
const NOTE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M15 2H6a2 2 0 0 0-2 2v16a2 2 0 0 0 2 2h12a2 2 0 0 0 2-2V7Z" /><path d="M14 2v4a2 2 0 0 0 2 2h4" /><path d="M10 9H8" /><path d="M16 13H8" /><path d="M16 17H8" /></svg>"#;
const GLOBE_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><circle cx="12" cy="12" r="10" /><path d="M12 2a14.5 14.5 0 0 0 0 20 14.5 14.5 0 0 0 0-20" /><path d="M2 12h20" /></svg>"#;
const CHART_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M3 3v16a2 2 0 0 0 2 2h16" /><path d="M18 17V9" /><path d="M13 17V5" /><path d="M8 17v-3" /></svg>"#;

/// The parts: a simulator, notes, a translator, a chat bridge's settings, then sixty
/// plugins of three or four parts each.
pub fn parts() -> Vec<Part> {
    let part = |plugin: &str, id: &str, title: &str, icon: Option<&str>, place, opens| Part {
        plugin: plugin.into(),
        id: id.into(),
        title: title.into(),
        icon: icon.map(str::to_owned),
        place,
        opens,
        platforms: Vec::new(),
    };
    let mut parts = vec![
        part(
            "ios-simulator",
            "device",
            "iOS Simulator",
            Some(PHONE_SVG),
            Place::Header,
            Opens::RightPanel,
        ),
        part(
            "notes",
            "scratch",
            "Scratch notes",
            Some(NOTE_SVG),
            Place::Rail,
            Opens::BottomPanel,
        ),
        part(
            "translator",
            "translate",
            "Translate the message",
            Some(GLOBE_SVG),
            Place::Composer,
            Opens::Window,
        ),
        part(
            "telegram",
            "account",
            "Telegram",
            None,
            Place::Settings,
            Opens::RightPanel,
        ),
    ];
    let places = [Place::Header, Place::Rail, Place::Composer];
    let opens = [Opens::RightPanel, Opens::BottomPanel, Opens::Window];
    let words = [
        "Metrics", "Logs", "Traces", "Queue", "Deploy", "Preview", "Tickets", "Wiki",
    ];
    for plugin in 0..60 {
        let name = format!("plugin-{plugin:02}");
        for index in 0..(3 + plugin % 2) {
            let word = words[(plugin + index) % words.len()];
            parts.push(part(
                &name,
                &format!("part-{index}"),
                &format!("{word} {plugin}"),
                (index == 0).then_some(CHART_SVG),
                places[(plugin + index) % places.len()],
                opens[index % opens.len()],
            ));
        }
    }
    parts
}

/// A view for a part: the simulator's phone, or one of every other widget.
pub fn view(key: &Key) -> Widget {
    let text = |text: &str, tone, size| Widget::Text {
        text: text.into(),
        tone,
        size,
    };
    let button = |id: &str, label: &str, primary| Widget::Button {
        id: id.into(),
        label: label.into(),
        primary,
        disabled: false,
    };
    let item = |id: &str, title: &str, detail: &str| ListItem {
        id: id.into(),
        title: title.into(),
        detail: detail.into(),
    };
    if key.plugin == "ios-simulator" {
        return Widget::Column {
            children: vec![
                Widget::Row {
                    children: vec![Widget::List {
                        id: "device".into(),
                        items: vec![
                            item("17pro", "iPhone 17 Pro", "iOS 26.1"),
                            item("air", "iPhone Air", "iOS 26.1"),
                        ],
                        selected: Some("17pro".into()),
                        menu: true,
                    }],
                },
                Widget::Screen {
                    id: "screen".into(),
                    width: 402,
                    height: 874,
                    phone: true,
                },
                Widget::Row {
                    children: vec![button("home", "Home", false), button("lock", "Lock", false)],
                },
            ],
        };
    }
    Widget::Column {
        children: vec![
            text("Scratch notes", Tone::Normal, Size::Title),
            text(
                "Notes kept beside the work, by the notes plugin.",
                Tone::Muted,
                Size::Small,
            ),
            Widget::Input {
                id: "note".into(),
                placeholder: "Write a note, Enter keeps it".into(),
                value: String::new(),
            },
            Widget::Row {
                children: vec![
                    button("keep", "Keep", true),
                    button("clear", "Clear", false),
                ],
            },
            Widget::Divider,
            Widget::List {
                id: "notes".into(),
                items: vec![
                    item("1", "Ask about the cache expiry", "today"),
                    item("2", "Rename the queue module", "yesterday"),
                    item("3", "Check the release notes", "Monday"),
                ],
                selected: Some("2".into()),
                menu: false,
            },
            Widget::Progress {
                text: "Syncing".into(),
            },
            text("Saved", Tone::Good, Size::Small),
        ],
    }
}

/// A picture for the simulator's screen: a dusk sky from violet to orange.
pub fn frame() -> Vec<u8> {
    let (width, height) = (402, 874);
    let picture = image::RgbImage::from_fn(width, height, |_, y| {
        let t = y as f32 / height as f32;
        let mix = |a: f32, b: f32| (a + (b - a) * t) as u8;
        image::Rgb([mix(70., 240.), mix(40., 140.), mix(140., 80.)])
    });
    let mut jpeg = Vec::new();
    let encoded = image::codecs::jpeg::JpegEncoder::new(&mut jpeg).encode_image(&picture);
    if let Err(error) = encoded {
        eprintln!("cannot make a frame: {error}");
    }
    jpeg
}

/// Opens a part as a click would, without the core, and gives it its made-up view.
pub fn open(ws: &mut Workspace, value: &str, cx: &mut Context<Workspace>) {
    let Some((plugin, part)) = value.split_once('/') else {
        eprintln!("plugins=open:PLUGIN/PART");
        return;
    };
    let key = Key {
        plugin: plugin.into(),
        part: part.into(),
    };
    ws.show_plugin_part(key.clone(), cx);
    ws.plugin_view(key.plugin.clone(), key.part.clone(), view(&key), cx);
    if key.plugin == "ios-simulator" {
        ws.plugin_frame(key.plugin, key.part, "screen".into(), frame(), cx);
    }
}
