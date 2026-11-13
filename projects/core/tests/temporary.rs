//! A temporary chat against a real core: what is said in it and what is attached to it
//! never reaches the config folder, the database file or the workspace.

use std::path::Path;

use dusk_core::app::{Paths, start_hub};
use dusk_protocol::client::{Hello, PROTOCOL_VERSION};

const MARK: &str = "ZEBRA-QUARTZ-7731";

/// Every file under `root` whose bytes hold the mark.
fn holding(root: &Path, found: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            holding(&path, found);
        } else if let Ok(bytes) = std::fs::read(&path)
            && bytes
                .windows(MARK.len())
                .any(|part| part == MARK.as_bytes())
        {
            found.push(path.display().to_string());
        }
    }
}

#[tokio::test]
async fn a_temporary_chat_leaves_nothing_on_disk() {
    let root = std::env::temp_dir().join(format!("dusk-temporary-{}", std::process::id()));
    let folder = root.join("project");
    std::fs::create_dir_all(&folder).unwrap();
    let paths = Paths::new(root.join("config"), root.join("work"));
    let hub = start_hub(paths, Some(root.join("state.db"))).await.unwrap();
    let client = hub
        .attach(Hello {
            protocol: PROTOCOL_VERSION,
            client: "test".to_owned(),
        })
        .unwrap();
    let threads = client.threads();

    let stored = threads.start(folder.clone()).await.unwrap();
    let temporary = threads.start_temporary(folder.clone()).await.unwrap();
    // A message, and a paste long enough to be kept as a file.
    let paste = format!("{MARK}\n").repeat(400);
    threads
        .send(&temporary, MARK.to_owned(), false)
        .await
        .unwrap();
    threads.send(&temporary, paste, false).await.unwrap();
    threads.rename(&temporary, MARK).await.unwrap();

    let listed = threads.list().await.unwrap();
    let flag = |id: &str| listed.iter().find(|t| t.id == id).map(|t| t.temporary);
    assert_eq!(flag(&temporary), Some(true));
    assert_eq!(flag(&stored), Some(false));
    assert!(
        !threads.messages(&temporary).await.unwrap().is_empty()
            || !threads.local_unsent(&temporary).await.unwrap().is_empty(),
        "the chat itself has what was sent"
    );
    assert!(threads.search(MARK).await.unwrap().is_empty());

    let mut found = Vec::new();
    holding(&root, &mut found);
    assert!(found.is_empty(), "written to disk: {found:?}");
    // One folder in the workspace: the stored chat's.
    assert_eq!(std::fs::read_dir(root.join("work")).unwrap().count(), 1);

    // The same words in a stored chat are found, so the search above could have failed.
    threads.send(&stored, MARK.to_owned(), false).await.unwrap();
    holding(&root, &mut found);
    assert!(!found.is_empty(), "a stored chat is on disk");
}
