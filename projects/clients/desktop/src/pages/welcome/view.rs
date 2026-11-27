//! The welcome drawn: the page in view in the middle, scrolling when the window is too
//! short for it, Skip setup pinned in the corner above it, and pinned under it Back, a
//! dot for each page and Next, so turning a page never moves them. The first and the last
//! page are here; the Model, Access and Tools pages have their own files.

use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{Page, access, form, tools};
use crate::{
    ui::{Type, motion},
    workspace::Workspace,
};

/// Wide enough for an address and a tool's sentence, narrow enough to read as one thing.
pub const WIDTH: f32 = 460.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let page = ws.welcome.page.unwrap_or(Page::Hello);
    let body = match page {
        Page::Hello => hello(cx),
        Page::Model => form::render(ws, cx),
        Page::Access => access::render(ws, cx),
        Page::Tools => tools::render(ws, cx),
        Page::Ready => ready(ws, cx),
    };
    let body = div().w_full().max_w(px(WIDTH)).child(body);
    let id = SharedString::from(format!("welcome-page-{page:?}"));
    // Centred while it fits, scrolled when it does not; each page keeps its own place.
    // Upright only, so a sideways swipe turns the page instead of scrolling it.
    let scroll = div()
        .id(SharedString::from(format!("welcome-scroll-{page:?}")))
        .flex_1()
        .min_h_0()
        .w_full()
        .overflow_y_scroll()
        .restrict_scroll_to_axis()
        .child(
            div()
                .w_full()
                .min_h_full()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .px_6()
                .py_4()
                .child(motion::slide(id, body, ws.welcome.toward)),
        );
    // Skip setup has a row of its own above the page, so a page scrolled under it never meets it.
    let top = div()
        .flex_none()
        .w_full()
        .flex()
        .justify_end()
        .px_6()
        .pt_4()
        .child(skip(cx));
    div()
        .id("welcome")
        .size_full()
        .flex()
        .flex_col()
        .on_scroll(cx.listener(|ws, event: &ScrollEvent, window, cx| {
            let step = ws.welcome.swipe.feed(event);
            if step != 0 {
                ws.welcome_turn(step, window, cx);
            }
        }))
        .child(top)
        .child(scroll)
        .child(
            div()
                .flex_none()
                .w_full()
                .flex()
                .justify_center()
                .px_6()
                .pt_4()
                .pb_8()
                .child(nav(ws, page, cx)),
        )
}

/// Skip setup, quiet in the top corner: the whole welcome is done.
fn skip(cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, foreground) = (theme.muted_foreground, theme.foreground);
    div()
        .id("welcome-skip")
        .small()
        .text_color(muted)
        .hover(|link| link.text_color(foreground))
        .cursor_pointer()
        .child("Skip setup")
        .on_click(cx.listener(|ws, _, _, cx| ws.welcome_finish(cx)))
}

/// Back, the dots and Next, a row as wide as the page.
fn nav(ws: &Workspace, page: Page, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (accent, idle) = (theme.primary, theme.border);
    let connected = form::connected(ws).is_some();
    let dots = div()
        .flex()
        .flex_row()
        .gap_1p5()
        .children(Page::ALL.map(|dot| {
            let on = dot == page;
            div()
                .id(SharedString::from(format!("welcome-dot-{dot:?}")))
                .size(px(8.))
                .rounded_full()
                .bg(if on { accent } else { idle })
                .when(!on, |dot| dot.cursor_pointer())
                .on_click(cx.listener(move |ws, _, window, cx| ws.welcome_go(dot, window, cx)))
        }));
    let next = match page {
        // "for now": the corner's "Skip setup" ends the whole welcome, this one only a step.
        Page::Model if !connected => "Skip for now",
        Page::Ready => "Start",
        _ => "Next",
    };
    let next = Button::new("welcome-next")
        .large()
        .label(next)
        .when(page != Page::Model || connected, |button| button.primary())
        .when(page == Page::Model && !connected, |button| button.ghost())
        .on_click(cx.listener(|ws, _, window, cx| ws.welcome_turn(1, window, cx)));
    let back = Button::new("welcome-back")
        .large()
        .ghost()
        .label("Back")
        .on_click(cx.listener(|ws, _, window, cx| ws.welcome_turn(-1, window, cx)));
    div()
        .w_full()
        .max_w(px(WIDTH))
        .flex()
        .flex_row()
        .items_center()
        .gap_2()
        // Two equal sides keep the dots centred, and a wide Next never covers them.
        .child(
            div()
                .flex_1()
                .flex()
                .when(page != Page::Hello, |side| side.child(back)),
        )
        .child(dots.flex_none())
        .child(div().flex_1().flex().justify_end().child(next))
}

fn hello(cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .child(div().display().child("duskcode"))
        .child(
            div()
                .body()
                .text_center()
                .text_color(muted)
                .child("An agent that works in your projects, on the model you choose."),
        )
}

fn ready(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    let model = match form::connected(ws) {
        Some(name) => format!("Your chats will use {name}."),
        None => "Connect a model any time in Settings, under AI.".to_owned(),
    };
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .child(div().title().child("You are ready"))
        .child(div().body().text_center().text_color(muted).child(model))
        .child(
            div()
                .small()
                .text_center()
                .text_color(muted)
                .child("Tools can be switched on or off later in Customize."),
        )
}
