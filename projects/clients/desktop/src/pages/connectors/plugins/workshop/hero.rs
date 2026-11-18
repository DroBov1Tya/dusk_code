//! Featured: the official picks as one card 280 high, one pick at a time. The cover at
//! the art's aspect on the left, a hairline, then the panel: where it comes from, the
//! name, three lines of what it is and its facts, and at the foot its slot, Details,
//! and the arrows round the dots. Narrower than three columns the cover is on top. It
//! never moves by itself; a change fades in.

use crate::ui::elevation::{Elevation, Level};
use dusk_protocol::plugin::{Kind, Listing, Tier};
use gpui_kit::{
    component::{
        ActiveTheme, Icon, IconName,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{Grid, actions, capsule, covers::Shown, grid, tier};
use crate::{
    ui::{OPEN, Type, size, widgets},
    workspace::Workspace,
};

const PICKS: usize = 5;
/// The hero's height when the cover is beside the panel.
const HEIGHT: f32 = 280.;
/// Narrower than three columns, the cover goes on top.
const SIDE_BY_SIDE: f32 = 744.;
const RADIUS: f32 = 12.;
const DOT: f32 = 8.;
const DOT_CURRENT: f32 = 20.;
/// More than three lines never show.
const TEXT_CHARS: usize = 400;

/// The first official listings, in the core's order.
pub fn featured<'a>(shown: &[&'a Listing]) -> Vec<&'a Listing> {
    shown
        .iter()
        .copied()
        .filter(|listing| listing.tier == Tier::Official)
        .take(PICKS)
        .collect()
}

pub fn render(
    ws: &Workspace,
    picks: &[&Listing],
    grid: Grid,
    cx: &mut Context<Workspace>,
) -> AnimationElement<Div> {
    let count = picks.len();
    let index = ws.workshop.hero % count;
    let listing = picks[index];
    let muted = cx.theme().muted_foreground;
    let source = match listing.source.as_str() {
        "" => "duskcode",
        source => source,
    };
    let panel = div()
        .flex()
        .flex_col()
        .gap(px(size::GAP_S))
        .min_w_0()
        .child(
            div()
                .meta()
                .text_color(muted)
                .child(format!("From {source} \u{b7} {} of {count}", index + 1)),
        )
        .child(div().display().truncate().child(listing.name.clone()))
        .child(
            div()
                .body()
                .text_color(muted)
                .line_clamp(3)
                .text_ellipsis()
                .child(capsule::clip(&listing.description, TEXT_CHARS)),
        )
        .child(facts(listing, cx));
    let opened = listing.clone();
    let foot = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_S))
        .h(px(size::CONTROL))
        .child(actions::slot(listing, ws, false, cx))
        .child(
            Button::new("hero-more")
                .outline()
                .label("Details")
                .on_click(
                    cx.listener(move |ws, _, window, cx| ws.open_listing(&opened, window, cx)),
                ),
        )
        .child(div().flex_1())
        .when(count > 1, |foot| foot.child(steps(index, count, cx)));
    banner(ws.cover(listing), listing.kind, grid.width, panel, foot, cx)
        // Each pick fades in as it comes, so the change is seen without a slide.
        .with_animation(("hero", index), Animation::new(OPEN), |hero, delta| {
            hero.opacity(0.35 + 0.65 * delta)
        })
}

/// A cover with its panel in one card: side by side 280 high with a hairline between,
/// or the cover on top when narrow. The pack's page draws its banner with this too.
pub fn banner(shown: Shown, kind: Kind, width: f32, panel: Div, foot: Div, cx: &App) -> Div {
    let theme = cx.theme();
    let border = theme.border;
    let inner = width - 2.;
    let frame = div()
        .flex()
        .w(px(width))
        .rounded(px(RADIUS))
        .elevation(Level::Raised, cx);
    let text = div()
        .flex()
        .flex_col()
        .justify_between()
        .gap(px(size::GAP_L))
        .flex_1()
        .min_w_0()
        .p(px(size::GAP_XL))
        .child(panel)
        .child(foot);
    let radius = px(RADIUS - 1.);
    if width < SIDE_BY_SIDE {
        let corners = Corners {
            top_left: radius,
            top_right: radius,
            ..Corners::default()
        };
        return frame
            .flex_col()
            .child(capsule::art(shown, kind, inner, corners, cx))
            .child(div().h(px(1.)).bg(border))
            .child(text);
    }
    let corners = Corners {
        top_left: radius,
        bottom_left: radius,
        ..Corners::default()
    };
    let art_w = (HEIGHT * grid::ASPECT).round();
    frame
        .flex_row()
        .h(px(HEIGHT + 2.))
        .child(capsule::art(shown, kind, art_w, corners, cx))
        .child(div().w(px(1.)).h_full().bg(border))
        .child(text)
}

/// The tier's mark, the kind (with its skills for a pack), the version, the category and,
/// for a mirror the user added, its repository's GitHub stars, on one line that wraps.
pub fn facts(listing: &Listing, cx: &App) -> Div {
    let muted = cx.theme().muted_foreground;
    let word = |text: String| div().meta().text_color(muted).child(text);
    let kind = match listing.kind {
        Kind::Pack if !listing.skills.is_empty() => format!(
            "{} \u{b7} {}",
            super::kind_word(listing.kind),
            widgets::count(listing.skills.len(), "skill")
        ),
        kind => super::kind_word(kind).to_owned(),
    };
    div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_center()
        .gap(px(size::GAP_M))
        .child(tier::mark(listing.tier, cx))
        .child(word(kind))
        .child(word(format!("v{}", listing.version)))
        .when(!listing.category.is_empty(), |row| {
            row.child(word(widgets::capitalize(&listing.category)))
        })
        .when_some(listing.repo_stars.as_ref(), |row, stars| {
            row.child(word(stars.label()))
        })
}

/// The categories (or a format 2 list's one category) and tags as quiet chips; none when
/// it has neither.
pub fn tags(listing: &Listing, cx: &App) -> Option<Div> {
    let categories = match listing.categories.is_empty() {
        true => std::slice::from_ref(&listing.category),
        false => listing.categories.as_slice(),
    };
    let mut words: Vec<String> = categories
        .iter()
        .chain(&listing.tags)
        .filter(|word| !word.is_empty())
        .map(|word| widgets::capitalize(word))
        .collect();
    let mut seen = std::collections::BTreeSet::new();
    words.retain(|word| seen.insert(word.clone()));
    if words.is_empty() {
        return None;
    }
    let theme = cx.theme();
    let (muted, chip) = (theme.muted_foreground, theme.background);
    Some(
        div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(size::GAP_XS))
            .children(words.into_iter().map(move |word| {
                div()
                    .flex()
                    .items_center()
                    .h(px(size::GAP_XL))
                    .px(px(size::GAP_S))
                    .rounded_md()
                    .bg(chip)
                    .meta()
                    .text_color(muted)
                    .child(word)
            })),
    )
}

/// Previous, a dot a pick (the one in view long, in the accent), next.
fn steps(index: usize, count: usize, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (accent, quiet) = (theme.primary, theme.muted_foreground.opacity(0.4));
    let step = |id: &'static str, icon: IconName, by: usize| {
        Button::new(id)
            .ghost()
            .icon(Icon::new(icon))
            .on_click(cx.listener(move |ws, _, _, cx| {
                ws.workshop.hero = (ws.workshop.hero % count + by) % count;
                cx.notify();
            }))
    };
    let back = step("hero-back", IconName::ChevronLeft, count - 1);
    let next = step("hero-next", IconName::ChevronRight, 1);
    let dots = div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_XS))
        .children((0..count).map(|each| {
            let current = each == index;
            div()
                .id(("hero-dot", each))
                .w(px(if current { DOT_CURRENT } else { DOT }))
                .h(px(DOT))
                .rounded_full()
                .cursor_pointer()
                .bg(if current { accent } else { quiet })
                .on_click(cx.listener(move |ws, _, _, cx| {
                    ws.workshop.hero = each;
                    cx.notify();
                }))
        }));
    div()
        .flex()
        .flex_row()
        .items_center()
        .gap(px(size::GAP_XS))
        .child(back)
        .child(dots)
        .child(next)
}

/// A round button of a control's size over a picture: the gallery's arrows.
pub fn round(id: impl Into<ElementId>, icon: IconName, cx: &App) -> Stateful<Div> {
    let theme = cx.theme();
    let (back, hover, foreground) = (theme.background, theme.list_hover, theme.foreground);
    div()
        .id(id)
        .absolute()
        .size(px(size::CONTROL))
        .rounded_full()
        .flex()
        .items_center()
        .justify_center()
        .cursor_pointer()
        .bg(back.opacity(0.85))
        .hover(move |arrow| arrow.bg(hover))
        .child(Icon::new(icon).size(px(size::ICON)).text_color(foreground))
}
