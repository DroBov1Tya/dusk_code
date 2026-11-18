use std::{
    fs,
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use parking_lot::Mutex;

use super::{
    Changed, Folder, Indexer, MAX_FILE_BYTES, Skipped, Status,
    index::PATHS_WAIT,
    refresh::{Limits, Seams},
};
use crate::{
    project::ledger,
    state::State,
    testing::{temp_dir, temp_state},
    tools::{ToolScope, console::project_folder},
};

/// A source file that makes the test parser panic, as a fault in a grammar would.
const PANICS: &str = "fn panics_the_parser() {}\n";
/// A source file the test parser gives up on as past its time limit.
const SLOW: &str = "fn too_slow_for_the_parser() {}\n";

/// A saved project in a folder of its own, and an indexer the test drives by hand.
struct Fixture {
    state: Arc<State>,
    dir: PathBuf,
    indexer: Arc<Indexer>,
    on: Arc<AtomicBool>,
    /// Every file the indexer opened, relative to the project, in order.
    opened: Arc<Mutex<Vec<String>>>,
    /// The status as each read saw it.
    seen: Arc<Mutex<Vec<Status>>>,
}

impl Fixture {
    async fn new(name: &str) -> Self {
        Self::with(name, Limits::default()).await
    }

    async fn with(name: &str, limits: Limits) -> Self {
        let dir = temp_dir(name);
        fs::create_dir_all(dir.join("src")).unwrap();
        // The temp folder is behind a link on macOS; a project is saved resolved.
        let dir = dir.canonicalize().unwrap();
        let state = Arc::new(temp_state(name).await);
        let shown = dir.to_str().unwrap();
        state.create_project("p", "P", "", shown).await.unwrap();
        let on = Arc::new(AtomicBool::new(true));
        let opened = Arc::new(Mutex::new(Vec::new()));
        let seen = Arc::new(Mutex::new(Vec::new()));
        let watched: Arc<Mutex<Option<Arc<Indexer>>>> = Arc::default();
        let real = Seams::real();
        let seams = Seams {
            read: Arc::new({
                let (opened, seen, watched) = (opened.clone(), seen.clone(), watched.clone());
                let (root, read) = (dir.clone(), real.read);
                move |path| {
                    let relative = path.strip_prefix(&root).unwrap_or(path);
                    opened.lock().push(relative.to_str().unwrap().to_owned());
                    if let Some(indexer) = &*watched.lock() {
                        seen.lock().push(indexer.status(&root));
                    }
                    read(path)
                }
            }),
            parse: Arc::new(move |language, path, source| {
                assert!(source != PANICS.as_bytes(), "the parser fell over");
                let mut parsed = (real.parse)(language, path, source)?;
                if source == SLOW.as_bytes() {
                    parsed.outline = Err(Skipped::TooSlow);
                }
                Ok(parsed)
            }),
        };
        let switch = Arc::clone(&on);
        let indexer = Arc::new(Indexer::new(
            Arc::clone(&state),
            Arc::new(move || switch.load(Ordering::Relaxed)),
            seams,
            limits,
        ));
        *watched.lock() = Some(Arc::clone(&indexer));
        Self {
            state,
            dir,
            indexer,
            on,
            opened,
            seen,
        }
    }

    fn folder(&self) -> Folder {
        Folder {
            root: self.dir.clone(),
            skip: Vec::new(),
        }
    }

    fn project(&self) -> &str {
        self.dir.to_str().unwrap()
    }

    fn write(&self, path: &str, text: &str) {
        let file = self.dir.join(path);
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(file, text).unwrap();
    }

    /// Runs every waiting job and says how many there were.
    async fn work(&self) -> usize {
        let mut jobs = 0;
        while self.indexer.step().await {
            jobs += 1;
        }
        jobs
    }

    /// Refreshes the project and gives the files that were opened for it.
    async fn refresh(&self) -> Vec<String> {
        self.opened.lock().clear();
        self.indexer.refresh(self.folder());
        self.work().await;
        self.opened.lock().clone()
    }

    /// Hands the indexer changed paths and gives the files that were opened for them.
    async fn changed(&self, paths: &[(&str, Option<String>)]) -> Vec<String> {
        self.opened.lock().clear();
        let paths = paths.iter().map(|(path, hash)| Changed {
            path: (*path).to_owned(),
            hash: hash.clone(),
        });
        self.indexer.changed(self.folder(), paths.collect());
        self.work().await;
        self.opened.lock().clone()
    }

    /// The indexed paths, sorted.
    async fn paths(&self) -> Vec<String> {
        let stamps = self.state.graph_stamps(self.project()).await.unwrap();
        let mut paths: Vec<_> = stamps.into_keys().collect();
        paths.sort();
        paths
    }

    /// The files that define `name`.
    async fn defines(&self, name: &str) -> Vec<String> {
        let found = self.state.graph_where(self.project(), name, None, None);
        found.await.unwrap().into_iter().map(|d| d.path).collect()
    }

    async fn rows(&self) -> i64 {
        sqlx::query_scalar("SELECT COUNT(*) FROM graph_files")
            .fetch_one(&self.state.pool)
            .await
            .unwrap()
    }
}

#[tokio::test]
async fn a_refresh_opens_only_what_changed() {
    let fixture = Fixture::new("index-refresh").await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    fixture.write("src/b.rs", "pub fn beta() { Alpha; }\n");
    assert_eq!(fixture.refresh().await, ["src/a.rs", "src/b.rs"]);
    assert_eq!(fixture.paths().await, ["src/a.rs", "src/b.rs"]);
    assert_eq!(fixture.defines("Alpha").await, ["src/a.rs"]);
    let before = fixture.state.graph_stamps(fixture.project()).await.unwrap();

    // Nothing changed: the walk alone, no file opened.
    assert!(fixture.refresh().await.is_empty());

    // One file changed: it alone is read, and its rows alone are rewritten.
    fixture.write("src/a.rs", "pub struct Gamma;\npub struct Delta;\n");
    assert_eq!(fixture.refresh().await, ["src/a.rs"]);
    assert!(fixture.defines("Alpha").await.is_empty());
    assert_eq!(fixture.defines("Gamma").await, ["src/a.rs"]);
    assert_eq!(fixture.defines("beta").await, ["src/b.rs"]);
    let after = fixture.state.graph_stamps(fixture.project()).await.unwrap();
    assert_eq!(after["src/b.rs"], before["src/b.rs"]);
    assert_ne!(after["src/a.rs"].hash, before["src/a.rs"].hash);

    // Touched, the text the same: read once to see that, then only its stamp moves.
    let later = std::time::SystemTime::now() + Duration::from_secs(60);
    let file = fs::File::options()
        .append(true)
        .open(fixture.dir.join("src/b.rs"));
    file.unwrap().set_modified(later).unwrap();
    assert_eq!(fixture.refresh().await, ["src/b.rs"]);
    let touched = fixture.state.graph_stamps(fixture.project()).await.unwrap();
    assert_eq!(touched["src/b.rs"].hash, after["src/b.rs"].hash);
    assert_ne!(touched["src/b.rs"].stamp, after["src/b.rs"].stamp);
    assert!(fixture.refresh().await.is_empty());

    // A deleted file: its rows go.
    fs::remove_file(fixture.dir.join("src/a.rs")).unwrap();
    assert!(fixture.refresh().await.is_empty());
    assert_eq!(fixture.paths().await, ["src/b.rs"]);
    assert!(fixture.defines("Gamma").await.is_empty());
}

#[cfg(unix)]
#[tokio::test]
async fn links_secrets_and_other_files_are_never_opened() {
    let fixture = Fixture::new("index-refused").await;
    let outside = temp_dir("index-refused-outside");
    fs::create_dir_all(outside.join("deep")).unwrap();
    fs::write(outside.join("secret.rs"), "pub struct Outside;\n").unwrap();
    fs::write(outside.join("deep/more.rs"), "pub struct Deeper;\n").unwrap();
    let link = |to: &Path, at: &str| std::os::unix::fs::symlink(to, fixture.dir.join(at)).unwrap();
    link(&outside.join("secret.rs"), "src/link.rs");
    link(&outside, "src/linked");
    link(&fixture.dir, "src/loop");
    // Secrets, by the ledger's rule, even under a name the graph reads.
    fixture.write(".env", "KEY=1\n");
    fixture.write(".env.rs", "pub struct Env;\n");
    fixture.write(".ssh/key.rs", "pub struct Key;\n");
    // Not a language of the graph.
    fixture.write("notes.txt", "pub struct Notes;\n");
    fixture.write("Makefile", "all:\n");
    // Ignored, with no git here.
    fixture.write(".gitignore", "target/\n");
    fixture.write("target/built.rs", "pub struct Built;\n");
    fixture.write(".git/hooks/hook.rs", "pub struct Hook;\n");
    // A pipe under a source name would hold a read for ever.
    let pipe = fixture.dir.join("src/pipe.rs");
    let made = std::process::Command::new("mkfifo").arg(&pipe).status();
    assert!(made.unwrap().success());
    // Folders nested far deeper than source is.
    let deep = "d/".repeat(60);
    fixture.write(&format!("{deep}leaf.rs"), "pub struct Leaf;\n");
    fixture.write("src/real.rs", "pub struct Real;\n");

    let leaf = format!("{deep}leaf.rs");
    assert_eq!(fixture.refresh().await, [leaf.as_str(), "src/real.rs"]);
    assert_eq!(fixture.paths().await, [leaf.as_str(), "src/real.rs"]);
    for name in ["Outside", "Deeper", "Env", "Key", "Notes", "Built", "Hook"] {
        assert!(fixture.defines(name).await.is_empty(), "{name}");
    }

    // Named by a caller, the same files are refused: a path is no way around the walk.
    let absolute = outside.join("secret.rs");
    let named = [
        "src/link.rs",
        "src/linked/secret.rs",
        "src/linked/deep/more.rs",
        "src/pipe.rs",
        ".env.rs",
        ".ssh/key.rs",
        "notes.txt",
        "../index-refused-outside/secret.rs",
        "src/../../index-refused-outside/secret.rs",
        absolute.to_str().unwrap(),
    ]
    .map(|path| (path, None));
    assert!(fixture.changed(&named).await.is_empty());
    assert_eq!(fixture.paths().await, [leaf.as_str(), "src/real.rs"]);
    assert!(fixture.defines("Outside").await.is_empty());

    // A folder of the thread's own inside the project is left out both ways.
    fixture.write("work/scratch.rs", "pub struct Scratch;\n");
    let folder = Folder {
        root: fixture.dir.clone(),
        skip: vec![fixture.dir.join("work")],
    };
    fixture.opened.lock().clear();
    fixture.indexer.refresh(folder.clone());
    let scratch = Changed {
        path: "work/scratch.rs".to_owned(),
        hash: None,
    };
    fixture.indexer.changed(folder, vec![scratch]);
    fixture.work().await;
    assert!(fixture.opened.lock().is_empty());
    assert!(fixture.defines("Scratch").await.is_empty());
}

#[tokio::test]
async fn a_file_past_the_size_cap_is_a_row_without_symbols() {
    let fixture = Fixture::new("index-large").await;
    let mut large = String::from("pub struct Large;\n");
    large.push_str(&"// filler\n".repeat(MAX_FILE_BYTES / 10 + 1));
    fixture.write("src/large.rs", &large);
    fixture.write("src/small.rs", "pub struct Small;\n");
    // The large one is not read at all.
    assert_eq!(fixture.refresh().await, ["src/small.rs"]);
    let project = fixture.project();
    let row = fixture.state.graph_file(project, "src/large.rs").await;
    let row = row.unwrap().unwrap();
    assert_eq!(row.skipped, Some(Skipped::TooLarge));
    assert_eq!(row.module, "large");
    assert!(fixture.defines("Large").await.is_empty());
    assert_eq!(fixture.defines("Small").await, ["src/small.rs"]);
    // Nor when a caller names it, whatever hash it gives.
    let named = [("src/large.rs", Some(ledger::sha256(large.as_bytes())))];
    assert!(fixture.changed(&named).await.is_empty());
    assert!(fixture.refresh().await.is_empty());
}

#[tokio::test]
async fn a_changed_path_is_opened_unless_its_hash_is_the_stored_one() {
    let fixture = Fixture::new("index-paths").await;
    let first = "pub struct First;\n";
    fixture.write("src/a.rs", first);
    fixture.write("src/b.rs", "pub struct Other;\n");
    fixture.refresh().await;

    // The ledger hashed what is indexed already: not opened.
    let same = [("src/a.rs", Some(ledger::sha256(first.as_bytes())))];
    assert!(fixture.changed(&same).await.is_empty());

    // A new hash: that file is read, and no other.
    let second = "pub struct Second;\n";
    fixture.write("src/a.rs", second);
    let new = [("src/a.rs", Some(ledger::sha256(second.as_bytes())))];
    assert_eq!(fixture.changed(&new).await, ["src/a.rs"]);
    assert_eq!(fixture.defines("Second").await, ["src/a.rs"]);
    assert!(fixture.defines("First").await.is_empty());
    assert!(fixture.changed(&new).await.is_empty());

    // A new file and a deleted one, as the ledger gives them.
    fixture.write("src/c.rs", "pub struct Third;\n");
    fs::remove_file(fixture.dir.join("src/b.rs")).unwrap();
    let edits = [("src/b.rs", None), ("src/c.rs", None)];
    assert_eq!(fixture.changed(&edits).await, ["src/c.rs"]);
    assert_eq!(fixture.paths().await, ["src/a.rs", "src/c.rs"]);
    assert!(fixture.defines("Other").await.is_empty());
}

#[tokio::test]
async fn only_a_saved_projects_own_folder_is_indexed() {
    let fixture = Fixture::new("index-unsaved").await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    let unsaved = temp_dir("index-unsaved-other");
    fs::create_dir_all(&unsaved).unwrap();
    let unsaved = unsaved.canonicalize().unwrap();
    fs::write(unsaved.join("b.rs"), "pub struct Beta;\n").unwrap();
    for root in [unsaved, fixture.dir.join("src")] {
        let folder = Folder {
            root: root.clone(),
            skip: Vec::new(),
        };
        fixture.indexer.refresh(folder.clone());
        let named = ["a.rs", "b.rs"].map(|path| Changed {
            path: path.to_owned(),
            hash: None,
        });
        fixture.indexer.changed(folder, named.into());
        fixture.work().await;
        assert!(fixture.opened.lock().is_empty(), "{}", root.display());
        assert_eq!(fixture.indexer.status(&root), Status::Off);
    }
    assert_eq!(fixture.rows().await, 0);
}

#[tokio::test]
async fn with_the_switch_off_nothing_is_queued_or_opened() {
    let fixture = Fixture::new("index-off").await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    fixture.on.store(false, Ordering::Relaxed);
    fixture.indexer.refresh(fixture.folder());
    fixture.indexer.unrecorded(&fixture.dir);
    fixture.indexer.turn_ended(fixture.folder(), false);
    let named = [("src/a.rs", None)];
    assert!(fixture.changed(&named).await.is_empty());
    assert_eq!(fixture.work().await, 0);
    assert_eq!(fixture.rows().await, 0);
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::Off);

    // Switched off after a request was queued: the job is dropped unopened, and what a
    // refresh stored before is no longer offered.
    fixture.on.store(true, Ordering::Relaxed);
    assert_eq!(fixture.refresh().await, ["src/a.rs"]);
    assert!(matches!(
        fixture.indexer.status(&fixture.dir),
        Status::Ready(_)
    ));
    fixture.write("src/a.rs", "pub struct Changed;\n");
    fixture.indexer.refresh(fixture.folder());
    fixture.on.store(false, Ordering::Relaxed);
    fixture.opened.lock().clear();
    assert_eq!(fixture.work().await, 1);
    assert!(fixture.opened.lock().is_empty());
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::Off);
}

#[tokio::test]
async fn requests_for_one_project_are_one_job() {
    let fixture = Fixture::new("index-merged").await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    fixture.write("src/b.rs", "pub struct Beta;\n");
    fixture.indexer.refresh(fixture.folder());
    fixture.indexer.refresh(fixture.folder());
    let named = |path: &str| Changed {
        path: path.to_owned(),
        hash: None,
    };
    let (indexer, folder) = (&fixture.indexer, fixture.folder());
    indexer.changed(folder.clone(), vec![named("src/a.rs")]);
    indexer.changed(folder, vec![named("src/a.rs"), named("src/b.rs")]);
    assert_eq!(fixture.work().await, 1);
    // Each file was read once: by its path, and then the walk found it as stored.
    assert_eq!(*fixture.opened.lock(), ["src/a.rs", "src/b.rs"]);
    assert_eq!(fixture.paths().await, ["src/a.rs", "src/b.rs"]);
}

#[tokio::test]
async fn the_status_is_building_until_a_refresh_ends() {
    let fixture = Fixture::new("index-status").await;
    fixture.write("src/a.rs", "pub struct Alpha;\npub fn uses() { Alpha; }\n");
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::Off);
    fixture.indexer.refresh(fixture.folder());
    // Asked for is not begun: only the worker knows the folder is a saved project.
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::Off);
    fixture.work().await;
    assert_eq!(*fixture.seen.lock(), [Status::Building]);
    let Status::Ready(counts) = fixture.indexer.status(&fixture.dir) else {
        panic!("not ready: {:?}", fixture.indexer.status(&fixture.dir));
    };
    assert_eq!((counts.files, counts.symbols, counts.skipped), (1, 2, 0));
    assert!(counts.refs >= 1);

    // The next refresh of this run does not take the index away while it works.
    fixture.write("src/b.rs", "pub struct Beta;\n");
    fixture.seen.lock().clear();
    fixture.refresh().await;
    assert!(matches!(fixture.seen.lock()[..], [Status::Ready(_)]));
}

#[tokio::test]
async fn a_project_past_a_cap_has_no_graph() {
    let limits = Limits {
        files: 2,
        ..Limits::default()
    };
    let fixture = Fixture::with("index-cap", limits).await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    fixture.write("src/b.rs", "pub struct Beta;\n");
    fixture.refresh().await;
    assert_eq!(fixture.rows().await, 2);
    fixture.write("src/c.rs", "pub struct Gamma;\n");
    assert!(fixture.refresh().await.is_empty());
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::TooLarge);
    assert_eq!(fixture.rows().await, 0);
    // A changed path does not start it again.
    assert!(fixture.changed(&[("src/a.rs", None)]).await.is_empty());
    assert_eq!(fixture.rows().await, 0);

    let limits = Limits {
        refs: 3,
        ..Limits::default()
    };
    let fixture = Fixture::with("index-cap-refs", limits).await;
    fixture.write("src/a.rs", "pub fn a() { B; C; D; E; }\n");
    fixture.refresh().await;
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::TooLarge);
    assert_eq!(fixture.rows().await, 0);

    let limits = Limits {
        walk: Duration::ZERO,
        ..Limits::default()
    };
    let fixture = Fixture::with("index-cap-walk", limits).await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    assert!(fixture.refresh().await.is_empty());
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::TooLarge);
}

/// Definitions are counted for the project as refs are: across files, each under its own
/// cap, and again over what is stored.
#[tokio::test]
async fn a_project_past_the_symbols_cap_has_no_graph() {
    let limits = Limits {
        symbols: 3,
        ..Limits::default()
    };
    let fixture = Fixture::with("index-cap-symbols", limits).await;
    fixture.write("src/a.rs", "pub struct Alpha;\npub struct Beta;\n");
    fixture.refresh().await;
    assert!(matches!(
        fixture.indexer.status(&fixture.dir),
        Status::Ready(_)
    ));
    assert_eq!(fixture.rows().await, 1);
    // Only the new file is parsed; the stored ones bring the project past the cap.
    fixture.write("src/b.rs", "pub struct Gamma;\npub struct Delta;\n");
    fixture.refresh().await;
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::TooLarge);
    assert_eq!(fixture.rows().await, 0);

    let fixture = Fixture::with("index-cap-symbols-one-refresh", limits).await;
    for (path, name) in [("src/a.rs", "Alpha"), ("src/b.rs", "Beta")] {
        fixture.write(
            path,
            &format!("pub struct {name};\npub fn {name}_new() {{}}\n"),
        );
    }
    fixture.refresh().await;
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::TooLarge);
    assert_eq!(fixture.rows().await, 0);
}

/// Files that each run into the parse's time limit would hold the one worker for hours:
/// past a number of them, or past a time for the refresh, the project has no graph.
#[tokio::test]
async fn a_project_that_takes_too_long_to_parse_has_no_graph() {
    let limits = Limits {
        slow_files: 1,
        ..Limits::default()
    };
    let fixture = Fixture::with("index-cap-slow", limits).await;
    fixture.write("src/a.rs", SLOW);
    fixture.write("src/b.rs", "pub struct Beta;\n");
    fixture.refresh().await;
    assert!(matches!(
        fixture.indexer.status(&fixture.dir),
        Status::Ready(_)
    ));
    fixture.write("src/c.rs", SLOW);
    fixture.write("src/d.rs", SLOW);
    fixture.refresh().await;
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::TooLarge);
    assert_eq!(fixture.rows().await, 0);

    let limits = Limits {
        parse_time: Duration::ZERO,
        ..Limits::default()
    };
    let fixture = Fixture::with("index-cap-parse-time", limits).await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    assert!(!fixture.refresh().await.is_empty());
    assert_eq!(fixture.indexer.status(&fixture.dir), Status::TooLarge);
    assert_eq!(fixture.rows().await, 0);
}

#[tokio::test]
async fn a_parser_that_panics_costs_one_file() {
    let fixture = Fixture::new("index-panic").await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    fixture.write("src/bad.rs", PANICS);
    fixture.write("src/c.rs", "pub struct Gamma;\n");
    fixture.refresh().await;
    assert_eq!(fixture.paths().await, ["src/a.rs", "src/c.rs"]);
    assert!(matches!(
        fixture.indexer.status(&fixture.dir),
        Status::Ready(_)
    ));
    // The worker goes on: the file is tried again, and taken once it parses.
    assert_eq!(fixture.refresh().await, ["src/bad.rs"]);
    fixture.write("src/bad.rs", "pub struct Mended;\n");
    assert_eq!(fixture.refresh().await, ["src/bad.rs"]);
    assert_eq!(fixture.defines("Mended").await, ["src/bad.rs"]);
}

/// The graph's key is `projects.dir`, the ledger's is `changes.project`, and the
/// indexer's is the thread's folder: one folder must be spelled one way by all three,
/// also when it is reached through a link.
#[cfg(unix)]
#[tokio::test]
async fn a_thread_folder_is_spelled_as_its_saved_project() {
    let fixture = Fixture::new("index-spelling").await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    let link = temp_dir("index-spelling-link");
    std::os::unix::fs::symlink(&fixture.dir, &link).unwrap();
    let saved = crate::project::save(&fixture.state, None, "linked", "", &link).await;
    // The folder is the fixture's project already: saved once, by its resolved path.
    assert!(saved.is_err());
    fixture.state.remove_project("p").await.unwrap();
    let saved = crate::project::save(&fixture.state, None, "linked", "", &link).await;
    let saved = saved.unwrap();

    let workspace = temp_dir("index-spelling-workspace");
    fs::create_dir_all(&workspace).unwrap();
    let scope = ToolScope::new(&link, &workspace).unwrap();
    let folder = project_folder(&scope);
    assert_eq!(folder.root, saved.dir);
    // What `Watch::record` writes as `changes.project`.
    assert_eq!(scope.cwd().to_string_lossy(), saved.dir.to_str().unwrap());
    assert!(
        fixture
            .state
            .graph_project(&scope.cwd().to_string_lossy())
            .await
            .unwrap()
    );
    assert!(
        !fixture
            .state
            .graph_project(link.to_str().unwrap())
            .await
            .unwrap()
    );

    fixture.indexer.refresh(folder);
    fixture.work().await;
    assert_eq!(fixture.paths().await, ["src/a.rs"]);
}

#[tokio::test]
async fn a_turn_end_refreshes_what_the_ledger_did_not_record() {
    let fixture = Fixture::new("index-turn").await;
    fixture.write("src/a.rs", "pub struct Alpha;\n");
    // Recorded: the edits came as they were made.
    fixture.indexer.turn_ended(fixture.folder(), true);
    assert_eq!(fixture.work().await, 0);
    // The ledger is off.
    fixture.indexer.turn_ended(fixture.folder(), false);
    assert_eq!(fixture.work().await, 1);
    // The ledger gave up on a command: once, for the turn it was in.
    fixture.indexer.unrecorded(&fixture.dir);
    fixture.indexer.turn_ended(fixture.folder(), true);
    assert_eq!(fixture.work().await, 1);
    fixture.indexer.turn_ended(fixture.folder(), true);
    assert_eq!(fixture.work().await, 0);
}

#[tokio::test]
async fn the_worker_takes_jobs_and_a_caller_can_wait_for_its_paths() {
    let dir = temp_dir("index-worker");
    fs::create_dir_all(dir.join("src")).unwrap();
    let dir = dir.canonicalize().unwrap();
    fs::write(dir.join("src/a.rs"), "pub struct Alpha;\n").unwrap();
    let state = Arc::new(temp_state("index-worker").await);
    let project = dir.to_str().unwrap();
    state.create_project("p", "P", "", project).await.unwrap();
    let (indexer, worker) = Indexer::start(Arc::clone(&state), || true);
    let folder = Folder {
        root: dir.clone(),
        skip: Vec::new(),
    };
    // Nothing waits: the wait ends at once.
    let started = Instant::now();
    indexer.settle(&dir, Duration::from_secs(30)).await;
    assert!(started.elapsed() < Duration::from_secs(5));

    let named = Changed {
        path: "src/a.rs".to_owned(),
        hash: None,
    };
    indexer.changed(folder.clone(), vec![named]);
    indexer.settle(&dir, Duration::from_secs(30)).await;
    assert!(
        state
            .graph_file(project, "src/a.rs")
            .await
            .unwrap()
            .is_some()
    );

    // A refresh is not waited for, so the status is asked until it is ready.
    indexer.refresh(folder);
    let started = Instant::now();
    while !matches!(indexer.status(&dir), Status::Ready(_)) {
        assert!(started.elapsed() < Duration::from_secs(30), "never ready");
        tokio::time::sleep(Duration::from_millis(5)).await;
    }
    assert!(PATHS_WAIT < Duration::from_secs(1));
    worker.abort();
}

/// Prints what indexing this repository costs: `cargo test -p dusk_core -- --ignored
/// --nocapture indexing_this_repository`. The numbers of the design's section 1.
#[tokio::test]
#[ignore = "a measurement, not a check"]
async fn indexing_this_repository() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root.canonicalize().unwrap();
    let project = root.to_str().unwrap();
    let database = temp_dir("index-speed").join("state.db");
    let state = Arc::new(State::open(&database).await.unwrap());
    state.create_project("p", "P", "", project).await.unwrap();
    let size = || -> u64 {
        ["", "-wal"]
            .iter()
            .filter_map(|suffix| {
                let mut name = database.clone().into_os_string();
                name.push(suffix);
                fs::metadata(name).ok()
            })
            .map(|file| file.len())
            .sum()
    };
    let before = size();
    let indexer = Indexer::new(
        Arc::clone(&state),
        Arc::new(|| true),
        Seams::real(),
        Limits::default(),
    );
    let folder = Folder {
        root: root.clone(),
        skip: Vec::new(),
    };
    let timed = async |job: &dyn Fn()| {
        job();
        let started = Instant::now();
        assert!(indexer.step().await);
        started.elapsed()
    };

    let full = timed(&|| indexer.refresh(folder.clone())).await;
    let Status::Ready(counts) = indexer.status(&root) else {
        panic!("not ready: {:?}", indexer.status(&root));
    };
    let grown = size() - before;
    let unchanged = timed(&|| indexer.refresh(folder.clone())).await;

    // One file from nothing: the largest of up to 2,000 lines, its rows removed first.
    let lines_of = |path: &str| {
        let text = fs::read(root.join(path)).unwrap_or_default();
        text.iter().filter(|byte| **byte == b'\n').count()
    };
    let stamps = state.graph_stamps(project).await.unwrap();
    let lines: usize = stamps.keys().map(|path| lines_of(path)).sum();
    let (one, one_lines) = stamps
        .keys()
        .map(|path| (path.clone(), lines_of(path)))
        .filter(|(_, lines)| *lines <= 2_000)
        .max_by_key(|(_, lines)| *lines)
        .unwrap();
    state
        .remove_graph_files(project, std::slice::from_ref(&one))
        .await
        .unwrap();
    let named = Changed {
        path: one.clone(),
        hash: None,
    };
    let update = timed(&|| indexer.changed(folder.clone(), vec![named.clone()])).await;

    println!("project graph of this repository, {} profile", profile());
    println!("files {}, lines {lines}", counts.files);
    println!(
        "symbols {}, refs {}, skipped {}",
        counts.symbols, counts.refs, counts.skipped
    );
    println!("full index {full:?} (the worker idles half of its database time)");
    println!("refresh with nothing changed {unchanged:?}");
    println!("one file ({one}, {one_lines} lines) {update:?}, the same idling included");
    println!("database grew by {grown} bytes");
}

fn profile() -> &'static str {
    if cfg!(debug_assertions) {
        "dev"
    } else {
        "release"
    }
}
