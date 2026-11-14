//! One terminal's screen: what the shell wrote, parsed into a grid of cells. The shell
//! runs in the core; this only reads its bytes. What a program in it asks the terminal to
//! answer (its size, its cursor) comes back as bytes to send on, and its title as text.
//! Everything in here is untrusted program output: the clipboard is closed to it, and a
//! title is cut to plain short text.

use std::sync::Arc;

use alacritty_terminal::{
    Term,
    event::{Event, EventListener},
    grid::{Dimensions, Scroll},
    term::{Config, Osc52, TermMode, cell::Flags},
    vte::ansi::{Color, CursorShape, NamedColor, Processor},
};
use parking_lot::Mutex;

/// Lines kept above the screen to scroll back to.
const HISTORY: usize = 5000;
const TITLE_CHARS: usize = 48;

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Size {
    pub cols: u16,
    pub rows: u16,
}

impl Dimensions for Size {
    fn total_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn screen_lines(&self) -> usize {
        usize::from(self.rows)
    }

    fn columns(&self) -> usize {
        usize::from(self.cols)
    }
}

/// A color as the program named it; the window's palette says what it looks like.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Ink {
    Foreground,
    Background,
    /// One of the 256 indexed colors; the first sixteen are the theme's.
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub struct Style {
    pub fg: Ink,
    pub bg: Ink,
    pub bold: bool,
    pub italic: bool,
    pub underline: bool,
    pub dim: bool,
}

/// Cells of one style standing side by side in a row.
pub struct Run {
    pub col: usize,
    pub text: String,
    /// How many cells it covers: a wide character covers two.
    pub cells: usize,
    pub style: Style,
}

/// What the screen shows now.
pub struct Picture {
    pub rows: Vec<Vec<Run>>,
    /// Row and column of the cursor, when it is in sight.
    pub cursor: Option<(usize, usize)>,
}

#[derive(Default)]
struct Outbox {
    replies: Vec<u8>,
    title: Option<String>,
}

#[derive(Clone, Default)]
struct Listener(Arc<Mutex<Outbox>>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let mut outbox = self.0.lock();
        match event {
            Event::PtyWrite(text) => outbox.replies.extend_from_slice(text.as_bytes()),
            Event::Title(title) => {
                let clean: String = title
                    .chars()
                    .filter(|c| !c.is_control())
                    .take(TITLE_CHARS)
                    .collect();
                outbox.title = Some(clean);
            }
            Event::ResetTitle => outbox.title = Some(String::new()),
            // The clipboard, colors and the rest are not this program's to ask for.
            _ => {}
        }
    }
}

pub struct Screen {
    term: Term<Listener>,
    parser: Processor,
    listener: Listener,
    pub size: Size,
}

impl Screen {
    pub fn new(size: Size) -> Self {
        let listener = Listener::default();
        let config = Config {
            scrolling_history: HISTORY,
            osc52: Osc52::Disabled,
            ..Config::default()
        };
        Self {
            term: Term::new(config, &size, listener.clone()),
            parser: Processor::new(),
            listener,
            size,
        }
    }

    /// Takes what the shell wrote; returns what the terminal answers to it, to send back.
    pub fn feed(&mut self, bytes: &[u8]) -> Vec<u8> {
        self.parser.advance(&mut self.term, bytes);
        std::mem::take(&mut self.listener.0.lock().replies)
    }

    /// The title a program set since the last look; empty when it took its title back.
    pub fn take_title(&mut self) -> Option<String> {
        self.listener.0.lock().title.take()
    }

    pub fn resize(&mut self, size: Size) {
        if size != self.size {
            self.size = size;
            self.term.resize(size);
        }
    }

    /// Scrolls the view through the history: up for a positive count.
    pub fn scroll(&mut self, lines: i32) {
        self.term.scroll_display(Scroll::Delta(lines));
    }

    pub fn follow(&mut self) {
        self.term.scroll_display(Scroll::Bottom);
    }

    /// Arrows go as the application asked, and a paste is fenced when it asked for that.
    pub fn app_cursor(&self) -> bool {
        self.term.mode().contains(TermMode::APP_CURSOR)
    }

    pub fn bracketed_paste(&self) -> bool {
        self.term.mode().contains(TermMode::BRACKETED_PASTE)
    }

    /// A full-screen program has no history: the wheel is its arrows.
    pub fn alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    pub fn picture(&self) -> Picture {
        let content = self.term.renderable_content();
        let offset = content.display_offset as i32;
        let mut rows: Vec<Vec<Run>> = (0..self.size.rows).map(|_| Vec::new()).collect();
        for cell in content.display_iter {
            let Some(row) = usize::try_from(cell.point.line.0 + offset)
                .ok()
                .and_then(|row| rows.get_mut(row))
            else {
                continue;
            };
            let flags = cell.flags;
            // The second half of a wide character is drawn with the first.
            if flags.intersects(Flags::WIDE_CHAR_SPACER | Flags::LEADING_WIDE_CHAR_SPACER) {
                continue;
            }
            let (mut fg, mut bg) = (ink(cell.fg), ink(cell.bg));
            if flags.contains(Flags::INVERSE) {
                (fg, bg) = (bg, fg);
            }
            let style = Style {
                fg,
                bg,
                bold: flags.contains(Flags::BOLD),
                italic: flags.contains(Flags::ITALIC),
                underline: flags.intersects(Flags::ALL_UNDERLINES),
                dim: flags.contains(Flags::DIM),
            };
            let shown = if flags.contains(Flags::HIDDEN) || cell.c == '\0' {
                ' '
            } else {
                cell.c
            };
            let cells = if flags.contains(Flags::WIDE_CHAR) {
                2
            } else {
                1
            };
            let col = cell.point.column.0;
            match row.last_mut() {
                // A wide character stands alone, so the cells after it stay in their columns.
                Some(run) if run.style == style && run.col + run.cells == col && cells == 1 => {
                    run.text.push(shown);
                    run.cells += 1;
                }
                _ => row.push(Run {
                    col,
                    text: shown.to_string(),
                    cells,
                    style,
                }),
            }
        }
        let cursor = (content.cursor.shape != CursorShape::Hidden)
            .then(|| {
                let row = usize::try_from(content.cursor.point.line.0 + offset).ok()?;
                (row < usize::from(self.size.rows)).then_some((row, content.cursor.point.column.0))
            })
            .flatten();
        Picture { rows, cursor }
    }
}

fn ink(color: Color) -> Ink {
    match color {
        Color::Spec(rgb) => Ink::Rgb(rgb.r, rgb.g, rgb.b),
        Color::Indexed(index) => Ink::Indexed(index),
        Color::Named(name) => match name {
            NamedColor::Background => Ink::Background,
            // The first sixteen names are the indexed colors of the same numbers.
            name if (name as usize) < 16 => Ink::Indexed(name as u8),
            NamedColor::DimBlack
            | NamedColor::DimRed
            | NamedColor::DimGreen
            | NamedColor::DimYellow
            | NamedColor::DimBlue
            | NamedColor::DimMagenta
            | NamedColor::DimCyan
            | NamedColor::DimWhite => {
                Ink::Indexed((name as usize - NamedColor::DimBlack as usize) as u8)
            }
            _ => Ink::Foreground,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text(screen: &Screen, row: usize) -> String {
        screen.picture().rows[row]
            .iter()
            .map(|run| run.text.as_str())
            .collect::<String>()
            .trim_end()
            .to_owned()
    }

    #[test]
    fn output_lands_in_the_grid_and_color_splits_runs() {
        let mut screen = Screen::new(Size { cols: 20, rows: 4 });
        screen.feed(b"hi \x1b[31mred\x1b[0m\r\nnext");
        assert_eq!(text(&screen, 0), "hi red");
        assert_eq!(text(&screen, 1), "next");
        let picture = screen.picture();
        let red = picture.rows[0]
            .iter()
            .find(|run| run.text == "red")
            .unwrap();
        assert!(red.style.fg == Ink::Indexed(1) && red.col == 3);
        assert_eq!(picture.cursor, Some((1, 4)));
    }

    #[test]
    fn hostile_output_cannot_reach_the_clipboard_or_break_the_title() {
        let mut screen = Screen::new(Size { cols: 20, rows: 4 });
        // A clipboard write, a clipboard read, and a title with control bytes in it.
        let replies = screen.feed(b"\x1b]52;c;aGVsbG8=\x07\x1b]52;c;?\x07\x1b]0;a\x1b[31mb\x07");
        assert!(
            replies.is_empty(),
            "nothing is answered about the clipboard"
        );
        let title = screen.take_title().unwrap_or_default();
        assert!(title.chars().all(|c| !c.is_control()) && title.len() <= TITLE_CHARS);
        // Garbage and a cut-off sequence do not panic.
        screen.feed(&[0xff, 0x1b, b'[', 0x9b, 0x00, 0x1b]);
        screen.feed(b"\x1b[999;999H\x1b[2J");
    }

    #[test]
    fn a_device_query_is_answered_through_the_shell() {
        let mut screen = Screen::new(Size { cols: 20, rows: 4 });
        // "Where is the cursor?" is answered with its row and column.
        assert_eq!(screen.feed(b"ab\x1b[6n"), b"\x1b[1;3R");
    }
}
