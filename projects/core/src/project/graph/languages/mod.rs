//! The languages the graph reads. A language is a grammar linked into the binary, one
//! query file and the few functions of its `Spec`. The query speaks in the captures
//! `Role` names, so the parse itself knows no language. To add one: its file, its query,
//! a variant of `Language` and a line in `LANGUAGES`.

mod rust;

use std::sync::OnceLock;

use anyhow::{Context, Result, anyhow};
use tree_sitter::{Node, Query};

use super::outline::SymbolKind;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Language {
    Rust,
}

/// What a language brings.
pub struct Spec {
    pub language: Language,
    /// File extensions, without the dot.
    pub extensions: &'static [&'static str],
    pub grammar: fn() -> tree_sitter::Language,
    pub query: &'static str,
    /// Whether a file of this name holds tests. A folder named `tests` says so for
    /// every language.
    pub test_file: fn(&str) -> bool,
    /// The module a file is, from its path.
    pub module: fn(&str) -> String,
    /// The bytes that open a string able to run to the end of the file. One of them in
    /// an `@error` means a string was never closed.
    pub quotes: &'static [u8],
    /// The source with the text of its strings and comments replaced by spaces, byte for
    /// byte. The parser is handed this and never the file, so nothing written in a string
    /// or a comment can come out as a name, however the parser recovers from an error.
    pub blank: fn(&[u8]) -> Vec<u8>,
    /// Gives every name an `@import` node brings in.
    pub imports: fn(Node, &[u8], ImportSink),
}

/// Takes one imported name: the node it is written at, the name, its path as written.
pub type ImportSink<'a> = &'a mut dyn FnMut(Node, &str, &str);

/// What a capture of a query means.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    /// `@name`: the name of the definition captured in the same pattern.
    Name,
    /// `@def.function`, `.type`, `.trait`, `.constant`, `.module`, `.test`: a definition,
    /// from its first line to its last. A function inside a `Type` scope is a method.
    Def(SymbolKind),
    /// `@scope.type`, `.module`, `.code`: a body whose definitions have it as parent.
    Scope(ScopeKind),
    /// `@scope.name`: the name of the scope captured in the same pattern.
    ScopeName,
    /// `@use`: a name the file uses.
    Use,
    /// `@import`: an import, handed to `Spec::imports`.
    Import,
    /// `@error`: text the parser could not read.
    Error,
    /// `@_anything`: for the query's own predicates.
    Private,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScopeKind {
    /// An impl, a trait, a class: its functions are methods.
    Type,
    Module,
    /// A function body: what is defined inside has no parent.
    Code,
}

/// A language ready to parse: built once, on first use.
pub struct Loaded {
    pub spec: &'static Spec,
    pub grammar: tree_sitter::Language,
    pub query: Query,
    /// The role of each capture, by its index in the query.
    pub roles: Vec<Role>,
}

struct Entry {
    spec: &'static Spec,
    loaded: OnceLock<Result<Loaded, String>>,
}

static LANGUAGES: [Entry; 1] = [Entry {
    spec: &rust::SPEC,
    loaded: OnceLock::new(),
}];

impl Language {
    /// The language of a file, by its extension; `None` for a file the graph does not read.
    pub fn of(path: &str) -> Option<Self> {
        let (_, extension) = file_name(path).rsplit_once('.')?;
        let entry = LANGUAGES
            .iter()
            .find(|entry| entry.spec.extensions.contains(&extension))?;
        Some(entry.spec.language)
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Rust => "rust",
        }
    }

    /// The language the store spelled `text`; `None` for one this build does not read,
    /// as a row written by a later version.
    pub fn parse(text: &str) -> Option<Self> {
        LANGUAGES
            .iter()
            .map(|entry| entry.spec.language)
            .find(|language| language.as_str() == text)
    }

    pub(super) fn load(self) -> Result<&'static Loaded> {
        let entry = LANGUAGES
            .iter()
            .find(|entry| entry.spec.language == self)
            .with_context(|| format!("{} is not in the list of languages", self.as_str()))?;
        entry
            .loaded
            .get_or_init(|| load(entry.spec).map_err(|error| format!("{error:#}")))
            .as_ref()
            .map_err(|error| anyhow!("cannot load {}: {error}", self.as_str()))
    }
}

impl Loaded {
    pub fn is_test(&self, path: &str) -> bool {
        let (folders, name) = path.rsplit_once('/').unwrap_or(("", path));
        folders.split('/').any(|folder| folder == "tests") || (self.spec.test_file)(name)
    }
}

fn load(spec: &'static Spec) -> Result<Loaded> {
    let grammar = (spec.grammar)();
    let query = Query::new(&grammar, spec.query).map_err(|error| anyhow!("its query: {error}"))?;
    let roles = query
        .capture_names()
        .iter()
        .map(|capture| role(capture).with_context(|| format!("its query captures @{capture}")))
        .collect::<Result<_>>()?;
    Ok(Loaded {
        spec,
        grammar,
        query,
        roles,
    })
}

fn role(capture: &str) -> Option<Role> {
    Some(match capture {
        "name" => Role::Name,
        "def.function" => Role::Def(SymbolKind::Function),
        "def.type" => Role::Def(SymbolKind::Type),
        "def.trait" => Role::Def(SymbolKind::Trait),
        "def.constant" => Role::Def(SymbolKind::Constant),
        "def.module" => Role::Def(SymbolKind::Module),
        "def.test" => Role::Def(SymbolKind::Test),
        "scope.type" => Role::Scope(ScopeKind::Type),
        "scope.module" => Role::Scope(ScopeKind::Module),
        "scope.code" => Role::Scope(ScopeKind::Code),
        "scope.name" => Role::ScopeName,
        "use" => Role::Use,
        "import" => Role::Import,
        "error" => Role::Error,
        _ if capture.starts_with('_') => Role::Private,
        _ => return None,
    })
}

/// The last part of a path whose parts are joined by `/`.
fn file_name(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(_, name)| name)
}
