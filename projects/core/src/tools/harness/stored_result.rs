//! `read_history` with `result`: one stored tool result, paged by lines or searched. The
//! text is cut here, so a big result never has to be sent whole again.

/// Lines a page holds unless asked for fewer.
pub const DEFAULT_LINES: usize = 200;
pub const MAX_LINES: usize = 400;
/// Bytes of text one answer carries at most.
const MAX_BYTES: usize = 8 * 1024;
const MAX_MATCHES: usize = 50;
const CONTEXT: usize = 2;

/// Lines `offset` (1-based) on, at most `limit` and `MAX_BYTES`, under a header that
/// says what was shown and where to read on.
pub fn page(sequence: u64, tool: &str, text: &str, offset: usize, limit: usize) -> String {
    let total = text.lines().count();
    if total == 0 {
        return format!("#{sequence} {tool} result, empty");
    }
    if offset > total {
        return format!(
            "#{sequence} {tool} result has {total} lines; offset {offset} is past its end"
        );
    }
    let mut body = String::new();
    let mut last = offset - 1;
    let mut cut = false;
    for line in text.lines().skip(offset - 1).take(limit) {
        if body.len() + line.len() + 1 > MAX_BYTES {
            cut = true;
            // A first line over the cap is shown in part, so a page is never empty.
            if last < offset {
                body.push_str(truncated(line, MAX_BYTES));
                body.push('\n');
                last += 1;
            }
            break;
        }
        body.push_str(line);
        body.push('\n');
        last += 1;
    }
    let mut out = format!("#{sequence} {tool} result, lines {offset}-{last} of {total}");
    if last < total {
        out.push_str(&format!("; next offset {}", last + 1));
    }
    out.push('\n');
    out.push_str(&body);
    if cut {
        out.push_str("[cut at 8 KiB]");
    }
    out
}

/// The lines containing `query` (any case), numbered, each with its neighbours; at most
/// `MAX_MATCHES` of them and `MAX_BYTES` of text.
pub fn search(sequence: u64, tool: &str, text: &str, query: &str) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let needle = query.to_lowercase();
    let hits: Vec<usize> = (0..lines.len())
        .filter(|&index| lines[index].to_lowercase().contains(&needle))
        .collect();
    let shown = &hits[..hits.len().min(MAX_MATCHES)];
    let mut out = format!(
        "#{sequence} {tool} result, {} lines; {} matching",
        lines.len(),
        hits.len()
    );
    let mut body = String::new();
    let mut printed = 0; // lines before this index are done
    let mut cut = false;
    'matches: for &hit in shown {
        let first = hit.saturating_sub(CONTEXT).max(printed);
        let last = (hit + CONTEXT).min(lines.len() - 1);
        if first > printed && printed > 0 {
            body.push_str("--\n");
        }
        for (index, text) in lines.iter().enumerate().take(last + 1).skip(first) {
            let mark = if shown.binary_search(&index).is_ok() {
                ':'
            } else {
                '-'
            };
            let line = format!("{}{mark} {text}\n", index + 1);
            if body.len() + line.len() > MAX_BYTES {
                cut = true;
                break 'matches;
            }
            body.push_str(&line);
        }
        printed = last + 1;
    }
    out.push('\n');
    out.push_str(&body);
    if cut {
        out.push_str("[cut at 8 KiB; search for something narrower]");
    } else if hits.len() > shown.len() {
        out.push_str(&format!(
            "[{} more matches not shown; search for something narrower]",
            hits.len() - shown.len()
        ));
    }
    out
}

/// `line` cut to at most `max` bytes at a character boundary.
fn truncated(line: &str, max: usize) -> &str {
    let mut end = max.min(line.len());
    while !line.is_char_boundary(end) {
        end -= 1;
    }
    &line[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn numbered(lines: usize) -> String {
        (1..=lines).map(|n| format!("line {n}\n")).collect()
    }

    #[test]
    fn pages_say_where_to_read_on_until_the_end() {
        let text = numbered(450);
        let first = page(7, "run_command", &text, 1, 200);
        assert!(
            first.starts_with(
                "#7 run_command result, lines 1-200 of 450; next offset 201\nline 1\n"
            )
        );
        assert!(first.ends_with("line 200\n"));
        let last = page(7, "run_command", &text, 401, 200);
        assert!(last.starts_with("#7 run_command result, lines 401-450 of 450\nline 401\n"));
        assert!(!last.contains("next offset"));
    }

    #[test]
    fn a_page_is_cut_at_8_kib_on_a_line_boundary() {
        let text: String = (0..100).map(|n| format!("{n:0>99}\n")).collect();
        let out = page(1, "read_file", &text, 1, 200);
        assert!(out.ends_with("[cut at 8 KiB]"));
        let (header, body) = out.split_once('\n').unwrap();
        assert!(body.len() <= MAX_BYTES + "[cut at 8 KiB]".len());
        // 100-byte lines: 81 fit in 8192 bytes.
        assert!(
            header.contains("lines 1-81 of 100; next offset 82"),
            "{header}"
        );
    }

    #[test]
    fn one_huge_line_is_shown_in_part() {
        let out = page(1, "run_command", &"é".repeat(9000), 1, 200);
        assert!(out.starts_with("#1 run_command result, lines 1-1 of 1\n"));
        assert!(out.ends_with("[cut at 8 KiB]"));
    }

    #[test]
    fn an_offset_past_the_end_is_said() {
        let out = page(1, "run_command", &numbered(3), 9, 200);
        assert!(out.contains("has 3 lines; offset 9 is past its end"));
    }

    #[test]
    fn search_numbers_matches_with_two_lines_around() {
        let text = numbered(30);
        let out = search(5, "run_command", &text, "LINE 10");
        assert_eq!(
            out,
            "#5 run_command result, 30 lines; 1 matching\n\
             8- line 8\n9- line 9\n10: line 10\n11- line 11\n12- line 12\n"
        );
    }

    #[test]
    fn close_matches_share_their_context() {
        let out = search(5, "run_command", "a\nneedle\nb\nneedle\nc\n", "needle");
        assert_eq!(
            out,
            "#5 run_command result, 5 lines; 2 matching\n\
             1- a\n2: needle\n3- b\n4: needle\n5- c\n"
        );
    }

    #[test]
    fn search_stops_at_50_matches_and_says_so() {
        let text = "hit\nx\nx\nx\nx\n".repeat(60);
        let out = search(5, "run_command", &text, "hit");
        assert_eq!(out.matches(": hit").count(), 50);
        assert!(out.contains("60 matching"));
        assert!(out.ends_with("[10 more matches not shown; search for something narrower]"));
    }

    #[test]
    fn search_text_is_capped_too() {
        let text = format!("{}\n", "needle ".repeat(1000)).repeat(20);
        let out = search(5, "run_command", &text, "needle");
        assert!(out.len() < MAX_BYTES + 200);
        assert!(out.contains("[cut at 8 KiB"));
    }

    #[test]
    fn no_match_is_plain() {
        assert_eq!(
            search(5, "run_command", "a\nb\n", "zzz"),
            "#5 run_command result, 2 lines; 0 matching\n"
        );
    }
}
