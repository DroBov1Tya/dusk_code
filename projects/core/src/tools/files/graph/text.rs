//! The wording of `project_graph`'s answers, in the formats of design section 4, and the
//! caps every answer keeps. Paths and names come from the repository and the model: each
//! is made plain before it is written, so none can add a line or forge a note.

use crate::{
    project::graph::{
        Language, Skipped,
        ask::{Answer, Dependents, Miss, Missing, Reply, Uses},
    },
    state::graph::Definition,
};

/// Lines of rows an answer shows, at most.
pub(super) const MAX_ROWS: usize = 16;
pub(super) const MAX_LINE_CHARS: usize = 160;
pub(super) const MAX_ANSWER_BYTES: usize = 2048;
/// Names shown on a row of `touches` or `tests`; the rest are counted.
const NAMES_SHOWN: usize = 2;
/// Files of tests named on the `tests:` line of `touches`.
const TESTS_SHOWN: usize = 4;
/// Other definitions a guessed row of `uses` may mean, named on its last line.
const OTHERS_SHOWN: usize = 3;
const CHANGED: &str = " (changed since indexed)";
const SEARCH: &str = "For text, use search_files.";
pub(super) const CUT: char = '…';

pub(super) fn render(reply: &Reply) -> String {
    let mark = |path: &str| {
        if reply.changed.contains(path) {
            CHANGED
        } else {
            ""
        }
    };
    let lines = match &reply.answer {
        Answer::Definitions { name, rows, total } => {
            let mut lines = vec![format!(
                "{}: {}",
                plain(name),
                counted(*total, "definition")
            )];
            lines.extend(rows.iter().map(|row| {
                let span = if row.end_line > row.line {
                    format!("{}-{}", row.line, row.end_line)
                } else {
                    row.line.to_string()
                };
                let parent = row
                    .parent
                    .as_deref()
                    .map(|parent| format!(" in {}", plain(parent)))
                    .unwrap_or_default();
                let path = plain(&row.path);
                format!(
                    "{} {path}:{span}{parent}{}",
                    row.kind.as_str(),
                    mark(&row.path)
                )
            }));
            lines.extend(more(*total, rows.len()));
            lines
        }
        Answer::Dependencies { name, rows } => {
            let mut lines = vec![format!(
                "{}: no definition in the index; a dependency in {}",
                plain(name),
                counted(rows.len() as u64, "manifest")
            )];
            lines.extend(rows.iter().map(|row| {
                let version = row
                    .version
                    .as_deref()
                    .map(|version| format!(" {}", plain(version)))
                    .unwrap_or_default();
                format!(
                    "{}:{}{version}{}",
                    plain(&row.path),
                    row.line,
                    mark(&row.path)
                )
            }));
            lines
        }
        Answer::Uses(uses) => uses_lines(uses, &mark),
        Answer::Dependents(found) => dependents_lines(found, &mark),
        Answer::Tests {
            subject,
            file,
            rows,
        } => {
            let subject = format!(
                "{}{}",
                plain(subject),
                if *file { mark(subject) } else { "" }
            );
            if rows.is_empty() {
                vec![format!(
                    "{subject}: no file of tests uses it in the index. {SEARCH}"
                )]
            } else {
                let mut lines = vec![format!(
                    "{subject}: tests in {}",
                    counted(rows.len() as u64, "file")
                )];
                lines.extend(rows.iter().map(|row| {
                    let tests = match row.tests {
                        0 => "a file of tests".to_owned(),
                        tests => counted(u64::from(tests), "test"),
                    };
                    let names = if row.names.is_empty() {
                        String::new()
                    } else {
                        format!(", use {}", names(&row.names))
                    };
                    format!("{}{}: {tests}{names}", plain(&row.path), mark(&row.path))
                }));
                lines
            }
        }
        Answer::File(file) => {
            let module = if file.module.is_empty() {
                "the root module".to_owned()
            } else {
                format!("module {}", plain(&file.module))
            };
            let tests = if file.is_test {
                ", a file of tests"
            } else {
                ""
            };
            let skipped = file
                .skipped
                .map(|why| format!(", not parsed: {}", why_skipped(why)))
                .unwrap_or_default();
            vec![format!(
                "{}{}: an indexed {} file, {module}{tests}{skipped}. Ask `touches` or `tests` \
                 with it.",
                plain(&file.path),
                mark(&file.path),
                language(file.language)
            )]
        }
        Answer::NotParsed { path, why } => vec![format!(
            "{}{}: not parsed ({}), so what it defines is not known. {SEARCH}",
            plain(path),
            mark(path),
            why_skipped(*why)
        )],
        Answer::TooCommon { name, definitions } => vec![format!(
            "{}: too common ({definitions} definitions); give `Type::name` or a `path`.",
            plain(name)
        )],
        Answer::Miss(miss) => vec![miss_line(miss)],
    };
    finish(lines)
}

fn uses_lines(uses: &Uses, mark: &dyn Fn(&str) -> &'static str) -> Vec<String> {
    let name = plain(&uses.name);
    let defined = match &uses.defined {
        Some(defined) => format!(
            "{} {}:{}{}",
            defined.kind.as_str(),
            plain(&defined.path),
            defined.line,
            mark(&defined.path)
        ),
        None => "no definition in the index".to_owned(),
    };
    if uses.rows.is_empty() {
        return vec![format!("{name} ({defined}): no file uses it. {SEARCH}")];
    }
    let shown = uses.rows.len() as u64;
    let verb = if uses.total == 1 { "uses" } else { "use" };
    let shown_part = if uses.total > shown {
        format!(", {shown} shown")
    } else {
        String::new()
    };
    let mut lines = vec![format!(
        "{name} ({defined}): {} {verb} it{shown_part}, most uses first",
        counted(uses.total, "file")
    )];
    lines.extend(uses.rows.iter().map(|row| {
        let guess = if row.guess { "? " } else { "" };
        let times = if row.uses > 1 {
            format!(" x{}", row.uses)
        } else {
            String::new()
        };
        format!(
            "{guess}{}:{}{times}{}",
            plain(&row.path),
            row.line,
            mark(&row.path)
        )
    }));
    let mut last: Vec<String> = more(uses.total, uses.rows.len()).into_iter().collect();
    if uses.rows.iter().any(|row| row.guess) && !uses.others.is_empty() {
        let others: Vec<String> = uses.others.iter().take(OTHERS_SHOWN).map(located).collect();
        last.push(format!("`?`: the file may mean {}.", others.join(", ")));
    }
    if !last.is_empty() {
        lines.push(last.join(" "));
    }
    lines
}

fn dependents_lines(found: &Dependents, mark: &dyn Fn(&str) -> &'static str) -> Vec<String> {
    let path = format!("{}{}", plain(&found.path), mark(&found.path));
    let changed_with = (!found.changed_with.is_empty()).then(|| {
        let files: Vec<String> = found
            .changed_with
            .iter()
            .map(|file| format!("{} x{}", plain(&file.path), file.times))
            .collect();
        format!("changed with it: {}", files.join(", "))
    });
    if found.code.is_empty() && found.tests.is_empty() {
        return match changed_with {
            Some(changed_with) => vec![
                format!("{path}: no other file uses what it defines"),
                changed_with,
            ],
            None => vec![format!(
                "{path}: no other file uses what it defines. {SEARCH}"
            )],
        };
    }
    let listed = found.code.len() + found.tests.len().min(TESTS_SHOWN);
    let total = if found.capped {
        format!("{}+ files", found.total)
    } else {
        counted(found.total as u64, "file")
    };
    let verb = if found.total == 1 && !found.capped {
        "uses"
    } else {
        "use"
    };
    let shown = if found.capped || listed < found.total {
        format!(", {} shown", found.code.len())
    } else {
        String::new()
    };
    let mut lines = vec![format!("{path}: {total} {verb} what it defines{shown}")];
    lines.extend(found.code.iter().map(|row| {
        format!(
            "{} ({}){}",
            plain(&row.path),
            names(&row.names),
            mark(&row.path)
        )
    }));
    if !found.tests.is_empty() {
        let mut tests: Vec<String> = found
            .tests
            .iter()
            .take(TESTS_SHOWN)
            .map(|test| format!("{}{}", plain(test), mark(test)))
            .collect();
        if found.tests.len() > TESTS_SHOWN {
            tests.push(format!("{} more", found.tests.len() - TESTS_SHOWN));
        }
        lines.push(format!("tests: {}", tests.join(", ")));
    }
    lines.extend(changed_with);
    lines
}

fn miss_line(miss: &Miss) -> String {
    let name = plain(&miss.name);
    let index = {
        let languages: Vec<&str> = miss.languages.iter().copied().map(language).collect();
        if languages.is_empty() {
            counted(miss.files, "file")
        } else {
            format!("{}: {}", counted(miss.files, "file"), languages.join(", "))
        }
    };
    match &miss.missing {
        Missing::Definition => {
            let under = miss
                .folder
                .as_deref()
                .map(|folder| format!(" under {}", plain(folder)))
                .unwrap_or_default();
            let nearest = if miss.nearest.is_empty() {
                String::new()
            } else {
                let nearest: Vec<String> = miss.nearest.iter().map(|name| plain(name)).collect();
                format!(" Nearest names: {}.", nearest.join(", "))
            };
            format!("{name}: no definition{under} in the index ({index}).{nearest} {SEARCH}")
        }
        Missing::File { defined_in: None } => format!(
            "{name}: not a file of the index ({index}). Give a path from the project's root; \
             for text, use search_files."
        ),
        Missing::File {
            defined_in: Some(defined),
        } => format!(
            "{name}: not a file of the index; it is defined in {}. Ask `touches` with that path.",
            located(defined)
        ),
    }
}

/// `N more: give `path` to narrow.` when rows were left out.
fn more(total: u64, shown: usize) -> Option<String> {
    let left = total.saturating_sub(shown as u64);
    (left > 0).then(|| format!("{left} more: give `path` to narrow."))
}

fn located(definition: &Definition) -> String {
    format!("{}:{}", plain(&definition.path), definition.line)
}

/// The first names and how many more.
fn names(names: &[String]) -> String {
    let mut shown: Vec<String> = names.iter().take(NAMES_SHOWN).map(|n| plain(n)).collect();
    if names.len() > NAMES_SHOWN {
        shown.push(format!("{} more", names.len() - NAMES_SHOWN));
    }
    shown.join(", ")
}

fn counted(count: u64, noun: &str) -> String {
    if count == 1 {
        format!("1 {noun}")
    } else {
        format!("{count} {noun}s")
    }
}

fn language(language: Language) -> &'static str {
    match language {
        Language::Rust => "Rust",
    }
}

fn why_skipped(why: Skipped) -> &'static str {
    match why {
        Skipped::TooLarge => "over 512 KiB",
        Skipped::TooSlow => "too slow to parse",
        Skipped::TooMany => "too many symbols",
        Skipped::LongLines => "lines too long",
    }
}

/// Text from the repository or the model, on one line: it cannot add a line or forge a
/// note of the harness.
fn plain(text: &str) -> String {
    crate::message::plain::plain(text)
}

/// The caps: rows, characters a line, bytes in all, each cut on a character boundary.
fn finish(lines: Vec<String>) -> String {
    let text = lines
        .into_iter()
        // A header, the rows and a last line.
        .take(MAX_ROWS + 2)
        .map(|line| cut_line(&line))
        .collect::<Vec<_>>()
        .join("\n");
    cut_bytes(&text, MAX_ANSWER_BYTES)
}

fn cut_line(line: &str) -> String {
    if line.chars().count() <= MAX_LINE_CHARS {
        return line.to_owned();
    }
    let mut cut: String = line.chars().take(MAX_LINE_CHARS - 1).collect();
    cut.push(CUT);
    cut
}

fn cut_bytes(text: &str, max: usize) -> String {
    if text.len() <= max {
        return text.to_owned();
    }
    let mut end = max - CUT.len_utf8();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{CUT}", &text[..end])
}
