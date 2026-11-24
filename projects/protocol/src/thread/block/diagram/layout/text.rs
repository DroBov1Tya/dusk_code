//! The texts of a diagram: the styles they are set in, how wide they are, and how a long
//! one wraps. A client measures with its own fonts where it can (`Measure`) and draws
//! each text in the size and weight its style names, so the boxes fit what it draws.

/// How a text of a diagram is set. Each client draws a style in these sizes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextStyle {
    /// A node's name: semibold.
    Label,
    /// A node's second line, a participant's or an event's: regular, quiet.
    Note,
    /// The words on an edge: regular, small.
    Edge,
    /// A group's name over its box: semibold, small capitals of the quiet colour.
    Group,
    /// The date of an event: medium, small, quiet.
    Axis,
}

impl TextStyle {
    /// The font size in pixels.
    pub fn size(self) -> f32 {
        match self {
            Self::Label => 13.,
            Self::Note => 11.5,
            Self::Edge | Self::Axis => 11.,
            Self::Group => 10.5,
        }
    }

    /// The height of one line in pixels.
    pub fn line_height(self) -> f32 {
        match self {
            Self::Label => 17.,
            Self::Note => 15.,
            Self::Edge | Self::Axis | Self::Group => 14.,
        }
    }

    /// Whether it is set heavier than regular: semibold for a label and a group's name,
    /// medium for a date.
    pub fn weight(self) -> u16 {
        match self {
            Self::Label | Self::Group => 600,
            Self::Axis => 500,
            Self::Note | Self::Edge => 400,
        }
    }
}

/// How wide a text is drawn in a style, in pixels.
pub trait Measure {
    fn width(&self, text: &str, style: TextStyle) -> f32;
}

/// A measure from the shapes of letters, for a client that cannot ask its fonts: a
/// proportional sans serif, a little wide so a box never cuts its text.
pub struct Estimate;

impl Measure for Estimate {
    fn width(&self, text: &str, style: TextStyle) -> f32 {
        let ems: f32 = text.chars().map(em_width).sum();
        let bold = if style.weight() >= 600 { 1.05 } else { 1. };
        ems * style.size() * bold
    }
}

/// About how wide a character is, in ems.
fn em_width(c: char) -> f32 {
    match c {
        'i' | 'l' | 'j' | '\'' | '|' | '!' | '.' | ',' | ':' | ';' => 0.28,
        ' ' | 'f' | 't' | 'r' | 'I' | '(' | ')' | '[' | ']' | '/' | '-' => 0.36,
        'm' | 'w' | 'M' | 'W' | '@' | '%' => 0.86,
        'A'..='Z' => 0.68,
        '0'..='9' => 0.58,
        c if c.is_ascii() => 0.55,
        // Wide letters of East Asian scripts, and emoji: a full em.
        c if ('\u{1100}'..='\u{115F}').contains(&c)
            || ('\u{2E80}'..='\u{A4CF}').contains(&c)
            || ('\u{AC00}'..='\u{D7A3}').contains(&c)
            || ('\u{F900}'..='\u{FAFF}').contains(&c)
            || ('\u{FE30}'..='\u{FE4F}').contains(&c)
            || ('\u{FF00}'..='\u{FF60}').contains(&c)
            || ('\u{1F300}'..='\u{1FAFF}').contains(&c) =>
        {
            1.
        }
        // Combining marks take no room of their own.
        c if ('\u{0300}'..='\u{036F}').contains(&c) => 0.,
        _ => 0.62,
    }
}

/// `text` in lines no wider than `width`, at most `lines` of them; the last ends in an
/// ellipsis when the text did not fit. Words break only where they must.
pub fn wrap(
    measure: &dyn Measure,
    text: &str,
    style: TextStyle,
    width: f32,
    lines: usize,
) -> Vec<String> {
    let fits = |line: &str| measure.width(line, style) <= width;
    let mut out: Vec<String> = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        let joined = if current.is_empty() {
            word.to_owned()
        } else {
            format!("{current} {word}")
        };
        if fits(&joined) {
            current = joined;
            continue;
        }
        if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
        // A word wider than a line is cut where it reaches the edge.
        let mut rest = word;
        while !fits(rest) {
            let cut = cut_at(measure, rest, style, width);
            out.push(rest[..cut].to_owned());
            rest = &rest[cut..];
        }
        current = rest.to_owned();
    }
    if !current.is_empty() {
        out.push(current);
    }
    if out.len() > lines.max(1) {
        out.truncate(lines.max(1));
        let last = out.last_mut().map(std::mem::take).unwrap_or_default();
        let mut kept = last.trim_end().to_owned();
        while !kept.is_empty() && !fits(&format!("{kept}\u{2026}")) {
            kept.pop();
        }
        if let Some(line) = out.last_mut() {
            *line = format!("{}\u{2026}", kept.trim_end());
        }
    }
    out
}

/// The byte index where `word` must be cut to fit `width`, at least one character in.
fn cut_at(measure: &dyn Measure, word: &str, style: TextStyle, width: f32) -> usize {
    let mut end = word.chars().next().map_or(word.len(), char::len_utf8);
    for (at, c) in word.char_indices().skip(1) {
        if measure.width(&word[..at + c.len_utf8()], style) > width {
            break;
        }
        end = at + c.len_utf8();
    }
    end
}

/// The widest of `lines`.
pub(super) fn widest(measure: &dyn Measure, lines: &[String], style: TextStyle) -> f32 {
    lines
        .iter()
        .map(|line| measure.width(line, style))
        .fold(0., f32::max)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_long_text_wraps_and_ends_in_an_ellipsis() {
        let lines = wrap(
            &Estimate,
            "one two three four five six seven eight nine ten",
            TextStyle::Label,
            60.,
            2,
        );
        assert_eq!(lines.len(), 2);
        assert!(lines[1].ends_with('\u{2026}'), "{lines:?}");
        for line in &lines {
            assert!(Estimate.width(line, TextStyle::Label) <= 60., "{line}");
        }
    }

    #[test]
    fn a_word_wider_than_a_line_is_cut() {
        let lines = wrap(&Estimate, &"x".repeat(40), TextStyle::Note, 50., 9);
        assert!(lines.len() > 1);
        assert_eq!(lines.concat(), "x".repeat(40));
    }
}
