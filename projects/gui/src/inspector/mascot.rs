//! The fox: the mascot of duskcode, one for each subagent. A fox is the animal of the dusk.
//! It sits upright, seen from the front, and lies down content when its work is done. It is built
//! from layers (the body, the tail, the eyes, and what it holds or what hovers over it), so
//! every state has frames of its own: it thinks with dots that fill and eyes turned up,
//! writes with a pencil, hammers, searches with a lens, reads a page, lies with its tail
//! curled and sparks when done, and stands dizzy under stars when it failed.

use std::time::Duration;

use gpui_kit::*;

use crate::chat::transcript::AgentState;

/// The fox's own picture is 36 by 32 cells; the margin on the left holds what it holds.
const W: usize = 32;
const H: usize = 29;
const AT_X: i32 = 0;
const AT_Y: i32 = 3;

type Grid = [[u8; W]; H];

// Generated from shapes by a script; edit the picture, not these rows.
const SIT_BODY: [&str; 26] = [
    "........................",
    "......k..........k......",
    ".....kbkk......kkbk.....",
    ".....kohhkkkkkkhhdk.....",
    ".....koeehhhhhheedk.....",
    ".....kooooooooooodk.....",
    ".....kooooooooooodk.....",
    "....khoovvoooovvoohk....",
    "....kovvvvoooovvvvdk....",
    "....kvvvvvvoovvvvvvk....",
    ".....kvvvvvnvvvvvvk.....",
    "....khdvvvvvvvvvvdhk....",
    ".....kkkkddddddkkkk.....",
    ".........kkkkkk.........",
    "........khhvvhhk........",
    ".......khovvvvohk.......",
    ".......koovvvvodk.......",
    ".......koovvvvodk.......",
    "......khooovvooohk......",
    ".....khooooddoooohk.....",
    "......kddbbkkbbddk......",
    ".......kkkk..kkkk.......",
    "........................",
    "........................",
    "........................",
    "........................",
];
const SIT_LOW: [&str; 26] = [
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "..kk....................",
    ".kvvk...................",
    "kvvvvk..................",
    "kvovvk..................",
    "kooovk..................",
    "kooodk..................",
    "koooohk.................",
    "kdoooohkk...............",
    ".kdoooohhk..............",
    "..kdooooohk.............",
    "...kdddddk..............",
    "....kkkkk...............",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
];
const SIT_MID: [&str; 26] = [
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "..kkk...................",
    ".kvvvk..................",
    ".kvvvk..................",
    "kvovk...................",
    "hooohk..................",
    "kooodk..................",
    "kooodk..................",
    "koooohk.................",
    "kddooohkk...............",
    ".kkoooohhk..............",
    "..kdoodoohk.............",
    "...kddkddk..............",
    "....kk.kk...............",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
];
const SIT_HIGH: [&str; 26] = [
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "...kkk..................",
    "..kvvvk.................",
    "..kvvvk.................",
    ".khvvvk.................",
    "khoovk..................",
    "kooodk..................",
    "kooodk..................",
    "kooodk..................",
    "koooohk.................",
    "kddooohk................",
    ".kkoooohk...............",
    "..kdoooohk..............",
    "...kdddoohk.............",
    "....kkkddk..............",
    ".......kk...............",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
];
const LIE_BODY: [&str; 26] = [
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "....k.......k...........",
    "...kbk....kkbk..........",
    "...kehkkkkhedk..........",
    "...keohhhhodk...........",
    "...koooooooohk..........",
    "..khvvvooovvvkkkkk......",
    "..kvvvvvnvvvvhhhhhkk....",
    "...kvvvvvvvvoooooohhk...",
    "....kvoovooooooooooohk..",
    "...kvvvvvvooooooooooohk.",
    "...kvvvvvvooooooooooodk.",
    "....kvvvvooooooooodddk..",
    ".....kkkkdddddddddkkk...",
    ".........kkkkkkkkk......",
    "........................",
    "........................",
    "........................",
];
const LIE_A: [&str; 26] = [
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "...................kkk..",
    "..................khhhk.",
    "..................kooohk",
    ".......kkk.kk..kkkhoodk.",
    "......kvvvkhhkkhhhooodk.",
    "......kvvvhoohkooodoodk.",
    "......kvvkdoodkdodkddk..",
    ".......kk.kddk.kdk.kk...",
    "...........kk...k.......",
    "........................",
];
const LIE_B: [&str; 26] = [
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "........................",
    "....................kk..",
    "...................khhk.",
    "..................khoohk",
    ".....kk...........kooodk",
    "....kvvkk..kk..kkkhoodk.",
    "....kvvvhkkhhkkhhhooodk.",
    ".....kkoohhoohkoooddddk.",
    "......kddkdoodkdodkkkk..",
    ".......kk.kddk.kdk......",
    "...........kk...k.......",
    "........................",
];

#[derive(Clone, Copy)]
enum Pose {
    Sit,
    Lie,
}

#[derive(Clone, Copy)]
enum Eyes {
    Open,
    /// Turned up: thinking.
    Up,
    /// Closed and smiling: content.
    Happy,
    /// A red cross each: it failed.
    Dizzy,
}

#[derive(Clone, Copy)]
enum Tail {
    Low,
    Mid,
    High,
}

/// What is in the margin: what it holds, or what hovers over it.
#[derive(Clone, Copy)]
enum Fx {
    /// Three dots of which this many are lit.
    Dots(u8),
    /// A pencil in one of two places, a line growing after it.
    Pencil(u8),
    /// A hammer up (`false`) or down.
    Hammer(bool),
    /// A lens over the page, in one of two places.
    Lens(u8),
    /// A page whose lines are read, one of two.
    Page(u8),
    /// Sparks of finished work.
    Sparkle(u8),
    /// Stars that circle, one of three places.
    Stars(u8),
}

#[derive(Clone, Copy)]
struct Frame {
    pose: Pose,
    /// How far the fox sits above its usual place, in cells.
    lift: i32,
    eyes: Eyes,
    tail: Tail,
    fx: Fx,
    /// How long the frame stays.
    ms: u64,
}

const fn frame(pose: Pose, lift: i32, eyes: Eyes, tail: Tail, fx: Fx, ms: u64) -> Frame {
    Frame {
        pose,
        lift,
        eyes,
        tail,
        fx,
        ms,
    }
}

/// What a tool's name says it does, for the thing the fox holds.
fn tool_fx(name: &str, step: u8) -> Fx {
    use dusk_protocol::agent::Doing;
    match Doing::of_tool(name) {
        Doing::Searching => Fx::Lens(step),
        Doing::Reading => Fx::Page(step),
        Doing::Planning => Fx::Pencil(step),
        _ => Fx::Hammer(step == 1),
    }
}

fn frames(state: &AgentState) -> Vec<Frame> {
    use {Eyes::*, Pose::*, Tail::*};
    match state {
        // It thinks with the dots filling, eyes turned up, and rocks a little.
        AgentState::Thinking => vec![
            frame(Sit, 0, Up, Low, Fx::Dots(1), 420),
            frame(Sit, 0, Up, Low, Fx::Dots(2), 420),
            frame(Sit, 1, Up, Mid, Fx::Dots(3), 420),
            frame(Sit, 0, Up, Mid, Fx::Dots(0), 420),
        ],
        // It writes: the pencil goes and comes, the tail wags quickly.
        AgentState::Writing => vec![
            frame(Sit, 0, Open, Mid, Fx::Pencil(0), 200),
            frame(Sit, 0, Open, High, Fx::Pencil(1), 200),
            frame(Sit, 0, Open, Mid, Fx::Pencil(0), 200),
            frame(Sit, 1, Open, Low, Fx::Pencil(1), 200),
        ],
        AgentState::Tool(name) => vec![
            frame(Sit, 0, Open, Mid, tool_fx(name, 0), 260),
            frame(Sit, 0, Open, High, tool_fx(name, 1), 260),
        ],
        // Done: it lies down, content, the tail curling, and the sparks come and go.
        AgentState::Done => vec![
            frame(Lie, 0, Happy, Low, Fx::Sparkle(0), 900),
            frame(Lie, 0, Happy, Mid, Fx::Sparkle(1), 900),
        ],
        AgentState::Failed => vec![
            frame(Sit, 0, Dizzy, Low, Fx::Stars(0), 320),
            frame(Sit, 0, Dizzy, Low, Fx::Stars(1), 320),
            frame(Sit, 0, Dizzy, Low, Fx::Stars(2), 320),
        ],
    }
}

fn put(grid: &mut Grid, x: i32, y: i32, cell: u8) {
    if (0..W as i32).contains(&x) && (0..H as i32).contains(&y) {
        grid[y as usize][x as usize] = cell;
    }
}

fn put_all(grid: &mut Grid, cells: &[(i32, i32)], cell: u8) {
    for (x, y) in cells {
        put(grid, *x, *y, cell);
    }
}

/// A layer onto the grid at the fox's place, `lift` cells up.
fn stamp(grid: &mut Grid, layer: &[&str], lift: i32) {
    for (y, row) in layer.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            if cell != b'.' {
                put(grid, x as i32 + AT_X, y as i32 + AT_Y - lift, cell);
            }
        }
    }
}

/// The two eyes on the face: a dark dash each. A cell that is not part of the eye is the
/// face's own color, cream when sitting and orange when lying.
fn eyes(grid: &mut Grid, pose: Pose, lift: i32, state: Eyes) {
    let (y, xs, rows): (i32, [i32; 2], [u8; 3]) = match pose {
        Pose::Sit => (
            7,
            [7, 16],
            match state {
                Eyes::Open => *b"vkk",
                Eyes::Up => *b"kkv",
                Eyes::Happy => *b"vkv",
                Eyes::Dizzy => *b"rvr",
            },
        ),
        Pose::Lie => (
            13,
            [5, 10],
            match state {
                Eyes::Open => *b"kko",
                Eyes::Up => *b"kko",
                Eyes::Happy => *b"oko",
                Eyes::Dizzy => *b"rwr",
            },
        ),
    };
    for x in xs {
        for (dy, cell) in rows.iter().enumerate() {
            put(grid, x + AT_X, y + dy as i32 + AT_Y - lift, *cell);
        }
    }
}

/// The picture of one frame: the tail, the body, the eyes, and what is in the margin.
fn compose(frame: Frame) -> Grid {
    let mut grid = [[b'.'; W]; H];
    let (tail, body): (&[&str], &[&str]) = match (frame.pose, frame.tail) {
        (Pose::Sit, Tail::Low) => (&SIT_LOW, &SIT_BODY),
        (Pose::Sit, Tail::Mid) => (&SIT_MID, &SIT_BODY),
        (Pose::Sit, Tail::High) => (&SIT_HIGH, &SIT_BODY),
        (Pose::Lie, Tail::Low) => (&LIE_A, &LIE_BODY),
        (Pose::Lie, _) => (&LIE_B, &LIE_BODY),
    };
    stamp(&mut grid, tail, frame.lift);
    stamp(&mut grid, body, frame.lift);
    eyes(&mut grid, frame.pose, frame.lift, frame.eyes);
    fx(&mut grid, frame.fx);
    grid
}

/// The margin, in canvas cells, to the left of the right hand (at 17, 22): dots `a`, handles
/// `b`, steel `g`, glass `c`, paper `W`, gold `y`, ink `k`.
/// A cell of what the fox holds, placed from the picture of the hand.
fn held(grid: &mut Grid, x: i32, y: i32, cell: u8) {
    put(grid, 35 - x, y + 2, cell);
}

fn held_all(grid: &mut Grid, cells: &[(i32, i32)], cell: u8) {
    for (x, y) in cells {
        held(grid, *x, *y, cell);
    }
}

fn fx(grid: &mut Grid, fx: Fx) {
    match fx {
        Fx::Dots(lit) => {
            for (index, x) in [10, 12, 14].into_iter().enumerate() {
                let cell = if (index as u8) < lit { b'a' } else { b'g' };
                put(grid, x, 3, cell);
            }
        }
        Fx::Pencil(step) => {
            let shift = i32::from(step);
            held_all(
                grid,
                &[
                    (16, 21),
                    (15, 20 - shift),
                    (14, 19 - shift),
                    (13, 18 - shift),
                ],
                b'y',
            );
            held(grid, 12, 17 - shift, b'k');
            // The line it leaves grows.
            for x in 0..(2 + 2 * shift) {
                held(grid, 8 + x, 24, b'W');
            }
        }
        Fx::Hammer(down) => {
            if down {
                held_all(grid, &[(16, 21), (15, 22), (14, 23)], b'b');
                held_all(grid, &[(10, 22), (11, 22), (12, 22), (13, 22)], b'g');
                held_all(grid, &[(10, 23), (11, 23), (12, 23), (13, 23)], b'g');
                held_all(grid, &[(9, 26), (10, 26)], b'y');
            } else {
                held_all(grid, &[(16, 21), (15, 20), (14, 19), (13, 18)], b'b');
                held_all(grid, &[(10, 15), (11, 15), (12, 15), (13, 15)], b'g');
                held_all(grid, &[(10, 16), (11, 16), (12, 16), (13, 16)], b'g');
            }
        }
        Fx::Lens(step) => {
            let (dx, dy) = if step == 0 { (0, 0) } else { (-1, 1) };
            let ring = [
                (9, 15),
                (10, 15),
                (11, 15),
                (8, 16),
                (12, 16),
                (8, 17),
                (12, 17),
                (9, 18),
                (10, 18),
                (11, 18),
            ];
            for (x, y) in ring {
                held(grid, x + dx, y + dy, b'c');
            }
            held_all(
                grid,
                &[(13 + dx, 19 + dy), (14 + dx, 20 + dy), (15 + dx, 21 + dy)],
                b'b',
            );
        }
        Fx::Page(step) => {
            for y in 14..22 {
                for x in 7..13 {
                    held(grid, x, y, b'W');
                }
            }
            let line = 16 + i32::from(step);
            held_all(
                grid,
                &[
                    (8, line),
                    (9, line),
                    (10, line),
                    (8, line + 2),
                    (9, line + 2),
                ],
                b'g',
            );
        }
        Fx::Sparkle(step) => {
            let (a, b) = if step == 0 {
                ((4, 10), (42, 12))
            } else {
                ((6, 8), (40, 14))
            };
            for (x, y) in [a, b] {
                put_all(
                    grid,
                    &[(x, y), (x - 1, y), (x + 1, y), (x, y - 1), (x, y + 1)],
                    b'y',
                );
            }
        }
        Fx::Stars(step) => {
            let places = [
                [(8, 3), (12, 2), (16, 3)],
                [(10, 3), (14, 2), (18, 3)],
                [(12, 3), (16, 2), (7, 2)],
            ];
            put_all(grid, &places[usize::from(step) % 3], b'y');
        }
    }
}

/// The color of a cell. `accent` is the color the caller gives the thinking dots.
fn paint(cell: u8, accent: Hsla) -> Option<Hsla> {
    let rgb = |hex: u32| Some(rgb(hex).into());
    match cell {
        b'o' => rgb(0xf08a3e),
        b'h' => rgb(0xf7a862),
        b'd' => rgb(0xcf6628),
        b'w' => rgb(0xf8dcb8),
        b'v' => rgb(0xfdf6ea),
        b'e' => rgb(0xd97a62),
        b'W' => rgb(0xf4f6fb),
        b'k' => rgb(0x2a1214),
        b's' => rgb(0x4a2a1c),
        b'n' => rgb(0x3a1016),
        b'y' => rgb(0xf2c14e),
        b'c' => rgb(0x7cc7e8),
        b'b' => rgb(0x5a2e1b),
        b'g' => rgb(0x8c93a6),
        b'r' => rgb(0xe5484d),
        b'a' => Some(accent),
        _ => None,
    }
}

/// The fox with each cell `unit` pixels wide: `color` is the accent of its dots.
pub fn render(
    id: impl Into<ElementId>,
    state: &AgentState,
    color: Hsla,
    unit: f32,
) -> impl IntoElement {
    let frames = frames(state);
    let total: u64 = frames.iter().map(|frame| frame.ms).sum();
    let grids: Vec<Grid> = frames.iter().map(|frame| compose(*frame)).collect();
    let times: Vec<u64> = frames.iter().map(|frame| frame.ms).collect();
    div()
        .w(px(W as f32 * unit))
        .h(px(H as f32 * unit))
        .flex_none()
        .with_animation(
            id,
            Animation::new(Duration::from_millis(total)).repeat(),
            move |figure, phase| {
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
            },
        )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[::core::prelude::v1::test]
    fn the_layers_fit_the_canvas_and_every_state_has_frames() {
        for layer in [
            &SIT_BODY[..],
            &SIT_LOW[..],
            &SIT_MID[..],
            &SIT_HIGH[..],
            &LIE_BODY[..],
            &LIE_A[..],
            &LIE_B[..],
        ] {
            assert_eq!(layer.len(), 26);
            assert!(layer.iter().all(|row| row.len() == 24));
        }
        let states = [
            AgentState::Thinking,
            AgentState::Writing,
            AgentState::Tool("run_command".into()),
            AgentState::Tool("search_files".into()),
            AgentState::Done,
            AgentState::Failed,
        ];
        for state in &states {
            let frames = frames(state);
            assert!(frames.len() >= 2 && frames.iter().all(|f| f.ms > 0));
            for frame in frames {
                compose(frame);
            }
        }
    }

    #[::core::prelude::v1::test]
    fn a_tool_is_held_by_what_it_does() {
        assert!(matches!(tool_fx("search_files", 0), Fx::Lens(_)));
        assert!(matches!(tool_fx("read_file", 0), Fx::Page(_)));
        assert!(matches!(tool_fx("plan_edit", 0), Fx::Pencil(_)));
        assert!(matches!(tool_fx("run_command", 1), Fx::Hammer(true)));
    }
}
