//! Answers with pictures laid out as media cards: a picture on a line of its own becomes
//! a rounded cover on the left, and what follows it, up to the next picture or heading,
//! sits beside it. The rest is plain markdown. Pictures load only over http(s).

use gpui_kit::{component::ActiveTheme, *};

use super::links;

const COVER_W: f32 = 104.;
const COVER_H: f32 = 148.;

enum Block {
    Text(String),
    Card { url: String, text: String },
}

/// An answer is read, not scanned: lines a little taller than the interface's.
fn reading(text: impl IntoElement) -> AnyElement {
    div()
        .line_height(relative(1.65))
        .child(text)
        .into_any_element()
}

pub fn render(id: &str, text: &str, cx: &App) -> AnyElement {
    let blocks = split(text);
    if !blocks
        .iter()
        .any(|block| matches!(block, Block::Card { .. }))
    {
        return reading(links::markdown(
            SharedString::from(id.to_owned()),
            text.to_owned(),
        ));
    }
    let theme = cx.theme();
    let (border, surface) = (theme.border, theme.muted);
    let mut column = div().flex().flex_col().gap_4();
    for (index, block) in blocks.into_iter().enumerate() {
        let key = SharedString::from(format!("{id}-{index}"));
        column = column.child(match block {
            Block::Text(text) => reading(links::markdown(key, text)),
            Block::Card { url, text } => div()
                .flex()
                .flex_row()
                .gap_4()
                .pb_4()
                .border_b_1()
                .border_color(border)
                .child(
                    div()
                        .w(px(COVER_W))
                        .h(px(COVER_H))
                        .flex_none()
                        .rounded_xl()
                        .overflow_hidden()
                        .bg(surface)
                        .child(
                            img(SharedUri::from(url))
                                .size_full()
                                .object_fit(ObjectFit::Cover),
                        ),
                )
                .child(div().flex_1().min_w_0().child(links::markdown(key, text)))
                .into_any_element(),
        });
    }
    column.into_any_element()
}

/// The text cut at each picture that stands on a line of its own.
fn split(text: &str) -> Vec<Block> {
    let mut blocks = Vec::new();
    let mut current = String::new();
    let mut card: Option<String> = None;
    let flush = |current: &mut String, card: &mut Option<String>, blocks: &mut Vec<Block>| {
        let text = std::mem::take(current);
        match card.take() {
            Some(url) => blocks.push(Block::Card {
                url,
                text: text.trim().to_owned(),
            }),
            None if !text.trim().is_empty() => blocks.push(Block::Text(text)),
            None => {}
        }
    };
    for line in text.lines() {
        if let Some(url) = lone_picture(line) {
            flush(&mut current, &mut card, &mut blocks);
            card = Some(url);
            continue;
        }
        // A heading or a rule ends the card it follows.
        let trimmed = line.trim_start();
        if card.is_some() && (trimmed.starts_with('#') || trimmed.starts_with("---")) {
            flush(&mut current, &mut card, &mut blocks);
        }
        current.push_str(line);
        current.push('\n');
    }
    flush(&mut current, &mut card, &mut blocks);
    blocks
}

/// The address of `![alt](url)` alone on a line, when it is http(s).
fn lone_picture(line: &str) -> Option<String> {
    let line = line.trim();
    let rest = line.strip_prefix("![")?;
    let (_, rest) = rest.split_once("](")?;
    let url = rest.strip_suffix(')')?;
    let url = url.split_whitespace().next()?;
    (url.starts_with("https://") || url.starts_with("http://")).then(|| url.to_owned())
}

#[cfg(test)]
mod tests {
    use super::{Block, lone_picture, split};

    #[test]
    fn a_lone_picture_starts_a_card_until_the_next_heading() {
        let text = "Intro\n\n![Monster](https://e.com/m.jpg)\n**1. Monster**\n- a\n\n## Next\nmore";
        let blocks = split(text);
        assert_eq!(blocks.len(), 3);
        assert!(
            matches!(&blocks[1], Block::Card { url, text } if url == "https://e.com/m.jpg" && text.contains("- a"))
        );
        assert!(matches!(&blocks[2], Block::Text(text) if text.starts_with("## Next")));
        assert_eq!(lone_picture("![x](file:///etc/passwd)"), None);
        assert_eq!(lone_picture("text ![x](https://e.com/a.png)"), None);
    }
}
