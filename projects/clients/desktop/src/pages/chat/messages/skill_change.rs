//! A skill the agent changed with skill-evolve: a row under the step that made the change,
//! with the reason, a disclosure to the diff and Undo. The skill's text and the reason are
//! data from a file: drawn as plain text, never as markup.

use dusk_protocol::catalog::{DiffKind, SkillChange, SkillDiff, SkillSaveAction};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName, Sizable,
        button::{Button, ButtonVariants},
    },
    prelude::FluentBuilder,
    *,
};

use super::{
    SKILL,
    activity::{disclosure, disclosure_icon, step_muted},
};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

/// A change as the chat holds it, with what the row shows of its request state.
pub struct SkillRow {
    pub change: SkillChange,
    /// Fetched once, when the row is first opened.
    pub diff: Option<SkillDiff>,
    pub loading: bool,
    /// The undo request runs.
    pub busy: bool,
    /// The undo request's failure.
    pub error: Option<String>,
    /// The diff request's failure, apart so an undo does not clear it.
    pub diff_error: Option<String>,
}

impl SkillRow {
    pub fn new(change: SkillChange) -> Self {
        Self {
            change,
            diff: None,
            loading: false,
            busy: false,
            error: None,
            diff_error: None,
        }
    }
}

/// Text from a skill file or an agent: no control or bidi characters, which could move or
/// hide what the row says. `tabs`: a tab stays (diff lines keep their indentation).
pub fn plain(text: &str, tabs: bool) -> String {
    text.chars()
        .filter(|c| {
            (!c.is_control() || (tabs && *c == '\t'))
                && !matches!(*c as u32, 0x200B..=0x200F | 0x202A..=0x202E | 0x2066..=0x2069)
        })
        .collect()
}

/// The words of the row.
pub fn label(change: &SkillChange) -> String {
    let verb = match change.action {
        SkillSaveAction::Created => "created",
        SkillSaveAction::Updated => "improved",
        SkillSaveAction::Deleted => "deleted",
    };
    let skill = plain(&change.skill, false);
    let reason = plain(change.reason.lines().next().unwrap_or_default(), false);
    if reason.trim().is_empty() {
        format!("Skill {skill} {verb}")
    } else {
        format!("Skill {skill} {verb}: {}", reason.trim())
    }
}

/// The rows of the changes the step `call_id` made.
pub fn rows(ws: &Workspace, shown: &str, call_id: &str, cx: &mut Context<Workspace>) -> Vec<Div> {
    let Some(chat) = ws.chats.get(shown) else {
        return Vec::new();
    };
    chat.skills
        .iter()
        .filter(|row| row.change.call_id == call_id)
        .map(|row| skill_row(ws, shown, row, cx))
        .collect()
}

fn skill_row(ws: &Workspace, shown: &str, row: &SkillRow, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (accent, muted, danger) = (theme.primary, step_muted(cx), theme.danger);
    let change = &row.change;
    let id = change.id;
    let key = (id as usize & (SKILL - 1)) | SKILL;
    let open = ws.unfolded.contains(&(shown.to_owned(), key));
    let text = label(change);
    let thread = shown.to_owned();
    let header = disclosure(SharedString::from(format!("skill-{shown}-{id}")), open, cx)
        .flex_1()
        .aria_label(text.clone())
        .child(widgets::icon(IconName::BookOpen, accent))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .text_color(if change.undone { muted } else { accent })
                .child(text),
        )
        .when(change.undone, |header| {
            header.child(div().flex_none().meta().text_color(muted).child("undone"))
        })
        .when(!change.undone && !change.can_undo, |header| {
            header.child(
                div()
                    .flex_none()
                    .meta()
                    .text_color(muted)
                    .child("previous text not kept"),
            )
        })
        .child(disclosure_icon(open, muted))
        .on_click(cx.listener(move |ws, _, _, cx| ws.open_skill_change(&thread, id, cx)));
    let mut line = div().flex().items_center().gap_2().child(header);
    if change.can_undo {
        let (thread, busy) = (shown.to_owned(), row.busy);
        line = line.child(
            Button::new(SharedString::from(format!("skill-undo-{shown}-{id}")))
                .ghost()
                .small()
                .label("Undo")
                .tooltip("Put the skill back as it was before this change")
                .disabled(busy)
                .on_click(cx.listener(move |ws, _, _, cx| ws.undo_skill(&thread, id, cx))),
        );
    }
    let mut block = div().flex().flex_col().child(line);
    if let Some(error) = &row.error {
        block = block.child(
            div()
                .pl_6()
                .meta()
                .text_color(danger)
                .child(plain(error, false)),
        );
    }
    if open {
        block = block.child(widgets::unfold(
            SharedString::from(format!("unfold-skill-{shown}-{id}")),
            div().pl_4().pt_1().child(diff_view(shown, row, cx)),
        ));
    }
    block
}

fn diff_view(shown: &str, row: &SkillRow, cx: &mut Context<Workspace>) -> Div {
    let theme = cx.theme();
    let (accent, muted, text, surface) = (
        theme.primary,
        theme.muted_foreground,
        theme.foreground,
        theme.muted,
    );
    let Some(diff) = &row.diff else {
        let words = match (&row.diff_error, row.loading) {
            (Some(error), _) => plain(error, false),
            (None, true) => "Loading...".to_owned(),
            (None, false) => "No diff to show".to_owned(),
        };
        return div().child(widgets::muted(words, cx));
    };
    let prefs = crate::ui::theme::prefs(cx);
    let mono = prefs
        .mono_font
        .clone()
        .unwrap_or_else(|| "Menlo".to_owned());
    let size = prefs.mono_size;
    let mut lines = div()
        .id(SharedString::from(format!(
            "skill-diff-lines-{shown}-{}",
            row.change.id
        )))
        .flex()
        .flex_col()
        .max_h(px(320.))
        .overflow_y_scroll()
        .rounded_md()
        .bg(surface)
        .py_1();
    for line in &diff.lines {
        // One accent: what was added carries it, what was removed fades and is struck.
        let (sign, color) = match line.kind {
            DiffKind::Added => ("+", text),
            DiffKind::Removed => ("\u{2212}", muted),
            DiffKind::Same => (" ", muted),
        };
        let mut body = div().flex_1().min_w_0().text_color(color);
        if line.kind == DiffKind::Removed {
            body = body.line_through();
        }
        lines = lines.child(
            div()
                .flex()
                .px_2()
                .text_size(px(size))
                .font_family(mono.clone())
                .when(line.kind == DiffKind::Added, |row| {
                    row.bg(accent.opacity(0.15))
                })
                .child(
                    div()
                        .w(px(14.))
                        .flex_none()
                        .text_color(if line.kind == DiffKind::Added {
                            accent
                        } else {
                            muted
                        })
                        .child(sign),
                )
                .child(body.child(plain(&line.text, true))),
        );
    }
    let mut block = div().flex().flex_col().gap_1().child(lines);
    if diff.cut {
        block = block.child(div().meta().text_color(muted).child("More lines follow"));
    }
    block
}

impl Workspace {
    /// Unfolds or folds the diff of a change; the first unfold asks the core for it.
    pub fn open_skill_change(&mut self, thread: &str, id: i64, cx: &mut Context<Self>) {
        self.fold(thread, (id as usize & (SKILL - 1)) | SKILL, cx);
        let Some(row) = self
            .chats
            .get_mut(thread)
            .and_then(|chat| chat.skills.iter_mut().find(|row| row.change.id == id))
        else {
            return;
        };
        if row.diff.is_some() || row.loading {
            return;
        }
        row.loading = true;
        row.diff_error = None;
        let thread = thread.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.catalog().skill_diff(id).await },
            move |ws, result, cx| {
                if let Some(row) = ws
                    .chats
                    .get_mut(&thread)
                    .and_then(|chat| chat.skills.iter_mut().find(|row| row.change.id == id))
                {
                    row.loading = false;
                    match result {
                        Ok(diff) => {
                            row.diff = Some(diff);
                            row.diff_error = None;
                        }
                        // A viewer of a shared chat gets the row, not the diff.
                        Err(error) => row.diff_error = Some(error.to_string()),
                    }
                }
                ws.measure_again = true;
                cx.notify();
            },
        );
        cx.notify();
    }

    pub fn undo_skill(&mut self, thread: &str, id: i64, cx: &mut Context<Self>) {
        let Some(row) = self
            .chats
            .get_mut(thread)
            .and_then(|chat| chat.skills.iter_mut().find(|row| row.change.id == id))
        else {
            return;
        };
        if row.busy {
            return;
        }
        row.busy = true;
        row.error = None;
        let thread = thread.to_owned();
        self.core.call(
            cx,
            move |client| async move { client.catalog().undo_skill_change(id).await },
            move |ws, result, cx| {
                if let Some(chat) = ws.chats.get_mut(&thread) {
                    match result {
                        Ok(change) => chat.skill_changed(change),
                        Err(error) => {
                            if let Some(row) =
                                chat.skills.iter_mut().find(|row| row.change.id == id)
                            {
                                row.busy = false;
                                row.error = Some(error.to_string());
                            }
                        }
                    }
                }
                ws.measure_again = true;
                cx.notify();
            },
        );
        self.measure_again = true;
        cx.notify();
    }
}

#[cfg(test)]
mod tests {
    use super::{SkillChange, SkillSaveAction, label, plain};

    fn change(reason: &str) -> SkillChange {
        SkillChange {
            id: 1,
            skill: "deploy".into(),
            call_id: "c".into(),
            action: SkillSaveAction::Updated,
            reason: reason.into(),
            changed_at_ms: 0,
            undone: false,
            can_undo: true,
        }
    }

    #[test]
    fn text_is_one_plain_line() {
        assert_eq!(
            label(&change("faster\u{202E}\nsecond line")),
            "Skill deploy improved: faster"
        );
        assert_eq!(label(&change("")), "Skill deploy improved");
        // Control and bidi characters go; a tab stays only where asked.
        assert_eq!(plain("a\u{1b}[31m\tb\u{202E}", true), "a[31m\tb");
        assert_eq!(plain("a\tb", false), "ab");
    }

    #[test]
    fn rows_come_from_typed_events_and_replace_by_id() {
        use dusk_protocol::{thread::ThreadEvent, tools::ToolResult};
        let mut chat = crate::pages::chat::transcript::Transcript::default();
        // The text marker of a tool result is not a row.
        chat.apply(ThreadEvent::ToolFinished {
            call_id: "c".into(),
            result: ToolResult::text("[skill-save] {}".into()),
        });
        assert!(chat.skills.is_empty());
        chat.apply(ThreadEvent::SkillChanged(change("one")));
        let mut undone = change("one");
        undone.undone = true;
        undone.can_undo = false;
        chat.apply(ThreadEvent::SkillChanged(undone));
        // A stale history list read before the undo does not bring Undo back.
        chat.skills_loaded(vec![change("one")]);
        assert_eq!(chat.skills.len(), 1);
        assert!(chat.skills[0].change.undone && !chat.skills[0].change.can_undo);
    }
}
