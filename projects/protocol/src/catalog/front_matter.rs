//! The `---` block a skill or agent file opens with, apart from the text after it.

use anyhow::{Context, Result, anyhow};

const FENCE: &str = "---";

/// Splits `---\n<front>\n---\n<body>` into the front matter between the fences and the
/// body after them. A byte order mark before the first fence is skipped, a line ending
/// may be `\r\n`, and the closing fence may have trailing blanks; a line that only starts
/// with `---` is not a fence.
pub fn split_front_matter(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix('\u{FEFF}')
        .unwrap_or(text)
        .strip_prefix(FENCE)
        .and_then(|rest| {
            rest.strip_prefix('\n')
                .or_else(|| rest.strip_prefix("\r\n"))
        })
        .context("the file must start with a --- front matter line")?;
    let mut offset = 0;
    for line in rest.split_inclusive('\n') {
        if line.trim_end() == FENCE {
            return Ok((&rest[..offset], &rest[offset + line.len()..]));
        }
        offset += line.len();
    }
    Err(anyhow!("front matter has no closing --- line"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_whole_fence_line_closes_the_front_matter() {
        let (front, body) = split_front_matter("---\nname: a\n---\nbody\n").unwrap();
        assert_eq!((front, body), ("name: a\n", "body\n"));
        let (front, body) = split_front_matter("---\r\nname: a\r\n---  \r\nbody").unwrap();
        assert_eq!((front, body), ("name: a\r\n", "body"));
        let (front, body) = split_front_matter("---\na: 1\n---- rule ----\n---\nb").unwrap();
        assert_eq!((front, body), ("a: 1\n---- rule ----\n", "b"));
        let (front, body) = split_front_matter("\u{FEFF}---\nname: a\n---\nbody").unwrap();
        assert_eq!((front, body), ("name: a\n", "body"));
        assert_eq!(split_front_matter("---\n---\nbody").unwrap(), ("", "body"));
        assert_eq!(
            split_front_matter("---\nname: a\n---").unwrap(),
            ("name: a\n", "")
        );
        assert!(split_front_matter("no front matter").is_err());
        assert!(split_front_matter("x\u{FEFF}---\na\n---\n").is_err());
        assert!(split_front_matter("---\nname: a\n").is_err());
    }
}
