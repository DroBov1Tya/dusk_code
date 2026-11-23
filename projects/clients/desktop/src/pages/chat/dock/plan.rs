//! The plan card over the composer: its title, how long the team has run and the way to
//! stop it, one line of how the plan stands, then a row a wave with a square a task. Only
//! the wave in work is unfolded; a click unfolds another, or a task to its details.

use crate::ui::elevation::{Elevation, Level};

use gpui_kit::{
    component::{ActiveTheme, IconName},
    prelude::FluentBuilder,
    *,
};

use crate::{
    pages::team::waves,
    ui::{
        Type,
        widgets::{self, Focusable},
    },
    workspace::Workspace,
};

/// The waves' tallest before they scroll, so a long plan leaves the chat in sight.
const WAVES_MAX: f32 = 300.;

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Option<Div> {
    let chat = ws.shown_chat()?;
    let all = ws
        .dock
        .counts_of(ws.shown.as_deref())
        .map_or(0, |counts| counts.total());
    if chat.tasks.is_empty() && all == 0 {
        return None;
    }
    let open = ws.dock.plan_open;
    // While the plan is written, its title carries the light.
    let planning = chat
        .work(dusk_protocol::system::now_ms())
        .is_some_and(|work| work.kind == dusk_protocol::thread::WorkKind::Planning);
    let theme = cx.theme();
    let (muted, border) = (theme.muted_foreground, theme.border);
    let team = chat.team.as_ref();
    let running = team.is_some_and(|team| team.ended().is_none());
    // A team says how long it ran, and once it ended, how.
    let elapsed = team.map(|team| match team.ended() {
        None => format!("{} min", team.used.minutes),
        Some(_) => team.progress_note(),
    });
    let stop = running.then(|| crate::pages::team::module::stop("plan-stop", cx));
    let title = div()
        .flex()
        .flex_row()
        .items_center()
        .gap_3()
        .child(
            div()
                .flex_1()
                .min_w_0()
                .body()
                .font_weight(FontWeight::MEDIUM)
                .child(crate::pages::chat::activity::plan_label(planning, cx)),
        )
        .children(elapsed.map(|elapsed| div().flex_none().small().text_color(muted).child(elapsed)))
        .children(crate::pages::team::module::open_link(team.is_some(), cx))
        .children(stop)
        .child(widgets::icon(
            if open {
                IconName::ChevronDown
            } else {
                IconName::ChevronRight
            },
            muted,
        ));
    let head = div()
        .id("plan-line")
        .flex()
        .flex_col()
        .gap_0p5()
        .px_5()
        .pt_4()
        .pb_3()
        .cursor_pointer()
        .focusable_row(cx)
        .child(title)
        .child(waves::summary(&chat.tasks, cx))
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_plan(cx)));
    let came =
        (ws.dock.tasks_came.0.as_deref() == ws.shown.as_deref()).then_some(&ws.dock.tasks_came.1);
    let body = open.then(|| {
        div()
            .id("plan-waves")
            .max_h(px(WAVES_MAX))
            .overflow_y_scroll()
            .border_t_1()
            .border_color(border.opacity(0.5))
            .pt_2()
            .pb_1()
            .child(waves::render(
                ws,
                waves::View {
                    key: "plan",
                    folds: &ws.dock.waves,
                    of: |ws| &mut ws.dock.waves,
                    came,
                    inset: 44.,
                },
                cx,
            ))
    });
    Some(
        crate::pages::chat::composer::dock_lane(
            div()
                .flex()
                .flex_col()
                .rounded_xl()
                .elevation(Level::Raised, cx)
                .overflow_hidden()
                .child(head)
                .when_some(body, |card, body| card.child(body)),
        )
        .pb_2(),
    )
}
