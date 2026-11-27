//! The first screen of a clean install: while the core has no provider, the chat page
//! shows five pages in order, turned sideways: hello, a model to connect, how much the
//! agent may do without asking, the tools that stay off until switched on, and ready. Once shown it stays until Start or Skip setup,
//! even after a provider connects on the way; finishing is kept in the window's
//! preferences, so it never comes back. People who already have a provider never see it.
//! - `view`: the frame, the dots and buttons, the first and last pages;
//! - `form`: the Model page, the connection the Model settings page makes;
//! - `access`: the Access page, the default approval mode;
//! - `tools`: the Tools page and its switches;
//! - `swipe`: a sideways trackpad swipe, from the scroll events;
//! - `name`: the provider's name, taken from the address.

mod access;
mod form;
mod name;
mod swipe;
mod tools;
mod view;

pub use view::render;

use dusk_protocol::{
    client::{Answer, Command, ProviderTarget, Secret},
    config::ProviderKind,
    model::CheckFailure,
};
use gpui_kit::{
    component::input::{InputEvent, InputState},
    *,
};

use crate::workspace::{Center, Workspace};

/// The pages in the order they are turned.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Page {
    Hello,
    Model,
    Access,
    Tools,
    Ready,
}

impl Page {
    pub const ALL: [Page; 5] = [
        Page::Hello,
        Page::Model,
        Page::Access,
        Page::Tools,
        Page::Ready,
    ];

    fn index(self) -> usize {
        Self::ALL.iter().position(|page| *page == self).unwrap_or(0)
    }

    /// The page `step` places away, if there is one.
    fn turned(self, step: isize) -> Option<Page> {
        let index = self.index().checked_add_signed(step)?;
        Self::ALL.get(index).copied()
    }
}

/// Why the last attempt failed: the kind, when the core could tell it, and its words.
struct Failed {
    cause: Option<CheckFailure>,
    said: String,
}

pub struct Welcome {
    /// The page in view; none when the welcome is not shown.
    pub page: Option<Page>,
    /// Which way the last turn went, so the new page slides in from that side.
    toward: isize,
    /// What the provider is called in the model menu; empty takes the address's host.
    name: Entity<InputState>,
    address: Entity<InputState>,
    token: Entity<InputState>,
    /// The core's answer to the last attempt, when it failed.
    error: Option<Failed>,
    busy: bool,
    /// The provider connected here and the model it starts with, for the Model page.
    joined: Option<(String, Option<String>)>,
    /// A connection went through: once the welcome is finished, a new chat opens.
    connected: bool,
    swipe: swipe::Swipe,
}

impl Welcome {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let name = cx.new(|cx| InputState::new(window, cx).placeholder("Such as my-server"));
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("https://api.example.com/v1 or http://127.0.0.1:11434/v1")
        });
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Empty for a local server")
                .masked(true)
        });
        for field in [&name, &address, &token] {
            cx.subscribe_in(field, window, |ws, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    ws.welcome_connect(window, cx);
                }
            })
            .detach();
        }
        Self {
            page: None,
            toward: 0,
            name,
            address,
            token,
            error: None,
            busy: false,
            joined: None,
            connected: false,
            swipe: swipe::Swipe::default(),
        }
    }

    /// Whether a box of the form has the keyboard, so the arrows stay the box's.
    pub fn typing(&self, window: &Window, cx: &App) -> bool {
        [&self.name, &self.address, &self.token]
            .iter()
            .any(|field| field.focus_handle(cx).is_focused(window))
    }
}

/// Whether the chat page shows the welcome in its place.
pub fn shown(ws: &Workspace) -> bool {
    ws.welcome.page.is_some() && ws.center == Center::Chat
}

/// Before each frame: once the welcome is finished, a connection made in it opens a new
/// chat, which needs the window.
pub fn settle(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    if ws.welcome.page.is_none() && std::mem::take(&mut ws.welcome.connected) {
        let project = ws.project.clone();
        ws.new_chat(project.as_deref(), window, cx);
    }
}

impl Workspace {
    /// Asks the core for its providers once at start; none, on a welcome never finished,
    /// means a first run.
    pub fn ask_welcome(&mut self, cx: &mut Context<Self>) {
        if crate::ui::theme::prefs(cx).welcome_done {
            return;
        }
        self.core.call(
            cx,
            |client| async move { client.command(Command::Provider { name: None }).await },
            |ws, result, cx| match result {
                Ok(Answer::Providers { providers, .. }) if providers.is_empty() => {
                    ws.welcome.page = Some(Page::Hello);
                    // The Access and Tools pages read the settings.
                    ws.load_settings(cx);
                    cx.notify();
                }
                Ok(_) => {}
                Err(error) => tracing::warn!("cannot list the providers: {error:#}"),
            },
        );
    }

    /// Shows `page`, sliding in from the side it lies on.
    pub fn welcome_go(&mut self, page: Page, window: &mut Window, cx: &mut Context<Self>) {
        let Some(from) = self.welcome.page else {
            return;
        };
        if from == page {
            return;
        }
        self.welcome.toward = if page.index() > from.index() { 1 } else { -1 };
        self.welcome.page = Some(page);
        // A box left behind would keep the arrows; the window takes them back.
        if self.welcome.typing(window, cx) {
            window.focus(&self.focus, cx);
        }
        cx.notify();
    }

    /// The next page (`1`) or the one before (`-1`); past the last page is Start.
    pub fn welcome_turn(&mut self, step: isize, window: &mut Window, cx: &mut Context<Self>) {
        let Some(page) = self.welcome.page else {
            return;
        };
        match page.turned(step) {
            Some(next) => self.welcome_go(next, window, cx),
            None if step > 0 => self.welcome_finish(cx),
            None => {}
        }
    }

    /// Start, Skip setup and Escape outside the form's boxes: the welcome is done for good.
    pub fn welcome_finish(&mut self, cx: &mut Context<Self>) {
        if self.welcome.page.take().is_none() {
            return;
        }
        self.welcome.error = None;
        crate::ui::theme::change(cx, |prefs| prefs.welcome_done = true);
        cx.notify();
    }

    /// Escape while the welcome shows: in a box of the form it only leaves the box, so
    /// leaving one never ends the welcome for good; else it finishes the welcome. True
    /// when it did either.
    pub fn welcome_escape(&mut self, window: &mut Window, cx: &mut Context<Self>) -> bool {
        if !shown(self) {
            return false;
        }
        if self.welcome.typing(window, cx) {
            window.focus(&self.focus, cx);
            cx.notify();
            return true;
        }
        self.welcome_finish(cx);
        true
    }

    /// Checks the server first, since saving makes no request and only the check can say
    /// it is unreachable or refuses the token; then saves it. The token leaves the box at
    /// once, as on the Model page, and goes to the core only.
    pub fn welcome_connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let welcome = &mut self.welcome;
        if welcome.busy || welcome.page != Some(Page::Model) {
            return;
        }
        let address = welcome.address.read(cx).value().trim().to_owned();
        if address.is_empty() {
            let said = "Type the server's address first.".into();
            welcome.error = Some(Failed { cause: None, said });
            cx.notify();
            return;
        }
        let token = Some(welcome.token.read(cx).value().to_string()).filter(|t| !t.is_empty());
        welcome
            .token
            .update(cx, |field, cx| field.set_value("", window, cx));
        welcome.busy = true;
        welcome.error = None;
        let typed = welcome.name.read(cx).value().trim().to_owned();
        let name = Some(typed)
            .filter(|name| !name.is_empty())
            .or_else(|| name::from_address(&address));
        let kind = ProviderKind::OpenAiCompatible;
        self.core.call(
            cx,
            move |client| async move {
                let settings = client.settings();
                let target = ProviderTarget::New {
                    kind,
                    base_url: address.clone(),
                    api_key: token.clone().map(Secret),
                };
                let status = settings.check_provider(target).await?;
                let first = match status.models {
                    Ok(models) => models.into_iter().next().map(|model| model.id),
                    Err(said) => {
                        let cause = status.failure;
                        return Ok(Err(Failed { cause, said }));
                    }
                };
                let name = settings.connect(kind, name, address, token).await?;
                Ok(Ok((name, first)))
            },
            |ws, result, cx| {
                ws.welcome.busy = false;
                match result {
                    Ok(Ok((name, first))) => {
                        ws.welcome.connected = true;
                        // The new chat can be used at once, as a first model is there.
                        if let Some(model) = &first {
                            ws.choose_model(format!("{name}/{model}"), cx);
                        }
                        ws.welcome.joined = Some((name, first));
                        ws.load_models(cx);
                    }
                    Ok(Err(failed)) => ws.welcome.error = Some(failed),
                    Err(error) => {
                        let said = format!("{error:#}");
                        ws.welcome.error = Some(Failed { cause: None, said });
                    }
                }
                cx.notify();
            },
        );
        cx.notify();
    }
}

#[cfg(feature = "screenshot")]
impl Workspace {
    /// For pictures: the address as if typed.
    pub fn welcome_address(&mut self, address: &str, window: &mut Window, cx: &mut Context<Self>) {
        self.welcome.address.update(cx, |field, cx| {
            field.set_value(address.to_owned(), window, cx)
        });
        cx.notify();
    }

    /// For pictures: the Model page as after a connection to `name`.
    pub fn welcome_joined(&mut self, name: &str) {
        let model = Some("llama-3.1-8b-instruct".to_owned());
        self.welcome.joined = Some((name.to_owned(), model));
    }
}
