//! The first screen of a clean install: while the core has no provider, the chat page
//! offers to connect one, with only an address and a token. The connection is the
//! Model page's own: the core checks the server, then saves it as OpenAI-compatible,
//! with the token in the keychain. Dismissed, it stays away until the app starts again;
//! once any provider is connected it does not come back.
//! - `view`: the two screens drawn;
//! - `name`: the provider's name, taken from the address.

mod name;
mod view;

pub use view::render;

use dusk_protocol::{
    client::{Answer, Command, ProviderTarget, Secret},
    config::ProviderKind,
};
use gpui_kit::{
    component::input::{InputEvent, InputState},
    *,
};

use crate::workspace::{Center, Workspace};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Step {
    /// Not shown: providers are known, the core has not answered yet, or "Not now".
    Hidden,
    /// The greeting and its one button.
    Intro,
    /// The address and the token.
    Form,
}

pub struct Welcome {
    pub step: Step,
    address: Entity<InputState>,
    token: Entity<InputState>,
    /// The core's answer to the last attempt, when it failed.
    error: Option<String>,
    busy: bool,
    /// A connection went through: the next frame, which has the window, opens a new chat.
    connected: bool,
}

impl Welcome {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let address = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("http://localhost:11434/v1 or https://api.example.com/v1")
        });
        let token = cx.new(|cx| {
            InputState::new(window, cx)
                .placeholder("Empty for a local server")
                .masked(true)
        });
        for field in [&address, &token] {
            cx.subscribe_in(field, window, |ws, _, event: &InputEvent, window, cx| {
                if let InputEvent::PressEnter { .. } = event {
                    ws.welcome_connect(window, cx);
                }
            })
            .detach();
        }
        Self {
            step: Step::Hidden,
            address,
            token,
            error: None,
            busy: false,
            connected: false,
        }
    }
}

/// Whether the chat page shows the welcome in its place.
pub fn shown(ws: &Workspace) -> bool {
    ws.welcome.step != Step::Hidden && ws.center == Center::Chat && ws.providers.is_empty()
}

/// Before each frame: a provider connected anywhere ends the welcome for good, and a
/// connection made here opens a new chat.
pub fn settle(ws: &mut Workspace, window: &mut Window, cx: &mut Context<Workspace>) {
    if !ws.providers.is_empty() {
        ws.welcome.step = Step::Hidden;
    }
    if std::mem::take(&mut ws.welcome.connected) {
        let project = ws.project.clone();
        ws.new_chat(project.as_deref(), window, cx);
    }
}

impl Workspace {
    /// Asks the core for its providers once at start; none means a clean install.
    pub fn ask_welcome(&mut self, cx: &mut Context<Self>) {
        self.core.call(
            cx,
            |client| async move { client.command(Command::Provider { name: None }).await },
            |ws, result, cx| match result {
                Ok(Answer::Providers { providers, .. }) if providers.is_empty() => {
                    ws.welcome.step = Step::Intro;
                    cx.notify();
                }
                Ok(_) => {}
                Err(error) => tracing::warn!("cannot list the providers: {error:#}"),
            },
        );
    }

    pub fn welcome_form(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.welcome.step = Step::Form;
        self.welcome
            .address
            .update(cx, |field, cx| field.focus(window, cx));
        cx.notify();
    }

    /// Escape on the form goes back to the greeting. True when it did.
    pub fn welcome_back(&mut self, cx: &mut Context<Self>) -> bool {
        if !shown(self) || self.welcome.step != Step::Form {
            return false;
        }
        self.welcome.step = Step::Intro;
        self.welcome.error = None;
        cx.notify();
        true
    }

    fn welcome_dismiss(&mut self, cx: &mut Context<Self>) {
        self.welcome.step = Step::Hidden;
        cx.notify();
    }

    /// Checks the server first, since saving makes no request and only the check can say
    /// it is unreachable or refuses the token; then saves it. The token leaves the box at
    /// once, as on the Model page, and goes to the core only.
    pub fn welcome_connect(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let welcome = &mut self.welcome;
        if welcome.busy || welcome.step != Step::Form {
            return;
        }
        let address = welcome.address.read(cx).value().trim().to_owned();
        if address.is_empty() {
            welcome.error = Some("Type the server's address first.".into());
            cx.notify();
            return;
        }
        let token = Some(welcome.token.read(cx).value().to_string()).filter(|t| !t.is_empty());
        welcome
            .token
            .update(cx, |field, cx| field.set_value("", window, cx));
        welcome.busy = true;
        welcome.error = None;
        let name = name::from_address(&address);
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
                if let Err(error) = settings.check_provider(target).await?.models {
                    anyhow::bail!(error);
                }
                settings.connect(kind, name, address, token).await
            },
            |ws, result, cx| {
                ws.welcome.busy = false;
                match result {
                    Ok(_) => {
                        ws.welcome.step = Step::Hidden;
                        ws.welcome.connected = true;
                        ws.load_models(cx);
                    }
                    Err(error) => ws.welcome.error = Some(format!("{error:#}")),
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
}
