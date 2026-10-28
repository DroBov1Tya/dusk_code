//! Skills: instructions the agent loads when it needs them. Any folder under `skills/`
//! that holds a `SKILL.md` is a skill named by its path, such as `git/release-notes`.
//! Skills form a graph: `links` lead from one skill to others, and `entry` marks the
//! skills the model is offered first.
//!
//! ```markdown
//! ---
//! description: Preparing a release
//! entry: true
//! links: [git/release-notes, git/changelog]
//! ---
//! Steps the agent follows...
//! ```

use std::{
    collections::BTreeMap,
    fs,
    path::{Path, PathBuf},
};

use anyhow::{Context, Result, anyhow, ensure};
use serde::Deserialize;

use crate::{paths::is_plain_folder_name, tools::read_regular};

const SKILL_FILE: &str = "SKILL.md";
const MAX_SKILL_BYTES: u64 = 256 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 1024;
const MAX_DEPTH: usize = 8;
const MAX_SKILLS: usize = 1000;
const FENCE: &str = "---";

/// Every skill found under one directory, with what is wrong with the rest.
#[derive(Debug, Default)]
pub struct SkillGraph {
    skills: BTreeMap<String, Skill>,
    problems: Vec<SkillProblem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    /// The path from the skills directory, with `/` between folders.
    pub name: String,
    /// One line, as the model reads it in a skill list.
    pub description: String,
    /// Offered to the model before it has loaded any skill.
    pub entry: bool,
    /// Skills this one leads to, as written; see `SkillGraph::links` for the valid ones.
    pub links: Vec<String>,
    file: PathBuf,
}

/// A skill that could not be read, or a link to a skill that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SkillProblem {
    pub skill: String,
    pub reason: String,
}

/// Other front matter fields, such as `license`, are allowed and ignored.
#[derive(Deserialize)]
struct FrontMatter {
    name: Option<String>,
    description: String,
    #[serde(default)]
    entry: bool,
    #[serde(default)]
    links: Vec<String>,
}

impl SkillGraph {
    /// Reads the tree now, so new and edited skills apply at once. Symlinks are not
    /// followed; depth and size are bounded. A missing directory has no skills.
    pub fn load(directory: &Path) -> Self {
        let mut graph = Self::default();
        let mut pending = vec![(directory.to_owned(), Vec::<String>::new())];
        while let Some((folder, path)) = pending.pop() {
            if !path.is_empty() && folder.join(SKILL_FILE).exists() {
                graph.add(&folder, path.join("/"));
            }
            if path.len() < MAX_DEPTH && graph.skills.len() < MAX_SKILLS {
                pending.extend(subfolders(&folder).map(|name| {
                    let mut child = path.clone();
                    child.push(name.clone());
                    (folder.join(name), child)
                }));
            }
        }
        graph.check_links();
        graph
    }

    pub fn get(&self, name: &str) -> Option<&Skill> {
        self.skills.get(name)
    }

    pub fn skills(&self) -> impl Iterator<Item = &Skill> {
        self.skills.values()
    }

    pub fn entries(&self) -> impl Iterator<Item = &Skill> {
        self.skills().filter(|skill| skill.entry)
    }

    /// The existing skills `skill` leads to.
    pub fn links<'a>(&'a self, skill: &'a Skill) -> impl Iterator<Item = &'a Skill> {
        skill.links.iter().filter_map(|name| self.get(name))
    }

    pub fn problems(&self) -> &[SkillProblem] {
        &self.problems
    }

    fn add(&mut self, folder: &Path, name: String) {
        match read_skill(folder, &name) {
            Ok(skill) => {
                self.skills.insert(name, skill);
            }
            Err(error) => self.problems.push(SkillProblem {
                skill: name,
                reason: format!("{error:#}"),
            }),
        }
    }

    fn check_links(&mut self) {
        for skill in self.skills.values() {
            for link in skill
                .links
                .iter()
                .filter(|link| !self.skills.contains_key(*link))
            {
                self.problems.push(SkillProblem {
                    skill: skill.name.clone(),
                    reason: format!("links to {link}, which does not exist"),
                });
            }
        }
    }
}

impl Skill {
    /// The instructions after the front matter, read now so edits apply at once.
    pub fn instructions(&self) -> Result<String> {
        let text = read_text(&self.file)?;
        let (_, body) = split_front_matter(&text)?;
        Ok(body.trim().to_owned())
    }
}

/// Real folders with plain names, sorted in reverse so popping visits them in order.
fn subfolders(folder: &Path) -> impl Iterator<Item = String> {
    let mut names: Vec<String> = fs::read_dir(folder)
        .into_iter()
        .flatten()
        .filter_map(|entry| entry.ok())
        .filter(|entry| entry.file_type().is_ok_and(|kind| kind.is_dir()))
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| is_plain_folder_name(name))
        .collect();
    names.sort_by(|a, b| b.cmp(a));
    names.into_iter()
}

fn read_skill(folder: &Path, name: &str) -> Result<Skill> {
    let file = folder.join(SKILL_FILE);
    let text = read_text(&file)?;
    let (front, _) = split_front_matter(&text)?;
    let front: FrontMatter =
        serde_yaml::from_str(front).map_err(|_| anyhow!("front matter is not valid YAML"))?;
    if let Some(declared) = &front.name {
        let folder_name = name.rsplit('/').next().unwrap_or(name);
        ensure!(
            declared == name || declared == folder_name,
            "name {declared} differs from the path {name}"
        );
    }
    let description = front
        .description
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    ensure!(!description.is_empty(), "description is empty");
    ensure!(
        description.chars().count() <= MAX_DESCRIPTION_CHARS,
        "description is longer than {MAX_DESCRIPTION_CHARS} characters"
    );
    Ok(Skill {
        name: name.to_owned(),
        description,
        entry: front.entry,
        links: front.links,
        file,
    })
}

/// The skill file must be a regular file; a symlink is not followed.
fn read_text(file: &Path) -> Result<String> {
    let bytes = read_regular(file, MAX_SKILL_BYTES).context("cannot read SKILL.md")?;
    String::from_utf8(bytes).map_err(|_| anyhow!("SKILL.md is not valid UTF-8 text"))
}

/// Splits `---\n<front>\n---\n<body>`.
fn split_front_matter(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix(FENCE)
        .and_then(|rest| {
            rest.strip_prefix('\n')
                .or_else(|| rest.strip_prefix("\r\n"))
        })
        .context("SKILL.md must start with a --- front matter line")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == FENCE {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(anyhow!("front matter has no closing --- line"))
}
