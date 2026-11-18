//! What a parse gives: the names a file defines and the names it refers to. The shapes
//! follow the tables of the graph (`graph_files`, `graph_symbols`, `graph_refs`), and the
//! bounds those tables check are held by the types here.

use std::borrow::Borrow;

use super::languages::Language;

/// The longest name kept, in characters.
pub const MAX_NAME_CHARS: usize = 128;
/// The longest import path kept, in characters.
pub const MAX_IMPORT_PATH_CHARS: usize = 200;

/// Declares a kind the store keeps as text. The spellings are those of the CHECK lists of
/// the graph's migration; the store writes `as_str` and reads back with `parse`, so the
/// kinds are declared once, here.
macro_rules! stored_kind {
    ($(#[$doc:meta])* $name:ident { $($variant:ident = $text:literal),+ $(,)? }) => {
        $(#[$doc])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum $name {
            $($variant),+
        }

        impl $name {
            pub const ALL: &[Self] = &[$(Self::$variant),+];

            pub fn as_str(self) -> &'static str {
                match self {
                    $(Self::$variant => $text),+
                }
            }

            pub fn parse(text: &str) -> Option<Self> {
                Self::ALL.iter().copied().find(|kind| kind.as_str() == text)
            }
        }
    };
}

/// One parsed file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedFile {
    pub language: Language,
    /// The module the file is, derived from its path; empty for the root of a crate.
    pub module: String,
    /// Whether the path says the file holds tests.
    pub is_test: bool,
    /// What the file holds, or the cap it went past: such a file has no symbols.
    pub outline: Result<Outline, Skipped>,
}

/// The symbols in the order they are written, and the refs by first line.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Outline {
    pub symbols: Vec<Symbol>,
    pub refs: Vec<Ref>,
}

stored_kind!(
    /// Why a file has no symbols: it went past a cap.
    Skipped {
        TooLarge = "too_large",
        TooSlow = "too_slow",
        TooMany = "too_many",
        LongLines = "long_lines",
    }
);

/// A definition. Lines count from 1 and `end_line` is never before `line`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Symbol {
    pub name: Name,
    /// The type, impl, trait or module the symbol is written in.
    pub parent: Option<Name>,
    pub kind: SymbolKind,
    pub line: u32,
    pub end_line: u32,
}

stored_kind!(
    /// What a symbol is. A `mod`, `namespace` or class body is a `Module`.
    SymbolKind {
        Function = "function",
        Method = "method",
        Type = "type",
        Trait = "trait",
        Constant = "constant",
        Module = "module",
        Test = "test",
    }
);

/// A name the file refers to: once per name and kind, with the first line and the count.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Ref {
    pub name: Name,
    pub kind: RefKind,
    /// The path as written, for an import.
    pub path: Option<ImportPath>,
    pub line: u32,
    pub uses: u32,
}

stored_kind!(
    /// What a file's reference to a name is: a use in its code, an import (with the path
    /// as written) or a dependency of a manifest (with the version asked for; no parser
    /// gives one until manifests are read, task G5c of the design).
    RefKind {
        Use = "use",
        Import = "import",
        Dependency = "dependency",
    }
);

/// An identifier: 1 to 128 letters, digits and `_`. A name can hold no space, quote or
/// control character, so text from a string or a comment cannot pass for one.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Name(String);

impl Name {
    pub fn new(text: &str) -> Option<Self> {
        let fits = bounded(text, MAX_NAME_CHARS, |c| c.is_alphanumeric() || c == '_');
        fits.then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

// Hashes and compares as its text, so a map of names is looked up by `&str`.
impl Borrow<str> for Name {
    fn borrow(&self) -> &str {
        &self.0
    }
}

/// An import path as written, without spaces: 1 to 200 characters of names and the
/// marks paths are joined with (`::`, `.`, `/`, `-`, `@`, `*`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportPath(String);

impl ImportPath {
    pub fn new(text: &str) -> Option<Self> {
        let fits = bounded(text, MAX_IMPORT_PATH_CHARS, |c| {
            c.is_alphanumeric() || matches!(c, '_' | ':' | '.' | '/' | '-' | '@' | '*')
        });
        fits.then(|| Self(text.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Whether `text` is 1 to `max` characters, each one `allowed`.
fn bounded(text: &str, max: usize, allowed: impl Fn(char) -> bool) -> bool {
    // A character is at most four bytes: a longer text is refused before it is walked.
    !text.is_empty()
        && text.len() <= max * 4
        && text.chars().count() <= max
        && text.chars().all(allowed)
}
