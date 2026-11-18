//! The four questions of the project graph:
//! from the store's rows to an answer that says which rows are guesses, where it was
//! capped, what a miss knows, and which of its files changed since they were indexed.
//! The wording is the tool's. The name asked about is data: it is matched against the
//! store and never opens a file.

mod answer;
mod confidence;
mod stale;

use std::path::Path;

use anyhow::{Context, Result, ensure};

use crate::state::{
    State,
    graph::{GraphFile, TOUCHES_LIMIT},
};

pub use answer::{Answer, Dependents, Miss, Missing, Reply, Usage, Uses};
// Tests stamp their fixtures as the disk does.
#[cfg(test)]
pub use stale::on_disk;

/// A name with more definitions than this says nothing by itself (`len`, `new`).
pub const MAX_DEFINITIONS: u64 = 30;
/// Rows each answer shows.
pub const WHERE_ROWS: usize = 10;
pub const USES_ROWS: usize = 15;
pub const TOUCHES_ROWS: usize = 8;
/// Files of an answer compared with the disk, one look each.
pub const MAX_CHECKED: usize = 16;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Question {
    /// Where the name is defined.
    Where,
    /// Which files use it.
    Uses,
    /// Which files use what a file defines.
    Touches,
    /// Which files of tests cover a file or a symbol.
    Tests,
}

/// One question about one project.
#[derive(Debug, Clone, Copy)]
pub struct Asked<'a> {
    pub question: Question,
    /// A symbol, `Type::member`, `Type.member` or a file's path, as the model wrote it.
    pub name: &'a str,
    /// Only files under this folder: relative to the project, `/` between parts.
    pub folder: Option<&'a str>,
}

/// What the model named.
enum Subject {
    File(GraphFile),
    /// A path the index does not have.
    NoFile(String),
    Symbol {
        name: String,
        parent: Option<String>,
    },
}

/// Answers one question from the index of the project at `root` (canonical, as saved),
/// which has `files` indexed files.
pub async fn ask(state: &State, root: &Path, files: u64, asked: Asked<'_>) -> Result<Reply> {
    let project = root.to_str().context("the folder name is not UTF-8")?;
    let asking = Asking {
        state,
        project,
        files,
        asked,
        shown: asked.name.trim(),
    };
    let answer = match subject(state, project, root, asked.name).await? {
        Subject::File(file) => asking.about_file(file).await?,
        Subject::NoFile(path) => Answer::Miss(
            asking
                .miss(&path, Missing::File { defined_in: None }, None)
                .await?,
        ),
        Subject::Symbol { name, parent } => asking.about_symbol(&name, parent).await?,
    };
    let changed = stale::changed_since_indexed(state, project, root, answer.files()).await?;
    Ok(Reply { answer, changed })
}

struct Asking<'a> {
    state: &'a State,
    project: &'a str,
    files: u64,
    asked: Asked<'a>,
    /// The name as asked, for the answer to repeat.
    shown: &'a str,
}

impl Asking<'_> {
    async fn about_file(&self, file: GraphFile) -> Result<Answer> {
        let (state, project, folder) = (self.state, self.project, self.asked.folder);
        if let (Some(why), Question::Uses | Question::Touches | Question::Tests) =
            (file.skipped, self.asked.question)
        {
            return Ok(Answer::NotParsed {
                path: file.path,
                why,
            });
        }
        Ok(match self.asked.question {
            Question::Where => Answer::File(file),
            Question::Uses | Question::Touches => {
                let rows = state.graph_touches(project, &file.path, folder).await?;
                let changed_with = state.graph_changed_with(project, &file.path).await?;
                let total = rows.len();
                let (tests, mut code): (Vec<_>, Vec<_>) =
                    rows.into_iter().partition(|row| row.is_test);
                code.truncate(TOUCHES_ROWS);
                Answer::Dependents(Dependents {
                    path: file.path,
                    total,
                    capped: i64::try_from(total).is_ok_and(|total| total >= TOUCHES_LIMIT),
                    code,
                    tests: tests.into_iter().map(|row| row.path).collect(),
                    changed_with,
                })
            }
            Question::Tests => Answer::Tests {
                rows: state
                    .graph_tests_of_file(project, &file.path, folder)
                    .await?,
                subject: file.path,
                file: true,
            },
        })
    }

    async fn about_symbol(&self, name: &str, parent: Option<String>) -> Result<Answer> {
        let (state, project, folder) = (self.state, self.project, self.asked.folder);
        // A path such as `config::Limits` names a module, not the type a symbol is
        // written in: with nothing inside such a parent, the name alone is asked.
        let parent = match parent {
            Some(parent)
                if state
                    .graph_name_counts(project, name, Some(&parent), None)
                    .await?
                    .definitions
                    > 0 =>
            {
                Some(parent)
            }
            _ => None,
        };
        let parent = parent.as_deref();
        let counts = state
            .graph_name_counts(project, name, parent, folder)
            .await?;
        if counts.definitions > MAX_DEFINITIONS {
            return Ok(Answer::TooCommon {
                name: self.shown.to_owned(),
                definitions: counts.definitions,
            });
        }
        match self.asked.question {
            Question::Where => {
                let mut rows = state.graph_where(project, name, parent, folder).await?;
                if rows.is_empty() {
                    return self.no_definition(name).await;
                }
                rows.truncate(WHERE_ROWS);
                Ok(Answer::Definitions {
                    name: self.shown.to_owned(),
                    rows,
                    total: counts.definitions,
                })
            }
            Question::Uses => {
                // Judged against every definition in the project, not only those under
                // the folder: a file uses what is defined anywhere.
                let definitions = state.graph_where(project, name, parent, None).await?;
                let mut rows = state.graph_uses(project, name, folder).await?;
                if rows.is_empty() && definitions.is_empty() {
                    return self.no_definition(name).await;
                }
                rows.truncate(USES_ROWS);
                let ids: Vec<i64> = rows.iter().map(|row| row.file_id).collect();
                let imports = if definitions.len() > 1 {
                    state.graph_imports(project, &ids).await?
                } else {
                    Vec::new()
                };
                let rows = confidence::judged(name, &definitions, &imports, rows);
                let mut definitions = definitions.into_iter();
                Ok(Answer::Uses(Uses {
                    name: self.shown.to_owned(),
                    defined: definitions.next(),
                    others: definitions.collect(),
                    total: counts.users,
                    rows,
                }))
            }
            Question::Touches => {
                let defined_in = state
                    .graph_where(project, name, parent, None)
                    .await?
                    .into_iter()
                    .next();
                let missing = Missing::File { defined_in };
                Ok(Answer::Miss(self.miss(self.shown, missing, None).await?))
            }
            Question::Tests => {
                let rows = state.graph_tests_of_symbol(project, name, folder).await?;
                if rows.is_empty() && counts.definitions == 0 {
                    return self.no_definition(name).await;
                }
                Ok(Answer::Tests {
                    subject: self.shown.to_owned(),
                    file: false,
                    rows,
                })
            }
        }
    }

    /// No symbol of the name: a dependency of that name, else the nearest names.
    async fn no_definition(&self, name: &str) -> Result<Answer> {
        let mut rows = self.state.graph_dependencies(self.project, name).await?;
        if !rows.is_empty() {
            rows.truncate(WHERE_ROWS);
            return Ok(Answer::Dependencies {
                name: self.shown.to_owned(),
                rows,
            });
        }
        let miss = self
            .miss(self.shown, Missing::Definition, Some(name))
            .await?;
        Ok(Answer::Miss(miss))
    }

    async fn miss(&self, name: &str, missing: Missing, near: Option<&str>) -> Result<Miss> {
        let nearest = match near {
            Some(near) => self.state.graph_nearest_names(self.project, near).await?,
            None => Vec::new(),
        };
        Ok(Miss {
            name: name.to_owned(),
            missing,
            folder: self.asked.folder.map(str::to_owned),
            files: self.files,
            languages: self.state.graph_languages(self.project).await?,
            nearest,
        })
    }
}

/// Reads what the model named: the path of an indexed file is that file, a path the
/// index does not have is a miss, and anything else a symbol, split at its last `::` or
/// `.` into the type it is written in and its own name.
async fn subject(state: &State, project: &str, root: &Path, name: &str) -> Result<Subject> {
    let name = name.trim();
    ensure!(!name.is_empty(), "`name` is empty");
    let path = relative(root, name);
    if path.contains('/') || path.contains('.') {
        if let Some(file) = state.graph_file(project, path).await? {
            return Ok(Subject::File(file));
        }
        if path.contains('/') {
            return Ok(Subject::NoFile(path.to_owned()));
        }
    }
    let (parent, member) = if let Some((parent, member)) = name.rsplit_once("::") {
        (parent.rsplit("::").next(), member)
    } else if let Some((parent, member)) = name.rsplit_once('.') {
        (parent.rsplit('.').next(), member)
    } else {
        (None, name)
    };
    ensure!(
        !member.is_empty(),
        "`name` ends with a separator; give the member after it"
    );
    Ok(Subject::Symbol {
        name: member.to_owned(),
        parent: parent
            .filter(|parent| !parent.is_empty())
            .map(str::to_owned),
    })
}

/// `name` relative to the project when it is written from its root or with `./`. Text
/// only: nothing on disk is looked at.
fn relative<'a>(root: &Path, name: &'a str) -> &'a str {
    let name = name.strip_prefix("./").unwrap_or(name);
    root.to_str()
        .and_then(|root| name.strip_prefix(root))
        .and_then(|rest| rest.strip_prefix('/'))
        .unwrap_or(name)
}
