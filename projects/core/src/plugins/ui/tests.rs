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
    *'"type":"open","part":"phone"'*)
      printf '%s\n' '{"type":"notice","text":"opened"}' ;;
    *'"type":"event","part":"phone","widget":"refresh"'*)
      printf '%s\n' '{"type":"notice","text":"duplicate refresh"}' ;;
    *'"type":"event","part":"phone","widget":"probe"'*)
      printf '%s\n' '{"type":"notice","text":"probe reached"}' ;;
    *'"type":"open","part":"main"'*)
      printf '%s\n' '{"type":"view","part":"main","root":{"kind":"text","text":"hi"}}'
      printf '%s\n' '{"type":"frame","part":"main","widget":"screen","jpeg":"/9j/2Q=="}' ;;
    *'"type":"event","part":"main","widget":"refresh"'*)
      printf '%s\n' '{"type":"notice","text":"refreshed"}' ;;
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
        clients: Vec::new(),
    }
}

fn spec(plugin: &str, mut parts: Vec<(Part, bool)>, marker: &str) -> UiSpec {
    let paths = crate::testing::temp_paths("ui-host");
    let folder = paths.rules_root().join("plugins").join(plugin);
    let data = paths.config_root().join("plugin-data").join(plugin);
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
    for (part, _) in &mut parts {
        part.plugin = plugin.into();
    }
    UiSpec {
        plugin: plugin.into(),
        command: vec![
            "/bin/sh".into(),
            folder.join("ui.sh").display().to_string(),
            marker.into(),
        ],
        isolation,
        parts,
    }
}

fn host() -> UiHost {
    let host = UiHost::default();
    host.set(vec![spec(
        "p",
        vec![
            (part("main"), true),
            (part("bad"), true),
            (part("long"), true),
            (part("elsewhere"), false),
        ],
        "fixture",
    )]);
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

#[tokio::test]
async fn redetect_opens_once_or_refreshes_the_running_part() {
    let host = host();
    let (sender, mut queue) = mpsc::unbounded_channel();
    host.redetect(1, sender.clone(), "p", "main", None).unwrap();
    assert!(matches!(next(&mut queue).await, Event::PluginView { .. }));
    assert!(matches!(next(&mut queue).await, Event::PluginFrame { .. }));
    host.redetect(1, sender, "p", "main", host.generation("p"))
        .unwrap();
    assert!(
        matches!(next(&mut queue).await, Event::PluginNotice { text, .. } if text == "refreshed")
    );
}

#[tokio::test]
async fn redetect_preserves_both_phone_owners_when_the_sandbox_restarts() {
    let host = UiHost::default();
    let phone = |marker| spec("ios-simulator", vec![(part("phone"), true)], marker);
    host.set(vec![phone("before")]);
    let (one, mut to_one) = mpsc::unbounded_channel();
    let (two, mut to_two) = mpsc::unbounded_channel();
    host.open(1, one.clone(), "ios-simulator", "phone").unwrap();
    assert!(
        matches!(next(&mut to_one).await, Event::PluginNotice { text, .. } if text == "opened")
    );
    host.open(2, two, "ios-simulator", "phone").unwrap();
    assert!(
        matches!(next(&mut to_two).await, Event::PluginNotice { text, .. } if text == "opened")
    );
    assert!(
        matches!(next(&mut to_one).await, Event::PluginNotice { text, .. } if text == "opened")
    );

    let previous = host.generation("ios-simulator");
    host.set(vec![phone("after")]);
    host.redetect(1, one, "ios-simulator", "phone", previous)
        .unwrap();
    host.event(2, "ios-simulator", "phone", "probe", UiEvent::Click)
        .unwrap();
    for queue in [&mut to_one, &mut to_two] {
        assert!(matches!(next(queue).await, Event::PluginNotice { text, .. } if text == "opened"));
        assert!(
            matches!(next(queue).await, Event::PluginNotice { text, .. } if text == "probe reached")
        );
    }
}
