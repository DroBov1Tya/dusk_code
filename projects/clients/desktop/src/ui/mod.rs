//! The design system: the few values every column is built from, and the pieces made of
//! them. A column places modules inside this structure; it does not choose its own sizes.
//!
//! - Space is a 4 px grid: 4, 8, 12, 16, 24, 32. Rows and controls are 32 px.
//! - Type is one family on a scale of five sizes, set in rems so the user's size choice
//!   scales all of it: meta, small, body, title, display. Weight 400 for reading, 500 for
//!   what can be pressed or names a group, 600 for titles. No capitals for labels.
//! - Surfaces step up in lightness: panel, page, surface, raised. Structure is told by
//!   those steps and by space; a hairline appears only where two areas of one surface meet.
//! - One accent (selection, focus, the primary action) and one signal (work going on).
//! - Motion answers a person's action or what arrived, eased out in 120-250 ms and played
//!   once (see `motion`): a menu opens in 160 ms, a page fades in over 150 ms.

pub mod avatar;
pub mod clipboard;
pub mod controls;
pub mod elevation;
pub mod icons;
pub mod layout;
pub mod looping;
pub mod motion;
pub mod phone;
pub mod press;
pub mod theme;
pub mod widgets;

use std::time::Duration;

use gpui_kit::{
    component::{ActiveTheme, Icon, IconName, Sizable, button::Button},
    *,
};

use self::looping::Loop;

/// How long a menu takes to appear.
pub const OPEN: Duration = Duration::from_millis(160);
/// How long a fold takes to open.
pub const UNFOLD: Duration = Duration::from_millis(180);

/// The scale every size is taken from: multiples of the 4 px grid, so edges line up
/// across columns.
pub mod size {
    /// A button, an input, a menu row: what a pointer aims at.
    pub const CONTROL: f32 = 32.;
    /// A row of a list in a column.
    pub const ROW: f32 = 36.;
    /// The header's height and the width of the rail and of the composer's sides.
    pub const BAR: f32 = 52.;
    pub const ICON: f32 = 16.;
    pub const ICON_LARGE: f32 = 20.;
    /// The space scale above by name, for places that compute with it: 4, 8, 12, 16, 24, 32.
    pub const GAP_XS: f32 = 4.;
    pub const GAP_S: f32 = 8.;
    pub const GAP_M: f32 = 12.;
    pub const GAP_L: f32 = 16.;
    pub const GAP_XL: f32 = 24.;
    pub const GAP_XXL: f32 = 32.;
}

/// Row heights.
pub const ROW: f32 = size::CONTROL;

/// The type scale. Each step is a size and a weight, so a place says what the text is,
/// not how big.
pub trait Type: Styled + Sized {
    /// Counts, hints, timestamps.
    fn meta(self) -> Self {
        self.text_size(rems(0.78))
    }
    /// Secondary lines and labels of groups.
    fn small(self) -> Self {
        self.text_size(rems(0.86))
    }
    /// What people read and press.
    fn body(self) -> Self {
        self.text_size(rems(0.95))
    }
    /// A page's or a window's title.
    fn title(self) -> Self {
        self.text_size(rems(1.3)).font_weight(FontWeight::SEMIBOLD)
    }
    /// The one large name of a store page or a featured pick.
    fn display(self) -> Self {
        self.text_size(rems(1.8))
            .line_height(rems(2.2))
            .font_weight(FontWeight::BOLD)
    }
    /// The name of a group of rows.
    fn group_name(self) -> Self {
        self.small().font_weight(FontWeight::MEDIUM)
    }
}

impl Type for Div {}
impl<E: Styled + Sized> Type for Stateful<E> where Stateful<E>: Styled {}

/// A group's name above its rows, in the quiet color.
pub fn group(text: impl Into<SharedString>, cx: &App) -> Div {
    div()
        .px_4()
        .pt_5()
        .pb_1p5()
        .group_name()
        .text_color(cx.theme().muted_foreground)
        .child(text.into())
}

/// A page's title with one line saying what the page is for.
pub fn page_title(title: &str, about: &str, cx: &App) -> Div {
    div()
        .flex()
        .flex_col()
        .gap_1()
        .pb_4()
        .child(div().title().child(title.to_owned()))
        .child(
            div()
                .small()
                .text_color(cx.theme().muted_foreground)
                .child(about.to_owned()),
        )
}

/// A ring that turns while something is read, in the signal color.
pub fn spinner(cx: &App) -> impl IntoElement {
    Icon::new(IconName::LoaderCircle)
        .small()
        .text_color(cx.theme().muted_foreground)
        .looping(Duration::from_millis(900), |icon, delta| {
            icon.rotate(percentage(delta))
        })
}

/// "Reading..." with the ring: a wait that shows it is under way.
pub fn loading(text: &str, cx: &App) -> Div {
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        .px_3()
        .py_2()
        .small()
        .text_color(cx.theme().muted_foreground)
        .child(spinner(cx))
        .child(text.to_owned())
}

/// An empty place: one line that says what goes here, and the one thing to do about it,
/// so a page with nothing on it is never a dead end. `action` is made with `action`.
pub fn empty(icon: impl Into<Icon>, line: &str, action: Option<Button>, cx: &App) -> Div {
    let theme = cx.theme();
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .px_6()
        .py_8()
        .text_center()
        .child(
            div()
                .size(px(48.))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .bg(theme.muted)
                .child(
                    icon.into()
                        .size(px(size::ICON_LARGE))
                        .text_color(theme.muted_foreground),
                ),
        )
        .child(div().max_w(px(420.)).body().child(line.to_owned()))
        .children(action)
}

/// The button of an empty place; the caller adds what a click does.
pub fn action(id: impl Into<ElementId>, label: &'static str) -> Button {
    Button::new(id).outline().label(label)
}
