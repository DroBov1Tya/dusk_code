//! The fox: the mascot of duskcode, one for each subagent. A fox is the animal of the dusk,
//! and this one keeps to it: a dark cloak with a crimson lining and a high collar, a clasp
//! in the agent's color, and narrow eyes that glow. It is drawn in 8-bit cells and holds
//! nothing: what it does shows in the fox itself. The eyes turn, the ears twitch, the tail
//! flicks, the clasp pulses and the hem of the cloak lifts, each activity in its own way.

use std::time::Duration;

use dusk_protocol::agent::Doing;
use gpui_kit::*;

use crate::pages::chat::transcript::AgentState;

const W: usize = 22;
const H: usize = 22;

type Grid = [[u8; W]; H];

/// Cells laid over the picture at a place; a space keeps what is under it.
type Patch = (usize, usize, &'static [&'static str]);

// The fox at rest without its tail. The head and the cloak mirror about the middle; the
// last two columns are room for the tail.
const BODY: [&str; H] = [
    "...k............k.....",
    "..kdk..........kdk....",
    "..kddk........kddk....",
    "..koeokkkkkkkkoeok....",
    "..kooooooooooooook....",
    "..kooooooooooooook....",
    "..kooooooooooooook....",
    ".khoooooooooooooohk...",
    "kCkvvoooooooooovvkCk..",
    "kCCkvvvvnnnnvvvvkCCk..",
    "kcCCkvvvvvvvvvvkCCck..",
    "kcCCCkkkkkkkkkkCCCck..",
    "kcCCCCCCmaamCCCCCCck..",
    "kccCCCCmvvvvmCCCCcck..",
    "kccCCCmovvvvomCCCcck..",
    "kccCCCmoovvoomCCCcck..",
    "kccCCmooovvooomCCcck..",
    "kcccCmoooooooomCccck..",
    "kcccCmookookoomCccck..",
    "kcccCmkookkookmCccck..",
    "kckcCkkvvkkvvkkCckck..",
    "k.k.kk.kk..kk.kk.k.k..",
];

#[derive(Clone, Copy)]
enum Eyes {
    /// Narrow and slanted: at rest.
    Sly,
    /// Under a lowered lid: intent on the work.
    Narrow,
    Wide,
    /// Turned up: thinking.
    Up,
    Left,
    Right,
    Closed,
    /// Closed and content.
    Happy,
    /// Gone out: it failed.
    Dim,
}

#[derive(Clone, Copy)]
enum Ears {
    Up,
    TwitchLeft,
    TwitchRight,
    Droop,
}

#[derive(Clone, Copy)]
enum Gem {
    Lit,
    Dark,
    Hot,
}

#[derive(Clone, Copy)]
struct Frame {
    eyes: Eyes,
    ears: Ears,
    /// The tail's tip flicked aside.
    flick: bool,
    gem: Gem,
    /// The cloak's hem lifted, as in a gust.
    flare: bool,
    /// How long the frame stays.
    ms: u64,
}

const fn frame(eyes: Eyes, ears: Ears, flick: bool, gem: Gem, flare: bool, ms: u64) -> Frame {
    Frame {
        eyes,
        ears,
        flick,
        gem,
        flare,
        ms,
    }
}

/// The name of the motion, its frames, and whether it goes round.
fn frames(state: &AgentState) -> (&'static str, Vec<Frame>, bool) {
    use Ears::{Droop, TwitchLeft, TwitchRight};
    use Eyes::*;
    use Gem::*;
    let up = Ears::Up;
    match state {
        // Looks up while the clasp pulses; an ear twitches at a thought.
        AgentState::Thinking => (
            "thinking",
            vec![
                frame(Eyes::Up, up, false, Lit, false, 320),
                frame(Eyes::Up, up, false, Dark, false, 320),
                frame(Eyes::Up, TwitchLeft, false, Lit, false, 160),
                frame(Eyes::Up, up, false, Dark, false, 320),
            ],
            true,
        ),
        // Intent eyes and a quick tail, like a hand that writes.
        AgentState::Writing => (
            "writing",
            vec![
                frame(Narrow, up, false, Lit, false, 220),
                frame(Narrow, up, true, Lit, false, 160),
                frame(Narrow, up, false, Lit, false, 220),
                frame(Closed, up, true, Lit, false, 100),
            ],
            true,
        ),
        AgentState::Tool(name) => {
            let doing = Doing::of_tool(name);
            let frames = match doing {
                // Wide eyes dart from side to side, the ear on that side turning with them.
                Doing::Searching => vec![
                    frame(Left, TwitchLeft, false, Lit, false, 300),
                    frame(Wide, up, false, Lit, false, 160),
                    frame(Right, TwitchRight, false, Lit, false, 300),
                    frame(Wide, up, false, Lit, false, 160),
                ],
                // The eyes go along a line, then a blink turns the page.
                Doing::Reading => vec![
                    frame(Left, up, false, Lit, false, 360),
                    frame(Sly, up, false, Lit, false, 300),
                    frame(Right, up, false, Lit, false, 360),
                    frame(Closed, up, false, Lit, false, 120),
                ],
                // Eyes shut, the clasp breathing, the tail slow.
                Doing::Planning => vec![
                    frame(Closed, up, false, Lit, false, 520),
                    frame(Closed, up, true, Dark, false, 520),
                ],
                // The cloak lifts and the clasp burns while the command runs.
                _ => vec![
                    frame(Wide, up, false, Hot, true, 200),
                    frame(Wide, up, false, Lit, false, 200),
                    frame(Wide, up, true, Hot, true, 200),
                    frame(Narrow, up, false, Lit, false, 200),
                ],
            };
            (doing.label(), frames, true)
        }
        // Two wags of the tail, and it stays content.
        AgentState::Done => (
            "done",
            vec![
                frame(Happy, up, true, Lit, false, 160),
                frame(Happy, up, false, Lit, false, 160),
                frame(Happy, up, true, Lit, false, 160),
                frame(Happy, up, false, Lit, false, 1),
            ],
            false,
        ),
        // The light goes out and the ears fall.
        AgentState::Failed => (
            "failed",
            vec![
                frame(Dim, TwitchLeft, false, Dark, false, 160),
                frame(Dim, Droop, false, Dark, false, 240),
                frame(Dim, Droop, false, Dark, false, 1),
            ],
            false,
        ),
    }
}

fn lay(grid: &mut Grid, patch: Patch) {
    let (x, y, rows) = patch;
    for (dy, row) in rows.iter().enumerate() {
        for (dx, cell) in row.bytes().enumerate() {
            if cell != b' ' && x + dx < W && y + dy < H {
                grid[y + dy][x + dx] = cell;
            }
        }
    }
}

/// Both eyes: the right one is the left one mirrored, except when they look aside.
fn eyes(grid: &mut Grid, eyes: Eyes) {
    let (left, right): (&[&str], &[&str]) = match eyes {
        Eyes::Sly => (&["E  ", " EE"], &["  E", "EE "]),
        Eyes::Narrow => (&["kkk", " EE"], &["kkk", "EE "]),
        Eyes::Wide => (&[" EE", " EE"], &["EE ", "EE "]),
        Eyes::Up => (&["EEE", "ooo"], &["EEE", "ooo"]),
        Eyes::Left => (&["ooo", "EEk"], &["ooo", "EEk"]),
        Eyes::Right => (&["ooo", "kEE"], &["ooo", "kEE"]),
        Eyes::Closed => (&["ooo", "kkk"], &["ooo", "kkk"]),
        Eyes::Happy => (&[" k ", "k k"], &[" k ", "k k"]),
        Eyes::Dim => (&["ooo", "xkx"], &["ooo", "xkx"]),
    };
    lay(grid, (4, 5, left));
    lay(grid, (13, 5, right));
}

/// The picture of one frame.
fn compose(frame: Frame) -> Grid {
    let mut grid = [[b'.'; W]; H];
    for (y, row) in BODY.iter().enumerate() {
        grid[y].copy_from_slice(row.as_bytes());
    }
    let tail: &[&str] = if frame.flick {
        &[
            ".kk..", "kvvk.", "kvhk.", "kohok", "koook", "koodk", "koodk", "kodk.", "kdk..",
            "kk...",
        ]
    } else {
        &[
            "..kk.", ".kvvk", ".kvhk", "kohok", "koook", "koodk", "koodk", "kodk.", "kdk..",
            "kk...",
        ]
    };
    lay(&mut grid, (17, 10, tail));
    if frame.flare {
        lay(
            &mut grid,
            (0, 20, &["kcckCkkvvkkvvkkCkcck", ".k.kk..kk..kk..kk.k."]),
        );
    }
    match frame.ears {
        Ears::Up => {}
        Ears::TwitchLeft => lay(&mut grid, (1, 0, &["..k...", ".kdk..", ".kddk."])),
        Ears::TwitchRight => lay(&mut grid, (13, 0, &["...k..", "..kdk.", ".kddk."])),
        Ears::Droop => {
            lay(&mut grid, (2, 0, &["....", "....", "kkkk"]));
            lay(&mut grid, (14, 0, &["....", "....", "kkkk"]));
        }
    }
    eyes(&mut grid, frame.eyes);
    match frame.gem {
        Gem::Lit => {}
        Gem::Dark => lay(&mut grid, (9, 12, &["AA"])),
        Gem::Hot => lay(&mut grid, (9, 12, &["FF"])),
    }
    grid
}

/// The color of a cell. `accent` is the agent's color, worn as the clasp.
fn paint(cell: u8, accent: Hsla) -> Option<Hsla> {
    let rgb = |hex: u32| Some(rgb(hex).into());
    match cell {
        // Fur: body, light, dark, cream, inner ear.
        b'o' => rgb(0xc94a2c),
        b'h' => rgb(0xe26a3c),
        b'd' => rgb(0x7d2a26),
        b'v' => rgb(0xf0dccb),
        b'e' => rgb(0x5a1c2e),
        // Outline and nose.
        b'k' | b'n' => rgb(0x1a0e1e),
        // Cloak: lit, shaded, lining.
        b'C' => rgb(0x35264f),
        b'c' => rgb(0x231833),
        b'm' => rgb(0x9c1f3c),
        // Eyes: glowing, gone out.
        b'E' => rgb(0xff6a4d),
        b'x' => rgb(0x7a5a66),
        // The clasp: lit, dark, burning.
        b'a' => Some(accent),
        b'A' => Some(accent.opacity(0.35)),
        b'F' => rgb(0xffc27a),
        _ => None,
    }
}

/// The fox with each cell `unit` pixels wide; `color` is the agent's, worn as the clasp.
pub fn render(
    id: impl Into<ElementId>,
    state: &AgentState,
    color: Hsla,
    unit: f32,
) -> impl IntoElement {
    let (motion, frames, repeat) = frames(state);
    let total: u64 = frames.iter().map(|frame| frame.ms).sum();
    let grids: Vec<Grid> = frames.iter().map(|frame| compose(*frame)).collect();
    let times: Vec<u64> = frames.iter().map(|frame| frame.ms).collect();
    let mut animation = Animation::new(Duration::from_millis(total)).with_max_fps(20.);
    if repeat {
        animation = animation.repeat();
    }
    // The outer ID belongs to the agent; a new activity starts its own timeline.
    div().id(id).flex_none().child(
        div()
            .w(px(W as f32 * unit))
            .h(px(H as f32 * unit))
            .flex_none()
            .with_animation(motion, animation, move |figure, phase| {
                let mut at = (phase * total as f32) as u64;
                let mut index = 0;
                for (frame, ms) in times.iter().enumerate() {
                    index = frame;
                    if at < *ms {
                        break;
                    }
                    at -= ms;
                }
                let grid = grids[index];
                figure.child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            for (row, line) in grid.iter().enumerate() {
                                for (column, cell) in line.iter().enumerate() {
                                    let Some(fill_color) = paint(*cell, color) else {
                                        continue;
                                    };
                                    let origin = point(
                                        bounds.origin.x + px(column as f32 * unit),
                                        bounds.origin.y + px(row as f32 * unit),
                                    );
                                    let cell_size = size(px(unit), px(unit));
                                    window.paint_quad(fill(
                                        Bounds {
                                            origin,
                                            size: cell_size,
                                        },
                                        fill_color,
                                    ));
                                }
                            }
                        },
                    )
                    .size_full(),
                )
            }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn states() -> Vec<AgentState> {
        vec![
            AgentState::Thinking,
            AgentState::Writing,
            AgentState::Tool("search_files".into()),
            AgentState::Tool("read_file".into()),
            AgentState::Tool("plan_edit".into()),
            AgentState::Tool("run_command".into()),
            AgentState::Done,
            AgentState::Failed,
        ]
    }

    #[::core::prelude::v1::test]
    fn the_cloaked_fox_mirrors_and_every_cell_has_a_color() {
        for row in BODY {
            let cells = row.as_bytes();
            assert_eq!(cells.len(), W);
            let fox = &cells[..W - 2];
            let mirrored: Vec<u8> = fox.iter().rev().copied().collect();
            assert_eq!(fox, &mirrored[..], "{row}");
        }
        for state in states() {
            let (_, frames, _) = frames(&state);
            assert!(frames.len() >= 2 && frames.iter().all(|frame| frame.ms > 0));
            for frame in frames {
                for cell in compose(frame).iter().flatten() {
                    assert!(*cell == b'.' || paint(*cell, gpui_kit::white()).is_some());
                }
            }
        }
    }

    #[::core::prelude::v1::test]
    fn each_activity_moves_in_its_own_way() {
        let motions: Vec<Vec<Grid>> = states()
            .iter()
            .map(|state| frames(state).1.into_iter().map(compose).collect())
            .collect();
        for (index, motion) in motions.iter().enumerate() {
            assert!(!motions[..index].contains(motion));
        }
    }
}
