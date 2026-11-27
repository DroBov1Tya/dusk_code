//! Made-up sources of the shown chat, and each kind of them opened to read, for the
//! screenshot script: `fake=sources` lists them in the side panel, `modal=source-KIND`
//! opens one (`md`, `code`, `long`, `picture`, `binary`) without asking the core.

use std::sync::Arc;

use dusk_protocol::thread::Source;
use gpui_kit::*;

use crate::{
    modal::{
        Modal,
        source::{Reading, SourceView},
    },
    workspace::Workspace,
};

fn source(n: usize, name: &str, bytes: u64, pinned: bool, delivered: bool) -> Source {
    Source {
        source_id: format!("fake-source-{n}"),
        name: name.to_owned(),
        file: format!("{n:06}-{name}"),
        bytes,
        pinned,
        delivered,
        plan: false,
    }
}

fn sources() -> Vec<Source> {
    vec![
        source(1, "brief.md", 2_310, true, true),
        source(2, "cache.rs", 1_204, false, true),
        source(3, "pasted-3.txt", 412_000, false, false),
        source(4, "screen.png", 48_200, false, true),
        source(5, "contract.pdf", 1_830_000, false, true),
    ]
}

pub fn fake_sources(ws: &mut Workspace, cx: &mut Context<Workspace>) {
    ws.sources = sources();
    cx.notify();
}

const BRIEF: &str = "# Cache rewrite\n\nThe cache keeps **expired entries** until they are read. \
That makes `get()` slow on a cold start.\n\n## What to do\n\n- Remove an entry when it is read past its time\n\
- Keep the size bound at 10 000 entries\n- Add a test for the eviction order\n\n\
| Case | Now | After |\n|---|---|---|\n| cold start | 340 ms | 40 ms |\n| warm | 2 ms | 2 ms |\n\n\
> Keep the public API as it is; the callers are in three crates.\n";

const CODE: &str = "use std::collections::HashMap;\nuse std::time::{Duration, Instant};\n\n\
/// Entries past their time are removed when read.\npub struct Cache<V> {\n    entries: HashMap<String, (Instant, V)>,\n    ttl: Duration,\n}\n\n\
impl<V: Clone> Cache<V> {\n    pub fn get(&mut self, key: &str) -> Option<V> {\n        let (at, value) = self.entries.get(key)?;\n        if at.elapsed() > self.ttl {\n            self.entries.remove(key);\n            return None;\n        }\n        Some(value.clone())\n    }\n}\n";

pub fn open(ws: &mut Workspace, kind: &str, cx: &mut Context<Workspace>) {
    let all = sources();
    let (source, reading) = match kind {
        "md" => (
            all[0].clone(),
            Reading::Text {
                text: BRIEF.to_owned(),
                next: None,
                local: None,
                more: false,
            },
        ),
        "code" => (
            all[1].clone(),
            Reading::Text {
                text: CODE.to_owned(),
                next: None,
                local: None,
                more: false,
            },
        ),
        "long" => {
            let text = (1..=1200)
                .map(|line| {
                    format!(
                        "{line:>5}  2026-11-27 10:{:02}:{:02} INFO request served in {} ms\n",
                        line / 60 % 60,
                        line % 60,
                        line % 37 + 3
                    )
                })
                .collect::<String>();
            let next = Some(text.len() as u64);
            (
                all[2].clone(),
                Reading::Text {
                    text,
                    next,
                    local: None,
                    more: false,
                },
            )
        }
        "picture" => (
            all[3].clone(),
            Reading::Picture {
                image: Some(Arc::new(Image::from_bytes(
                    ImageFormat::Png,
                    super::screenshot::demo_png(),
                ))),
                local: None,
            },
        ),
        _ => (all[4].clone(), Reading::Binary { local: None }),
    };
    ws.modal = Some(Modal::Source(SourceView {
        thread_id: ws.shown.clone().unwrap_or_default(),
        source,
        reading,
    }));
    cx.notify();
}
