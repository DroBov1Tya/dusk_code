use std::time::Duration;

use dusk_protocol::{
    client::Event,
    plugin::{Opens, Part, Place, UiEvent, Widget},
};
use tokio::sync::mpsc;

use super::{UiHost, UiSpec};
use crate::plugins::Isolation;

/// A plugin interface in a file: a view and a frame for `main`, a view too large to draw
/// for `bad`, a line past the limit for `long`.
const SCRIPT: &str = r#"#!/bin/sh
while IFS= read -r line; do
  case "$line" in
    *'"type":"open","part":"main"'*)
      printf '%s\n' '{"type":"view","part":"main","root":{"kind":"text","text":"hi"}}'
      printf '%s\n' '{"type":"frame","part":"main","widget":"screen","jpeg":"/9j/2Q=="}' ;;
    *'"type":"open","part":"bad"'*)
      printf '%s\n' '{"type":"view","part":"bad","root":{"kind":"screen","id":"s","width":0,"height":1}}' ;;
    *'"type":"open","part":"long"'*)
      head -c 5000000 /dev/zero | tr '\0' a; echo ;;
  esac
done
"#;

fn part(id: &str) -> Part {
    Part {
        plugin: "p".into(),
        id: id.into(),
        title: id.into(),
        icon: None,
        place: Place::Header,
        opens: Opens::RightPanel,
        platforms: Vec::new(),
    }
}

fn host() -> UiHost {
    let paths = crate::testing::temp_paths("ui-host");
    let folder = paths.rules_root().join("plugins/p");
    let data = paths.config_root().join("plugin-data/p");
    std::fs::create_dir_all(&folder).unwrap();
    std::fs::create_dir_all(&data).unwrap();
    std::fs::write(folder.join("ui.sh"), SCRIPT).unwrap();
    let isolation = Isolation::new(
        &paths,
        &folder,
        &data,
        &Default::default(),
        Vec::new(),
        None,
    )
    .unwrap();
    let host = UiHost::default();
    host.set(vec![UiSpec {
        plugin: "p".into(),
        command: vec!["/bin/sh".into(), folder.join("ui.sh").display().to_string()],
        isolation,
        parts: vec![
            (part("main"), true),
            (part("bad"), true),
            (part("long"), true),
            (part("elsewhere"), false),
        ],
    }]);
    host
}

async fn next(queue: &mut mpsc::UnboundedReceiver<Event>) -> Event {
    tokio::time::timeout(Duration::from_secs(10), queue.recv())
        .await
        .expect("an event in time")
        .expect("the queue is open")
}

#[tokio::test]
async fn views_and_frames_go_to_the_opener_only() {
    let host = host();
    let (one, mut to_one) = mpsc::unbounded_channel();
    let (two, mut to_two) = mpsc::unbounded_channel();
    host.open(1, one, "p", "main").unwrap();
    match next(&mut to_one).await {
        Event::PluginView { part, root, .. } => {
            assert_eq!(part, "main");
            assert!(matches!(root, Widget::Text { .. }));
        }
        other => panic!("{other:?}"),
    }
    match next(&mut to_one).await {
        Event::PluginFrame { widget, jpeg, .. } => {
            assert_eq!(widget, "screen");
            assert_eq!(jpeg, [0xFF, 0xD8, 0xFF, 0xD9]);
        }
        other => panic!("{other:?}"),
    }
    // The second client opened nothing of it, so it may not use it and hears nothing.
    let click = UiEvent::Click;
    assert!(host.event(2, "p", "main", "boot", click.clone()).is_err());
    assert!(host.event(1, "p", "main", "boot", click).is_ok());
    // A view that cannot be drawn is refused, to the client that opened its part.
    host.open(2, two, "p", "bad").unwrap();
    match next(&mut to_two).await {
        Event::PluginNotice { text, .. } => assert!(text.contains("refused"), "{text}"),
        other => panic!("{other:?}"),
    }
    assert!(to_one.try_recv().is_err());
    let (three, _) = mpsc::unbounded_channel();
    assert!(host.open(3, three.clone(), "p", "elsewhere").is_err());
    assert!(host.open(3, three.clone(), "p", "absent").is_err());
    assert!(host.open(3, three, "q", "main").is_err());
}

#[tokio::test]
async fn a_line_past_the_limit_stops_the_interface() {
    let host = host();
    let (one, mut to_one) = mpsc::unbounded_channel();
    host.open(1, one.clone(), "p", "long").unwrap();
    match next(&mut to_one).await {
        Event::PluginNotice { text, .. } => assert!(text.contains("line over"), "{text}"),
        other => panic!("{other:?}"),
    }
    // The next open starts it again.
    host.open(1, one, "p", "main").unwrap();
    assert!(matches!(next(&mut to_one).await, Event::PluginView { .. }));
}
