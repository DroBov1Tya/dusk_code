//! The project graph's store (`docs/project-intelligence.md` sections 2 and 4): for each
//! source file of a saved project, the symbols it defines and the names it refers to.
//! Edges are stored by name and resolved when asked, so indexing one file rewrites that
//! file's rows and nothing else. Every row is derived from the folder and can be built
//! again. The answers here are rows; their wording is the tool's.

// The indexer (task G3) and the `project_graph` tool (task G4) call this; until they
// land only the tests do.
#![allow(dead_code)]

use std::collections::HashMap;

use anyhow::{Context, Result, ensure};
use serde_json::json;
use sqlx::{Row, sqlite::SqliteRow};

use crate::state::State;

/// Rows `graph_where` and `graph_dependencies` return: 10 to show, one to say there are more.
pub const WHERE_LIMIT: i64 = 11;
/// Rows `graph_uses` returns: 15 to show, one to say there are more.
pub const USES_LIMIT: i64 = 16;
/// Files `graph_touches` returns; the answer shows the first 8 and counts the rest.
pub const TOUCHES_LIMIT: i64 = 40;
/// Files `graph_changed_with` returns.
pub const CHANGED_WITH_LIMIT: i64 = 3;
/// Files `graph_tests_of_file` and `graph_tests_of_symbol` return.
pub const TESTS_LIMIT: i64 = 6;
/// Names `graph_nearest_names` returns.
pub const NEAREST_LIMIT: usize = 3;
/// A name defined in more files than this (`new`, `default`) says nothing about which
/// file depends on which, so `touches` and `tests` leave it out.
pub const COMMON_NAME_FILES: i64 = 3;
/// Files whose imports `graph_imports` reads at once: the rows of one `uses` answer.
pub const IMPORTS_FILES: usize = 16;
/// Distinct names `graph_nearest_names` compares; a miss is not worth a longer scan.
const NEAREST_CANDIDATES: i64 = 20_000;

/// Declares a kind stored as text, with the spellings of its table's CHECK list.
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

stored_kind!(
    /// The grammar a file was parsed with; `Manifest` is `Cargo.toml`, `package.json` or
    /// `pyproject.toml`.
    Language {
        Rust = "rust",
        Typescript = "typescript",
        Tsx = "tsx",
        Javascript = "javascript",
        Python = "python",
        Manifest = "manifest",
    }
);

stored_kind!(
    /// Why a file has no symbols: it went past a cap.
    Skipped {
        TooLarge = "too_large",
        TooSlow = "too_slow",
        TooMany = "too_many",
        LongLines = "long_lines",
    }
);

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

stored_kind!(
    /// What a file's reference to a name is: a use in its code, an import (with the path
    /// as written) or a dependency of a manifest (with the version asked for).
    RefKind {
        Use = "use",
        Import = "import",
        Dependency = "dependency",
    }
);

/// When a file was last seen as it is: equal stamps mean the file is not opened again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stamp {
    pub size: u64,
    pub modified_ms: i64,
}

/// A file as the indexer read it, with everything it defines and refers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    /// Relative to the project, `/` between parts.
    pub path: String,
    pub language: Language,
    /// SHA-256 in hex, as the change ledger's `hash_after`.
    pub hash: String,
    pub stamp: Stamp,
    pub module: String,
    pub is_test: bool,
    /// Set when the file went past a cap; it then has no symbols.
    pub skipped: Option<Skipped>,
    pub symbols: Vec<NewSymbol>,
    /// One per kind and name; a repeat is added to the first.
    pub refs: Vec<NewRef>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSymbol {
    pub name: String,
    /// The type, impl, class or module the symbol is written in.
    pub parent: Option<String>,
    pub kind: SymbolKind,
    pub line: u32,
    pub end_line: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewRef {
    pub kind: RefKind,
    pub name: String,
    /// The first line it is referred to on.
    pub line: u32,
    /// How many times.
    pub uses: u32,
    /// For an import the path as written; for a dependency the version asked for.
    pub path: Option<String>,
}

/// A stored file, without its symbols.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GraphFile {
    pub path: String,
    pub language: Language,
    pub hash: String,
    pub stamp: Stamp,
    pub module: String,
    pub is_test: bool,
    pub skipped: Option<Skipped>,
}

/// What a refresh compares a file on disk against.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredStamp {
    pub hash: String,
    pub stamp: Stamp,
}

/// A project's index in numbers, for its status.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GraphCounts {
    pub files: u64,
    pub symbols: u64,
    /// Files that went past a cap.
    pub skipped: u64,
}

/// A row of `where`: one definition of a name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Definition {
    pub kind: SymbolKind,
    pub parent: Option<String>,
    pub path: String,
    /// The defining file's module, which an import's path is matched against.
    pub module: String,
    pub line: u32,
    pub end_line: u32,
}

/// A manifest that asks for a dependency of the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependency {
    pub path: String,
    pub line: u32,
    pub version: Option<String>,
}

/// A row of `uses`: a file that refers to the name.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Use {
    pub file_id: i64,
    pub path: String,
    /// The first use, else the import.
    pub line: u32,
    /// Uses in code, else the import's count.
    pub uses: u32,
}

/// An import of one of the files of a `uses` answer, to tell two things of one name apart.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Import {
    pub file_id: i64,
    pub name: String,
    pub path: Option<String>,
}

/// A row of `touches`: a file that uses names the asked file defines.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Dependent {
    pub path: String,
    pub is_test: bool,
    /// The names it uses, sorted.
    pub names: Vec<String>,
}

/// A file changed in the same turns as the asked one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChangedWith {
    pub path: String,
    /// Turns that changed both; 2 or more.
    pub times: u32,
}

/// A row of `tests`: a file of tests that covers the asked file or symbol.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TestFile {
    pub path: String,
    /// The tests are written in the asked file, or in a file that defines the symbol.
    pub own: bool,
    /// Its symbols of kind `test`.
    pub tests: u32,
    /// The names it uses, sorted.
    pub names: Vec<String>,
}

/// The path filter of every question: a folder becomes the prefix `folder/`, matched with
/// `substr` so `%` and `_` in it are plain characters. An empty folder is no filter.
fn under(folder: Option<&str>) -> Option<String> {
    let folder = folder?.trim_end_matches('/');
    (!folder.is_empty()).then(|| format!("{folder}/"))
}

/// The names `touches` and `tests` follow: those the file `?2` of project `?1` defines,
/// apart from tests and names defined in more than `COMMON_NAME_FILES` files.
macro_rules! defined_names {
    () => {
        "target AS (SELECT file_id FROM graph_files WHERE project = ?1 AND path = ?2),
        defined AS (
            SELECT DISTINCT s.name FROM graph_symbols s
            WHERE s.file_id = (SELECT file_id FROM target) AND s.kind != 'test'
                AND (SELECT COUNT(DISTINCT o.file_id)
                     FROM graph_symbols o JOIN graph_files g ON g.file_id = o.file_id
                     WHERE o.name = s.name AND g.project = ?1) <= ?4
        ),
        dependents AS (
            SELECT f.file_id, f.path, f.is_test, COUNT(DISTINCT r.name) AS names,
                json_group_array(DISTINCT r.name) AS used
            FROM defined d
            JOIN graph_refs r ON r.name = d.name AND r.kind IN ('use', 'import')
            JOIN graph_files f ON f.file_id = r.file_id
            WHERE f.project = ?1 AND r.file_id != (SELECT file_id FROM target)
                AND (?3 IS NULL OR substr(f.path, 1, length(?3)) = ?3)
            GROUP BY f.file_id
        )"
    };
}

impl State {
    /// Replaces the rows of each file, all or none: the file row is written in place
    /// (its id kept), its symbols and refs are deleted and the new ones inserted.
    pub async fn replace_graph_files(&self, project: &str, files: &[IndexedFile]) -> Result<()> {
        let mut transaction = self.pool.begin().await.context("begin the graph batch")?;
        for file in files {
            let file_id: i64 = sqlx::query_scalar(
                "INSERT INTO graph_files
                    (project, path, language, hash, size, modified_ms, module, is_test, skipped)
                 VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)
                 ON CONFLICT (project, path) DO UPDATE SET language = excluded.language,
                    hash = excluded.hash, size = excluded.size,
                    modified_ms = excluded.modified_ms, module = excluded.module,
                    is_test = excluded.is_test, skipped = excluded.skipped
                 RETURNING file_id",
            )
            .bind(project)
            .bind(&file.path)
            .bind(file.language.as_str())
            .bind(&file.hash)
            .bind(size(file.stamp.size)?)
            .bind(file.stamp.modified_ms)
            .bind(&file.module)
            .bind(file.is_test)
            .bind(file.skipped.map(Skipped::as_str))
            .fetch_one(&mut *transaction)
            .await
            .with_context(|| format!("index {:?}", file.path))?;
            for clear in [
                "DELETE FROM graph_symbols WHERE file_id = ?",
                "DELETE FROM graph_refs WHERE file_id = ?",
            ] {
                sqlx::query(clear)
                    .bind(file_id)
                    .execute(&mut *transaction)
                    .await
                    .with_context(|| format!("clear the rows of {:?}", file.path))?;
            }
            // One statement per table, the rows as one bound JSON array: a file of
            // thousands of names is not thousands of round trips.
            let symbols: Vec<_> = file
                .symbols
                .iter()
                .map(|symbol| {
                    json!({
                        "name": symbol.name, "parent": symbol.parent, "kind": symbol.kind.as_str(),
                        "line": symbol.line, "end_line": symbol.end_line,
                    })
                })
                .collect();
            sqlx::query(
                "INSERT INTO graph_symbols (file_id, name, parent, kind, line, end_line)
                 SELECT ?, value ->> 'name', value ->> 'parent', value ->> 'kind',
                    value ->> 'line', value ->> 'end_line'
                 FROM json_each(?)",
            )
            .bind(file_id)
            .bind(serde_json::Value::from(symbols).to_string())
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("store the symbols of {:?}", file.path))?;
            let refs: Vec<_> = file
                .refs
                .iter()
                .map(|reference| {
                    json!({
                        "kind": reference.kind.as_str(), "name": reference.name,
                        "line": reference.line, "uses": reference.uses, "path": reference.path,
                    })
                })
                .collect();
            // `WHERE true` keeps SQLite from reading the upsert's ON as a join's.
            sqlx::query(
                "INSERT INTO graph_refs (file_id, kind, name, line, uses, path)
                 SELECT ?, value ->> 'kind', value ->> 'name', value ->> 'line',
                    value ->> 'uses', value ->> 'path'
                 FROM json_each(?) WHERE true
                 ON CONFLICT (file_id, kind, name) DO UPDATE SET
                    line = min(line, excluded.line), uses = uses + excluded.uses",
            )
            .bind(file_id)
            .bind(serde_json::Value::from(refs).to_string())
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("store the refs of {:?}", file.path))?;
        }
        transaction.commit().await.context("commit the graph batch")
    }

    /// Records new stamps of files whose text is the one indexed; nothing else changes.
    pub async fn restamp_graph_files(
        &self,
        project: &str,
        stamps: &[(String, Stamp)],
    ) -> Result<()> {
        let mut transaction = self.pool.begin().await.context("begin the stamps")?;
        for (path, stamp) in stamps {
            sqlx::query(
                "UPDATE graph_files SET size = ?, modified_ms = ? WHERE project = ? AND path = ?",
            )
            .bind(size(stamp.size)?)
            .bind(stamp.modified_ms)
            .bind(project)
            .bind(path)
            .execute(&mut *transaction)
            .await
            .with_context(|| format!("restamp {path:?}"))?;
        }
        transaction.commit().await.context("commit the stamps")
    }

    /// Removes files that are gone; their symbols and refs go with them. Returns how many
    /// were stored.
    pub async fn remove_graph_files(&self, project: &str, paths: &[String]) -> Result<u64> {
        let paths = serde_json::to_string(paths).context("encode the paths")?;
        let done = sqlx::query(
            "DELETE FROM graph_files
             WHERE project = ? AND path IN (SELECT value FROM json_each(?))",
        )
        .bind(project)
        .bind(paths)
        .execute(&self.pool)
        .await
        .context("remove graph files")?;
        Ok(done.rows_affected())
    }

    /// Removes a project's whole index, as when it grew past the caps.
    pub async fn remove_graph(&self, project: &str) -> Result<()> {
        sqlx::query("DELETE FROM graph_files WHERE project = ?")
            .bind(project)
            .execute(&self.pool)
            .await
            .context("remove the project graph")?;
        Ok(())
    }

    /// The hash and stamp of every indexed file of the project, by path.
    pub async fn graph_stamps(&self, project: &str) -> Result<HashMap<String, StoredStamp>> {
        sqlx::query("SELECT path, hash, size, modified_ms FROM graph_files WHERE project = ?")
            .bind(project)
            .try_map(|row: SqliteRow| {
                let path: String = row.try_get("path")?;
                let stored = StoredStamp {
                    hash: row.try_get("hash")?,
                    stamp: stamp(&row)?,
                };
                Ok((path, stored))
            })
            .fetch_all(&self.pool)
            .await
            .context("read the graph stamps")
            .map(|rows| rows.into_iter().collect())
    }

    /// One indexed file, when the project has it.
    pub async fn graph_file(&self, project: &str, path: &str) -> Result<Option<GraphFile>> {
        sqlx::query("SELECT * FROM graph_files WHERE project = ? AND path = ?")
            .bind(project)
            .bind(path)
            .try_map(|row: SqliteRow| {
                Ok(GraphFile {
                    path: row.try_get("path")?,
                    language: stored(&row, "language", Language::parse)?,
                    hash: row.try_get("hash")?,
                    stamp: stamp(&row)?,
                    module: row.try_get("module")?,
                    is_test: row.try_get("is_test")?,
                    skipped: row
                        .try_get::<Option<String>, _>("skipped")?
                        .map(|text| known(&text, Skipped::parse))
                        .transpose()?,
                })
            })
            .fetch_optional(&self.pool)
            .await
            .context("read a graph file")
    }

    pub async fn graph_counts(&self, project: &str) -> Result<GraphCounts> {
        let row = sqlx::query(
            "SELECT COUNT(*) AS files, COALESCE(SUM(skipped IS NOT NULL), 0) AS skipped,
                (SELECT COUNT(*) FROM graph_symbols s JOIN graph_files f USING (file_id)
                 WHERE f.project = ?1) AS symbols
             FROM graph_files WHERE project = ?1",
        )
        .bind(project)
        .fetch_one(&self.pool)
        .await
        .context("count the graph")?;
        let count = |column: &str| -> Result<u64> {
            u64::try_from(row.try_get::<i64, _>(column)?).context("negative count")
        };
        Ok(GraphCounts {
            files: count("files")?,
            symbols: count("symbols")?,
            skipped: count("skipped")?,
        })
    }

    /// `where`: the definitions of `name`, inside `parent` when given, files of code
    /// before files of tests. At most `WHERE_LIMIT` rows.
    pub async fn graph_where(
        &self,
        project: &str,
        name: &str,
        parent: Option<&str>,
        folder: Option<&str>,
    ) -> Result<Vec<Definition>> {
        sqlx::query(
            "SELECT s.kind, s.parent, f.path, f.module, s.line, s.end_line
             FROM graph_symbols s JOIN graph_files f USING (file_id)
             WHERE f.project = ?1 AND s.name = ?2 AND (?3 IS NULL OR s.parent = ?3)
                AND (?4 IS NULL OR substr(f.path, 1, length(?4)) = ?4)
             ORDER BY f.is_test, f.path, s.line LIMIT ?5",
        )
        .bind(project)
        .bind(name)
        .bind(parent)
        .bind(under(folder))
        .bind(WHERE_LIMIT)
        .try_map(|row: SqliteRow| {
            Ok(Definition {
                kind: stored(&row, "kind", SymbolKind::parse)?,
                parent: row.try_get("parent")?,
                path: row.try_get("path")?,
                module: row.try_get("module")?,
                line: number(&row, "line")?,
                end_line: number(&row, "end_line")?,
            })
        })
        .fetch_all(&self.pool)
        .await
        .context("find the definitions")
    }

    /// For a name with no definition: the manifests that ask for a dependency of that name.
    /// At most `WHERE_LIMIT` rows.
    pub async fn graph_dependencies(&self, project: &str, name: &str) -> Result<Vec<Dependency>> {
        sqlx::query(
            "SELECT f.path, r.line, r.path AS version
             FROM graph_refs r JOIN graph_files f USING (file_id)
             WHERE f.project = ?1 AND r.name = ?2 AND r.kind = 'dependency'
             ORDER BY f.path LIMIT ?3",
        )
        .bind(project)
        .bind(name)
        .bind(WHERE_LIMIT)
        .try_map(|row: SqliteRow| {
            Ok(Dependency {
                path: row.try_get("path")?,
                line: number(&row, "line")?,
                version: row.try_get("version")?,
            })
        })
        .fetch_all(&self.pool)
        .await
        .context("find the dependencies")
    }

    /// For a name with no definition: the defined names that share the longest prefix
    /// with it, ASCII case ignored. At most `NEAREST_LIMIT`.
    pub async fn graph_nearest_names(&self, project: &str, name: &str) -> Result<Vec<String>> {
        let Some(first) = name.chars().next() else {
            return Ok(Vec::new());
        };
        let candidates: Vec<String> = sqlx::query_scalar(
            "SELECT DISTINCT s.name FROM graph_symbols s JOIN graph_files f USING (file_id)
             WHERE f.project = ?1 AND s.name != ?2 AND lower(substr(s.name, 1, 1)) = lower(?3)
             ORDER BY s.name LIMIT ?4",
        )
        .bind(project)
        .bind(name)
        .bind(first.to_string())
        .bind(NEAREST_CANDIDATES)
        .fetch_all(&self.pool)
        .await
        .context("find names near the asked one")?;
        Ok(nearest(name, candidates))
    }

    /// `uses`: the files that refer to `name`, one row each, most uses first. At most
    /// `USES_LIMIT` rows.
    pub async fn graph_uses(
        &self,
        project: &str,
        name: &str,
        folder: Option<&str>,
    ) -> Result<Vec<Use>> {
        sqlx::query(
            "SELECT f.file_id, f.path,
                COALESCE(MIN(CASE WHEN r.kind = 'use' THEN r.line END), MIN(r.line)) AS line,
                COALESCE(SUM(CASE WHEN r.kind = 'use' THEN r.uses END), SUM(r.uses)) AS uses
             FROM graph_refs r JOIN graph_files f USING (file_id)
             WHERE f.project = ?1 AND r.name = ?2 AND r.kind IN ('use', 'import')
                AND (?3 IS NULL OR substr(f.path, 1, length(?3)) = ?3)
             GROUP BY f.file_id ORDER BY uses DESC, f.path LIMIT ?4",
        )
        .bind(project)
        .bind(name)
        .bind(under(folder))
        .bind(USES_LIMIT)
        .try_map(|row: SqliteRow| {
            Ok(Use {
                file_id: row.try_get("file_id")?,
                path: row.try_get("path")?,
                line: number(&row, "line")?,
                uses: number(&row, "uses")?,
            })
        })
        .fetch_all(&self.pool)
        .await
        .context("find the uses")
    }

    /// The imports of the files of a `uses` answer, at most `IMPORTS_FILES` of them, kept
    /// to the project.
    pub async fn graph_imports(&self, project: &str, file_ids: &[i64]) -> Result<Vec<Import>> {
        ensure!(
            file_ids.len() <= IMPORTS_FILES,
            "at most {IMPORTS_FILES} files at once"
        );
        let file_ids = serde_json::to_string(file_ids).context("encode the file ids")?;
        sqlx::query(
            "SELECT r.file_id, r.name, r.path
             FROM graph_refs r JOIN graph_files f USING (file_id)
             WHERE f.project = ? AND r.kind = 'import'
                AND r.file_id IN (SELECT value FROM json_each(?))
             ORDER BY r.file_id, r.name",
        )
        .bind(project)
        .bind(file_ids)
        .try_map(|row: SqliteRow| {
            Ok(Import {
                file_id: row.try_get("file_id")?,
                name: row.try_get("name")?,
                path: row.try_get("path")?,
            })
        })
        .fetch_all(&self.pool)
        .await
        .context("read the imports")
    }

    /// `touches`: the other files that use what the file at `path` defines, most names
    /// first. At most `TOUCHES_LIMIT` rows; none when the file is not indexed.
    pub async fn graph_touches(
        &self,
        project: &str,
        path: &str,
        folder: Option<&str>,
    ) -> Result<Vec<Dependent>> {
        sqlx::query(concat!(
            "WITH ",
            defined_names!(),
            " SELECT path, is_test, used FROM dependents ORDER BY names DESC, path LIMIT ?5"
        ))
        .bind(project)
        .bind(path)
        .bind(under(folder))
        .bind(COMMON_NAME_FILES)
        .bind(TOUCHES_LIMIT)
        .try_map(|row: SqliteRow| {
            Ok(Dependent {
                path: row.try_get("path")?,
                is_test: row.try_get("is_test")?,
                names: names(&row)?,
            })
        })
        .fetch_all(&self.pool)
        .await
        .context("find the files that depend on it")
    }

    /// "Changed with it": files the ledger saw changed in the same turn as `path` in two
    /// turns or more, most often first. At most `CHANGED_WITH_LIMIT` rows.
    pub async fn graph_changed_with(&self, project: &str, path: &str) -> Result<Vec<ChangedWith>> {
        sqlx::query(
            "SELECT b.path, COUNT(DISTINCT a.turn_id) AS times
             FROM changes a JOIN changes b ON b.turn_id = a.turn_id AND b.project = a.project
             WHERE a.project = ?1 AND a.path = ?2 AND b.path != a.path
             GROUP BY b.path HAVING times >= 2 ORDER BY times DESC, b.path LIMIT ?3",
        )
        .bind(project)
        .bind(path)
        .bind(CHANGED_WITH_LIMIT)
        .try_map(|row: SqliteRow| {
            Ok(ChangedWith {
                path: row.try_get("path")?,
                times: number(&row, "times")?,
            })
        })
        .fetch_all(&self.pool)
        .await
        .context("find the files changed with it")
    }

    /// `tests` of a file: the file itself when it holds tests, then the files of tests
    /// that use what it defines, most names first. At most `TESTS_LIMIT` rows.
    pub async fn graph_tests_of_file(
        &self,
        project: &str,
        path: &str,
        folder: Option<&str>,
    ) -> Result<Vec<TestFile>> {
        sqlx::query(concat!(
            "WITH ",
            defined_names!(),
            ",
            tested AS (
                SELECT file_id, path, is_test, 0 AS own, names, used FROM dependents
                UNION ALL
                SELECT f.file_id, f.path, f.is_test, 1, 0, '[]' FROM graph_files f
                WHERE f.file_id = (SELECT file_id FROM target)
                    AND (?3 IS NULL OR substr(f.path, 1, length(?3)) = ?3)
            )
            SELECT t.path, t.own, t.used,
                (SELECT COUNT(*) FROM graph_symbols x
                 WHERE x.file_id = t.file_id AND x.kind = 'test') AS tests
            FROM tested t
            WHERE t.is_test = 1 OR EXISTS (SELECT 1 FROM graph_symbols x
                WHERE x.file_id = t.file_id AND x.kind = 'test')
            ORDER BY t.own DESC, t.names DESC, t.path LIMIT ?5"
        ))
        .bind(project)
        .bind(path)
        .bind(under(folder))
        .bind(COMMON_NAME_FILES)
        .bind(TESTS_LIMIT)
        .try_map(|row: SqliteRow| {
            Ok(TestFile {
                path: row.try_get("path")?,
                own: row.try_get("own")?,
                tests: number(&row, "tests")?,
                names: names(&row)?,
            })
        })
        .fetch_all(&self.pool)
        .await
        .context("find the tests of the file")
    }

    /// `tests` of a symbol: files of tests that define or use `name`, those that define
    /// it first, then most uses. At most `TESTS_LIMIT` rows.
    pub async fn graph_tests_of_symbol(
        &self,
        project: &str,
        name: &str,
        folder: Option<&str>,
    ) -> Result<Vec<TestFile>> {
        sqlx::query(
            "SELECT f.path,
                EXISTS (SELECT 1 FROM graph_symbols d
                    WHERE d.file_id = f.file_id AND d.name = ?2 AND d.kind != 'test') AS own,
                (SELECT COUNT(*) FROM graph_symbols x
                    WHERE x.file_id = f.file_id AND x.kind = 'test') AS tests,
                (SELECT COALESCE(SUM(r.uses), 0) FROM graph_refs r
                    WHERE r.file_id = f.file_id AND r.name = ?2
                        AND r.kind IN ('use', 'import')) AS uses
             FROM graph_files f
             WHERE f.project = ?1
                AND f.file_id IN (
                    SELECT r.file_id FROM graph_refs r
                    WHERE r.name = ?2 AND r.kind IN ('use', 'import')
                    UNION
                    SELECT s.file_id FROM graph_symbols s WHERE s.name = ?2 AND s.kind != 'test')
                AND (f.is_test = 1 OR EXISTS (SELECT 1 FROM graph_symbols x
                    WHERE x.file_id = f.file_id AND x.kind = 'test'))
                AND (?3 IS NULL OR substr(f.path, 1, length(?3)) = ?3)
             ORDER BY own DESC, uses DESC, f.path LIMIT ?4",
        )
        .bind(project)
        .bind(name)
        .bind(under(folder))
        .bind(TESTS_LIMIT)
        .try_map(|row: SqliteRow| {
            let uses: i64 = row.try_get("uses")?;
            Ok(TestFile {
                path: row.try_get("path")?,
                own: row.try_get("own")?,
                tests: number(&row, "tests")?,
                names: if uses > 0 {
                    vec![name.to_owned()]
                } else {
                    Vec::new()
                },
            })
        })
        .fetch_all(&self.pool)
        .await
        .context("find the tests of the symbol")
    }
}

fn size(bytes: u64) -> Result<i64> {
    i64::try_from(bytes).context("file size is too large")
}

fn decode_error(message: String) -> sqlx::Error {
    sqlx::Error::Decode(message.into())
}

fn known<T>(text: &str, parse: fn(&str) -> Option<T>) -> sqlx::Result<T> {
    parse(text).ok_or_else(|| decode_error(format!("unknown graph value {text:?}")))
}

fn stored<T>(row: &SqliteRow, column: &str, parse: fn(&str) -> Option<T>) -> sqlx::Result<T> {
    known(row.try_get::<&str, _>(column)?, parse)
}

fn number(row: &SqliteRow, column: &str) -> sqlx::Result<u32> {
    let value: i64 = row.try_get(column)?;
    u32::try_from(value).map_err(|_| decode_error(format!("{column} out of range: {value}")))
}

fn stamp(row: &SqliteRow) -> sqlx::Result<Stamp> {
    let size: i64 = row.try_get("size")?;
    Ok(Stamp {
        size: u64::try_from(size).map_err(|_| decode_error(format!("negative size {size}")))?,
        modified_ms: row.try_get("modified_ms")?,
    })
}

/// The `used` column, a JSON array of names, sorted.
fn names(row: &SqliteRow) -> sqlx::Result<Vec<String>> {
    let mut names: Vec<String> = serde_json::from_str(row.try_get::<&str, _>("used")?)
        .map_err(|error| decode_error(format!("used names: {error}")))?;
    names.sort();
    Ok(names)
}

/// The candidates sharing the longest prefix with `name`, ASCII case ignored; ties go
/// to the closest length, then by name.
fn nearest(name: &str, candidates: Vec<String>) -> Vec<String> {
    let shared = |candidate: &str| {
        name.chars()
            .zip(candidate.chars())
            .take_while(|(a, b)| a.eq_ignore_ascii_case(b))
            .count()
    };
    let mut ranked: Vec<_> = candidates
        .into_iter()
        .map(|candidate| {
            let rank = (
                std::cmp::Reverse(shared(&candidate)),
                candidate.len().abs_diff(name.len()),
            );
            (rank, candidate)
        })
        .collect();
    ranked.sort();
    ranked
        .into_iter()
        .take(NEAREST_LIMIT)
        .map(|(_, candidate)| candidate)
        .collect()
}

#[cfg(test)]
#[path = "graph_tests.rs"]
mod tests;
