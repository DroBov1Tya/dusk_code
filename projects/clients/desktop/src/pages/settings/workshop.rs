//! Settings, Workshop: the repositories the user added beside the official store. The
//! core reads each list with the key typed here and decides everything; this page only
//! shows the rows and the result of a probe.

use dusk_protocol::plugin::{Probe, SourceRow};
use gpui_kit::{
    component::{
        ActiveTheme, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState},
    },
    *,
};

use super::{list, section, section_hint_color};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// The boxes of the Add form, the probe that passed for them, and the rows read last.
pub struct Repositories {
    url: Entity<InputState>,
    key: Entity<InputState>,
    /// The probe of what is in the boxes now; Add is offered only with it.
    probe: Option<Probe>,
    problem: Option<String>,
    pub rows: Option<Vec<SourceRow>>,
    /// Both boxes are emptied when next drawn, as that takes the window.
    clear: bool,
}

impl Repositories {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            url: cx.new(|cx| {
                InputState::new(window, cx).placeholder("https://github.com/owner/repository")
            }),
            key: cx.new(|cx| InputState::new(window, cx).placeholder("Its public key, base64")),
            probe: None,
            problem: None,
            rows: None,
            clear: false,
        }
    }

    pub fn settle(&mut self, window: &mut Window, cx: &mut App) {
        if std::mem::take(&mut self.clear) {
            for input in [&self.url, &self.key] {
                input.update(cx, |input, cx| input.set_value("", window, cx));
            }
        }
    }

    fn typed(&self, cx: &App) -> (String, String) {
        let value = |input: &Entity<InputState>| input.read(cx).value().trim().to_owned();
        (value(&self.url), value(&self.key))
    }
}

pub fn render(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (muted, danger, warning) = (section_hint_color(cx), theme.danger, theme.warning);
    let form = &ws.repositories;
    let mut add = section(
        "Add a repository",
        "A signed list of skills and plugins. Its items are marked Community: added by you, no key of duskcode vouches for them. The repository sees your address each time the store is read.",
        cx,
    )
    .child(
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(Input::new(&form.url).large().aria_label("Repository address"))
            .child(Input::new(&form.key).large().aria_label("Repository key")),
    );
    if let Some(problem) = &form.problem {
        add = add.child(div().small().text_color(danger).child(problem.clone()));
    }
    let mut buttons = div().flex().flex_row().justify_end().gap_2().child(
        Button::new("repository-check")
            .large()
            .label("Check")
            .on_click(cx.listener(|ws, _, _, cx| ws.probe_repository(cx))),
    );
    if let Some(probe) = &form.probe {
        add = add.child(div().small().text_color(warning).child(format!(
            "Signed list of {} items, {} of them plugins, valid until {}. Nobody checked them.",
            probe.entries,
            probe.plugins,
            probe.expires.get(..10).unwrap_or(&probe.expires)
        )));
        buttons = buttons.child(
            Button::new("repository-add")
                .large()
                .primary()
                .label("Add")
                .on_click(cx.listener(|ws, _, _, cx| ws.add_repository(cx))),
        );
    }
    add = add.child(buttons);
    let rows = match &form.rows {
        None => div().child(widgets::muted("Reading the repositories...", cx).px_0()),
        Some(rows) if rows.is_empty() => div()
            .small()
            .text_color(muted)
            .child("No repositories added."),
        Some(rows) => {
            let drawn: Vec<Div> = rows.iter().map(|row| source_row(row, cx)).collect();
            list(drawn, cx)
        }
    };
    div().flex().flex_col().gap_4().child(add).child(rows)
}

fn source_row(row: &SourceRow, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let muted = section_hint_color(cx);
    let (state, color) = match &row.error {
        None => (format!("{} items", row.entries), theme.success),
        Some(why) => (format!("Refused: {why}"), theme.danger),
    };
    let name = row.name.clone();
    super::line(
        &row.name,
        &format!("{}  ·  key {}", row.url, row.fingerprint),
        cx,
    )
    .child(
        div()
            .flex()
            .items_center()
            .gap_2()
            .max_w(px(260.))
            .small()
            .text_color(if row.error.is_some() { color } else { muted })
            .child(div().size(px(6.)).flex_shrink_0().rounded_full().bg(color))
            .child(state),
    )
    .child(
        Button::new(SharedString::from(format!("repository-remove-{name}")))
            .ghost()
            .label("Remove")
            .on_click(cx.listener(move |ws, _, _, cx| ws.remove_repository(&name, cx))),
    )
}

impl Workspace {
    pub fn load_repositories(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.plugins().sources().await },
            |ws, result, cx| match result {
                Ok(rows) => {
                    ws.repositories.rows = Some(rows);
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn probe_repository(&mut self, cx: &mut Context<Self>) {
        let (url, key) = self.repositories.typed(cx);
        self.repositories.probe = None;
        self.repositories.problem = None;
        cx.notify();
        self.core.call(
            cx,
            move |client| async move { client.plugins().probe_source(&url, &key).await },
            |ws, result, cx| {
                match result {
                    Ok(probe) => ws.repositories.probe = Some(probe),
                    Err(error) => ws.repositories.problem = Some(format!("{error:#}")),
                }
                cx.notify();
            },
        );
    }

    fn add_repository(&mut self, cx: &mut Context<Self>) {
        let (url, key) = self.repositories.typed(cx);
        self.core.call(
            cx,
            move |client| async move { client.plugins().add_source(&url, &key).await },
            |ws, result, cx| {
                match result {
                    Ok(()) => {
                        ws.repositories.probe = None;
                        ws.repositories.problem = None;
                        ws.repositories.clear = true;
                        ws.load_repositories(cx);
                    }
                    Err(error) => ws.repositories.problem = Some(format!("{error:#}")),
                }
                cx.notify();
            },
        );
    }

    fn remove_repository(&mut self, name: &str, cx: &mut Context<Self>) {
        let name = name.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.plugins().remove_source(&name).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_repositories(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
