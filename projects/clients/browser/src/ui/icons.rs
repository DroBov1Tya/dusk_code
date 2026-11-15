//! The workspace icons use the same geometries as Desktop.

use leptos::prelude::*;

#[derive(Clone, Copy)]
pub enum IconName {
    Menu,
    PanelRightOpen,
    PanelRightClose,
    Close,
    Plus,
    Inbox,
    Calendar,
    Network,
    LayoutDashboard,
    Settings,
    Temporary,
    Mic,
    ArrowUp,
    Square,
    Ellipsis,
    Delete,
    Search,
    ChevronDown,
    Copy,
    Play,
    Cpu,
    CircleCheck,
    MemoryStick,
    User,
    Palette,
    Bell,
    Eye,
    Github,
    RefreshCw,
    ChartPie,
    Settings2,
    Map,
    Bot,
    FileText,
    SquareTerminal,
    Check,
}

impl IconName {
    fn href(self) -> &'static str {
        match self {
            Self::Menu => "#dusk-icon-menu",
            Self::PanelRightOpen => "#dusk-icon-panel-right-open",
            Self::PanelRightClose => "#dusk-icon-panel-right-close",
            Self::Close => "#dusk-icon-close",
            Self::Plus => "#dusk-icon-plus",
            Self::Inbox => "#dusk-icon-inbox",
            Self::Calendar => "#dusk-icon-calendar",
            Self::Network => "#dusk-icon-network",
            Self::LayoutDashboard => "#dusk-icon-layout-dashboard",
            Self::Settings => "#dusk-icon-settings",
            Self::Temporary => "#dusk-icon-temporary",
            Self::Mic => "#dusk-icon-mic",
            Self::ArrowUp => "#dusk-icon-arrow-up",
            Self::Square => "#dusk-icon-square",
            Self::Ellipsis => "#dusk-icon-ellipsis",
            Self::Delete => "#dusk-icon-delete",
            Self::Search => "#dusk-icon-search",
            Self::ChevronDown => "#dusk-icon-chevron-down",
            Self::Copy => "#dusk-icon-copy",
            Self::Play => "#dusk-icon-play",
            Self::Cpu => "#dusk-icon-cpu",
            Self::CircleCheck => "#dusk-icon-circle-check",
            Self::MemoryStick => "#dusk-icon-memory-stick",
            Self::User => "#dusk-icon-user",
            Self::Palette => "#dusk-icon-palette",
            Self::Bell => "#dusk-icon-bell",
            Self::Eye => "#dusk-icon-eye",
            Self::Github => "#dusk-icon-github",
            Self::RefreshCw => "#dusk-icon-refresh-cw",
            Self::ChartPie => "#dusk-icon-chart-pie",
            Self::Settings2 => "#dusk-icon-settings-2",
            Self::Map => "#dusk-icon-map",
            Self::Bot => "#dusk-icon-bot",
            Self::FileText => "#dusk-icon-file-text",
            Self::SquareTerminal => "#dusk-icon-square-terminal",
            Self::Check => "#dusk-icon-check",
        }
    }
}

#[component]
pub fn IconDefinitions() -> impl IntoView {
    view! { <div class="icon-definitions" aria-hidden="true" inner_html=include_str!("../../assets/icons.svg")></div> }
}

#[component]
pub fn Icon(name: IconName) -> impl IntoView {
    view! {
        <svg class="glyph" viewBox="0 0 24 24" aria-hidden="true" focusable="false">
            <use href=name.href() />
        </svg>
    }
}
