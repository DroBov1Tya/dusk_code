//! Reading text for the file tools.

use std::path::Path;

use anyhow::{Context, Result, anyhow};

use crate::init::files::read_regular;

/// Larger files would not fit the result limit anyway.
const MAX_READ_BYTES: u64 = 1024 * 1024;

pub async fn read_text(path: &Path) -> Result<String> {
    let path = path.to_owned();
    let bytes = tokio::task::spawn_blocking(move || read_regular(&path, MAX_READ_BYTES))
        .await
        .context("cannot read file")??;
    String::from_utf8(bytes).map_err(|_| anyhow!("file is not valid UTF-8 text"))
}

#[cfg(test)]
mod tests {
    use std::fs;

    use serde_json::json;

    use crate::{
        testing::{temp_dir, tool_context},
        tools::{Tool, ToolScope, files::ReadFile},
    };

    #[tokio::test]
    async fn rejects_paths_outside_the_scope() {
        let root = temp_dir("files_outside");
        let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        let scope = ToolScope::new(&cwd, &workspace).unwrap();
        fs::write(root.join("secret"), "x").unwrap();
        let read = json!({ "path": "../secret" });
        assert!(ReadFile.call(read, tool_context(&scope)).await.is_err());
    }
}
