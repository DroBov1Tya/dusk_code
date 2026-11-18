//! Rust: what its files are called, and how a `use` splits into the names it imports.

mod blank;
#[cfg(test)]
mod blank_tests;

use std::rc::Rc;

use tree_sitter::Node;

use super::{ImportSink, Language, Spec};
use crate::project::graph::outline::MAX_IMPORT_PATH_CHARS;

pub const SPEC: Spec = Spec {
    language: Language::Rust,
    extensions: &["rs"],
    grammar: || tree_sitter_rust::LANGUAGE.into(),
    query: include_str!("rust.scm"),
    test_file,
    module,
    quotes: b"\"",
    blank: blank::blank,
    imports,
};

fn test_file(name: &str) -> bool {
    name == "tests.rs" || name.ends_with("_tests.rs")
}

/// `core/src/project/graph/mod.rs` is `project::graph`: the folders under the last `src`
/// and the file, without the names that stand for their folder.
fn module(path: &str) -> String {
    let path = path.strip_suffix(".rs").unwrap_or(path);
    let mut parts: Vec<&str> = path.split('/').collect();
    if let Some(src) = parts.iter().rposition(|part| *part == "src") {
        parts.drain(..=src);
    }
    if matches!(parts.last(), Some(&("mod" | "lib" | "main"))) {
        parts.pop();
    }
    parts.join("::")
}

/// Every name a `use` or an `extern crate` brings in, with its whole path: `a::{b, c::D}`
/// gives `b` as `a::b` and `D` as `a::c::D`. A glob gives the module it opens, with
/// `::*`. The tree is walked with a stack of our own, so a deep `use` cannot overflow.
fn imports(declaration: Node, source: &[u8], found: ImportSink) {
    // Each node waits with the path written before the list it is in.
    let mut waiting: Vec<(Node, Rc<str>)> = vec![(declaration, Rc::from(""))];
    let mut cursor = declaration.walk();
    while let Some((node, prefix)) = waiting.pop() {
        let field = |name| Some((node.child_by_field_name(name)?, prefix.clone()));
        match node.kind() {
            "use_declaration" => waiting.extend(field("argument")),
            "extern_crate_declaration" => waiting.extend(field("name")),
            "use_as_clause" => waiting.extend(field("path")),
            "use_list" => {
                let items = node.named_children(&mut cursor);
                waiting.extend(items.map(|item| (item, prefix.clone())));
            }
            "scoped_use_list" => {
                let path = joined(&prefix, node.child_by_field_name("path"), source);
                let list = node.child_by_field_name("list");
                waiting.extend(path.zip(list).map(|(path, list)| (list, Rc::from(path))));
            }
            "use_wildcard" => {
                if let Some(path) = joined(&prefix, node.named_child(0), source) {
                    found(node, last_name(&path), &format!("{path}::*"));
                }
            }
            "identifier" | "scoped_identifier" | "crate" | "self" | "super" => {
                if let Some(path) = joined(&prefix, Some(node), source) {
                    // `a::{self}` imports `a`.
                    let path = path.strip_suffix("::self").unwrap_or(&path);
                    found(node, last_name(path), path);
                }
            }
            _ => {}
        }
    }
}

/// `prefix` and the text of `part` as one path, without spaces. `None` when it is longer
/// than an import path may be: nothing under such a prefix is kept, so prefixes stay short
/// however deep the lists go.
fn joined(prefix: &str, part: Option<Node>, source: &[u8]) -> Option<String> {
    let part = match part {
        Some(node) => std::str::from_utf8(source.get(node.byte_range())?).ok()?,
        None => "",
    };
    if prefix.len() + part.len() > MAX_IMPORT_PATH_CHARS {
        return None;
    }
    let mut path = String::from(prefix);
    if !prefix.is_empty() && !part.is_empty() {
        path.push_str("::");
    }
    path.extend(part.chars().filter(|c| !c.is_whitespace()));
    Some(path)
}

fn last_name(path: &str) -> &str {
    path.rsplit_once("::").map_or(path, |(_, name)| name)
}
