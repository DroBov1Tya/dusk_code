//! What the indexer does with one project: `refresh` walks it and indexes what changed,
//! `update` indexes the paths it is given. Project files are untrusted input: a file is
//! found by a walk that follows no link, read as a regular file under the size cap, and
//! parsed off the async threads, where a panic costs that file and nothing else.

use std::{
    collections::{BTreeMap, HashSet},
    path::Path,
    sync::Arc,
    time::{Duration, Instant, UNIX_EPOCH},
};

use anyhow::{Context, Result};
use tracing::{info, warn};

use super::{Language, MAX_FILE_BYTES, ParsedFile, Skipped, index::Folder, parse};
use crate::{
    init::files::read_regular,
    project::{ledger, plain_relative},
    state::{
        State,
        graph::{GraphCounts, IndexedFile, NewRef, NewSymbol, Stamp},
    },
};

/// Files written in one transaction.
const BATCH: usize = 32;
/// The hash of a file past the size cap: it is never read, so it has none. No content
/// hashes to this, so such a file is looked at again whenever a caller names it.
const UNREAD: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// The caps of a project; past one it is `too_large` and has no graph.
#[derive(Debug, Clone, Copy)]
pub struct Limits {
    /// One walk may take this long: no command waits on it, so more than the ledger's.
    pub walk: Duration,
    /// Files of the languages the graph reads.
    pub files: usize,
    pub refs: u64,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            walk: Duration::from_secs(5),
            files: 20_000,
            refs: 1_500_000,
        }
    }
}

/// How a file is read and parsed. Tests put their own in, to count the files opened and
/// to make a parse panic.
#[derive(Clone)]
pub struct Seams {
    pub read: Arc<Read>,
    pub parse: Arc<Parse>,
}

/// The bytes of a regular file of at most `MAX_FILE_BYTES`, a link not followed.
type Read = dyn Fn(&Path) -> Result<Vec<u8>> + Send + Sync;
/// `parse`.
type Parse = dyn Fn(Language, &str, &[u8]) -> Result<ParsedFile> + Send + Sync;

impl Seams {
    pub fn real() -> Self {
        Self {
            read: Arc::new(|path| read_regular(path, MAX_FILE_BYTES as u64)),
            parse: Arc::new(parse),
        }
    }
}

/// One job's view of its project.
pub struct Work<'a> {
    pub state: &'a State,
    /// The folder as the database spells it: `projects.dir`.
    pub project: &'a str,
    pub folder: &'a Folder,
    pub seams: &'a Seams,
    pub limits: Limits,
}

pub enum Refreshed {
    Done(GraphCounts),
    TooLarge,
}

/// What became of one file.
enum Loaded {
    Parsed(IndexedFile),
    /// The text is the one indexed; only its stamp moved.
    Same(Stamp),
    /// Not a regular file inside the project any more.
    Gone,
    /// Logged; it is tried again by the next refresh.
    Failed,
}

#[derive(Default)]
struct Batch {
    files: Vec<IndexedFile>,
    stamps: Vec<(String, Stamp)>,
    gone: Vec<String>,
}

impl Work<'_> {
    /// Walks the project and brings its rows in line with it. Files whose size and
    /// change time are the stored ones are not opened.
    pub async fn refresh(&self) -> Result<Refreshed> {
        let (root, skip, budget) = (
            self.folder.root.clone(),
            self.folder.skip.clone(),
            self.limits.walk,
        );
        let walked = tokio::task::spawn_blocking(move || ledger::take_for(&root, &skip, budget))
            .await
            .context("the walk of the project did not end")?;
        let Some(walked) = walked else {
            return self.too_large("more files than a walk takes").await;
        };
        let mut found: Vec<(String, Stamp)> = walked
            .files
            .into_iter()
            .filter(|(path, _)| reads(path))
            .map(|(path, seen)| (path, stamp(seen)))
            .collect();
        if found.len() > self.limits.files {
            return self.too_large("more source files than a graph holds").await;
        }
        found.sort_by(|a, b| a.0.cmp(&b.0));
        let stored = self.state.graph_stamps(self.project).await?;
        let on_disk: HashSet<&str> = found.iter().map(|(path, _)| path.as_str()).collect();
        let gone: Vec<String> = stored
            .keys()
            .filter(|path| !on_disk.contains(path.as_str()))
            .cloned()
            .collect();
        if !gone.is_empty() {
            self.state.remove_graph_files(self.project, &gone).await?;
        }
        let changed = found
            .into_iter()
            .filter(|(path, stamp)| stored.get(path).is_none_or(|had| had.stamp != *stamp))
            .map(|(path, _)| {
                let hash = stored.get(&path).map(|had| had.hash.clone());
                (path, hash)
            })
            .collect();
        if !self.index(changed).await {
            return self
                .too_large("more names referred to than a graph holds")
                .await;
        }
        let counts = self.state.graph_counts(self.project).await?;
        if counts.refs > self.limits.refs {
            return self
                .too_large("more names referred to than a graph holds")
                .await;
        }
        Ok(Refreshed::Done(counts))
    }

    /// Indexes the given paths, without a walk. A path the graph does not read is left
    /// alone, and one whose hash is the stored one is not opened.
    pub async fn update(&self, paths: BTreeMap<String, Option<String>>) {
        let mut changed = Vec::new();
        for (path, hash) in paths {
            if !self.takes(&path) {
                continue;
            }
            let stored = match self.state.graph_file(self.project, &path).await {
                Ok(file) => file.map(|file| file.hash),
                Err(error) => {
                    warn!(
                        project = self.project,
                        path, "cannot read a graph file: {error:#}"
                    );
                    continue;
                }
            };
            if hash.is_some() && hash == stored {
                continue;
            }
            changed.push((path, stored));
        }
        // Past the cap here, the next refresh is what drops the project.
        self.index(changed).await;
    }

    /// Whether a path a caller named may be opened: one the walk could have given.
    fn takes(&self, path: &str) -> bool {
        let absolute = self.folder.root.join(path);
        reads(path)
            && plain_relative(Path::new(path)).is_ok()
            && !self
                .folder
                .skip
                .iter()
                .any(|skip| absolute.starts_with(skip))
    }

    /// Reads, parses and stores the files, each with the hash stored for it, in batches
    /// of one transaction. False when they held more refs than a project may.
    async fn index(&self, changed: Vec<(String, Option<String>)>) -> bool {
        let mut refs = 0u64;
        for files in changed.chunks(BATCH) {
            let mut batch = Batch::default();
            for (path, stored) in files {
                match self.load(path, stored.as_deref()).await {
                    Loaded::Parsed(file) => {
                        refs += file.refs.len() as u64;
                        batch.files.push(file);
                    }
                    Loaded::Same(stamp) => batch.stamps.push((path.clone(), stamp)),
                    Loaded::Gone => batch.gone.push(path.clone()),
                    Loaded::Failed => {}
                }
            }
            if refs > self.limits.refs {
                return false;
            }
            let took = self.store(&batch).await;
            // The database has one connection: it is left free for as long as it was held.
            tokio::time::sleep(took).await;
        }
        true
    }

    async fn load(&self, path: &str, stored: Option<&str>) -> Loaded {
        let (root, seams) = (self.folder.root.clone(), self.seams.clone());
        let (relative, stored) = (path.to_owned(), stored.map(str::to_owned));
        let loaded =
            tokio::task::spawn_blocking(move || load(&root, &relative, stored.as_deref(), &seams));
        match loaded.await {
            Ok(Ok(loaded)) => loaded,
            Ok(Err(error)) => {
                warn!(
                    project = self.project,
                    path, "cannot index a file: {error:#}"
                );
                Loaded::Failed
            }
            // The panic's own words may quote the file, so they are not logged.
            Err(error) => {
                let panicked = error.is_panic();
                warn!(
                    project = self.project,
                    path, panicked, "indexing a file did not end"
                );
                Loaded::Failed
            }
        }
    }

    /// Writes a batch and says how long the database was held. A failure is tried once
    /// more; after that the files keep their old stamps and the next refresh takes them.
    async fn store(&self, batch: &Batch) -> Duration {
        let started = Instant::now();
        if let Err(error) = self.write(batch).await {
            warn!(
                project = self.project,
                "cannot store a batch of the graph, once more: {error:#}"
            );
            if let Err(error) = self.write(batch).await {
                warn!(
                    project = self.project,
                    "cannot store a batch of the graph: {error:#}"
                );
            }
        }
        started.elapsed()
    }

    async fn write(&self, batch: &Batch) -> Result<()> {
        if !batch.files.is_empty() {
            self.state
                .replace_graph_files(self.project, &batch.files)
                .await?;
        }
        if !batch.stamps.is_empty() {
            self.state
                .restamp_graph_files(self.project, &batch.stamps)
                .await?;
        }
        if !batch.gone.is_empty() {
            self.state
                .remove_graph_files(self.project, &batch.gone)
                .await?;
        }
        Ok(())
    }

    async fn too_large(&self, why: &str) -> Result<Refreshed> {
        info!(project = self.project, "the project has no graph: {why}");
        self.state.remove_graph(self.project).await?;
        Ok(Refreshed::TooLarge)
    }
}

/// Whether the graph reads a file of this path: a language it knows, and not a place
/// where secrets are kept. Nothing else is ever opened.
fn reads(path: &str) -> bool {
    Language::of(path).is_some() && ledger::may_keep_text(path)
}

/// One file, read and parsed. Blocks; an error is logged by the caller with the path.
fn load(root: &Path, path: &str, stored: Option<&str>, seams: &Seams) -> Result<Loaded> {
    let language = Language::of(path).context("not a language the graph reads")?;
    let absolute = root.join(path);
    // Missing, a link, a folder, a FIFO or a device: not a file of the project.
    let Some(seen) = ledger::Stamp::of(&absolute) else {
        return Ok(Loaded::Gone);
    };
    if !inside(root, &absolute) {
        return Ok(Loaded::Gone);
    }
    if seen.size > MAX_FILE_BYTES as u64 {
        // Not read: the parse of nothing gives the module and whether it is a test file.
        let mut file = (seams.parse)(language, path, &[])?;
        file.outline = Err(Skipped::TooLarge);
        return Ok(Loaded::Parsed(indexed(
            path,
            UNREAD.to_owned(),
            stamp(seen),
            file,
        )));
    }
    // A file that grew past the cap since the look above fails here; the next refresh
    // sees its new size.
    let bytes = (seams.read)(&absolute)?;
    let hash = ledger::sha256(&bytes);
    if stored == Some(hash.as_str()) {
        return Ok(Loaded::Same(stamp(seen)));
    }
    let file = (seams.parse)(language, path, &bytes)?;
    Ok(Loaded::Parsed(indexed(path, hash, stamp(seen), file)))
}

/// Whether the folder of `file` is, links resolved, still under `root`: a folder on the
/// way swapped for a link since the walk does not lead the read out. The file itself is
/// opened without following a link.
pub(super) fn inside(root: &Path, file: &Path) -> bool {
    file.parent()
        .and_then(|folder| folder.canonicalize().ok())
        .is_some_and(|folder| folder.starts_with(root))
}

/// The stamp the store keeps: the change time in milliseconds, negative before 1970.
pub(super) fn stamp(seen: ledger::Stamp) -> Stamp {
    let millis = |time: Duration| i64::try_from(time.as_millis()).unwrap_or(i64::MAX);
    Stamp {
        size: seen.size,
        modified_ms: match seen.modified.duration_since(UNIX_EPOCH) {
            Ok(after) => millis(after),
            Err(before) => -millis(before.duration()),
        },
    }
}

/// A parsed file as the store takes it.
fn indexed(path: &str, hash: String, stamp: Stamp, file: ParsedFile) -> IndexedFile {
    let (skipped, outline) = match file.outline {
        Ok(outline) => (None, outline),
        Err(skipped) => (Some(skipped), Default::default()),
    };
    IndexedFile {
        path: path.to_owned(),
        language: file.language,
        hash,
        stamp,
        module: file.module,
        is_test: file.is_test,
        skipped,
        symbols: outline
            .symbols
            .into_iter()
            .map(|symbol| NewSymbol {
                name: symbol.name.as_str().to_owned(),
                parent: symbol.parent.map(|parent| parent.as_str().to_owned()),
                kind: symbol.kind,
                line: symbol.line,
                end_line: symbol.end_line,
            })
            .collect(),
        refs: outline
            .refs
            .into_iter()
            .map(|found| NewRef {
                kind: found.kind,
                name: found.name.as_str().to_owned(),
                line: found.line,
                uses: found.uses,
                path: found.path.map(|path| path.as_str().to_owned()),
            })
            .collect(),
    }
}
