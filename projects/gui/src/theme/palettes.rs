//! The themes. Surfaces stay near neutral, tinted only a little toward each theme's
//! world, so hours of looking at them do not tire the eyes; the world shows in the one
//! accent, used rarely. Hairlines are the text color at low alpha.

pub struct Palette {
    pub name: &'static str,
    pub dark: bool,
    /// The chat column.
    pub background: &'static str,
    /// The side columns: the deepest surface.
    pub panel: &'static str,
    /// Cards and inputs.
    pub surface: &'static str,
    /// An active row, a hovered card.
    pub raised: &'static str,
    pub foreground: &'static str,
    pub secondary: &'static str,
    pub muted: &'static str,
    pub accent: &'static str,
    pub on_accent: &'static str,
    pub success: &'static str,
    pub warning: &'static str,
    pub danger: &'static str,
}

pub const DEFAULT: &str = "Dusk";

pub const PALETTES: &[Palette] = &[
    // Old dark fantasy: ash and iron, worn parchment for the text, and the one warm
    // light of a bonfire for the accent. Grim, never evil, never neon.
    Palette {
        name: "Dusk",
        dark: true,
        background: "#1c1b18",
        panel: "#171614",
        surface: "#23211e",
        raised: "#2d2b27",
        foreground: "#e8e0d0",
        secondary: "#ada595",
        muted: "#878073",
        accent: "#d98a4e",
        on_accent: "#1c1b18",
        success: "#86a874",
        warning: "#d2a650",
        danger: "#c95a50",
    },
    Palette {
        name: "Light",
        dark: false,
        background: "#fafafa",
        panel: "#f4f4f5",
        surface: "#ffffff",
        raised: "#ececee",
        foreground: "#171717",
        secondary: "#525252",
        muted: "#737373",
        accent: "#2563c9",
        on_accent: "#ffffff",
        success: "#16a34a",
        warning: "#b7791f",
        danger: "#dc2626",
    },
    Palette {
        name: "Dark",
        dark: true,
        background: "#1b1b1d",
        panel: "#161618",
        surface: "#222225",
        raised: "#2b2b2f",
        foreground: "#ededed",
        secondary: "#a1a1a6",
        muted: "#85858b",
        accent: "#5b8def",
        on_accent: "#161618",
        success: "#4cc38a",
        warning: "#e0b050",
        danger: "#e5484d",
    },
    // Warm dusk light on wood and paper, with amber for the accent.
    Palette {
        name: "Solar Dusk",
        dark: true,
        background: "#1d1b18",
        panel: "#181614",
        surface: "#24221f",
        raised: "#2e2b27",
        foreground: "#efe9de",
        secondary: "#aaa398",
        muted: "#8a847a",
        accent: "#e3a447",
        on_accent: "#1d1b18",
        success: "#8fb37a",
        warning: "#e3a447",
        danger: "#e06a55",
    },
    // A sky gone dark around a black sun; its corona is the accent.
    Palette {
        name: "Eclipse",
        dark: true,
        background: "#161517",
        panel: "#111012",
        surface: "#1d1b1f",
        raised: "#272429",
        foreground: "#ece5e1",
        secondary: "#a69e9b",
        muted: "#837a78",
        accent: "#ef7440",
        on_accent: "#161517",
        success: "#86b07a",
        warning: "#e8a54a",
        danger: "#e0505a",
    },
    // Night water in ink blue, a violet flower for the accent and its golden heart.
    Palette {
        name: "Lotus",
        dark: true,
        background: "#12151e",
        panel: "#0e1118",
        surface: "#191d28",
        raised: "#232835",
        foreground: "#eef0f5",
        secondary: "#a3a9b8",
        muted: "#7a8296",
        accent: "#b99cf0",
        on_accent: "#12151e",
        success: "#6fcfae",
        warning: "#e7c35a",
        danger: "#ef6b8a",
    },
];

pub fn find(name: &str) -> &'static Palette {
    PALETTES
        .iter()
        .find(|palette| palette.name == name)
        .unwrap_or(&PALETTES[0])
}
