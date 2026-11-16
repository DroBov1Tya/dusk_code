//! The "About me" page: who the user is and the files `@me` attaches, then the
//! snippets each `@name` brings. The core keeps both as files under its rules folder.

use dusk_protocol::config::{Profile, ProfileFile, Snippet, is_name};
use gpui_kit::{
    component::{
        ActiveTheme, Disableable, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputEvent, InputState, Textarea, TextareaState},
        switch::Switch,
        tooltip::Tooltip,
    },
    prelude::FluentBuilder,
    *,
};

use super::{section, section_hint_color};
use crate::{
    ui::{Type, widgets},
    workspace::Workspace,
};

pub struct AboutMe {
    pub profile: Option<Profile>,
    pub snippets: Vec<Snippet>,
    pub about: Entity<TextareaState>,
    pub snippet_name: Entity<InputState>,
    pub snippet_text: Entity<TextareaState>,
    /// The snippet in the editor; none while a new one is written.
    pub open: Option<String>,
    /// Loaded text for the fields, put there on the next frame, which has the window.
    pub fill_about: Option<String>,
    pub fill_snippet: Option<(String, String)>,
    /// The profile as the core holds it; the fields are compared with it.
    stored: Profile,
    profile_save: SaveState,
    snippet_save: SaveState,
    profile_saving: bool,
    snippet_saving: bool,
    read_revision: u64,
    snippet_revision: u64,
    /// Tell when a field's text changes, so the note beside Save follows it.
    _changes: Vec<Subscription>,
}

/// What the last save came to, said beside its button until the text changes.
#[derive(Default)]
enum SaveState {
    #[default]
    Quiet,
    Saved,
    Failed(String),
}

impl AboutMe {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            profile: None,
            snippets: Vec::new(),
            about: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(4, 14)
                    .placeholder("Who you are, what you work on, how you like answers")
            }),
            snippet_name: cx.new(|cx| InputState::new(window, cx).placeholder("name")),
            snippet_text: cx.new(|cx| {
                TextareaState::new(window, cx)
                    .auto_grow(4, 14)
                    .placeholder("What @name brings into the message")
            }),
            open: None,
            fill_about: None,
            fill_snippet: None,
            stored: Profile::default(),
            profile_save: SaveState::Quiet,
            snippet_save: SaveState::Quiet,
            profile_saving: false,
            snippet_saving: false,
            read_revision: 0,
            snippet_revision: 0,
            _changes: Vec::new(),
        }
    }

    /// The profile on the page differs from the one the core holds.
    fn profile_dirty(&self, cx: &App) -> bool {
        self.profile.as_ref().is_some_and(|profile| {
            profile.files != self.stored.files
                || self.about.read(cx).value().as_ref() != self.stored.about.as_str()
        })
    }

    /// The snippet in the editor differs from the saved one, or is a new one with text.
    fn snippet_dirty(&self, cx: &App) -> bool {
        let name = self.snippet_name.read(cx).value();
        let text = self.snippet_text.read(cx).value();
        let saved = self.open.as_deref().and_then(|open| {
            self.snippets
                .iter()
                .find(|snippet| snippet.name.as_str() == open)
        });
        match saved {
            Some(saved) => {
                name.trim().trim_start_matches('@') != saved.name
                    || text.as_ref() != saved.text.as_str()
            }
            None => !name.trim().is_empty() || !text.trim().is_empty(),
        }
    }

    /// The names a message can mention, `me` first.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(dusk_protocol::config::ME)
            .chain(self.snippets.iter().map(|snippet| snippet.name.as_str()))
    }
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    if ws.me._changes.is_empty() {
        let about = cx.subscribe(&ws.me.about, |ws, _, event: &InputEvent, cx| {
            if matches!(event, InputEvent::Change) {
                ws.me.profile_save = SaveState::Quiet;
                cx.notify();
            }
        });
        ws.me._changes = vec![
            about,
            cx.subscribe(&ws.me.snippet_name, snippet_changed),
            cx.subscribe(&ws.me.snippet_text, snippet_changed),
        ];
    }
    if let Some(text) = ws.me.fill_about.take() {
        ws.me
            .about
            .update(cx, |about, cx| about.set_value(text, window, cx));
        // As the field holds it, so a text the field normalizes does not read as changed.
        ws.me.stored.about = ws.me.about.read(cx).value().to_string();
    }
    if let Some((name, text)) = ws.me.fill_snippet.take() {
        ws.me
            .snippet_name
            .update(cx, |input, cx| input.set_value(name, window, cx));
        ws.me
            .snippet_text
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }
    let muted = section_hint_color(cx);
    let profile_note = save_note(ws.me.profile_dirty(cx), &ws.me.profile_save, cx);
    let Some(profile) = &ws.me.profile else {
        return div().child(widgets::muted("Reading...", cx).px_0());
    };
    let mut body = section(
        "@me profile",
        "Write @me in a message to send this, with the files marked to send.",
        cx,
    )
    .child(Textarea::new(&ws.me.about).large().aria_label("About me"));
    for (index, file) in profile.files.iter().enumerate() {
        let path = file.path.display().to_string();
        let name = file.path.file_name().map_or_else(
            || file.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        body =
            body.child(
                div()
                    .flex()
                    .items_center()
                    .gap_2()
                    .child(
                        div()
                            .id(("me-file", index))
                            .flex_1()
                            .min_w_0()
                            .tooltip({
                                let path = path.clone();
                                move |window, cx| Tooltip::new(path.clone()).build(window, cx)
                            })
                            .child(div().body().truncate().child(name))
                            .child(div().small().text_color(muted).truncate().child(path)),
                    )
                    .child(div().small().text_color(muted).child("Send"))
                    .child(Switch::new(("me-send", index)).checked(file.send).on_click(
                        cx.listener(move |ws, on: &bool, _, cx| {
                            if let Some(file) = ws
                                .me
                                .profile
                                .as_mut()
                                .and_then(|profile| profile.files.get_mut(index))
                            {
                                file.send = *on;
                            }
                            ws.me.profile_save = SaveState::Quiet;
                            cx.notify();
                        }),
                    ))
                    .child(
                        Button::new(("me-remove", index))
                            .ghost()
                            .icon(IconName::Close)
                            .tooltip("Remove from the profile")
                            .on_click(cx.listener(move |ws, _, _, cx| {
                                if let Some(profile) = ws.me.profile.as_mut()
                                    && index < profile.files.len()
                                {
                                    profile.files.remove(index);
                                }
                                ws.me.profile_save = SaveState::Quiet;
                                cx.notify();
                            })),
                    ),
            );
    }
    body = body.child(
        div()
            .flex()
            .items_center()
            .justify_between()
            .flex_wrap()
            .gap_2()
            .child(
                Button::new("me-add")
                    .icon(IconName::Plus)
                    .label("Add files")
                    .on_click(cx.listener(|ws, _, _, cx| ws.add_profile_files(cx))),
            )
            .child(
                div()
                    .flex()
                    .items_center()
                    .gap_3()
                    .children(profile_note)
                    .child(
                        Button::new("me-save")
                            .primary()
                            .label(if ws.me.profile_saving {
                                "Saving..."
                            } else {
                                "Save profile"
                            })
                            .disabled(ws.me.profile_saving)
                            .on_click(cx.listener(|ws, _, _, cx| ws.save_profile(cx))),
                    ),
            ),
    );
    div()
        .flex()
        .flex_col()
        .gap_4()
        .child(body)
        .child(snippets(ws, cx))
}

/// The name is a line and the text an area, two kinds of field with one answer.
fn snippet_changed<T>(
    ws: &mut Workspace,
    _: Entity<T>,
    event: &InputEvent,
    cx: &mut Context<Workspace>,
) {
    if matches!(event, InputEvent::Change) {
        ws.me.snippet_save = SaveState::Quiet;
        cx.notify();
    }
}

/// Whether what is on the page is what the core holds: unsaved, saved, or why it was not.
fn save_note(dirty: bool, state: &SaveState, cx: &App) -> Option<Div> {
    let theme = cx.theme();
    let note = div().flex().items_center().gap_1().small();
    match state {
        SaveState::Failed(error) => Some(
            note.max_w(px(360.))
                .text_color(theme.danger)
                .child(format!("Not saved: {error}")),
        ),
        _ if dirty => Some(
            note.text_color(section_hint_color(cx))
                .child("Unsaved changes"),
        ),
        SaveState::Saved => Some(
            note.text_color(section_hint_color(cx))
                .child(widgets::icon(IconName::Check, theme.primary))
                .child("Saved"),
        ),
        SaveState::Quiet => None,
    }
}

/// The snippets by name, and the editor of the open one.
fn snippets(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let mut names = div().flex().flex_row().flex_wrap().gap_2();
    for (index, snippet) in ws.me.snippets.iter().enumerate() {
        let name = snippet.name.clone();
        let open = ws.me.open.as_deref() == Some(name.as_str());
        names = names.child(
            Button::new(("snippet", index))
                .when(open, |button| button.primary())
                .when(!open, |button| button.ghost())
                .label(format!("@{name}"))
                .on_click(cx.listener(move |ws, _, _, cx| ws.open_snippet(&name, cx))),
        );
    }
    names = names.child(
        Button::new("snippet-new")
            .icon(IconName::Plus)
            .label("New snippet")
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.me.snippet_revision += 1;
                ws.me.open = None;
                ws.me.fill_snippet = Some((String::new(), String::new()));
                ws.me.snippet_save = SaveState::Quiet;
                cx.notify();
            })),
    );
    let delete = ws.me.open.is_some().then(|| {
        Button::new("snippet-delete")
            .ghost()
            .label("Delete")
            .disabled(ws.me.snippet_saving)
            .on_click(cx.listener(|ws, _, _, cx| {
                if let Some(name) = ws.me.open.clone() {
                    ws.write_snippet(name, String::new(), cx);
                }
            }))
    });
    section(
        "Snippets",
        "Write @name in a message to bring a snippet in, such as a script or a checklist.",
        cx,
    )
    .child(names)
    .child(
        div()
            .flex()
            .flex_col()
            .gap_1()
            .max_w(px(280.))
            .child(div().group_name().child("Snippet name"))
            .child(
                Input::new(&ws.me.snippet_name)
                    .large()
                    .aria_label("Snippet name"),
            ),
    )
    .child(
        Textarea::new(&ws.me.snippet_text)
            .large()
            .aria_label("Snippet text"),
    )
    .child(
        div()
            .flex()
            .items_center()
            .justify_end()
            .flex_wrap()
            .gap_2()
            .children(save_note(ws.me.snippet_dirty(cx), &ws.me.snippet_save, cx))
            .children(delete)
            .child(
                Button::new("snippet-save")
                    .primary()
                    .label(if ws.me.snippet_saving {
                        "Saving..."
                    } else {
                        "Save snippet"
                    })
                    .disabled(ws.me.snippet_saving)
                    .on_click(cx.listener(|ws, _, _, cx| ws.save_snippet(cx))),
            ),
    )
}

impl Workspace {
    /// Reads the profile and the snippets again.
    pub fn load_about_me(&mut self, cx: &mut Context<Self>) {
        self.read_about_me(cx);
    }

    /// Reads the page again; after a save, what the core answers is what confirms it.
    fn read_about_me(&mut self, cx: &mut Context<Self>) {
        let revision = self.me.read_revision;
        self.core.call(
            cx,
            |client| async move {
                let catalog = client.catalog();
                Ok((catalog.profile().await?, catalog.snippets().await?))
            },
            move |ws, result, cx| match result {
                Ok((profile, snippets)) => {
                    if revision != ws.me.read_revision {
                        return;
                    }
                    // Refreshing the page keeps fields that are still being written.
                    if !ws.me.profile_dirty(cx) {
                        ws.me.fill_about = Some(profile.about.clone());
                        ws.me.profile = Some(profile.clone());
                    }
                    ws.me.stored = profile;
                    ws.me.snippets = snippets;
                    cx.notify();
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn add_profile_files(&mut self, cx: &mut Context<Self>) {
        let picked = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: false,
            multiple: true,
            prompt: Some("Add".into()),
        });
        cx.spawn(async move |ws, cx| {
            let Ok(Ok(Some(paths))) = picked.await else {
                return;
            };
            let _ = ws.update(cx, |ws, cx| {
                if let Some(profile) = ws.me.profile.as_mut() {
                    for path in paths {
                        if !profile.files.iter().any(|file| file.path == path) {
                            profile.files.push(ProfileFile { path, send: true });
                        }
                    }
                }
                ws.me.profile_save = SaveState::Quiet;
                cx.notify();
            });
        })
        .detach();
    }

    fn save_profile(&mut self, cx: &mut Context<Self>) {
        if self.me.profile_saving {
            return;
        }
        let Some(mut profile) = self.me.profile.clone() else {
            return;
        };
        profile.about = self.me.about.read(cx).value().to_string();
        self.me.profile_saving = true;
        self.me.profile_save = SaveState::Quiet;
        self.me.read_revision += 1;
        self.core.call(
            cx,
            move |client| async move {
                let catalog = client.catalog();
                catalog.save_profile(profile).await?;
                catalog.profile().await
            },
            |ws, result, cx| {
                ws.me.profile_saving = false;
                ws.me.read_revision += 1;
                match result {
                    Ok(profile) => {
                        ws.me.stored = profile;
                        ws.me.profile_save = if ws.me.profile_dirty(cx) {
                            SaveState::Quiet
                        } else {
                            SaveState::Saved
                        };
                    }
                    Err(error) => {
                        tracing::warn!("{error:#}");
                        ws.me.profile_save = SaveState::Failed(format!("{error:#}"));
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    fn open_snippet(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(snippet) = self.me.snippets.iter().find(|s| s.name == name) {
            self.me.snippet_revision += 1;
            self.me.open = Some(snippet.name.clone());
            self.me.snippet_save = SaveState::Quiet;
            self.me.fill_snippet = Some((snippet.name.clone(), snippet.text.clone()));
        }
        cx.notify();
    }

    /// Saves the editor's snippet under the name in its field.
    fn save_snippet(&mut self, cx: &mut Context<Self>) {
        if self.me.snippet_saving {
            return;
        }
        let name = self
            .me
            .snippet_name
            .read(cx)
            .value()
            .trim()
            .trim_start_matches('@')
            .to_owned();
        let text = self.me.snippet_text.read(cx).value().to_string();
        if !is_name(&name) || name == dusk_protocol::config::ME {
            self.me.snippet_save = SaveState::Failed(
                "a snippet name is letters, digits, - and _, and not \"me\"".to_owned(),
            );
            cx.notify();
            return;
        }
        // A new name moves the snippet, leaving no copy under the old one.
        let old = self.me.open.clone().filter(|old| *old != name);
        let origin = self.me.open.clone();
        let revision = self.me.snippet_revision;
        let saved_name = name.clone();
        self.me.snippet_saving = true;
        self.me.snippet_save = SaveState::Quiet;
        self.me.read_revision += 1;
        self.core.call(
            cx,
            move |client| async move {
                let catalog = client.catalog();
                catalog.save_snippet(&name, &text).await?;
                if let Some(old) = old {
                    catalog.save_snippet(&old, "").await?;
                }
                catalog.snippets().await
            },
            move |ws, result, cx| {
                ws.me.snippet_saving = false;
                ws.me.read_revision += 1;
                match result {
                    Ok(snippets) => {
                        ws.me.snippets = snippets;
                        if ws.me.snippet_revision == revision && ws.me.open == origin {
                            ws.me.open = Some(saved_name);
                            ws.me.snippet_save = if ws.me.snippet_dirty(cx) {
                                SaveState::Quiet
                            } else {
                                SaveState::Saved
                            };
                        }
                    }
                    Err(error) => {
                        tracing::warn!("{error:#}");
                        if ws.me.snippet_revision == revision && ws.me.open == origin {
                            ws.me.snippet_save = SaveState::Failed(format!("{error:#}"));
                        }
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }

    /// Empty text removes the snippet.
    fn write_snippet(&mut self, name: String, text: String, cx: &mut Context<Self>) {
        if self.me.snippet_saving {
            return;
        }
        let removed = text.trim().is_empty();
        let target = name.clone();
        let revision = self.me.snippet_revision;
        let before = (
            self.me.snippet_name.read(cx).value().to_string(),
            self.me.snippet_text.read(cx).value().to_string(),
        );
        self.me.snippet_saving = true;
        self.me.read_revision += 1;
        self.core.call(
            cx,
            move |client| async move {
                let catalog = client.catalog();
                catalog.save_snippet(&name, &text).await?;
                catalog.snippets().await
            },
            move |ws, result, cx| {
                ws.me.snippet_saving = false;
                ws.me.read_revision += 1;
                match result {
                    Ok(snippets) => {
                        ws.me.snippets = snippets;
                        if removed
                            && ws.me.snippet_revision == revision
                            && ws.me.open.as_deref() == Some(target.as_str())
                        {
                            ws.me.open = None;
                            if ws.me.snippet_name.read(cx).value().as_ref() == before.0
                                && ws.me.snippet_text.read(cx).value().as_ref() == before.1
                            {
                                ws.me.fill_snippet = Some((String::new(), String::new()));
                            }
                            ws.me.snippet_save = SaveState::Quiet;
                        }
                    }
                    Err(error) => ws.fail(error, cx),
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}
