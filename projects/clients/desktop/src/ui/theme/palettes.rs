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
    /// A solid fill for the final step of a destructive action. Optional: a palette
    /// without it leaves the kit's pale tint of `danger`.
    pub danger_button: Option<DangerButton>,
    /// The band over the message box and its tab: a shade beside `surface`, so the two
    /// read as one piece and the tab alone stays quiet on the page.
    pub band: &'static str,
    /// The sixteen colors a program in the terminal names: black, red, green, yellow,
    /// blue, magenta, cyan, white, then their bright ones. The theme's own hues, so a
    /// prompt and a diff look at home.
    pub terminal: [&'static str; 16],
}

/// The `button.danger.*` colors: the fill, the label on it, and the fill under the
/// pointer and when pressed. Hover and press move the fill away from the label's color,
/// so the label's contrast only grows.
pub struct DangerButton {
    pub background: &'static str,
    pub foreground: &'static str,
    pub hover: &'static str,
    pub active: &'static str,
}

pub const DEFAULT: &str = "Dusk";

pub const PALETTES: &[Palette] = &[
    // Dusk: the sky a little after sunset, deep indigo going to violet; an iris flower
    // for the accent and its golden heart. Each surface step is an even rise in
    // lightness, so cards and rows read without lines.
    Palette {
        name: "Dusk",
        dark: true,
        background: "#14141e",
        panel: "#0f0f17",
        surface: "#1c1c29",
        raised: "#272838",
        foreground: "#e6e3dd",
        secondary: "#b3aea6",
        muted: "#9d988f",
        accent: "#b4a0f8",
        on_accent: "#13121d",
        success: "#74d4b3",
        warning: "#ecc46e",
        danger: "#f27a96",
        danger_button: Some(DangerButton {
            background: "#f27a96",
            foreground: "#13121d",
            hover: "#f48aa3",
            active: "#f597ad",
        }),
        band: "#1a1a27",
        terminal: [
            "#272838", "#f27a96", "#74d4b3", "#ecc46e", "#8fb4f5", "#b4a0f8", "#7cc7e8", "#b3aea6",
            "#9d988f", "#f79bb0", "#93e6c8", "#f6d78e", "#a9c6fa", "#cbbcff", "#9fdcf4", "#e6e3dd",
        ],
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
        danger_button: Some(DangerButton {
            background: "#dc2626",
            foreground: "#ffffff",
            hover: "#c22121",
            active: "#ac1e1e",
        }),
        band: "#f5f5f6",
        terminal: [
            "#171717", "#c0392b", "#1e8e4a", "#a06a00", "#2563c9", "#8b3fc7", "#0e7c86", "#737373",
            "#525252", "#dc2626", "#16a34a", "#b7791f", "#3b82f6", "#a855f7", "#0891b2", "#404040",
        ],
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
        danger_button: Some(DangerButton {
            background: "#e5484d",
            foreground: "#161618",
            hover: "#e85e62",
            active: "#eb7074",
        }),
        band: "#202023",
        terminal: [
            "#2b2b2f", "#e5484d", "#4cc38a", "#e0b050", "#5b8def", "#b08af0", "#5cc4d6", "#a1a1a6",
            "#85858b", "#f0787c", "#76d6a6", "#ecc77c", "#86abf5", "#c7a9f6", "#8ad6e3", "#ededed",
        ],
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
        danger_button: Some(DangerButton {
            background: "#e06a55",
            foreground: "#1d1b18",
            hover: "#e47c69",
            active: "#e78b7a",
        }),
        band: "#22201d",
        terminal: [
            "#2e2b27", "#e06a55", "#8fb37a", "#e3a447", "#7fa3c7", "#c08fb0", "#7fb8a8", "#aaa398",
            "#8a847a", "#ea8a78", "#a9c796", "#edbb6e", "#9dbbd8", "#d2a9c5", "#9ccbbd", "#efe9de",
        ],
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
        danger_button: Some(DangerButton {
            background: "#e0505a",
            foreground: "#161517",
            hover: "#e4656e",
            active: "#e7767e",
        }),
        band: "#1b191d",
        terminal: [
            "#272429", "#e0505a", "#86b07a", "#e8a54a", "#7f9fd0", "#c58ad0", "#76b8c0", "#a69e9b",
            "#837a78", "#e97a82", "#a3c598", "#efbc74", "#9db7dd", "#d5a7de", "#97cdd3", "#ece5e1",
        ],
    },
];

pub fn find(name: &str) -> &'static Palette {
    PALETTES
        .iter()
        .find(|palette| palette.name == name)
        .unwrap_or(&PALETTES[0])
}
