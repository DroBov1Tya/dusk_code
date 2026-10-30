//! Skills and agents, both read the same way. Any folder under `skills/` that holds a
//! `SKILL.md`, or under `agents/` that holds an `AGENT.md`, is a node named by its path,
//! such as `git/release-notes`. Nodes form a graph: `links` lead from one to others, and
//! entries are the ones offered first. Every node is an entry unless it says
//! `entry: false`, so skills written for other tools work as they are. A skill's text is
//! instructions the agent loads; an agent's text is the system prompt of the thread it
//! works in.
//!
//! ```markdown
//! ---
//! description: Preparing a release
//! links: [git/release-notes, git/changelog]
//! tools: [read_file, run_command]   # agents only: the tools it gets; all when left out
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

pub const SKILL_FILE: &str = "SKILL.md";
pub const AGENT_FILE: &str = "AGENT.md";
const MAX_NODE_BYTES: u64 = 256 * 1024;
const MAX_DESCRIPTION_CHARS: usize = 1024;
const MAX_DEPTH: usize = 8;
const MAX_NODES: usize = 1000;
const FENCE: &str = "---";

/// Every node found under one directory, with what is wrong with the rest.
#[derive(Debug, Default)]
pub struct Graph {
    nodes: BTreeMap<String, Node>,
    problems: Vec<Problem>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Node {
    /// The path from the graph's directory, with `/` between folders.
    pub name: String,
    /// One line, as the model reads it in a list.
    pub description: String,
    /// Offered to the model first.
    pub entry: bool,
    /// Nodes this one leads to, as written; see `Graph::links` for the valid ones.
    pub links: Vec<String>,
    /// For an agent, the tools it may use; `None` gives it every tool.
    pub tools: Option<Vec<String>>,
    file: PathBuf,
}

/// A node that could not be read, or a link to a node that does not exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Problem {
    pub node: String,
    pub reason: String,
}

/// Other front matter fields, such as `license`, are allowed and ignored.
#[derive(Deserialize)]
struct FrontMatter {
    name: Option<String>,
    description: String,
    #[serde(default = "yes")]
    entry: bool,
    #[serde(default)]
    links: Vec<String>,
    tools: Option<Vec<String>>,
}

impl Graph {
    /// Reads the tree of `file` nodes now, so new and edited ones apply at once. Symlinks
    /// are not followed; depth and size are bounded. A missing directory has no nodes.
    pub fn load(directory: &Path, file: &str) -> Self {
        let mut graph = Self::default();
        let mut pending = vec![(directory.to_owned(), Vec::<String>::new())];
        while let Some((folder, path)) = pending.pop() {
            if !path.is_empty() && folder.join(file).exists() {
                graph.add(&folder.join(file), path.join("/"));
            }
            if path.len() < MAX_DEPTH && graph.nodes.len() < MAX_NODES {
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

    pub fn get(&self, name: &str) -> Option<&Node> {
        self.nodes.get(name)
    }

    pub fn nodes(&self) -> impl Iterator<Item = &Node> {
        self.nodes.values()
    }

    pub fn entries(&self) -> impl Iterator<Item = &Node> {
        self.nodes().filter(|node| node.entry)
    }

    /// The existing nodes `node` leads to.
    pub fn links<'a>(&'a self, node: &'a Node) -> impl Iterator<Item = &'a Node> {
        node.links.iter().filter_map(|name| self.get(name))
    }

    pub fn problems(&self) -> &[Problem] {
        &self.problems
    }

    fn add(&mut self, file: &Path, name: String) {
        match read_node(file, &name) {
            Ok(node) => {
                self.nodes.insert(name, node);
            }
            Err(error) => self.problems.push(Problem {
                node: name,
                reason: format!("{error:#}"),
            }),
        }
    }

    fn check_links(&mut self) {
        for node in self.nodes.values() {
            for link in node
                .links
                .iter()
                .filter(|link| !self.nodes.contains_key(*link))
            {
                self.problems.push(Problem {
                    node: node.name.clone(),
                    reason: format!("links to {link}, which does not exist"),
                });
            }
        }
    }
}

impl Node {
    pub fn file(&self) -> &Path {
        &self.file
    }

    /// The text after the front matter, read now so edits apply at once.
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

fn read_node(file: &Path, name: &str) -> Result<Node> {
    let text = read_text(file)?;
    parse_node(&text, name, file.to_owned())
}

/// Whether `text` would read as the node `name`: its front matter and description.
pub fn check_node(text: &str, name: &str) -> Result<()> {
    ensure!(
        text.len() as u64 <= MAX_NODE_BYTES,
        "the text is over {MAX_NODE_BYTES} bytes"
    );
    parse_node(text, name, PathBuf::new()).map(drop)
}

fn parse_node(text: &str, name: &str, file: PathBuf) -> Result<Node> {
    let (front, _) = split_front_matter(text)?;
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
    Ok(Node {
        name: name.to_owned(),
        description,
        entry: front.entry,
        links: front.links,
        tools: front.tools,
        file,
    })
}

fn yes() -> bool {
    true
}

/// The file must be a regular file; a symlink is not followed.
pub fn read_text(file: &Path) -> Result<String> {
    let bytes = read_regular(file, MAX_NODE_BYTES).context("cannot read the file")?;
    String::from_utf8(bytes).map_err(|_| anyhow!("the file is not valid UTF-8 text"))
}

/// Splits `---\n<front>\n---\n<body>`.
pub fn split_front_matter(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix(FENCE)
        .and_then(|rest| {
            rest.strip_prefix('\n')
                .or_else(|| rest.strip_prefix("\r\n"))
        })
        .context("the file must start with a --- front matter line")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == FENCE {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(anyhow!("front matter has no closing --- line"))
}
