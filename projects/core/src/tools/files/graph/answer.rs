//! `about`, `find` and `impact`: answers from the stored graph, a row a line, each part
//! capped and the whole under `text::MAX_BYTES`. Texts are agents' notes and are shown as
//! data: `(stale)` marks one written against an older version of its file.

use std::fmt::Write;

use anyhow::{Context, Result, ensure};

use super::{
    Arguments, Graph,
    map::{check_find, inside},
    text::Lines,
};
use crate::{
    state::graph::{EdgeView, FileView, Found, NodeView},
    tools::tool::ToolContext,
};

/// Rows of one part of an answer, read one more to say there are more.
const ROWS: i64 = 30;
/// Nodes `about <node>` describes, of those with the name.
const NODES_ABOUT: i64 = 5;
/// Files `impact` takes and gives.
const MAX_IMPACT_FILES: usize = 16;
const IMPACTED: usize = 40;

pub(super) async fn about(
    context: &ToolContext,
    graph: &Graph<'_>,
    arguments: &Arguments,
) -> Result<String> {
    let path = match arguments.path.as_deref() {
        Some(path) => Some(
            inside(&context.scope, path)
                .await?
                .context("give a file as `path`, not the project")?,
        ),
        None => None,
    };
    let mut out = Lines::default();
    match (arguments.node.as_deref(), path) {
        (Some(node), path) => about_node(graph, node.trim(), path.as_deref(), &mut out).await?,
        (None, Some(path)) => about_file(graph, &path, &mut out).await?,
        (None, None) => anyhow::bail!("give `path` of a file or `node`"),
    }
    Ok(out.finish())
}

async fn about_file(graph: &Graph<'_>, path: &str, out: &mut Lines) -> Result<()> {
    let state = graph.state;
    let Some(file) = state.graph_file(&graph.project, path).await? else {
        out.push(
            0,
            &format!("`{path}` is not in the graph: no such file, or ignored"),
        );
        return Ok(());
    };
    out.push(0, &file_head(&file));
    out.push(1, &summary_line(&file));
    let nodes = state
        .graph_file_nodes(&graph.project, path, ROWS + 1)
        .await?;
    section(out, "nodes", &nodes, |node| node_line(node, false));
    let edges = state
        .graph_edges_from(&graph.project, path, None, ROWS + 1)
        .await?;
    section(out, "edges out", &edges, edge_out);
    let into = state
        .graph_edges_into_file(&graph.project, path, ROWS + 1)
        .await?;
    section(out, "used by", &into, edge_in);
    Ok(())
}

async fn about_node(
    graph: &Graph<'_>,
    name: &str,
    path: Option<&str>,
    out: &mut Lines,
) -> Result<()> {
    ensure!(!name.is_empty(), "`node` is empty");
    let state = graph.state;
    let mut nodes = state
        .graph_nodes_named(&graph.project, name, ROWS + 1)
        .await?;
    if let Some(path) = path {
        nodes.retain(|node| node.path == path);
    }
    if nodes.is_empty() {
        out.push(0, &format!("no node `{name}` in the graph; try `find`"));
        return Ok(());
    }
    let more = nodes.len().saturating_sub(NODES_ABOUT as usize);
    for node in nodes.iter().take(NODES_ABOUT as usize) {
        out.push(0, &node_line(node, true));
        let edges = state
            .graph_edges_from(&graph.project, &node.path, Some(&node.name), ROWS + 1)
            .await?;
        section(out, "edges out", &edges, edge_out);
        let into = state
            .graph_edges_into_node(&graph.project, &node.path, &node.name, ROWS + 1)
            .await?;
        section(out, "used by", &into, edge_in);
    }
    if more > 0 {
        out.push(
            0,
            &format!("[{more} more nodes named `{name}`; give `path`]"),
        );
    }
    Ok(())
}

pub(super) async fn find(graph: &Graph<'_>, arguments: &Arguments) -> Result<String> {
    let text = check_find(arguments.find.as_deref())?;
    let found = graph
        .state
        .graph_find(&graph.project, text, ROWS + 1)
        .await?;
    let mut out = Lines::default();
    if found.is_empty() {
        out.push(
            0,
            &format!("nothing in the graph holds `{text}`; try `map` or search_files"),
        );
        return Ok(out.finish());
    }
    let (mut files, mut nodes) = (Vec::new(), Vec::new());
    for row in found {
        match row {
            Found::File(file) => files.push(file),
            Found::Node(node) => nodes.push(node),
        }
    }
    section(&mut out, "files", &files, |file| {
        let mut line = format!("{} {}", file.path, file.lines);
        if !file.summary.is_empty() {
            let _ = write!(line, ": {}{}", file.summary, stale(file.stale));
        }
        line
    });
    section(&mut out, "nodes", &nodes, |node| node_line(node, true));
    Ok(out.finish())
}

pub(super) async fn impact(
    context: &ToolContext,
    graph: &Graph<'_>,
    arguments: &Arguments,
) -> Result<String> {
    let given = arguments
        .files
        .as_deref()
        .context("give `files`: the files a change touches")?;
    ensure!(!given.is_empty(), "`files` is empty");
    ensure!(
        given.len() <= MAX_IMPACT_FILES,
        "`files` names at most {MAX_IMPACT_FILES} files"
    );
    let mut paths = Vec::with_capacity(given.len());
    for path in given {
        paths.push(
            inside(&context.scope, path)
                .await?
                .context("give files, not the project")?,
        );
    }
    let impacted = graph
        .state
        .graph_impact(&graph.project, &paths, IMPACTED + 1)
        .await?;
    let mut out = Lines::default();
    if impacted.is_empty() {
        out.push(
            0,
            "no recorded edge leads to these files; the graph knows only what agents recorded",
        );
        return Ok(out.finish());
    }
    let more = impacted.len() > IMPACTED;
    for file in impacted.iter().take(IMPACTED) {
        out.push(
            0,
            &format!(
                "{} ({} hop{}, {}; via {})",
                file.path,
                file.hops,
                if file.hops == 1 { "" } else { "s" },
                file.kinds.join(", "),
                file.via
            ),
        );
    }
    if more {
        out.push(
            0,
            &format!("[more than {IMPACTED} files; give fewer `files`]"),
        );
    }
    Ok(out.finish())
}

fn file_head(file: &FileView) -> String {
    let mut head = format!("{}: {} lines, {} bytes", file.path, file.lines, file.bytes);
    if !file.hashed {
        head.push_str("; not read (secrets or too large), so no facts");
    }
    head
}

fn summary_line(file: &FileView) -> String {
    if file.summary.is_empty() {
        return "summary: none yet; `record` one once you have read the file".into();
    }
    let by = if file.by_user { " (by the user)" } else { "" };
    format!("summary{by}: {}{}", file.summary, stale(file.stale))
}

fn node_line(node: &NodeView, with_path: bool) -> String {
    let mut line = format!("{} {}", node.kind, node.name);
    match (with_path, node.line, node.end_line) {
        (true, Some(start), Some(end)) => {
            let _ = write!(line, " ({}:{start}-{end})", node.path);
        }
        (true, Some(start), None) => {
            let _ = write!(line, " ({}:{start})", node.path);
        }
        (true, None, _) => {
            let _ = write!(line, " ({})", node.path);
        }
        (false, Some(start), Some(end)) => {
            let _ = write!(line, " {start}-{end}");
        }
        (false, Some(start), None) => {
            let _ = write!(line, " {start}");
        }
        (false, None, _) => {}
    }
    if !node.summary.is_empty() {
        let _ = write!(line, ": {}", node.summary);
    }
    if node.by_user {
        line.push_str(" (by the user)");
    }
    line.push_str(stale(node.stale));
    line
}

fn edge_out(edge: &EdgeView) -> String {
    match &edge.from {
        Some(from) => format!("{from} {} {}{}", edge.kind, edge.to, stale(edge.stale)),
        None => format!("{} {}{}", edge.kind, edge.to, stale(edge.stale)),
    }
}

fn edge_in(edge: &EdgeView) -> String {
    let from = match &edge.from {
        Some(from) => format!("{}#{from}", edge.path),
        None => edge.path.clone(),
    };
    format!("{from} {} {}{}", edge.kind, edge.to, stale(edge.stale))
}

fn stale(stale: bool) -> &'static str {
    if stale { " (stale)" } else { "" }
}

/// `rows` under a heading, one more than `ROWS` read to say there are more.
fn section<T>(out: &mut Lines, heading: &str, rows: &[T], line: impl Fn(&T) -> String) {
    if rows.is_empty() {
        return;
    }
    let shown = usize::try_from(ROWS).unwrap_or(usize::MAX);
    let more = if rows.len() > shown { "+" } else { "" };
    out.push(1, &format!("{heading} ({}{more}):", rows.len().min(shown)));
    for row in rows.iter().take(shown) {
        out.push(2, &line(row));
    }
}
