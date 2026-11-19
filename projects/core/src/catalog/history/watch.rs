//! Notices skill files that a tool call created or changed. The files are listed before
//! and after the call by size and change time; only a file whose text really differs
//! from the cached one, and that still reads as a skill, counts as a change.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use parking_lot::Mutex;

use crate::{
    catalog::graph::{Node, SKILL_FILE, node_name},
    init::files::read_regular,
};

/// Deeper folders are not skills; the walk stops there.
const MAX_DEPTH: usize = 8;
/// The same limit as a stored skill text.
const MAX_SKILL_BYTES: u64 = 256 * 1024;
/// Most text the cache keeps; a skill past it has no text before.
const MAX_CACHE_BYTES: usize = 8 * 1024 * 1024;

type Stamp = (u64, Option<SystemTime>);

/// The skill files with their size and change time.
pub type Stamps = HashMap<PathBuf, Stamp>;

/// One skill a call created or changed. `before` is `None` for a new skill, or when the
/// old text was not in the cache.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub skill: String,
    pub created: bool,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Default)]
struct Cache {
    texts: HashMap<PathBuf, (Stamp, String)>,
    bytes: usize,
}

impl Cache {
    fn put(&mut self, path: PathBuf, stamp: Stamp, text: String) {
        self.forget(&path);
        if self.bytes + text.len() <= MAX_CACHE_BYTES {
            self.bytes += text.len();
            self.texts.insert(path, (stamp, text));
        }
    }

    fn forget(&mut self, path: &Path) {
        if let Some((_, old)) = self.texts.remove(path) {
            self.bytes -= old.len();
        }
    }
}

/// Watches the skills folder. One for the runtime, shared by every thread.
pub struct Watch {
    skills: Option<PathBuf>,
    cache: Arc<Mutex<Cache>>,
}

impl Watch {
    pub fn new(skills: Option<PathBuf>) -> Self {
        Self {
            skills: skills.map(|folder| folder.canonicalize().unwrap_or(folder)),
            cache: Arc::default(),
        }
    }

    /// The files now, and their texts cached for the ones that changed since the last look.
    pub async fn before(&self) -> Option<Stamps> {
        let folder = self.skills.clone()?;
        let cache = Arc::clone(&self.cache);
        tokio::task::spawn_blocking(move || {
            let mut stamps = Stamps::new();
            walk(&folder, 0, &mut stamps);
            let mut cache = cache.lock();
            let gone: Vec<_> = cache
                .texts
                .keys()
                .filter(|path| !stamps.contains_key(*path))
                .cloned()
                .collect();
            for path in gone {
                cache.forget(&path);
            }
            for (path, stamp) in &stamps {
                if cache
                    .texts
                    .get(path)
                    .is_some_and(|(known, _)| known == stamp)
                {
                    continue;
                }
                match read_text(path) {
                    Some(text) => cache.put(path.clone(), *stamp, text),
                    None => cache.forget(path),
                }
            }
            stamps
        })
        .await
        .ok()
    }

    /// The skills created or changed since `before`, by name.
    pub async fn changed(&self, before: Stamps) -> Vec<Changed> {
        let Some(folder) = self.skills.clone() else {
            return Vec::new();
        };
        let cache = Arc::clone(&self.cache);
        let found = tokio::task::spawn_blocking(move || {
            let mut after = Stamps::new();
            walk(&folder, 0, &mut after);
            let mut paths: Vec<_> = after
                .iter()
                .filter(|(path, stamp)| before.get(*path) != Some(stamp))
                .map(|(path, stamp)| (path.clone(), *stamp))
                .collect();
            paths.sort();
            let mut cache = cache.lock();
            let mut found = Vec::new();
            for (path, stamp) in paths {
                let Some(text) = read_text(&path) else {
                    continue;
                };
                let known = cache.texts.get(&path).map(|(_, text)| text.clone());
                let created = !before.contains_key(&path);
                // A touched file with the same text is no change.
                let same = known.as_deref() == Some(text.as_str());
                cache.put(path.clone(), stamp, text.clone());
                let Some(skill) = node_name(&folder, SKILL_FILE, &path) else {
                    continue;
                };
                if same || Node::check(&text, &skill).is_err() {
                    continue;
                }
                found.push(Changed {
                    skill,
                    created,
                    before: if created { None } else { known },
                    after: text,
                });
            }
            found
        })
        .await;
        found.unwrap_or_default()
    }
}

fn read_text(path: &Path) -> Option<String> {
    String::from_utf8(read_regular(path, MAX_SKILL_BYTES).ok()?).ok()
}

/// Links are not followed, so a skill folder cannot point the walk elsewhere.
fn walk(folder: &Path, depth: usize, files: &mut Stamps) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        if kind.is_dir() && depth < MAX_DEPTH {
            walk(&path, depth + 1, files);
        } else if kind.is_file()
            && entry.file_name() == SKILL_FILE
            && let Ok(metadata) = entry.metadata()
        {
            files.insert(path, (metadata.len(), metadata.modified().ok()));
        }
    }
}
