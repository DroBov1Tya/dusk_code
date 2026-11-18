//! What a question gives back: rows from the store, with how sure each one is, where it
//! was capped and what a miss knows.

use std::collections::HashSet;

use crate::{
    project::graph::{Language, Skipped},
    state::graph::{ChangedWith, Definition, Dependency, Dependent, GraphFile, TestFile},
};

/// An answer and the files in it found changed on disk since they were indexed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Reply {
    pub answer: Answer,
    pub changed: HashSet<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answer {
    /// `where`: at most `WHERE_ROWS` of `total` definitions.
    Definitions {
        name: String,
        rows: Vec<Definition>,
        total: u64,
    },
    /// No definition, but manifests that ask for a dependency of the name.
    Dependencies {
        name: String,
        rows: Vec<Dependency>,
    },
    Uses(Uses),
    Dependents(Dependents),
    /// `tests`; no rows when no file of tests was found.
    Tests {
        subject: String,
        /// The subject is a file of the index.
        file: bool,
        rows: Vec<TestFile>,
    },
    /// `where` of a file.
    File(GraphFile),
    /// The file went past a cap, so what it defines is not known.
    NotParsed {
        path: String,
        why: Skipped,
    },
    TooCommon {
        name: String,
        definitions: u64,
    },
    Miss(Miss),
}

/// `uses`: files that refer to the name, most uses first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Uses {
    pub name: String,
    /// The definition the rows are judged against: the first `where` gives.
    pub defined: Option<Definition>,
    /// The other definitions a guessed row may mean.
    pub others: Vec<Definition>,
    /// Files that use the name; `rows` shows the first `USES_ROWS`.
    pub total: u64,
    pub rows: Vec<Usage>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Usage {
    pub path: String,
    pub line: u32,
    pub uses: u32,
    /// Matched by name only: the file may mean another definition.
    pub guess: bool,
}

/// `touches`: the files that use what a file defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependents {
    pub path: String,
    /// Files found, tests among them; `capped` when the query stopped at its limit.
    pub total: usize,
    pub capped: bool,
    /// Files of code, the first `TOUCHES_ROWS`.
    pub code: Vec<Dependent>,
    /// Files of tests, all found.
    pub tests: Vec<String>,
    pub changed_with: Vec<ChangedWith>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Miss {
    pub name: String,
    pub missing: Missing,
    pub folder: Option<String>,
    /// Indexed files of the project, and their languages.
    pub files: u64,
    pub languages: Vec<Language>,
    /// Defined names that come closest to the asked one.
    pub nearest: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Missing {
    /// No symbol of the name.
    Definition,
    /// A file was asked for and the name is none of the index; a symbol of that name may
    /// be defined somewhere.
    File { defined_in: Option<Definition> },
}
