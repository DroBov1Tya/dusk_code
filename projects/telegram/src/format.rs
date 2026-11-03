//! The agent's Markdown as Telegram HTML: headings and bold as `<b>`, inline code and
//! fenced blocks as `<code>` and `<pre>`, links as `<a>`, list marks as bullets.
//! Everything else is escaped text. Long answers are cut into messages at line ends.

const FENCE: &str = "```";

/// `markdown` in pieces that fit one message each: every piece as valid HTML, and as
/// the plain text to send if Telegram refuses the HTML.
pub fn messages(markdown: &str, max_chars: usize) -> Vec<(String, String)> {
    chunks(markdown, max_chars)
        .into_iter()
        .map(|chunk| (html(&chunk), chunk))
        .collect()
}

/// Cuts at line ends; a code block cut in two is closed and opened again.
fn chunks(markdown: &str, max_chars: usize) -> Vec<String> {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut fence: Option<String> = None;
    for line in markdown.lines() {
        let room = current.chars().count() + line.chars().count() + FENCE.len() + 1;
        if !current.is_empty() && room >= max_chars {
            if fence.is_some() {
                current.push_str(FENCE);
            }
            chunks.push(std::mem::take(&mut current));
            if let Some(opening) = &fence {
                current.push_str(opening);
                current.push('\n');
            }
        }
        // A single line past the limit goes alone, cut by characters.
        let mut line = line.to_owned();
        while line.chars().count() >= max_chars {
            let cut: String = line.chars().take(max_chars - 1).collect();
            line = line.chars().skip(max_chars - 1).collect();
            chunks.push(cut);
        }
        if line.trim_start().starts_with(FENCE) {
            fence = match fence {
                Some(_) => None,
                None => Some(line.trim_start().to_owned()),
            };
        }
        current.push_str(&line);
        current.push('\n');
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    chunks
}

pub fn html(markdown: &str) -> String {
    let mut out = String::new();
    let mut in_code = false;
    for line in markdown.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with(FENCE) {
            if in_code {
                out.push_str("</pre>\n");
            } else {
                out.push_str("<pre>");
            }
            in_code = !in_code;
            continue;
        }
        if in_code {
            out.push_str(&escape(line));
            out.push('\n');
            continue;
        }
        let heading = trimmed.trim_start_matches('#');
        if heading.len() < trimmed.len() && heading.starts_with(' ') {
            out.push_str(&format!("<b>{}</b>\n", inline(heading.trim())));
            continue;
        }
        let bullet = ["- ", "* ", "+ "]
            .iter()
            .find_map(|mark| trimmed.strip_prefix(mark));
        match bullet {
            Some(item) => {
                let indent = &line[..line.len() - trimmed.len()];
                out.push_str(&format!("{indent}\u{2022} {}\n", inline(item)));
            }
            None => {
                out.push_str(&inline(line));
                out.push('\n');
            }
        }
    }
    if in_code {
        out.push_str("</pre>\n");
    }
    out.trim_end().to_owned()
}

/// Code spans first, so nothing inside them is read as Markdown.
fn inline(text: &str) -> String {
    let mut out = String::new();
    let ticks = text.matches('`').count();
    for (index, part) in text.split('`').enumerate() {
        // A part after an unmatched backtick is plain text.
        if index % 2 == 1 && index < ticks {
            out.push_str(&format!("<code>{}</code>", escape(part)));
        } else {
            if index % 2 == 1 {
                out.push('`');
            }
            out.push_str(&links(&bold(&escape(part))));
        }
    }
    out
}

fn bold(text: &str) -> String {
    let parts: Vec<&str> = text.split("**").collect();
    if parts.len() < 3 {
        return text.to_owned();
    }
    let mut out = String::new();
    let pairs = (parts.len() - 1) / 2 * 2;
    for (index, part) in parts.iter().enumerate() {
        if index > 0 {
            out.push_str(match (index <= pairs, index % 2 == 1) {
                (true, true) => "<b>",
                (true, false) => "</b>",
                (false, _) => "**",
            });
        }
        out.push_str(part);
    }
    out
}

/// `[text](https://...)` on already escaped text; other targets stay as written.
fn links(text: &str) -> String {
    let mut out = String::new();
    let mut rest = text;
    while let Some(open) = rest.find('[') {
        let Some(close) = rest[open..].find("](").map(|at| open + at) else {
            break;
        };
        let Some(end) = rest[close..].find(')').map(|at| close + at) else {
            break;
        };
        let label = &rest[open + 1..close];
        let target = &rest[close + 2..end];
        let web = target.starts_with("https://") || target.starts_with("http://");
        if !web || target.contains('"') || label.contains('[') {
            out.push_str(&rest[..=open]);
            rest = &rest[open + 1..];
            continue;
        }
        out.push_str(&rest[..open]);
        out.push_str(&format!("<a href=\"{target}\">{label}</a>"));
        rest = &rest[end + 1..];
    }
    out.push_str(rest);
    out
}

fn escape(text: &str) -> String {
    text.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn markdown_becomes_escaped_telegram_html() {
        let markdown = "# Plan\n- **fix** `a<b` in [docs](https://x.dev/a?b=1&c=2)\n\
                        <script> & [bad](javascript:alert)\n```rust\nlet x = 1 < 2;\n```";
        assert_eq!(
            html(markdown),
            "<b>Plan</b>\n\u{2022} <b>fix</b> <code>a&lt;b</code> in \
             <a href=\"https://x.dev/a?b=1&amp;c=2\">docs</a>\n\
             &lt;script&gt; &amp; [bad](javascript:alert)\n<pre>let x = 1 &lt; 2;\n</pre>"
        );
        assert_eq!(html("a ** b"), "a ** b");
        assert_eq!(html("odd `tick"), "odd `tick");
    }

    #[test]
    fn long_answers_are_cut_with_code_blocks_closed() {
        let markdown = format!("intro\n```\n{}```\nend", "line of code\n".repeat(40));
        let pieces = chunks(&markdown, 100);
        assert!(pieces.len() > 1);
        for piece in &pieces {
            assert!(piece.chars().count() <= 100, "{piece}");
            assert_eq!(piece.matches(FENCE).count() % 2, 0, "{piece}");
        }
        let long = "x".repeat(250);
        assert!(
            chunks(&long, 100)
                .iter()
                .all(|piece| piece.chars().count() <= 100)
        );
    }
}
