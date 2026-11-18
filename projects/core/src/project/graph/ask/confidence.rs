//! How sure a row of `uses` is. Names are matched as text, so two things of one name are
//! told apart only by the imports of the file that uses one.

use std::collections::HashSet;

use super::Usage;
use crate::state::graph::{Definition, Import, Use};

/// Marks the rows that may mean another thing of the same name. With one definition every
/// row is taken as its use. With several, a row is sure when its file defines the name
/// or imports the first definition: by a path ending in its module, or by its parent.
pub(super) fn judged(
    name: &str,
    definitions: &[Definition],
    imports: &[Import],
    rows: Vec<Use>,
) -> Vec<Usage> {
    let defining: HashSet<&str> = definitions.iter().map(|d| d.path.as_str()).collect();
    let first = definitions.first().filter(|_| definitions.len() > 1);
    rows.into_iter()
        .map(|row| {
            let guess = first.is_some_and(|first| {
                !defining.contains(row.path.as_str())
                    && !imports
                        .iter()
                        .filter(|import| import.file_id == row.file_id)
                        .any(|import| leads_to(import, name, first))
            });
            Usage {
                path: row.path,
                line: row.line,
                uses: row.uses,
                guess,
            }
        })
        .collect()
}

/// Whether an import brings in the definition: it names the definition's parent, or its
/// path (without `::*`, or without the name itself) ends in the definition's module.
fn leads_to(import: &Import, name: &str, definition: &Definition) -> bool {
    if definition.parent.as_deref() == Some(import.name.as_str()) {
        return true;
    }
    let Some(path) = import.path.as_deref() else {
        return false;
    };
    let path = match path.strip_suffix("::*") {
        Some(module) => module,
        None if import.name == name => path
            .strip_suffix(name)
            .and_then(|rest| rest.strip_suffix("::"))
            .unwrap_or(path),
        None => path,
    };
    let module = definition.module.as_str();
    !module.is_empty()
        && (path == module
            || path
                .strip_suffix(module)
                .is_some_and(|rest| rest.ends_with("::")))
}
