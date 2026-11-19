//! A word with a light passing through its letters: each letter is the muted colour
//! mixed toward the text's by how near the light is. Only colours change, so the word
//! keeps its width and nothing beside it moves.

use std::time::Duration;

use gpui_kit::{component::Colorize, *};

use crate::ui::looping::Loop;

/// One pass and the rest after it.
const CYCLE: Duration = Duration::from_millis(2300);
/// The share of the cycle the light travels in.
const TRAVEL: f32 = 0.61;
/// How wide the light is, in letters.
const WIDTH: f32 = 2.2;

pub fn word(text: SharedString, base: Hsla, bright: Hsla) -> impl IntoElement {
    div().flex_none().looping(CYCLE, move |holder, phase| {
        let highlights = light(&text, phase)
            .into_iter()
            .map(|(range, near)| (range, base.mix_oklab(bright, near).into()));
        holder.child(StyledText::new(text.clone()).with_highlights(highlights.collect::<Vec<_>>()))
    })
}

/// How near the light is to each letter of `text` at `phase`, from 0 to 1, with the
/// letter's bytes. At the start of the cycle, where the word stands under reduced
/// motion, no letter is lit.
fn light(text: &str, phase: f32) -> Vec<(std::ops::Range<usize>, f32)> {
    let letters = text.chars().count() as f32;
    let travelled = (phase / TRAVEL).min(1.);
    // Eased at both ends, from before the first letter to past the last.
    let eased = 0.5 - 0.5 * (std::f32::consts::PI * travelled).cos();
    let at = -3. * WIDTH + (letters + 6. * WIDTH) * eased;
    text.char_indices()
        .enumerate()
        .map(|(index, (start, letter))| {
            let far = (index as f32 - at) / WIDTH;
            (start..start + letter.len_utf8(), (-far * far).exp())
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::light;

    #[test]
    fn the_light_crosses_the_word_and_leaves_it_dark_at_both_ends() {
        let near = |phase| -> Vec<f32> {
            light("Thinking", phase)
                .into_iter()
                .map(|(_, near)| near)
                .collect()
        };
        assert!(near(0.).iter().all(|near| *near < 0.01));
        assert!(near(0.9).iter().all(|near| *near < 0.01));
        let middle = near(super::TRAVEL / 2.);
        assert!(middle[3] > 0.8 || middle[4] > 0.8, "{middle:?}");
        assert!(middle[0] < middle[3]);
    }

    #[test]
    fn each_letter_keeps_its_own_bytes() {
        let ranges: Vec<_> = light("d\u{e9}j\u{e0}", 0.3)
            .into_iter()
            .map(|(range, _)| range)
            .collect();
        assert_eq!(ranges, [0..1, 1..3, 3..4, 4..6]);
    }
}
