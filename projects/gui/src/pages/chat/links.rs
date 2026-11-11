//! Markdown as the chat draws it: links open in the system browser and pictures load,
//! both only over http(s), so a message cannot reach a local file or another scheme.
//! One scheme of our own: a path in code, such as `src/main.rs:42`, becomes a link that
//! opens the file in the viewer. The core reads it, and only inside a project.

use gpui_kit::{component::text::TextView, *};

use crate::workspace::Workspace;

const FILE_SCHEME: &str = "dusk-file:";
/// Extensions a bare file name (no folder) must have to read as a path.
const EXTENSIONS: &[&str] = &[
    "rs", "toml", "md", "json", "yml", "yaml", "py", "js", "ts", "tsx", "jsx", "go", "c", "h",
    "cpp", "hpp", "java", "kt", "swift", "sh", "sql", "html", "css", "txt", "lock", "cfg", "ini",
    "xml", "rb", "php", "lua", "gradle", "proto",
];

pub fn markdown(id: impl Into<ElementId>, text: String) -> TextView {
    TextView::markdown(id, linkify(&text))
        .selectable(true)
        .code_block_actions(super::code::actions)
        .on_link_click(|url, _, window, cx| {
            if let Some(path) = url.strip_prefix(FILE_SCHEME) {
                if let Some(workspace) = window.root::<Workspace>().flatten() {
                    let path = path.to_owned();
                    workspace.update(cx, |ws, cx| ws.open_path(&path, cx));
                }
            } else if is_web(url) {
                cx.open_url(url);
            } else {
                tracing::warn!("a link that is not http(s) was not opened");
            }
        })
        .image_source(|uri| {
            if is_web(uri.as_ref()) {
                ImageSource::from(uri.clone())
            } else {
                // An empty source draws nothing instead of reading the disk.
                ImageSource::from(SharedUri::from(""))
            }
        })
}

fn is_web(url: &str) -> bool {
    url.starts_with("https://") || url.starts_with("http://")
}

/// Turns each path in a single-backtick span into a link of ours. Fenced code is left
/// alone, and the scheme is cut from anything the model wrote itself, so only a span
/// that reads as a relative path can carry it.
fn linkify(text: &str) -> String {
    let text = text.replace(FILE_SCHEME, "dusk-file-");
    let mut out = String::with_capacity(text.len());
    let mut fenced = false;
    for line in text.split_inclusive('\n') {
        if line.trim_start().starts_with("```") {
            fenced = !fenced;
        }
        if fenced || line.contains("``") {
            out.push_str(line);
            continue;
        }
        let mut rest = line;
        while let Some(start) = rest.find('`') {
            let after = &rest[start + 1..];
            let Some(end) = after.find('`') else { break };
            let span = &after[..end];
            out.push_str(&rest[..start]);
            if is_path(span) {
                out.push_str(&format!("[`{span}`]({FILE_SCHEME}{span})"));
            } else {
                out.push_str(&format!("`{span}`"));
            }
            rest = &after[end + 1..];
        }
        out.push_str(rest);
    }
    out
}

/// A relative path, with an optional `:line`, in the characters of file names.
fn is_path(span: &str) -> bool {
    let name = match span.rsplit_once(':') {
        Some((name, line)) if !line.is_empty() && line.bytes().all(|b| b.is_ascii_digit()) => name,
        _ => span,
    };
    let plain = name
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b"_./-+@".contains(&b));
    if name.is_empty()
        || !plain
        || name.starts_with('/')
        || name.starts_with('.') && !name.starts_with("./")
        || name.ends_with('/')
        || name.contains("..")
        || name.contains("//")
    {
        return false;
    }
    let Some((_, ext)) = name.rsplit_once('.') else {
        return false;
    };
    let named = name.contains('/');
    ext.starts_with(|c: char| c.is_ascii_alphabetic()) && (named || EXTENSIONS.contains(&ext))
}

#[cfg(test)]
mod tests {
    use super::*;

    // `use super::*` brings in the toolkit's own `test` macro; this is the standard one.
    #[::core::prelude::v1::test]
    fn only_a_relative_path_in_code_becomes_a_link() {
        let text = "see `src/main.rs:42`, `README.md` and `self.field`, not `/etc/passwd`, \
                    `../x.rs`, `1.2.3` or [x](dusk-file:/etc/passwd)\n```\n`a/b.rs`\n```\n";
        let out = linkify(text);
        assert!(out.contains("[`src/main.rs:42`](dusk-file:src/main.rs:42)"));
        assert!(out.contains("[`README.md`](dusk-file:README.md)"));
        assert!(out.contains("`self.field`") && !out.contains("](dusk-file:self.field)"));
        assert!(!out.contains("](dusk-file:/etc"));
        assert!(!out.contains("](dusk-file:../"));
        assert!(!out.contains("](dusk-file:1.2.3"));
        // Model-written links of ours lose the scheme; fenced code stays as written.
        assert!(out.contains("(dusk-file-/etc/passwd)"));
        assert!(out.contains("```\n`a/b.rs`\n```"));
    }
}
