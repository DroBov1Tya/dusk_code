//! File tools: read, write, edit and delete files inside the tool scope.

mod delete;
mod edit;
mod read;
mod write;

pub use delete::DeleteFile;
pub use edit::EditFile;
pub use read::ReadFile;
pub use write::WriteFile;

use std::path::Path;

use anyhow::{Context, Result, anyhow};
use serde::Deserialize;

use super::builtin::{read_regular, write_regular};

/// Larger files would not fit the result limit anyway.
const MAX_READ_BYTES: u64 = 1024 * 1024;

/// For tools that take only a path.
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PathArguments {
    pub path: String,
}

pub async fn read_text(path: &Path) -> Result<String> {
    let path = path.to_owned();
    let bytes = tokio::task::spawn_blocking(move || read_regular(&path, MAX_READ_BYTES))
        .await
        .context("cannot read file")??;
    String::from_utf8(bytes).map_err(|_| anyhow!("file is not valid UTF-8 text"))
}

pub async fn write_text(path: &Path, text: String) -> Result<()> {
    let path = path.to_owned();
    tokio::task::spawn_blocking(move || write_regular(&path, text.as_bytes()))
        .await
        .context("cannot write file")?
}

#[cfg(test)]
mod tests {
    use std::{fs, path::PathBuf};

    use serde_json::json;

    use super::*;
    use crate::tools::Tool;
    use crate::{
        testing::{temp_dir, tool_context},
        tools::ToolScope,
    };

    fn scope(name: &str) -> (ToolScope, PathBuf) {
        let root = temp_dir(name);
        let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
        fs::create_dir_all(&cwd).unwrap();
        fs::create_dir_all(&workspace).unwrap();
        (ToolScope::new(&cwd, &workspace).unwrap(), root)
    }

    #[tokio::test]
    async fn rejects_paths_outside_the_scope() {
        let (scope, root) = scope("files_outside");
        fs::write(root.join("secret"), "x").unwrap();
        let outside = "../secret";
        assert!(
            ReadFile
                .call(json!({ "path": outside }), tool_context(&scope))
                .await
                .is_err()
        );
        let write = json!({ "path": outside, "content": "y" });
        assert!(WriteFile.call(write, tool_context(&scope)).await.is_err());
        let edit = json!({ "path": outside, "old_text": "x", "new_text": "y" });
        assert!(EditFile.call(edit, tool_context(&scope)).await.is_err());
        assert_eq!(fs::read_to_string(root.join("secret")).unwrap(), "x");
    }

    #[tokio::test]
    async fn deletes_files_only() {
        let (scope, _) = scope("files_delete");
        fs::write(scope.cwd().join("a.txt"), "x").unwrap();
        fs::create_dir(scope.cwd().join("dir")).unwrap();
        let delete = |path: &str| DeleteFile.call(json!({ "path": path }), tool_context(&scope));
        delete("a.txt").await.unwrap();
        assert!(!scope.cwd().join("a.txt").exists());
        assert!(delete("dir").await.is_err());
        assert!(delete("../outside").await.is_err());
    }
}
