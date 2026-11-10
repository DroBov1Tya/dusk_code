//! The fox: the mascot of duskcode, drawn in 8-bit cells, one for each subagent. A fox is
//! the animal of the dusk. It is built from layers (the fox itself, its eyes, its tail, and
//! what it holds or what hovers over it), so every state gets its own motion from a few
//! frames: it thinks with dots that fill, writes with a pencil, hammers, searches with a
//! lens, reads a page, rests with its eyes closed and sparks when done, and is dizzy
//! under stars when it failed.

use std::time::Duration;

use gpui_kit::*;

use crate::chat::transcript::AgentState;

/// The canvas: the fox is 16 by 16 cells in the middle; the margin holds what it holds.
const W: usize = 20;
const H: usize = 18;
/// Where the fox's own corner stands on the canvas.
const AT: (i32, i32) = (2, 2);

type Grid = [[u8; W]; H];

/// The fox with its eyes left as `E`, drawn by `eyes`.
const FOX: [&str; 16] = [
    "..kk........kk..",
    "..kook....kook..",
    "..kooooooooook..",
    "..oooooooooooo..",
    "..oooEooooEooo..",
    ".wwooooooooooww.",
    ".wwwwoooooowwww.",
    "..wwwwwkkwwwww..",
    "...wwwwwwwwww...",
    "....oowwwwoo....",
    "...oowwwwwwoo...",
    "..ooowwwwwwooo..",
    "..oooowwwwoooo..",
    "..oooooooooooo..",
    "..kkkooooookkk..",
    "..kkk......kkk..",
];

#[derive(Clone, Copy)]
enum Eyes {
    Open,
    /// Looking up and to the side: thinking.
    Up,
    /// Closed: at rest.
    Closed,
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
    /// How far the fox sits above its usual place, in cells.
    lift: i32,
    eyes: Eyes,
    tail: Tail,
    fx: Fx,
    /// How long the frame stays.
    ms: u64,
}

const fn frame(lift: i32, eyes: Eyes, tail: Tail, fx: Fx, ms: u64) -> Frame {
    Frame {
        lift,
        eyes,
        tail,
        fx,
        ms,
    }
}

/// What a tool's name says it does, for the thing the fox holds.
fn tool_fx(name: &str, step: u8) -> Fx {
    let has = |words: &[&str]| words.iter().any(|word| name.contains(word));
    if has(&["search", "grep", "find", "glob"]) {
        Fx::Lens(step)
    } else if has(&["read", "fetch", "open", "load", "navigate", "snapshot"]) {
        Fx::Page(step)
    } else if has(&["write", "edit", "plan", "task", "remember", "name"]) {
        Fx::Pencil(step)
    } else {
        Fx::Hammer(step == 1)
    }
}

fn frames(state: &AgentState) -> Vec<Frame> {
    use {Eyes::*, Tail::*};
    match state {
        // It thinks with the dots filling, and rocks a little.
        AgentState::Thinking => vec![
            frame(0, Up, Low, Fx::Dots(1), 420),
            frame(0, Up, Low, Fx::Dots(2), 420),
            frame(1, Up, Mid, Fx::Dots(3), 420),
            frame(0, Up, Mid, Fx::Dots(0), 420),
        ],
        // It writes: the pencil goes and comes, the tail wags quickly.
        AgentState::Writing => vec![
            frame(0, Open, Mid, Fx::Pencil(0), 200),
            frame(0, Open, High, Fx::Pencil(1), 200),
            frame(0, Open, Mid, Fx::Pencil(0), 200),
            frame(1, Open, Low, Fx::Pencil(1), 200),
        ],
        AgentState::Tool(name) => vec![
            frame(0, Open, Mid, tool_fx(name, 0), 260),
            frame(0, Open, High, tool_fx(name, 1), 260),
        ],
        // At rest: it breathes, the tail up, the sparks come and go.
        AgentState::Done => vec![
            frame(0, Closed, High, Fx::Sparkle(0), 900),
            frame(1, Closed, High, Fx::Sparkle(1), 900),
        ],
        AgentState::Failed => vec![
            frame(0, Dizzy, Low, Fx::Stars(0), 320),
            frame(0, Dizzy, Low, Fx::Stars(1), 320),
            frame(0, Dizzy, Low, Fx::Stars(2), 320),
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

/// The picture of one frame: the fox, its eyes and tail, and what is in the margin.
fn compose(frame: Frame) -> Grid {
    let mut grid = [[b'.'; W]; H];
    let at = |x: i32, y: i32| (x + AT.0, y + AT.1 - frame.lift);
    for (y, row) in FOX.iter().enumerate() {
        for (x, cell) in row.bytes().enumerate() {
            let (cx, cy) = at(x as i32, y as i32);
            match cell {
                b'.' | b'E' => {}
                other => put(&mut grid, cx, cy, other),
            }
        }
    }
    // The eyes: two cells of ink in each case, other pixels for the cross.
    let eye = |grid: &mut Grid, x: i32, y: i32, cell: u8| {
        let (cx, cy) = at(x, y);
        put(grid, cx, cy, cell);
    };
    for x in [5, 10] {
        match frame.eyes {
            Eyes::Open => eye(&mut grid, x, 4, b'k'),
            Eyes::Up => {
                eye(&mut grid, x + 1, 3, b'k');
                eye(&mut grid, x + 1, 4, b'o');
                eye(&mut grid, x, 4, b'o');
            }
            Eyes::Closed => eye(&mut grid, x, 4, b'd'),
            Eyes::Dizzy => {
                eye(&mut grid, x, 4, b'o');
                for (dx, dy) in [(0, -1), (-1, 0), (1, 0), (0, 1)] {
                    eye(&mut grid, x + dx, 4 + dy, b'r');
                }
            }
        }
    }
    let tail: &[(i32, i32)] = match frame.tail {
        Tail::Low => &[(14, 13), (15, 13)],
        Tail::Mid => &[(14, 12), (15, 11)],
        Tail::High => &[(14, 12), (15, 11), (15, 10)],
    };
    for (x, y) in tail {
        let (cx, cy) = at(*x, *y);
        put(&mut grid, cx, cy, b'o');
    }
    let tip = match frame.tail {
        Tail::Low => (16, 12),
        Tail::Mid => (16, 10),
        Tail::High => (16, 9),
    };
    let (tx, ty) = at(tip.0, tip.1);
    put(&mut grid, tx, ty, b'w');
    fx(&mut grid, frame.fx);
    grid
}

/// The margin, in canvas cells: dots `a`, pencil and hammer handles `b`, steel `g`, glass
/// `c`, paper `w`, gold `y`, ink `k`.
fn fx(grid: &mut Grid, fx: Fx) {
    match fx {
        Fx::Dots(lit) => {
            for (index, x) in [13, 15, 17].into_iter().enumerate() {
                let cell = if (index as u8) < lit { b'a' } else { b'g' };
                put(grid, x, 0, cell);
            }
        }
        Fx::Pencil(step) => {
            let shift = i32::from(step);
            put_all(
                grid,
                &[(16, 9 - shift), (17, 8 - shift), (18, 7 - shift)],
                b'y',
            );
            put(grid, 19, 6 - shift, b'k');
            // The line it leaves grows.
            let length = 2 + 2 * shift;
            for x in 0..length {
                put(grid, 16 + x, 12, b'w');
            }
        }
        Fx::Hammer(down) => {
            if down {
                put_all(grid, &[(16, 9), (17, 9), (18, 9)], b'b');
                put_all(grid, &[(19, 8), (19, 9), (19, 10)], b'g');
                put_all(grid, &[(18, 12), (19, 12)], b'y');
            } else {
                put_all(grid, &[(16, 9), (17, 8), (18, 7)], b'b');
                put_all(grid, &[(18, 5), (19, 5), (18, 6), (19, 6)], b'g');
            }
        }
        Fx::Lens(step) => {
            let (dx, dy) = if step == 0 { (0, 0) } else { (-1, 1) };
            let ring = [
                (16, 6),
                (17, 6),
                (18, 6),
                (15, 7),
                (19, 7),
                (15, 8),
                (19, 8),
                (16, 9),
                (17, 9),
                (18, 9),
            ];
            for (x, y) in ring {
                put(grid, x + dx, y + dy, b'c');
            }
            put_all(grid, &[(19 + dx, 10 + dy)], b'b');
        }
        Fx::Page(step) => {
            for y in 6..11 {
                for x in 16..20 {
                    put(grid, x, y, b'w');
                }
            }
            let line = 7 + i32::from(step);
            put_all(
                grid,
                &[(17, line), (18, line), (17, line + 2), (18, line + 2)],
                b'g',
            );
        }
        Fx::Sparkle(step) => {
            let (a, b) = if step == 0 {
                ((1, 3), (18, 2))
            } else {
                ((2, 1), (17, 4))
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
                [(5, 0), (9, 1), (13, 0)],
                [(7, 0), (11, 1), (15, 0)],
                [(9, 0), (13, 1), (5, 1)],
            ];
            put_all(grid, &places[usize::from(step) % 3], b'y');
        }
    }
}

/// The color of a cell. `a` is the accent the caller gives (the signal color for work).
fn paint(cell: u8, accent: Hsla) -> Option<Hsla> {
    let rgb = |hex: u32| Some(rgb(hex).into());
    match cell {
        b'o' => rgb(0xe9793a),
        b'd' => rgb(0xb4561e),
        b'w' => rgb(0xf6e9d2),
        b'k' => rgb(0x2a1d1a),
        b'y' => rgb(0xf2c14e),
        b'c' => rgb(0x7cc7e8),
        b'b' => rgb(0x9a6a3c),
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
                    if at < *ms {
                        index = frame;
                        break;
                    }
                    at -= ms;
                    index = frame;
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
    fn the_fox_is_square_and_every_state_has_frames() {
        assert!(FOX.iter().all(|row| row.len() == 16));
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
            // Composing every frame must stay inside the canvas without panicking.
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
