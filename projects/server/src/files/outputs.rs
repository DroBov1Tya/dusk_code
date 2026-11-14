//! The pictures among the files agents made, for the artifacts gallery. The rule is the
//! one the core applies to previews: only files of the workspace, the artifact folder or a
//! saved project; and only pictures leave, so a text file is never served as data.

use super::cards::sniff;
use dusk_core::client::Hub;
use dusk_protocol::client::{Hello, PROTOCOL_VERSION};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

const MAX_BYTES: u64 = 16 * 1024 * 1024;

/// The picture at `path`, if it is one inside an allowed folder.
pub async fn read_picture(hub: &Arc<Hub>, path: &str) -> Option<(Vec<u8>, &'static str)> {
    let path = tokio::fs::canonicalize(path).await.ok()?;
    if !allowed(hub, &path).await {
        return None;
    }
    let meta = tokio::fs::metadata(&path).await.ok()?;
    if !meta.is_file() || meta.len() > MAX_BYTES {
        return None;
    }
    let bytes = tokio::fs::read(&path).await.ok()?;
    let kind = sniff(&bytes);
    kind.starts_with("image/").then_some((bytes, kind))
}

async fn allowed(hub: &Arc<Hub>, path: &Path) -> bool {
    let status = hub.status();
    let mut roots: Vec<PathBuf> = vec![status.workspace_dir, status.artifacts_dir];
    let hello = Hello {
        protocol: PROTOCOL_VERSION,
        client: "web".to_owned(),
    };
    if let Ok(client) = hub.attach(hello)
        && let Ok(projects) = client.projects().list().await
    {
        roots.extend(projects.into_iter().map(|project| project.dir));
    }
    for root in roots {
        if let Ok(root) = tokio::fs::canonicalize(root).await
            && path.starts_with(root)
        {
            return true;
        }
    }
    false
}
