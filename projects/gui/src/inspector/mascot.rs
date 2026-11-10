//! A small 8-bit figure for a subagent, two frames per state: it bobs while it
//! thinks, walks while it writes, swings a hammer while a tool runs, stands still
//! when done and shows crossed eyes when it failed.

use std::time::Duration;

use gpui_kit::*;

use crate::chat::transcript::AgentState;

/// Cells per side; each sprite row has exactly this many characters.
const GRID: usize = 10;

type Sprite = [&'static str; GRID];

const STAND: Sprite = [
    "..........",
    "...####...",
    "..######..",
    "..#.##.#..",
    "..######..",
    "..######..",
    "...####...",
    "...#..#...",
    "..##..##..",
    "..........",
];
const BOB: Sprite = [
    "..........",
    "..........",
    "...####...",
    "..######..",
    "..#.##.#..",
    "..######..",
    "..######..",
    "...####...",
    "..##..##..",
    "..........",
];
const STEP: Sprite = [
    "..........",
    "...####...",
    "..######..",
    "..#.##.#..",
    "..######..",
    "..######..",
    "...####...",
    "....##....",
    "...#..#...",
    "..........",
];
const HAMMER_UP: Sprite = [
    "#.........",
    "##..####..",
    ".#.######.",
    ".#.#.##.#.",
    "..#######.",
    "...######.",
    "....####..",
    "....#..#..",
    "...##..##.",
    "..........",
];
const HAMMER_DOWN: Sprite = [
    "..........",
    "....####..",
    "...######.",
    "...#.##.#.",
    "#########.",
    "##.######.",
    "....####..",
    "....#..#..",
    "...##..##.",
    "..........",
];
const CROSSED: Sprite = [
    "..........",
    "...####...",
    "..######..",
    "..#.##.#..",
    "..##..##..",
    "..######..",
    "...####...",
    "...#..#...",
    "..##..##..",
    "..........",
];

fn frames(state: &AgentState) -> [Sprite; 2] {
    match state {
        AgentState::Thinking => [STAND, BOB],
        AgentState::Writing => [STAND, STEP],
        AgentState::Tool(_) => [HAMMER_UP, HAMMER_DOWN],
        AgentState::Done => [STAND, STAND],
        AgentState::Failed => [CROSSED, CROSSED],
    }
}

/// The figure with each cell `unit` pixels wide, so it can stand small in a row or large
/// on a stage.
pub fn render(
    id: impl Into<ElementId>,
    state: &AgentState,
    color: Hsla,
    unit: f32,
) -> impl IntoElement {
    let [first, second] = frames(state);
    let side = px(GRID as f32 * unit);
    div().size(side).flex_none().with_animation(
        id,
        Animation::new(Duration::from_millis(700)).repeat(),
        move |figure, phase| {
            let sprite = if phase < 0.5 { first } else { second };
            figure.child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        for (row, line) in sprite.iter().enumerate() {
                            for (column, cell) in line.bytes().enumerate() {
                                if cell != b'#' {
                                    continue;
                                }
                                let origin = point(
                                    bounds.origin.x + px(column as f32 * unit),
                                    bounds.origin.y + px(row as f32 * unit),
                                );
                                let size = size(px(unit), px(unit));
                                window.paint_quad(fill(Bounds { origin, size }, color));
                            }
                        }
                    },
                )
                .size_full(),
            )
        },
    )
}

#[cfg(test)]
mod tests {
    use super::{BOB, CROSSED, GRID, HAMMER_DOWN, HAMMER_UP, STAND, STEP};

    #[test]
    fn every_sprite_is_square() {
        for sprite in [STAND, BOB, STEP, HAMMER_UP, HAMMER_DOWN, CROSSED] {
            assert!(sprite.iter().all(|row| row.len() == GRID));
        }
    }
}
