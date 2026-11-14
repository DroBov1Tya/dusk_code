//! A command the user ran from an answer, with its output.

use gpui_kit::{component::ActiveTheme, *};

use crate::{
    pages::chat::{links, transcript::CommandRun},
    ui::{Type, widgets},
    workspace::Workspace,
};

/// A command the user ran from an answer: its line, a bar that fills while it runs, then
/// its output as a code block. A command has no known length, so the bar closes on the
/// end like a kettle: fast at first, slower after, full only when the command is done.
pub fn command_card(key: &str, run: &CommandRun, cx: &mut Context<Workspace>) -> AnyElement {
    let theme = cx.theme();
    let (muted, accent, danger, track) = (
        theme.muted_foreground,
        theme.primary,
        theme.danger,
        theme.border,
    );
    let group = SharedString::from(format!("command-row-{key}"));
    let copied = match &run.result {
        Some(Ok(output)) => format!("$ {}\n{}", run.command, output.trim_end()),
        _ => run.command.clone(),
    };
    let mut card = div().group(group.clone()).flex().flex_col().gap_2().child(
        div()
            .flex()
            .items_center()
            .gap_1()
            .meta()
            .text_color(muted)
            .child(div().min_w_0().child(format!("$ {}", run.command)))
            .child(widgets::copy(
                SharedString::from(format!("command-copy-{key}")),
                &group,
                &copied,
            )),
    );
    match &run.result {
        None => {
            let started = run.started;
            card = card.child(
                div()
                    .h(px(3.))
                    .w_full()
                    .rounded_full()
                    .bg(track)
                    .overflow_hidden()
                    .child(
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(accent)
                            .w(relative(0.02))
                            // The animation only drives the redraws; the width follows the clock.
                            .with_animation(
                                SharedString::from(format!("run-bar-{key}")),
                                Animation::new(std::time::Duration::from_millis(250)).repeat(),
                                move |bar, _| bar.w(relative(run_fraction(started.elapsed()))),
                            ),
                    ),
            );
        }
        Some(Ok(output)) => {
            let fenced = format!("```\n{}\n```", output.trim_end());
            card = card.child(links::markdown(
                SharedString::from(format!("command-{key}")),
                fenced,
            ));
        }
        Some(Err(error)) => {
            card = card.child(div().meta().text_color(danger).child(error.clone()));
        }
    }
    card.into_any_element()
}

/// How full the bar is after `elapsed`: `1 - e^(-t/6s)`, held below the end.
fn run_fraction(elapsed: std::time::Duration) -> f32 {
    (1. - (-elapsed.as_secs_f32() / 6.).exp()).clamp(0.02, 0.97)
}
