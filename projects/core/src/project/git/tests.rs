//! The boundaries of what a client names: options, paths out of the project, force.

use dusk_protocol::project::Project;

use super::{check, remote::push_args};
use crate::testing::temp_dir;

fn project(name: &str) -> (Project, std::path::PathBuf) {
    let root = std::fs::canonicalize({
        let root = temp_dir(name);
        std::fs::create_dir_all(root.join("p/src")).unwrap();
        root
    })
    .unwrap();
    let project = Project {
        id: "p".into(),
        name: "p".into(),
        description: String::new(),
        dir: root.join("p"),
        created_at: 0,
    };
    (project, root)
}

#[tokio::test]
async fn a_path_is_never_an_option_and_never_leaves_the_project() {
    let (project, root) = project("git-paths");
    assert!(check::path(&project, "src").await.is_ok());
    assert!(check::path(&project, "src/deleted.rs").await.is_ok());
    for bad in ["-f", "--all", "..", "src/../..", "/etc/passwd", "../p", ""] {
        assert!(check::path(&project, bad).await.is_err(), "{bad}");
    }
    std::os::unix::fs::symlink(&root, project.dir.join("out")).unwrap();
    assert!(check::path(&project, "out").await.is_err());
    assert!(check::path(&project, "out/new.rs").await.is_err());
}

#[test]
fn a_push_is_never_forced() {
    let forcing = [
        "--force",
        "-f",
        "--force-with-lease",
        "--mirror",
        "--delete",
        "-d",
    ];
    for (branch, upstream) in [
        ("main", None),
        ("dusk/fix", Some(("origin", "refs/heads/dusk/fix"))),
    ] {
        let args = push_args(branch, upstream).unwrap();
        assert!(!args.iter().any(|arg| forcing.contains(&arg.as_str())));
        assert!(!args.iter().any(|arg| arg.starts_with('+')));
    }
    for bad in ["+main", "--force", "-f", "main:other"] {
        assert!(push_args(bad, None).is_err(), "{bad}");
    }
    assert!(push_args("main", Some(("--force", "refs/heads/main"))).is_err());
    assert!(push_args("main", Some(("origin", "refs/heads/+main"))).is_err());
}
