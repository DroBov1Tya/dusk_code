//! The profile and snippets a message names with `@`, kept as files the user can edit
//! anywhere: `rules/snippets/<name>.md` for each snippet, and `rules/me/ME.md` for the
//! profile, its body saying who the user is and its front matter listing their files:
//!
//! ```markdown
//! ---
//! files:
//!   - {path: "/Users/me/cv.pdf", send: true}
//! ---
//! I build compilers and write in Rust.
//! ```

use std::{
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, ensure};
use serde::Deserialize;

pub use dusk_protocol::config::{ME, Profile, ProfileFile, Snippet, is_name};
use dusk_protocol::{
    config::is_name_char,
    project::{PathCompletion, PathEntry},
};

use super::graph::{read_text, split_front_matter};
use crate::{init::create_private_dir, util::atomic::write_private};

const SNIPPETS_DIR: &str = "snippets";
const ME_DIR: &str = "me";
const ME_FILE: &str = "ME.md";
const MAX_FILES: usize = 32;
/// What one snippet or the profile may hold; a mention is read in full by the model.
const MAX_TEXT_BYTES: usize = 64 * 1024;

/// The user's mentions under the rules folder.
pub struct Mentions {
    rules: PathBuf,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Front {
    #[serde(default)]
    files: Vec<FrontFile>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct FrontFile {
    path: PathBuf,
    #[serde(default)]
    send: bool,
}

impl Mentions {
    pub fn new(rules: &Path) -> Self {
        Self {
            rules: rules.to_owned(),
        }
    }

    fn snippets_dir(&self) -> PathBuf {
        self.rules.join(SNIPPETS_DIR)
    }

    fn snippet_file(&self, name: &str) -> Result<PathBuf> {
        ensure!(
            is_name(name) && name != ME,
            "a snippet name is letters, digits, - and _, and not {ME:?}"
        );
        Ok(self.snippets_dir().join(format!("{name}.md")))
    }

    /// Every snippet, by name. A file that cannot be read is left out.
    pub fn snippets(&self) -> Result<Vec<Snippet>> {
        let Ok(listing) = fs::read_dir(self.snippets_dir()) else {
            return Ok(Vec::new());
        };
        let mut snippets: Vec<Snippet> = listing
            .flatten()
            .filter_map(|entry| {
                let file = entry.file_name();
                let name = file.to_str()?.strip_suffix(".md")?;
                if !is_name(name) || name == ME {
                    return None;
                }
                let text = read_text(&entry.path()).ok()?;
                Some(Snippet {
                    name: name.to_owned(),
                    text,
                })
            })
            .collect();
        snippets.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(snippets)
    }

    pub fn snippet(&self, name: &str) -> Option<String> {
        read_text(&self.snippet_file(name).ok()?).ok()
    }

    /// Writes the snippet; empty text removes it.
    pub fn save_snippet(&self, name: &str, text: &str) -> Result<()> {
        let file = self.snippet_file(name)?;
        if text.trim().is_empty() {
            return match fs::remove_file(&file) {
                Err(error) if error.kind() != std::io::ErrorKind::NotFound => {
                    Err(error).context("cannot remove the snippet")
                }
                _ => Ok(()),
            };
        }
        ensure!(text.len() <= MAX_TEXT_BYTES, "a snippet is at most 64 KiB");
        create_private_dir(&self.snippets_dir())?;
        write_private(&file, text.as_bytes())
    }

    /// The profile; none written yet is an empty one.
    pub fn profile(&self) -> Result<Profile> {
        let file = self.rules.join(ME_DIR).join(ME_FILE);
        if !file.exists() {
            return Ok(Profile::default());
        }
        let text = read_text(&file)?;
        let Ok((front, body)) = split_front_matter(&text) else {
            return Ok(Profile {
                about: text,
                files: Vec::new(),
            });
        };
        let front: Front =
            serde_saphyr::from_str(front).context("cannot read ME.md front matter")?;
        Ok(Profile {
            about: body.trim_start_matches('\n').to_owned(),
            files: front
                .files
                .into_iter()
                .map(|file| ProfileFile {
                    path: file.path,
                    send: file.send,
                })
                .collect(),
        })
    }

    pub fn save_profile(&self, profile: &Profile) -> Result<()> {
        ensure!(
            profile.files.len() <= MAX_FILES,
            "a profile lists at most {MAX_FILES} files"
        );
        ensure!(
            profile.about.len() <= MAX_TEXT_BYTES,
            "the profile is at most 64 KiB"
        );
        let mut text = String::from("---\nfiles:\n");
        if profile.files.is_empty() {
            text = String::from("---\nfiles: []\n");
        }
        for file in &profile.files {
            ensure!(
                file.path.is_absolute(),
                "a profile file needs its full path"
            );
            // A JSON string is a YAML scalar, so any path is written safely.
            let path = serde_json::to_string(&file.path.to_string_lossy())?;
            text.push_str(&format!("  - {{path: {path}, send: {}}}\n", file.send));
        }
        text.push_str("---\n");
        text.push_str(&profile.about);
        let folder = self.rules.join(ME_DIR);
        create_private_dir(&folder)?;
        write_private(&folder.join(ME_FILE), text.as_bytes())
    }
}

/// What a typed `@word` may go on with: `me` and the names of the snippets.
pub fn complete(rules: &Path, word: &str) -> PathCompletion {
    let Some(typed) = word
        .strip_prefix('@')
        .filter(|typed| typed.chars().all(is_name_char))
    else {
        return PathCompletion::default();
    };
    let snippets = Mentions::new(rules).snippets().unwrap_or_default();
    let names = std::iter::once(ME.to_owned()).chain(snippets.into_iter().map(|s| s.name));
    PathCompletion {
        entries: names
            .filter(|name| name.starts_with(typed) && name != typed)
            .map(|name| PathEntry {
                name,
                directory: false,
            })
            .collect(),
        partial: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testing::temp_dir;

    #[test]
    fn a_snippet_name_never_leaves_its_folder() {
        let mentions = Mentions::new(&temp_dir("mentions_names"));
        for name in ["../x", "a/b", "", ".hidden", "me"] {
            assert!(mentions.save_snippet(name, "text").is_err(), "{name}");
        }
        mentions.save_snippet("video_1", "Script").unwrap();
        assert_eq!(mentions.snippet("video_1").as_deref(), Some("Script"));
        mentions.save_snippet("video_1", " ").unwrap();
        assert!(mentions.snippets().unwrap().is_empty());
    }

    #[test]
    fn a_profile_reads_back_as_written() {
        let mentions = Mentions::new(&temp_dir("mentions_profile"));
        assert_eq!(mentions.profile().unwrap(), Profile::default());
        let profile = Profile {
            about: "I write \"Rust\".\n---\nnot front matter".into(),
            files: vec![ProfileFile {
                path: PathBuf::from("/tmp/a \"b\": c.pdf"),
                send: true,
            }],
        };
        mentions.save_profile(&profile).unwrap();
        assert_eq!(mentions.profile().unwrap(), profile);
        let relative = Profile {
            about: String::new(),
            files: vec![ProfileFile {
                path: PathBuf::from("cv.pdf"),
                send: false,
            }],
        };
        assert!(mentions.save_profile(&relative).is_err());
    }
}
