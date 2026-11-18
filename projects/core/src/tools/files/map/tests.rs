use std::fs;

use serde_json::{Value, json};

use super::{
    ProjectMap,
    text::{MAX_BYTES, MAX_LINES},
};
use crate::{
    testing::{temp_dir, tool_context},
    tools::{ToolScope, tool::Tool},
};

async fn run(scope: &ToolScope, arguments: Value) -> anyhow::Result<String> {
    ProjectMap.call(arguments, tool_context(scope)).await
}

fn scope(name: &str) -> ToolScope {
    let root = temp_dir(name);
    let (cwd, workspace) = (root.join("cwd"), root.join("workspace"));
    fs::create_dir_all(cwd.join("src/tools")).unwrap();
    fs::create_dir_all(cwd.join("target")).unwrap();
    fs::create_dir_all(&workspace).unwrap();
    fs::write(cwd.join(".gitignore"), "target/\n").unwrap();
    fs::write(cwd.join("target/out.rlib"), "x").unwrap();
    fs::write(cwd.join("src/main.rs"), "fn main() {}").unwrap();
    fs::write(cwd.join("src/tools/read_file.rs"), "x").unwrap();
    fs::write(cwd.join("README.md"), "x").unwrap();
    ToolScope::new(&cwd, &workspace).unwrap()
}

#[tokio::test]
async fn maps_by_name_and_finds_by_part_or_glob() {
    let scope = scope("map_find");
    let map = run(&scope, json!({})).await.unwrap();
    assert_eq!(
        map,
        "./ (4 files)\n  src/\n    tools/\n      read_file.rs\n    main.rs\n  .gitignore\n  \
         README.md\n"
    );
    let found = run(&scope, json!({ "find": "READ" })).await.unwrap();
    assert_eq!(found, "README.md\nsrc/tools/read_file.rs\n");
    let found = run(&scope, json!({ "find": "*.rs", "path": "src/tools" }))
        .await
        .unwrap();
    assert_eq!(found, "src/tools/read_file.rs\n");
    let none = run(&scope, json!({ "find": "*.rlib" })).await.unwrap();
    assert!(none.starts_with("no path matches"), "{none}");
}

#[tokio::test]
async fn a_large_tree_folds_deep_folders_under_the_caps() {
    let scope = scope("map_fold");
    for crate_name in ["core", "tui"] {
        for module in 0..40 {
            let dir = scope.cwd().join(format!("projects/{crate_name}/m{module}"));
            fs::create_dir_all(&dir).unwrap();
            for file in 0..10 {
                fs::write(dir.join(format!("f{file}.rs")), "x").unwrap();
            }
        }
    }
    let map = run(&scope, json!({})).await.unwrap();
    assert!(map.lines().count() <= MAX_LINES + 2, "{map}");
    assert!(map.len() <= MAX_BYTES + 200, "{map}");
    assert!(map.contains("\n    core/\n      m0/ (10 files)\n"), "{map}");
    // A folded folder opens when asked for.
    let one = run(&scope, json!({ "path": "projects/core/m7" }))
        .await
        .unwrap();
    assert!(
        one.starts_with("projects/core/m7/ (10 files)\n  f0.rs\n"),
        "{one}"
    );
}

#[tokio::test]
async fn a_path_out_of_the_project_is_refused() {
    let scope = scope("map_out");
    assert!(
        run(&scope, json!({ "path": "../workspace" }))
            .await
            .is_err()
    );
    assert!(run(&scope, json!({ "path": "/etc" })).await.is_err());
    #[cfg(unix)]
    {
        let outside = temp_dir("map_out_target");
        fs::create_dir_all(&outside).unwrap();
        fs::write(outside.join("secret.txt"), "x").unwrap();
        std::os::unix::fs::symlink(&outside, scope.cwd().join("link")).unwrap();
        let error = run(&scope, json!({ "path": "link" })).await.unwrap_err();
        assert!(error.to_string().contains("inside the project"), "{error}");
        let map = run(&scope, json!({})).await.unwrap();
        assert!(!map.contains("secret"), "{map}");
    }
}
