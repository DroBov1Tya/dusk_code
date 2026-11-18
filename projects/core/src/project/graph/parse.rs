//! One file into its outline. Source is untrusted input: every step has a cap, the time
//! is bounded, the tree is walked by a query cursor and never by recursion of ours, and
//! nothing read from the file is trusted to be in range or to be text.

use std::{
    cell::Cell,
    collections::HashMap,
    ops::{ControlFlow, Range},
    time::{Duration, Instant},
};

use anyhow::{Context, Result};
use tree_sitter::{
    Node, ParseOptions, ParseState, Parser, Point, QueryCursor, QueryCursorOptions,
    QueryCursorState, StreamingIterator, Tree,
};

use super::{
    languages::{Language, Loaded, Role, ScopeKind},
    outline::{ImportPath, Name, Outline, ParsedFile, Ref, RefKind, Skipped, Symbol, SymbolKind},
};

/// A larger file is `too_large`; the indexer reads no more than this.
pub const MAX_FILE_BYTES: usize = 512 * 1024;
/// A longer line is a minified bundle or data, not source: `long_lines`.
const MAX_LINE_BYTES: usize = 5_000;
/// The parse and the walk of one file together; past it the file is `too_slow`.
const BUDGET: Duration = Duration::from_millis(250);
/// More definitions make the file `too_many`.
const MAX_SYMBOLS: usize = 2_000;
/// Impls and bodies are not symbols, so they have their cap; past it, `too_many`.
const MAX_SCOPES: usize = 2 * MAX_SYMBOLS;
/// Distinct names a file refers to, imports and uses together; the rest are dropped.
const MAX_REFS: usize = 4_000;
/// Matches the cursor keeps in flight (the most it takes); the rest are dropped.
const MATCH_LIMIT: u32 = 65_536;
/// Levels of the syntax tree that are read; what is nested deeper is dropped. Source
/// written by a person stays under a hundred, and the cursor's work at a node grows with
/// its depth, so without this a file of open brackets holds it for seconds between two
/// looks at the clock.
const MAX_DEPTH: u32 = 500;
/// Uses looked at before they are counted by name; the rest are dropped. A file under
/// the size cap has no more names than this, only the same name captured twice.
const MAX_MENTIONS: usize = MAX_FILE_BYTES / 2;

/// The outline of `source`, the file at `path` (relative, parts joined by `/`). A file
/// past a cap is `Skipped`, a file with syntax errors gives what parsed, and no input
/// panics. An error is ours: the language could not be loaded, or the parser gave nothing.
pub fn parse(language: Language, path: &str, source: &[u8]) -> Result<ParsedFile> {
    let deadline = Instant::now() + BUDGET;
    parse_until(language, path, source, &|| Instant::now() >= deadline)
}

type Late<'a> = &'a dyn Fn() -> bool;

/// `parse` with the clock handed in: `late` says whether the time is up.
pub(super) fn parse_until(
    language: Language,
    path: &str,
    source: &[u8],
    late: Late,
) -> Result<ParsedFile> {
    let loaded = language.load()?;
    let outline = match outline(loaded, source, late) {
        Ok(outline) => Ok(outline),
        Err(Stop::Skipped(skipped)) => Err(skipped),
        Err(Stop::Failed(error)) => return Err(error.context(format!("cannot parse {path}"))),
    };
    Ok(ParsedFile {
        language,
        module: (loaded.spec.module)(path),
        is_test: loaded.is_test(path),
        outline,
    })
}

/// Why a parse ended early: a cap, which is an answer, or a failure of ours.
enum Stop {
    Skipped(Skipped),
    Failed(anyhow::Error),
}

impl From<Skipped> for Stop {
    fn from(skipped: Skipped) -> Self {
        Self::Skipped(skipped)
    }
}

impl From<anyhow::Error> for Stop {
    fn from(error: anyhow::Error) -> Self {
        Self::Failed(error)
    }
}

fn outline(loaded: &Loaded, source: &[u8], late: Late) -> Result<Outline, Stop> {
    if source.len() > MAX_FILE_BYTES {
        return Err(Skipped::TooLarge.into());
    }
    if source
        .split(|byte| *byte == b'\n')
        .any(|line| line.len() > MAX_LINE_BYTES)
    {
        return Err(Skipped::LongLines.into());
    }
    // From here on nothing reads the file itself: a string or a comment the parser took
    // for code has no text left to give.
    let source = &(loaded.spec.blank)(source);
    let tree = tree(loaded, source, late)?;
    let mut found = Found::default();
    found.collect(loaded, &tree, source, late)?;
    Ok(found.outline(source))
}

fn in_time(late: Late) -> Result<(), Stop> {
    if late() {
        Err(Skipped::TooSlow.into())
    } else {
        Ok(())
    }
}

fn tree(loaded: &Loaded, source: &[u8], late: Late) -> Result<Tree, Stop> {
    // A parser of its own for each file: one that was stopped would resume the old file.
    let mut parser = Parser::new();
    parser
        .set_language(&loaded.grammar)
        .context("the grammar does not fit the parser")?;
    let mut progress = |_: &ParseState| match in_time(late) {
        Ok(()) => ControlFlow::Continue(()),
        Err(_) => ControlFlow::Break(()),
    };
    let mut read = |offset: usize, _: Point| source.get(offset..).unwrap_or_default();
    let options = ParseOptions::new().progress_callback(&mut progress);
    let tree = parser.parse_with_options(&mut read, None, Some(options));
    // The parser asks about time only now and then, and a small file never: ask again.
    in_time(late)?;
    Ok(tree.context("the parser gave no tree")?)
}

/// What the query captured, as places in the file; names are read when all is known.
#[derive(Default)]
struct Found {
    defs: Vec<Def>,
    /// Where each definition is in `defs`, by its first byte: one node may match twice,
    /// as a function and as a test.
    def_at: HashMap<usize, usize>,
    scopes: Vec<Scope>,
    /// The same for `scopes`: one body may be captured with a name and without.
    scope_at: HashMap<usize, usize>,
    mentions: Vec<Mention>,
    imports: HashMap<Name, Import>,
    /// The bytes of import declarations: a name written there is not a use.
    import_ranges: Vec<Range<usize>>,
    /// Where a string was opened and never closed, if anywhere: the parser reads what
    /// follows as code, so nothing written from here on is kept.
    cut: Option<usize>,
}

struct Def {
    at: Range<usize>,
    name: Name,
    name_at: usize,
    kind: SymbolKind,
    line: u32,
    end_line: u32,
}

struct Scope {
    at: Range<usize>,
    name: Option<Name>,
    kind: ScopeKind,
}

struct Mention {
    at: Range<usize>,
    line: u32,
}

struct Import {
    at: usize,
    path: ImportPath,
    line: u32,
    uses: u32,
}

impl Found {
    fn collect(
        &mut self,
        loaded: &Loaded,
        tree: &Tree,
        source: &[u8],
        late: Late,
    ) -> Result<(), Stop> {
        let mut cursor = QueryCursor::new();
        cursor.set_match_limit(MATCH_LIMIT);
        cursor.set_max_start_depth(Some(MAX_DEPTH));
        let stopped = Cell::new(false);
        let mut progress = |_: &QueryCursorState| match in_time(late) {
            Ok(()) => ControlFlow::Continue(()),
            Err(_) => {
                stopped.set(true);
                ControlFlow::Break(())
            }
        };
        let options = QueryCursorOptions::new().progress_callback(&mut progress);
        let mut matches =
            cursor.matches_with_options(&loaded.query, tree.root_node(), source, options);
        while let Some(found) = matches.next() {
            in_time(late)?;
            let (mut name, mut def, mut scope, mut scope_name) = (None, None, None, None);
            for capture in found.captures() {
                let node = capture.node;
                match usize::try_from(capture.index)
                    .ok()
                    .and_then(|index| loaded.roles.get(index))
                {
                    Some(Role::Name) => name = Some(node),
                    Some(Role::Def(kind)) => def = Some((node, *kind)),
                    Some(Role::Scope(kind)) => scope = Some((node, *kind)),
                    Some(Role::ScopeName) => scope_name = Some(node),
                    Some(Role::Use) => self.mention(node),
                    Some(Role::Import) => self.import(loaded, node, source),
                    Some(Role::Error) => self.error(loaded, node, source),
                    Some(Role::Private) | None => {}
                }
            }
            if let (Some((node, kind)), Some(name)) = (def, name) {
                self.define(node, kind, name, source)?;
            }
            if let Some((node, kind)) = scope {
                self.scope(node, kind, scope_name, source)?;
            }
        }
        // A stopped cursor ends like one that reached the end of the file.
        if stopped.get() {
            return Err(Skipped::TooSlow.into());
        }
        Ok(())
    }

    fn define(
        &mut self,
        node: Node,
        kind: SymbolKind,
        name: Node,
        source: &[u8],
    ) -> Result<(), Stop> {
        if let Some(def) = self
            .def_at
            .get(&node.start_byte())
            .and_then(|index| self.defs.get_mut(*index))
        {
            // The mark of a test says more than the function it stands on.
            if kind == SymbolKind::Test {
                def.kind = kind;
            }
            return Ok(());
        }
        let Some(named) = text(name, source).and_then(Name::new) else {
            return Ok(());
        };
        if self.defs.len() >= MAX_SYMBOLS {
            return Err(Skipped::TooMany.into());
        }
        self.def_at.insert(node.start_byte(), self.defs.len());
        let line = line(node.start_position());
        self.defs.push(Def {
            at: node.byte_range(),
            name: named,
            name_at: name.start_byte(),
            kind,
            line,
            end_line: line.max(self::line(node.end_position())),
        });
        Ok(())
    }

    fn scope(
        &mut self,
        node: Node,
        kind: ScopeKind,
        name: Option<Node>,
        source: &[u8],
    ) -> Result<(), Stop> {
        let name = name.and_then(|name| text(name, source)).and_then(Name::new);
        if let Some(scope) = self
            .scope_at
            .get(&node.start_byte())
            .and_then(|index| self.scopes.get_mut(*index))
        {
            scope.name = scope.name.take().or(name);
            return Ok(());
        }
        if self.scopes.len() >= MAX_SCOPES {
            return Err(Skipped::TooMany.into());
        }
        self.scope_at.insert(node.start_byte(), self.scopes.len());
        self.scopes.push(Scope {
            at: node.byte_range(),
            name,
            kind,
        });
        Ok(())
    }

    fn mention(&mut self, node: Node) {
        if self.mentions.len() < MAX_MENTIONS {
            self.mentions.push(Mention {
                at: node.byte_range(),
                line: line(node.start_position()),
            });
        }
    }

    /// A quote the parser could not read as part of a string is one that opens a string
    /// with no end.
    fn error(&mut self, loaded: &Loaded, node: Node, source: &[u8]) {
        if self.cut.is_some_and(|cut| cut <= node.start_byte()) {
            return;
        }
        let quoted = source
            .get(node.byte_range())
            .is_some_and(|skipped| skipped.iter().any(|byte| loaded.spec.quotes.contains(byte)));
        if quoted {
            let cut = self
                .cut
                .map_or(node.start_byte(), |cut| cut.min(node.start_byte()));
            self.cut = Some(cut);
        }
    }

    fn import(&mut self, loaded: &Loaded, declaration: Node, source: &[u8]) {
        self.import_ranges.push(declaration.byte_range());
        (loaded.spec.imports)(declaration, source, &mut |node, name, path| {
            let line = line(node.start_position());
            if let Some(import) = self.imports.get_mut(name) {
                import.uses = import.uses.saturating_add(1);
                import.line = import.line.min(line);
                import.at = import.at.min(node.start_byte());
                return;
            }
            if self.imports.len() >= MAX_REFS {
                return;
            }
            if let (Some(name), Some(path)) = (Name::new(name), ImportPath::new(path)) {
                self.imports.insert(
                    name,
                    Import {
                        at: node.start_byte(),
                        path,
                        line,
                        uses: 1,
                    },
                );
            }
        });
    }

    fn outline(mut self, source: &[u8]) -> Outline {
        let cut = self.cut.unwrap_or(usize::MAX);
        self.defs.retain(|def| def.name_at < cut);
        self.imports.retain(|_, import| import.at < cut);
        let mut refs = self.uses(source, cut);
        refs.extend(self.imports.drain().map(|(name, import)| Ref {
            name,
            kind: RefKind::Import,
            path: Some(import.path),
            line: import.line,
            uses: import.uses,
        }));
        refs.sort_by(|a, b| {
            (a.line, &a.name, a.kind.as_str()).cmp(&(b.line, &b.name, b.kind.as_str()))
        });
        Outline {
            symbols: self.symbols(),
            refs,
        }
    }

    /// The uses by name: each place once, and not where a name is defined or imported.
    fn uses(&mut self, source: &[u8], cut: usize) -> Vec<Ref> {
        self.mentions.sort_by_key(|mention| mention.at.start);
        self.mentions.dedup_by_key(|mention| mention.at.start);
        self.import_ranges.sort_by_key(|range| range.start);
        let mut names_of_defs: Vec<usize> = self.defs.iter().map(|def| def.name_at).collect();
        names_of_defs.sort_unstable();

        let room = MAX_REFS.saturating_sub(self.imports.len());
        let mut uses: HashMap<Name, (u32, u32)> = HashMap::new();
        let mut imports = self.import_ranges.iter().peekable();
        for mention in &self.mentions {
            let start = mention.at.start;
            if start >= cut {
                break;
            }
            while imports.next_if(|range| range.end <= start).is_some() {}
            let imported = imports.peek().is_some_and(|range| range.start <= start);
            if imported || names_of_defs.binary_search(&start).is_ok() {
                continue;
            }
            let Some(name) = source
                .get(mention.at.clone())
                .and_then(|bytes| std::str::from_utf8(bytes).ok())
            else {
                continue;
            };
            if let Some((_, count)) = uses.get_mut(name) {
                *count = count.saturating_add(1);
            } else if uses.len() < room
                && let Some(name) = Name::new(name)
            {
                uses.insert(name, (mention.line, 1));
            }
        }
        uses.into_iter()
            .map(|(name, (line, uses))| Ref {
                name,
                kind: RefKind::Use,
                path: None,
                line,
                uses,
            })
            .collect()
    }

    /// The definitions in the order they are written, each with the scope it is in.
    /// Scopes nest, so one pass with a stack of the open ones finds the innermost.
    fn symbols(mut self) -> Vec<Symbol> {
        self.scopes.sort_by_key(|scope| scope.at.start);
        self.defs.sort_by_key(|def| def.at.start);

        let mut scopes = self.scopes.iter().peekable();
        let mut open: Vec<&Scope> = Vec::new();
        let mut symbols = Vec::with_capacity(self.defs.len());
        for def in self.defs {
            while let Some(scope) = scopes.next_if(|scope| scope.at.start <= def.at.start) {
                open.push(scope);
            }
            while open.last().is_some_and(|scope| scope.at.end < def.at.end) {
                open.pop();
            }
            let inside = open.last();
            let kind = match (def.kind, inside.map(|scope| scope.kind)) {
                (SymbolKind::Function, Some(ScopeKind::Type)) => SymbolKind::Method,
                (kind, _) => kind,
            };
            let parent = inside
                .filter(|scope| scope.kind != ScopeKind::Code)
                .and_then(|scope| scope.name.clone());
            symbols.push(Symbol {
                name: def.name,
                parent,
                kind,
                line: def.line,
                end_line: def.end_line,
            });
        }
        symbols
    }
}

fn text<'a>(node: Node, source: &'a [u8]) -> Option<&'a str> {
    std::str::from_utf8(source.get(node.byte_range())?).ok()
}

/// A point's line, counted from 1.
fn line(point: Point) -> u32 {
    u32::try_from(point.row).map_or(u32::MAX, |row| row.saturating_add(1))
}
