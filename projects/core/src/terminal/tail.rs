//! The newest output of a terminal, kept in memory only, so the agent can read what a
//! shell it was asked to work in printed. Raw bytes are kept; reading turns them into
//! plain lines: escape sequences out, a carriage return starting its line over.

use std::collections::VecDeque;

/// The most bytes kept of one terminal's output; older ones go first.
const KEPT: usize = 128 * 1024;

/// The longest title a program may set, in characters.
const MAX_TITLE: usize = 120;

#[derive(Default)]
pub struct Tail {
    bytes: VecDeque<u8>,
    /// The title a program set last with OSC 0 or 2, as the desktop's tab shows it.
    title: Option<String>,
    /// An OSC sequence cut between two reads, kept until it ends.
    osc: Option<Vec<u8>>,
}

impl Tail {
    pub fn push(&mut self, data: &[u8]) {
        self.find_title(data);
        self.bytes.extend(data);
        let over = self.bytes.len().saturating_sub(KEPT);
        self.bytes.drain(..over);
    }

    pub fn title(&self) -> Option<&str> {
        self.title.as_deref()
    }

    /// The last `lines` lines as plain text, without blank lines at the end.
    pub fn lines(&self, lines: usize) -> Vec<String> {
        let (front, back) = self.bytes.as_slices();
        let raw = [front, back].concat();
        let mut all = plain_lines(&String::from_utf8_lossy(&raw));
        while all.last().is_some_and(|line| line.trim().is_empty()) {
            all.pop();
        }
        let skip = all.len().saturating_sub(lines);
        all.split_off(skip)
    }

    fn find_title(&mut self, data: &[u8]) {
        let mut at = 0;
        while at < data.len() {
            if let Some(osc) = &mut self.osc {
                match data[at..]
                    .iter()
                    .position(|&byte| byte == 0x07 || byte == 0x1b)
                {
                    Some(end) => {
                        osc.extend_from_slice(&data[at..at + end]);
                        let osc = self.osc.take().unwrap_or_default();
                        self.title_from(&osc);
                        at += end + 1;
                    }
                    None => {
                        // A title longer than any kept is no title: stop collecting it.
                        osc.extend_from_slice(&data[at..]);
                        if osc.len() > MAX_TITLE * 4 + 2 {
                            self.osc = None;
                        }
                        return;
                    }
                }
            } else {
                match data[at..].windows(2).position(|pair| pair == b"\x1b]") {
                    Some(start) => {
                        self.osc = Some(Vec::new());
                        at += start + 2;
                    }
                    None => return,
                }
            }
        }
    }

    fn title_from(&mut self, osc: &[u8]) {
        let text = String::from_utf8_lossy(osc);
        let Some(title) = text.strip_prefix("0;").or_else(|| text.strip_prefix("2;")) else {
            return;
        };
        let title: String = title
            .chars()
            .filter(|c| !c.is_control())
            .take(MAX_TITLE)
            .collect();
        self.title = (!title.trim().is_empty()).then(|| title.trim().to_owned());
    }
}

/// Text a terminal was sent, as the lines it shows: escape sequences dropped, a carriage
/// return that does not end a line starting it over, a backspace taking a character back.
pub fn plain_lines(text: &str) -> Vec<String> {
    let mut lines = Vec::new();
    let mut line = String::new();
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\x1b' => skip_escape(&mut chars),
            '\n' => lines.push(std::mem::take(&mut line)),
            '\r' if chars.peek() == Some(&'\n') => {}
            '\r' => line.clear(),
            '\x08' => {
                line.pop();
            }
            '\t' => line.push('\t'),
            c if c.is_control() => {}
            c => line.push(c),
        }
    }
    lines.push(line);
    lines
}

/// Skips the rest of a sequence that began with ESC: CSI up to its final byte, OSC, DCS
/// and the like up to their terminator, a character set choice with its letter, or a
/// single character.
fn skip_escape(chars: &mut std::iter::Peekable<std::str::Chars<'_>>) {
    match chars.next() {
        Some('[') => {
            for c in chars.by_ref() {
                if ('\x40'..='\x7e').contains(&c) {
                    return;
                }
            }
        }
        Some(']' | 'P' | '_' | '^' | 'X') => {
            while let Some(c) = chars.next() {
                if c == '\x07' {
                    return;
                }
                if c == '\x1b' {
                    chars.next_if_eq(&'\\');
                    return;
                }
            }
        }
        Some('(' | ')' | '*' | '+' | '#' | '%') => {
            chars.next();
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn output_reads_as_the_lines_a_terminal_shows() {
        let text = "\x1b[1;32mok\x1b[0m done\r\nprogress 10%\rprogress 100%\n\
                    \x1b]0;title\x07ab\x08c\x1b(Bx\n\n";
        assert_eq!(
            plain_lines(text),
            ["ok done", "progress 100%", "acx", "", ""]
        );
    }

    #[test]
    fn the_tail_keeps_the_newest_output_and_the_last_title() {
        let mut tail = Tail::default();
        tail.push(b"\x1b]2;npm run ");
        tail.push(b"dev\x07");
        assert_eq!(tail.title(), Some("npm run dev"));
        tail.push(b"\x1b]0;zsh\x1b\\");
        assert_eq!(tail.title(), Some("zsh"));
        for line in 0..20_000 {
            tail.push(format!("line {line}\n").as_bytes());
        }
        assert!(tail.bytes.len() <= KEPT);
        assert_eq!(tail.lines(2), ["line 19998", "line 19999"]);
    }
}
