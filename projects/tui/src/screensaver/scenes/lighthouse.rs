//! A lighthouse on a storm coast, with a turning light and waves breaking below.

use std::f32::consts::TAU;

use crate::screensaver::paint::{Pt, Rgb, glow, gradient, noise, smooth};

const HORIZON: f32 = 0.68;
const LANTERN_Y: f32 = 0.316;
const BALCONY_HALF_WIDTH: f32 = 0.069;
const SWEEP_PERIOD: f32 = 17.0;
const AMBER: Rgb = Rgb::hex(0xffcc83);

pub fn storm_lighthouse(p: Pt, t: f32) -> Rgb {
    // Keep the tower in the window on tall terminals without stretching it.
    let room = (p.aspect * 0.5 - BALCONY_HALF_WIDTH).max(0.0);
    let shift = (p.aspect * 0.19).min(0.25).min(room);
    let coast = p.at(p.x + shift, p.y);
    if let Some(color) = tower(coast) {
        return color;
    }

    // The headland drops into the surf just to the right of the tower.
    let ridge = noise(coast.x * 18.0, 2.5);
    let cliff_edge = coast.x + ridge * 0.032;
    let crest = 0.59 + smooth(0.01, 0.27, cliff_edge) * 0.18 + ridge * 0.055;
    let foot = 0.98 - smooth(-0.36, 0.35, cliff_edge) * 0.24 + (ridge - 0.5) * 0.033;
    if p.y > crest && p.y < foot {
        let rock = noise(coast.x * 24.0, p.y * 27.0);
        let strata = (p.y * 107.0 + coast.x * 25.0 + rock * 6.0).sin() * 0.5 + 0.5;
        let light = smooth(crest + 0.25, crest, p.y) * (0.14 + strata * 0.40 + rock * 0.26);
        return Rgb::hex(0x0c1921).mix(Rgb::hex(0x5d6475), light);
    }

    if p.y < HORIZON {
        return sky(coast, t);
    }
    sea(coast, t, foot, cliff_edge)
}

fn sky(p: Pt, t: f32) -> Rgb {
    let dusk = gradient(
        &[
            (0.0, Rgb::hex(0x243049)),
            (0.32, Rgb::hex(0x3f4f67)),
            (0.55, Rgb::hex(0x76788a)),
            (HORIZON, Rgb::hex(0xc2a198)),
        ],
        p.y,
    );
    let wind = p.x * 2.1 - t * 0.019;
    let cloud = noise(wind + p.y * 2.4, p.y * 9.0) * 0.70
        + noise(wind * 3.1 + p.y * 4.0, p.y * 23.0) * 0.30;
    let bank = smooth(0.32, 0.64, cloud) * smooth(HORIZON, 0.46, p.y);
    let fringe = smooth(0.28, 0.46, cloud) * smooth(0.66, 0.45, p.y) * smooth(0.16, 0.54, p.y);
    let mut color = dusk
        .mix(Rgb::hex(0xa88791), fringe * 0.22)
        .mix(Rgb::hex(0x101d30), bank * 0.88);

    let headland = HORIZON - 0.008 - noise(p.x * 3.2, 1.5) * 0.045;
    let distant_shore = smooth(0.32, 0.74, p.x) * smooth(headland, headland + 0.009, p.y);
    color = color.mix(Rgb::hex(0x4f5e70), distant_shore * 0.7);

    // The projected beam changes its pitch and opens out as it sweeps toward us.
    let turn = t * TAU / SWEEP_PERIOD;
    let toward = (turn.cos() + 1.0) * 0.5;
    let pitch = -0.15 + 0.22 * turn.sin();
    let reach = 1.25 + 0.65 * toward;
    let distance = p.x.max(0.0);
    let width = 0.011 + distance * (0.033 + toward * 0.066);
    let axis = LANTERN_Y + pitch * distance;
    let beam = smooth(width, width * 0.28, (p.y - axis).abs())
        * smooth(reach, 0.04, distance)
        * smooth(0.0, 0.035, p.x)
        * (0.21 + toward * 0.40);
    let haze = glow(p, 0.0, LANTERN_Y, 0.13) * 0.58;
    color.mix(AMBER, beam * (0.67 + cloud * 0.33) + haze)
}

fn sea(p: Pt, t: f32, foot: f32, cliff_edge: f32) -> Rgb {
    let depth = (p.y - HORIZON) / (1.0 - HORIZON);
    let color = gradient(
        &[
            (0.0, Rgb::hex(0x556576)),
            (0.38, Rgb::hex(0x213d51)),
            (1.0, Rgb::hex(0x122832)),
        ],
        depth,
    );

    // The fronts bunch up toward the horizon, then move faster in the foreground.
    let bend = noise(p.x * 3.4 - t * 0.038, depth * 4.0) - 0.5;
    let phase = (depth + 0.065).ln() * 7.5 - t * 0.78 + p.x * 2.7 + bend * 1.1;
    let wave = (phase.sin() + 1.0) * 0.5;
    let crest = smooth(0.83, 1.0, wave);
    let broken = noise(p.x * 18.0 + t * 0.075, phase * 0.2);
    let shore = smooth(0.38, 0.12, cliff_edge) * smooth(0.07, 0.0, (p.y - foot).abs());
    let spray = noise(p.x * 62.0 + t * 0.15, depth * 82.0);
    let whitecap = smooth(0.30, 0.73, broken) * (0.50 + spray * 0.50);
    let foam = crest * (whitecap * 0.44 + shore * (0.22 + whitecap * 0.46));
    color
        .mix(Rgb::hex(0x577c8b), wave * 0.27)
        .mix(Rgb::hex(0xc6dddb), foam)
}

fn tower(p: Pt) -> Option<Rgb> {
    let x = p.x.abs();
    let y = p.y;
    let roof = x < 0.063 && y >= 0.236 + x * 0.83 && y < 0.292;
    let finial = x < 0.005 && (0.222..0.246).contains(&y);
    if roof || finial {
        let lit = smooth(-0.025, 0.04, p.x);
        return Some(Rgb::hex(0x181925).mix(Rgb::hex(0x703c3b), lit * 0.66));
    }
    if x < 0.064 && (0.287..0.298).contains(&y) {
        return Some(Rgb::hex(0x292630));
    }
    if x < 0.046 && (0.298..0.337).contains(&y) {
        let frame = x > 0.039 || (x - 0.018).abs() < 0.004 || y > 0.330;
        let lamp = glow(p, 0.0, LANTERN_Y, 0.044);
        return Some(if frame {
            Rgb::hex(0x67483a)
        } else {
            AMBER.mix(Rgb::hex(0xfff6d3), lamp)
        });
    }
    if x < BALCONY_HALF_WIDTH && (0.337..0.358).contains(&y) {
        let rail = y > 0.349 || y < 0.341 || (x * 75.0).fract() < 0.22;
        return rail.then_some(Rgb::hex(0x252732));
    }
    let half_width = 0.040 + (y - 0.358) * 0.094;
    if (0.358..0.666).contains(&y) && x < half_width {
        let lit = smooth(-0.075, 0.060, p.x) * 0.7 + 0.17;
        let red = (0.427..0.497).contains(&y) || y > 0.568;
        let (shadow, face) = if red {
            (Rgb::hex(0x492d38), Rgb::hex(0xb76c5e))
        } else {
            (Rgb::hex(0x62616b), Rgb::hex(0xe3d4bb))
        };
        let door = (p.x - 0.024).abs() < 0.012 && y > 0.608;
        let window = (p.x - 0.015).abs() < 0.008 && (0.473..0.501).contains(&y);
        if door || window {
            return Some(Rgb::hex(0x202831));
        }
        return Some(shadow.mix(face, lit));
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_ends_of_the_balcony_are_visible_on_a_narrow_screen() {
        for (aspect, left, right) in [(0.136, -0.066, 0.066), (0.16, -0.078, 0.056)] {
            let edge = Pt {
                x: left,
                y: 0.355,
                aspect,
                dot: (0, 0),
            };
            let railing = Rgb::hex(0x252732);
            assert_eq!(storm_lighthouse(edge, 0.0), railing);
            assert_eq!(storm_lighthouse(edge.at(right, edge.y), 0.0), railing);
        }
    }

    #[test]
    fn the_beam_sweeps_while_the_tower_stays_put() {
        let center = Pt {
            x: -0.19,
            y: LANTERN_Y,
            aspect: 1.0,
            dot: (0, 0),
        };
        let quarter_turn = SWEEP_PERIOD / 4.0;
        let lantern = storm_lighthouse(center, 0.0);
        assert!(lantern.luma() > 0.75);
        assert_eq!(lantern, storm_lighthouse(center, quarter_turn));

        let high = center.at(center.x + 0.5, LANTERN_Y - 0.15 * 0.5);
        let low = center.at(center.x + 0.5, LANTERN_Y + 0.07 * 0.5);
        assert!(storm_lighthouse(high, 0.0).luma() > storm_lighthouse(low, 0.0).luma());
        assert!(
            storm_lighthouse(low, quarter_turn).luma()
                > storm_lighthouse(high, quarter_turn).luma()
        );
    }
}
