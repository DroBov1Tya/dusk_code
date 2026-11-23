//! The welcome drawn: the page in view in the middle, Skip in the corner, and under the
//! page Back, a dot for each page and Next. The first and the last page are here; the
//! Model and Tools pages have their own files.

use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{Page, form, tools};
use crate::{
    platform::icon,
    ui::{Type, motion},
    workspace::Workspace,
};

/// Wide enough for an address and a tool's sentence, narrow enough to read as one thing.
pub const WIDTH: f32 = 460.;
/// Pages stand at least this tall, so the buttons under them stay where they are.
const HEIGHT: f32 = 440.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Stateful<Div> {
    let page = ws.welcome.page.unwrap_or(Page::Hello);
    let body = match page {
        Page::Hello => hello(cx),
        Page::Model => form::render(ws, cx),
        Page::Tools => tools::render(ws, cx),
        Page::Ready => ready(ws, cx),
    };
    let body = div()
        .w_full()
        .max_w(px(WIDTH))
        .min_h(px(HEIGHT))
        .flex()
        .flex_col()
        .justify_center()
        .child(body);
    let id = SharedString::from(format!("welcome-page-{page:?}"));
    div()
        .id("welcome")
        .relative()
        .size_full()
        .flex()
        .flex_col()
        .items_center()
        .justify_center()
        .gap_8()
        .px_6()
        .on_scroll(cx.listener(|ws, event: &ScrollEvent, window, cx| {
            let step = ws.welcome.swipe.feed(event);
            if step != 0 {
                ws.welcome_turn(step, window, cx);
            }
        }))
        .child(skip(cx))
        .child(motion::slide(id, body, ws.welcome.toward))
        .child(nav(ws, page, cx))
}

/// Skip, quiet in the top corner: the whole welcome is done.
fn skip(cx: &mut Context<Workspace>) -> Stateful<Div> {
    let theme = cx.theme();
    let (muted, foreground) = (theme.muted_foreground, theme.foreground);
    div()
        .id("welcome-skip")
        .absolute()
        .top_4()
        .right_6()
        .small()
        .text_color(muted)
        .hover(|link| link.text_color(foreground))
        .cursor_pointer()
        .child("Skip")
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
        .gap_2()
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
        Page::Model if !connected => "Skip",
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
        .justify_between()
        .child(
            div()
                .w(px(96.))
                .when(page != Page::Hello, |side| side.child(back)),
        )
        .child(dots)
        .child(div().w(px(96.)).flex().justify_end().child(next))
}

fn hello(cx: &mut Context<Workspace>) -> Div {
    let muted = cx.theme().muted_foreground;
    div()
        .flex()
        .flex_col()
        .items_center()
        .gap_3()
        .child(img(icon::mark()).size(px(64.)).flex_none())
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
        .child(img(icon::mark()).size(px(48.)).flex_none())
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
