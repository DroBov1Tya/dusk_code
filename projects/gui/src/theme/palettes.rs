//! The themes, as a few base colors each; `theme` spreads them over every component.

pub struct Palette {
    pub name: &'static str,
    pub dark: bool,
    /// The chat column.
    pub background: &'static str,
    /// The side columns.
    pub panel: &'static str,
    /// Cards, inputs and hovered rows.
    pub surface: &'static str,
    pub border: &'static str,
    pub foreground: &'static str,
    pub muted: &'static str,
    pub accent: &'static str,
    pub on_accent: &'static str,
    pub success: &'static str,
    pub warning: &'static str,
    pub danger: &'static str,
}

pub const DEFAULT: &str = "Dusk";

pub const PALETTES: &[Palette] = &[
    Palette {
        name: "Dusk",
        dark: true,
        background: "#15131d",
        panel: "#1a1725",
        surface: "#221e30",
        border: "#2e2942",
        foreground: "#e9e5f5",
        muted: "#8e88a6",
        accent: "#b48cff",
        on_accent: "#15131d",
        success: "#7fd1a4",
        warning: "#f2b96b",
        danger: "#f27a8a",
    },
    Palette {
        name: "Light",
        dark: false,
        background: "#ffffff",
        panel: "#f7f7f8",
        surface: "#efeff2",
        border: "#e4e4e7",
        foreground: "#18181b",
        muted: "#71717a",
        accent: "#4f46e5",
        on_accent: "#ffffff",
        success: "#16a34a",
        warning: "#d97706",
        danger: "#dc2626",
    },
    Palette {
        name: "Dark",
        dark: true,
        background: "#0f0f11",
        panel: "#151518",
        surface: "#1d1d21",
        border: "#2a2a2f",
        foreground: "#ededef",
        muted: "#8b8b93",
        accent: "#6d8cff",
        on_accent: "#0f0f11",
        success: "#4ade80",
        warning: "#fbbf24",
        danger: "#f87171",
    },
    Palette {
        name: "Solar Dusk",
        dark: true,
        background: "#1b1410",
        panel: "#221913",
        surface: "#2c2019",
        border: "#3e2d21",
        foreground: "#f6e7d7",
        muted: "#a8907a",
        accent: "#f59e0b",
        on_accent: "#1b1410",
        success: "#a3c47a",
        warning: "#fbbf24",
        danger: "#ef6b54",
    },
    // A sky gone dark around a black sun, with its corona for the accent.
    Palette {
        name: "Eclipse",
        dark: true,
        background: "#0a090c",
        panel: "#100d11",
        surface: "#1a1418",
        border: "#2d1f25",
        foreground: "#ecdcd4",
        muted: "#8f7a76",
        accent: "#ff6a2b",
        on_accent: "#0a090c",
        success: "#9bc27a",
        warning: "#ffb347",
        danger: "#e0404f",
    },
    // Night water in indigo, a glowing violet flower and its golden heart.
    Palette {
        name: "Lotus",
        dark: true,
        background: "#0b1030",
        panel: "#0e1438",
        surface: "#161d4b",
        border: "#252e68",
        foreground: "#ebe8ff",
        muted: "#9c9fcc",
        accent: "#c39bff",
        on_accent: "#0b1030",
        success: "#7fe0c4",
        warning: "#f5c451",
        danger: "#ff7aa8",
    },
];

pub fn find(name: &str) -> &'static Palette {
    PALETTES
        .iter()
        .find(|palette| palette.name == name)
        .unwrap_or(&PALETTES[0])
}
