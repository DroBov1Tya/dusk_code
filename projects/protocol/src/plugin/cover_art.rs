//! The colours that stand in for a cover a plugin does not bring: a soft diagonal
//! gradient between two neighbouring hues, picked from the plugin's name so the same
//! plugin wears the same colours on every machine and in every client. Each client only
//! draws what this hands it.

/// A colour as hue (degrees, `0..360`), saturation and lightness (both `0..=1`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hsl {
    pub h: f32,
    pub s: f32,
    pub l: f32,
}

impl Hsl {
    /// The colour in 8-bit sRGB.
    pub fn rgb(self) -> [u8; 3] {
        let chroma = (1. - (2. * self.l - 1.).abs()) * self.s;
        let sector = self.h.rem_euclid(360.) / 60.;
        let second = chroma * (1. - (sector % 2. - 1.).abs());
        let (r, g, b) = match sector as u8 {
            0 => (chroma, second, 0.),
            1 => (second, chroma, 0.),
            2 => (0., chroma, second),
            3 => (0., second, chroma),
            4 => (second, 0., chroma),
            _ => (chroma, 0., second),
        };
        let lift = self.l - chroma / 2.;
        [r, g, b].map(|c| ((c + lift).clamp(0., 1.) * 255.).round() as u8)
    }
}

/// The gradient a cover-less plugin is drawn on, and the ink for what sits on top of it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CoverArt {
    /// The gradient's direction in degrees, `0` pointing up and turning clockwise (as CSS
    /// `linear-gradient` and gpui's `linear_gradient` both read it).
    pub angle: f32,
    pub from: Hsl,
    pub to: Hsl,
    /// For the kind's glyph and the words over the gradient; readable on both ends.
    pub ink: Hsl,
}

/// Saturation of the gradient's ends, light and dark theme.
pub const SATURATION_LIGHT: (f32, f32) = (0.42, 0.56);
pub const SATURATION_DARK: (f32, f32) = (0.34, 0.48);
/// Lightness of the gradient's ends: pale on a light theme, deep on a dark one, so the
/// theme's own text colour reads on it.
pub const LIGHTNESS_LIGHT: (f32, f32) = (0.80, 0.87);
pub const LIGHTNESS_DARK: (f32, f32) = (0.22, 0.29);
/// How far the second hue sits from the first, in degrees: near enough to agree.
const HUE_STEP: (u64, u64) = (30, 60);
/// The directions the gradient may run in: all diagonal, top-left to bottom-right.
const ANGLES: [f32; 3] = [125., 135., 145.];

/// The cover art for the plugin called `name`, for a light or a dark theme.
pub fn cover_art(name: &str, dark: bool) -> CoverArt {
    let hash = hash(name);
    let hue = (hash % 360) as f32;
    let step = HUE_STEP.0 + (hash >> 16) % (HUE_STEP.1 - HUE_STEP.0 + 1);
    let second = (hue + step as f32) % 360.;
    let angle = ANGLES[((hash >> 32) % ANGLES.len() as u64) as usize];
    let (saturation, lightness) = if dark {
        (SATURATION_DARK, LIGHTNESS_DARK)
    } else {
        (SATURATION_LIGHT, LIGHTNESS_LIGHT)
    };
    // A little of the hash in the saturation, so two plugins of one hue still differ.
    let share = ((hash >> 40) % 101) as f32 / 100.;
    let s = saturation.0 + (saturation.1 - saturation.0) * share;
    // The light end first: the gradient falls into shade toward the bottom right.
    let from = Hsl {
        h: hue,
        s,
        l: lightness.1,
    };
    let to = Hsl {
        h: second,
        s,
        l: lightness.0,
    };
    let ink = if dark {
        Hsl {
            h: hue,
            s: 0.2,
            l: 0.95,
        }
    } else {
        Hsl {
            h: hue,
            s: 0.3,
            l: 0.17,
        }
    };
    CoverArt {
        angle,
        from,
        to,
        ink,
    }
}

impl CoverArt {
    /// The same art, hushed for a cover that is still coming or could not be fetched:
    /// half the colour, its lightness drawn toward the theme's surface.
    pub fn quiet(self, dark: bool) -> Self {
        let surface = if dark { 0.2 } else { 0.92 };
        let hush = |c: Hsl| Hsl {
            h: c.h,
            s: c.s * 0.5,
            l: c.l + (surface - c.l) * 0.4,
        };
        Self {
            from: hush(self.from),
            to: hush(self.to),
            ..self
        }
    }
}

/// FNV-1a of `name`, finished with SplitMix64's mixer so that names a letter apart land
/// far apart. Unlike `RandomState` it gives the same answer on every run and machine.
fn hash(name: &str) -> u64 {
    let mut hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    hash = (hash ^ (hash >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    hash = (hash ^ (hash >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    hash ^ (hash >> 31)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn luminance(c: Hsl) -> f32 {
        let [r, g, b] = c.rgb().map(|v| {
            let v = f32::from(v) / 255.;
            if v <= 0.040_45 {
                v / 12.92
            } else {
                ((v + 0.055) / 1.055).powf(2.4)
            }
        });
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn contrast(a: Hsl, b: Hsl) -> f32 {
        let (a, b) = (luminance(a), luminance(b));
        (a.max(b) + 0.05) / (a.min(b) + 0.05)
    }

    fn names() -> impl Iterator<Item = String> {
        (0..400).map(|n| format!("plugin-{n}"))
    }

    const PINNED: f32 = 220.;

    #[test]
    fn same_name_same_colours() {
        for dark in [false, true] {
            assert_eq!(cover_art("github", dark), cover_art("github", dark));
        }
        // One plugin keeps its hues across themes; only the lightness changes.
        assert_eq!(
            cover_art("github", true).from.h,
            cover_art("github", false).from.h
        );
        // Pinned, so a change to the hash (which would recolour every placeholder) is seen.
        assert_eq!(cover_art("github", true).from.h, PINNED);
        assert_ne!(
            cover_art("github", true).from.h,
            cover_art("gitlab", true).from.h
        );
    }

    #[test]
    fn names_spread_over_the_wheel() {
        let mut buckets = [0_u32; 12];
        for name in names() {
            buckets[(cover_art(&name, true).from.h / 30.) as usize] += 1;
        }
        // 400 names over 12 buckets of 30 degrees: every bucket used, none crowded.
        assert!(
            buckets.iter().all(|&n| (12..=60).contains(&n)),
            "{buckets:?}"
        );
    }

    #[test]
    fn colours_stay_in_bounds() {
        for name in names().chain(["".into(), "é".into()]) {
            for dark in [false, true] {
                let art = cover_art(&name, dark);
                let (s, l) = if dark {
                    (SATURATION_DARK, LIGHTNESS_DARK)
                } else {
                    (SATURATION_LIGHT, LIGHTNESS_LIGHT)
                };
                for end in [art.from, art.to] {
                    assert!((0. ..360.).contains(&end.h));
                    assert!((s.0..=s.1).contains(&end.s), "{name}: {end:?}");
                    assert!((l.0..=l.1).contains(&end.l), "{name}: {end:?}");
                }
                let step = (art.to.h - art.from.h).rem_euclid(360.);
                assert!((30. ..=60.).contains(&step), "{name}: {step}");
                assert!(ANGLES.contains(&art.angle));
            }
        }
    }

    #[test]
    fn ink_reads_on_both_ends() {
        for name in names() {
            for dark in [false, true] {
                let art = cover_art(&name, dark);
                for end in [art.from, art.to] {
                    let ratio = contrast(art.ink, end);
                    assert!(ratio >= 4.5, "{name} dark={dark}: {ratio}");
                }
                // Quieter art keeps the glyph readable too (a graphic needs 3:1).
                let quiet = art.quiet(dark);
                for end in [quiet.from, quiet.to] {
                    assert!(contrast(art.ink, end) >= 3., "{name} dark={dark}");
                }
            }
        }
    }

    #[test]
    fn rgb_matches_known_colours() {
        assert_eq!(
            Hsl {
                h: 0.,
                s: 1.,
                l: 0.5
            }
            .rgb(),
            [255, 0, 0]
        );
        assert_eq!(
            Hsl {
                h: 120.,
                s: 1.,
                l: 0.25
            }
            .rgb(),
            [0, 128, 0]
        );
        assert_eq!(
            Hsl {
                h: 240.,
                s: 0.,
                l: 1.
            }
            .rgb(),
            [255, 255, 255]
        );
        assert_eq!(
            Hsl {
                h: 210.,
                s: 0.5,
                l: 0.4
            }
            .rgb(),
            [51, 102, 153]
        );
    }
}
