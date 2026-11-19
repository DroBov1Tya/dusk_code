//! Notices skill files that a tool call created or changed. The files are listed before
//! and after the call by size and change time; only a file whose text really differs
//! from the call's own baseline, and that still reads as a skill, counts as a change.

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::SystemTime,
};

use parking_lot::Mutex;

use crate::{
    catalog::graph::{Node, SKILL_FILE, is_node_folder_name, node_name},
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

/// What one call starts from: the files and the texts they had. Every call has its own,
/// so another thread's look cannot hide this call's change.
pub struct Baseline {
    stamps: Stamps,
    texts: HashMap<PathBuf, Arc<str>>,
}

/// One skill a call created or changed. `before` is `None` for a new skill, or when the
/// old text was not in the baseline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Changed {
    pub skill: String,
    pub created: bool,
    pub before: Option<String>,
    pub after: String,
}

#[derive(Default)]
struct Cache {
    texts: HashMap<PathBuf, (Stamp, Arc<str>)>,
    /// Files not cached (too big, not text, over budget), by the stamp they were seen at,
    /// so they are not read again at every call.
    skipped: HashMap<PathBuf, Stamp>,
    /// The last change reported for a file, so overlapping calls record it once.
    recorded: HashMap<PathBuf, Stamp>,
    bytes: usize,
}

impl Cache {
    fn put(&mut self, path: PathBuf, stamp: Stamp, text: Arc<str>) {
        self.forget(&path);
        if self.bytes + text.len() <= MAX_CACHE_BYTES {
            self.bytes += text.len();
            self.texts.insert(path, (stamp, text));
        } else {
            self.skipped.insert(path, stamp);
        }
    }

    fn forget(&mut self, path: &Path) {
        if let Some((_, old)) = self.texts.remove(path) {
            self.bytes -= old.len();
        }
        self.skipped.remove(path);
    }

    fn knows(&self, path: &Path, stamp: &Stamp) -> bool {
        self.texts
            .get(path)
            .is_some_and(|(known, _)| known == stamp)
            || self.skipped.get(path) == Some(stamp)
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

    /// The files now with their texts, to compare a call's end against.
    pub async fn before(&self) -> Option<Baseline> {
        let folder = self.skills.clone()?;
        let cache = Arc::clone(&self.cache);
        tokio::task::spawn_blocking(move || {
            let mut stamps = Stamps::new();
            walk(&folder, 0, &mut stamps);
            let mut cache = cache.lock();
            let gone: Vec<_> = cache
                .texts
                .keys()
                .chain(cache.skipped.keys())
                .chain(cache.recorded.keys())
                .filter(|path| !stamps.contains_key(*path))
                .cloned()
                .collect();
            for path in gone {
                cache.forget(&path);
                cache.recorded.remove(&path);
            }
            for (path, stamp) in &stamps {
                if cache.knows(path, stamp) {
                    continue;
                }
                match read_text(path) {
                    Some(text) => cache.put(path.clone(), *stamp, text),
                    None => {
                        cache.forget(path);
                        cache.skipped.insert(path.clone(), *stamp);
                    }
                }
            }
            let texts = cache
                .texts
                .iter()
                .map(|(path, (_, text))| (path.clone(), Arc::clone(text)))
                .collect();
            Baseline { stamps, texts }
        })
        .await
        .ok()
    }

    /// The skills created or changed since `before`, by name. A change another call
    /// reported already is not reported again.
    pub async fn changed(&self, before: Baseline) -> Vec<Changed> {
        let Some(folder) = self.skills.clone() else {
            return Vec::new();
        };
        let cache = Arc::clone(&self.cache);
        let found = tokio::task::spawn_blocking(move || {
            let mut after = Stamps::new();
            walk(&folder, 0, &mut after);
            let mut paths: Vec<_> = after
                .iter()
                .filter(|(path, stamp)| before.stamps.get(*path) != Some(stamp))
                .map(|(path, stamp)| (path.clone(), *stamp))
                .collect();
            paths.sort();
            let mut cache = cache.lock();
            let mut found = Vec::new();
            for (path, stamp) in paths {
                let Some(text) = read_text(&path) else {
                    cache.forget(&path);
                    cache.skipped.insert(path, stamp);
                    continue;
                };
                let old = before.texts.get(&path).cloned();
                let created = !before.stamps.contains_key(&path);
                cache.put(path.clone(), stamp, Arc::clone(&text));
                let Some(skill) = node_name(&folder, SKILL_FILE, &path) else {
                    continue;
                };
                // A touched file with the same text is no change.
                if old.as_deref() == Some(&*text) || Node::check(&text, &skill).is_err() {
                    continue;
                }
                if cache.recorded.get(&path) == Some(&stamp) {
                    continue;
                }
                cache.recorded.insert(path, stamp);
                found.push(Changed {
                    skill,
                    created,
                    before: old.map(|text| text.to_string()),
                    after: text.to_string(),
                });
            }
            found
        })
        .await;
        found.unwrap_or_default()
    }
}

fn read_text(path: &Path) -> Option<Arc<str>> {
    let text = String::from_utf8(read_regular(path, MAX_SKILL_BYTES).ok()?).ok()?;
    Some(text.into())
}

/// Links are not followed, so a skill folder cannot point the walk elsewhere. Folders
/// that are no skill folders (hidden, dependencies, build output) are not entered.
fn walk(folder: &Path, depth: usize, files: &mut Stamps) {
    let Ok(entries) = fs::read_dir(folder) else {
        return;
    };
    for entry in entries.flatten() {
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        let path = entry.path();
        let name = entry.file_name();
        if kind.is_dir() {
            let enter = name
                .to_str()
                .is_some_and(|name| !name.starts_with('.') && is_node_folder_name(name));
            if enter && depth < MAX_DEPTH {
                walk(&path, depth + 1, files);
            }
        } else if kind.is_file()
            && name == SKILL_FILE
            && let Ok(metadata) = entry.metadata()
        {
            files.insert(path, (metadata.len(), metadata.modified().ok()));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    fn skill(text: &str) -> String {
        format!("---\ndescription: Demo.\n---\n{text}\n")
    }

    /// A call whose baseline was taken before another call looked still sees its change.
    #[tokio::test]
    async fn an_overlapping_look_does_not_hide_a_change() {
        let root = temp_dir("watch_overlap");
        let file = root.join("demo/SKILL.md");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, skill("one")).unwrap();
        let watch = Watch::new(Some(root));
        let a = watch.before().await.unwrap();
        fs::write(&file, skill("two!")).unwrap();
        let b = watch.before().await.unwrap();
        let changes = watch.changed(a).await;
        assert_eq!(changes.len(), 1);
        assert_eq!(changes[0].before.as_deref(), Some(skill("one").as_str()));
        // The same change is not recorded a second time.
        assert!(watch.changed(b).await.is_empty());
    }

    #[tokio::test]
    async fn hidden_folders_are_not_skills() {
        let root = temp_dir("watch_hidden");
        let watch = Watch::new(Some(root.clone()));
        let before = watch.before().await.unwrap();
        fs::create_dir_all(root.join(".git/demo")).unwrap();
        fs::write(root.join(".git/demo/SKILL.md"), skill("x")).unwrap();
        assert!(watch.changed(before).await.is_empty());
    }
}
