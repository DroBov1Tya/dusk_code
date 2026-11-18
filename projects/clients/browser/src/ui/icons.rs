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
    Blocks,
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
    GitBranch,
    RefreshCw,
    ChartPie,
    Settings2,
    Map,
    Bot,
    FileText,
    SquareTerminal,
    Check,
    TierOfficial,
    TierVerified,
    TierCommunity,
    TierLocal,
    Filter,
    ChevronLeft,
    ChevronRight,
    TriangleAlert,
    ExternalLink,
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
            Self::Blocks => "#dusk-icon-blocks",
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
            Self::GitBranch => "#dusk-icon-git-branch",
            Self::RefreshCw => "#dusk-icon-refresh-cw",
            Self::ChartPie => "#dusk-icon-chart-pie",
            Self::Settings2 => "#dusk-icon-settings-2",
            Self::Map => "#dusk-icon-map",
            Self::Bot => "#dusk-icon-bot",
            Self::FileText => "#dusk-icon-file-text",
            Self::SquareTerminal => "#dusk-icon-square-terminal",
            Self::Check => "#dusk-icon-check",
            Self::TierOfficial => "#dusk-icon-tier-official",
            Self::TierVerified => "#dusk-icon-tier-verified",
            Self::TierCommunity => "#dusk-icon-tier-community",
            Self::TierLocal => "#dusk-icon-tier-local",
            Self::Filter => "#dusk-icon-filter",
            Self::ChevronLeft => "#dusk-icon-chevron-left",
            Self::ChevronRight => "#dusk-icon-chevron-right",
            Self::TriangleAlert => "#dusk-icon-triangle-alert",
            Self::ExternalLink => "#dusk-icon-external-link",
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
