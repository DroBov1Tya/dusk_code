//! The "About me" page: who the user is and the files `@me` attaches, then the
//! snippets each `@name` brings. The core keeps both as files under its rules folder.

use dusk_protocol::mentions::{Profile, ProfileFile, Snippet, is_name};
use gpui_kit::{
    component::{
        ActiveTheme, IconName, Sizable,
        button::{Button, ButtonVariants},
        input::{Input, InputState, Textarea, TextareaState},
        switch::Switch,
    },
    prelude::FluentBuilder,
    *,
};

use super::line;
use crate::{widgets, workspace::Workspace};

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
        }
    }

    /// The names a message can mention, `me` first.
    pub fn names(&self) -> impl Iterator<Item = &str> {
        std::iter::once(dusk_protocol::mentions::ME)
            .chain(self.snippets.iter().map(|snippet| snippet.name.as_str()))
    }
}

pub fn render(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) -> Div {
    if let Some(text) = ws.me.fill_about.take() {
        ws.me
            .about
            .update(cx, |about, cx| about.set_value(text, window, cx));
    }
    if let Some((name, text)) = ws.me.fill_snippet.take() {
        ws.me
            .snippet_name
            .update(cx, |input, cx| input.set_value(name, window, cx));
        ws.me
            .snippet_text
            .update(cx, |input, cx| input.set_value(text, window, cx));
    }
    let muted = cx.theme().muted_foreground;
    let Some(profile) = &ws.me.profile else {
        return div().child(widgets::muted("Reading...", cx).px_0());
    };
    let mut body = div()
        .flex()
        .flex_col()
        .gap_2()
        .child(widgets::caption("@me", cx))
        .child(
            div()
                .text_sm()
                .text_color(muted)
                .child("Write @me in a message to send this, with the files marked to send."),
        )
        .child(
            div()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .child(Textarea::new(&ws.me.about).appearance(false)),
        );
    for (index, file) in profile.files.iter().enumerate() {
        let name = file.path.file_name().map_or_else(
            || file.path.display().to_string(),
            |name| name.to_string_lossy().into_owned(),
        );
        body = body.child(
            line(&name, &file.path.display().to_string(), cx)
                .child(div().text_xs().text_color(muted).child("send"))
                .child(
                    Switch::new(("me-send", index))
                        .checked(file.send)
                        .small()
                        .on_click(cx.listener(move |ws, on: &bool, _, cx| {
                            if let Some(file) = ws
                                .me
                                .profile
                                .as_mut()
                                .and_then(|profile| profile.files.get_mut(index))
                            {
                                file.send = *on;
                            }
                            cx.notify();
                        })),
                )
                .child(
                    Button::new(("me-remove", index))
                        .ghost()
                        .xsmall()
                        .icon(IconName::Close)
                        .tooltip("Remove from the profile")
                        .on_click(cx.listener(move |ws, _, _, cx| {
                            if let Some(profile) = ws.me.profile.as_mut()
                                && index < profile.files.len()
                            {
                                profile.files.remove(index);
                            }
                            cx.notify();
                        })),
                ),
        );
    }
    body = body.child(
        div()
            .flex()
            .flex_row()
            .gap_2()
            .pt_1()
            .child(
                Button::new("me-add")
                    .small()
                    .icon(IconName::Plus)
                    .label("Add files")
                    .on_click(cx.listener(|ws, _, _, cx| ws.add_profile_files(cx))),
            )
            .child(
                Button::new("me-save")
                    .small()
                    .primary()
                    .label("Save")
                    .on_click(cx.listener(|ws, _, _, cx| ws.save_profile(cx))),
            ),
    );
    body.child(div().pt_6().child(widgets::caption("Snippets", cx)))
        .child(div().text_sm().text_color(muted).child(
            "Write @name in a message to bring a snippet in, such as a script or a checklist.",
        ))
        .child(snippets(ws, cx))
}

/// The snippets by name, and the editor of the open one.
fn snippets(ws: &Workspace, cx: &mut Context<Workspace>) -> Div {
    let mut names = div().flex().flex_row().flex_wrap().gap_1();
    for (index, snippet) in ws.me.snippets.iter().enumerate() {
        let name = snippet.name.clone();
        let open = ws.me.open.as_deref() == Some(name.as_str());
        names = names.child(
            Button::new(("snippet", index))
                .small()
                .when(open, |button| button.primary())
                .when(!open, |button| button.ghost())
                .label(format!("@{name}"))
                .on_click(cx.listener(move |ws, _, _, cx| ws.open_snippet(&name, cx))),
        );
    }
    names = names.child(
        Button::new("snippet-new")
            .small()
            .ghost()
            .icon(IconName::Plus)
            .label("New")
            .on_click(cx.listener(|ws, _, _, cx| {
                ws.me.open = None;
                ws.me.fill_snippet = Some((String::new(), String::new()));
                cx.notify();
            })),
    );
    let delete = ws.me.open.is_some().then(|| {
        Button::new("snippet-delete")
            .small()
            .ghost()
            .label("Delete")
            .on_click(cx.listener(|ws, _, _, cx| {
                if let Some(name) = ws.me.open.clone() {
                    ws.write_snippet(name, String::new(), cx);
                }
            }))
    });
    div()
        .flex()
        .flex_col()
        .gap_2()
        .child(names)
        .child(div().w(px(240.)).child(Input::new(&ws.me.snippet_name)))
        .child(
            div()
                .p_2()
                .rounded_md()
                .border_1()
                .border_color(cx.theme().border)
                .child(Textarea::new(&ws.me.snippet_text).appearance(false)),
        )
        .child(
            div()
                .flex()
                .flex_row()
                .gap_2()
                .child(
                    Button::new("snippet-save")
                        .small()
                        .primary()
                        .label("Save")
                        .on_click(cx.listener(|ws, _, _, cx| ws.save_snippet(cx))),
                )
                .children(delete),
        )
}

impl Workspace {
    /// Reads the profile and the snippets again.
    pub fn load_about_me(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move {
                let catalog = client.catalog();
                Ok((catalog.profile().await?, catalog.snippets().await?))
            },
            |ws, result, cx| match result {
                Ok((profile, snippets)) => {
                    ws.me.fill_about = Some(profile.about.clone());
                    ws.me.profile = Some(profile);
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
                cx.notify();
            });
        })
        .detach();
    }

    fn save_profile(&mut self, cx: &mut Context<Self>) {
        let Some(mut profile) = self.me.profile.clone() else {
            return;
        };
        profile.about = self.me.about.read(cx).value().to_string();
        self.core.call(
            cx,
            move |client| async move { client.catalog().save_profile(profile).await },
            |ws, result, cx| match result {
                Ok(()) => ws.load_about_me(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    fn open_snippet(&mut self, name: &str, cx: &mut Context<Self>) {
        if let Some(snippet) = self.me.snippets.iter().find(|s| s.name == name) {
            self.me.open = Some(snippet.name.clone());
            self.me.fill_snippet = Some((snippet.name.clone(), snippet.text.clone()));
        }
        cx.notify();
    }

    /// Saves the editor's snippet under the name in its field.
    fn save_snippet(&mut self, cx: &mut Context<Self>) {
        let name = self
            .me
            .snippet_name
            .read(cx)
            .value()
            .trim()
            .trim_start_matches('@')
            .to_owned();
        let text = self.me.snippet_text.read(cx).value().to_string();
        if !is_name(&name) || name == dusk_protocol::mentions::ME {
            self.fail(
                anyhow::anyhow!("a snippet name is letters, digits, - and _, and not \"me\""),
                cx,
            );
            return;
        }
        // A new name moves the snippet, leaving no copy under the old one.
        let old = self.me.open.clone().filter(|old| *old != name);
        self.me.open = Some(name.clone());
        self.core.call(
            cx,
            move |client| async move {
                let catalog = client.catalog();
                catalog.save_snippet(&name, &text).await?;
                if let Some(old) = old {
                    catalog.save_snippet(&old, "").await?;
                }
                Ok(())
            },
            |ws, result, cx| match result {
                Ok(()) => ws.load_about_me(cx),
                Err(error) => ws.fail(error, cx),
            },
        );
    }

    /// Empty text removes the snippet.
    fn write_snippet(&mut self, name: String, text: String, cx: &mut Context<Self>) {
        let removed = text.trim().is_empty();
        self.core.call(
            cx,
            move |client| async move { client.catalog().save_snippet(&name, &text).await },
            move |ws, result, cx| match result {
                Ok(()) => {
                    if removed {
                        ws.me.open = None;
                        ws.me.fill_snippet = Some((String::new(), String::new()));
                    }
                    ws.load_about_me(cx);
                }
                Err(error) => ws.fail(error, cx),
            },
        );
    }
}
