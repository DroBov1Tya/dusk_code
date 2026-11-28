//! The first screen of a clean install, as the desktop app has it: five pages walked
//! sideways in order (hello, a model, how much the agent may do alone, the tools, ready), with dots, Back and Next and
//! the arrow keys. It shows while the core has no provider and the user has not finished
//! it, and it stays until Start or Skip setup even once a provider connects in the middle.
//! "Finished" is this browser's own preference, kept in its storage.

mod access;
mod provider;
mod tools;

use crate::{api, chat::Chat, state::state};
use access::Access;
use dusk_protocol::client::{Answer, Command};
use leptos::{ev, prelude::*, task::spawn_local};
use provider::Provider;
use tools::Tools;

const FINISHED: &str = "duskcode-welcome-finished";

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Page {
    Hello,
    Model,
    Access,
    Tools,
    Ready,
}

const PAGES: [Page; 5] = [
    Page::Hello,
    Page::Model,
    Page::Access,
    Page::Tools,
    Page::Ready,
];

impl Page {
    fn index(self) -> usize {
        self as usize
    }

    fn step(self, by: isize) -> Option<Self> {
        let next = self.index().checked_add_signed(by)?;
        PAGES.get(next).copied()
    }
}

/// The welcome's state, kept by the shell so leaving for Customize and coming back to
/// the chat finds it where it was.
#[derive(Clone, Copy)]
pub struct Welcome {
    /// The page shown; none once it is finished or was never needed.
    page: RwSignal<Option<Page>>,
    /// The way the last move went, for the slide: -1 back, 1 forward.
    toward: RwSignal<isize>,
    /// The provider connected here, by the name the core saved it under.
    connected: RwSignal<Option<String>>,
    name: RwSignal<String>,
    address: RwSignal<String>,
}

impl Welcome {
    pub fn new() -> Self {
        Self {
            page: RwSignal::new(None),
            toward: RwSignal::new(1),
            connected: RwSignal::new(None),
            name: RwSignal::new(String::new()),
            address: RwSignal::new(String::new()),
        }
    }

    /// Asks the core for its providers once the link is up (the first status says so);
    /// none means a clean install, unless this browser finished the welcome before.
    pub fn ask(self) {
        if finished() {
            return;
        }
        let asked = StoredValue::new(false);
        Effect::new(move |_| {
            if state().status.with(Option::is_none) || asked.get_value() {
                return;
            }
            asked.set_value(true);
            spawn_local(async move {
                if let Ok(Answer::Providers { providers, .. }) =
                    api::command(Command::Provider { name: None }).await
                    && providers.is_empty()
                {
                    self.page.set(Some(Page::Hello));
                }
            });
        });
    }

    fn go(self, page: Page) {
        let Some(from) = self.page.get_untracked() else {
            return;
        };
        if page != from {
            self.toward.set(if page > from { 1 } else { -1 });
            self.page.set(Some(page));
        }
    }

    fn step(self, by: isize) {
        if let Some(page) = self.page.get_untracked().and_then(|p| p.step(by)) {
            self.go(page);
        }
    }

    /// Start or Skip setup: the welcome is done for this browser.
    fn finish(self) {
        self.page.set(None);
        if let Some(storage) = storage() {
            let _ = storage.set_item(FINISHED, "1");
        }
    }
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok().flatten()
}

fn finished() -> bool {
    storage().is_some_and(|s| s.get_item(FINISHED).ok().flatten().is_some())
}

/// The chat page, or the welcome in its place.
#[component]
pub fn ChatOrWelcome(id: Option<String>, welcome: Welcome) -> impl IntoView {
    // Only showing or not rebuilds; a page change re-renders the body alone, so the
    // focus stays on Next, Back or the dot that was pressed.
    let shown = Memo::new(move |_| welcome.page.with(Option::is_some));
    move || match shown.get() {
        false => view! { <Chat id=id.clone() /> }.into_any(),
        true => view! { <Pages welcome /> }.into_any(),
    }
}

#[component]
fn Pages(welcome: Welcome) -> impl IntoView {
    // Arrows walk the pages and Escape finishes, but only for keys meant for the
    // welcome: not one a drawer or menu already took, not one pressed inside a drawer
    // over it, nor any while a drawer makes the welcome inert (focus on the body never
    // reaches the drawer's own handler). Escape in a box only leaves the box; arrows
    // there move the caret.
    let root = NodeRef::<leptos::html::Div>::new();
    let keys = window_event_listener(ev::keydown, move |event| {
        let inert = root
            .get_untracked()
            .is_some_and(|root| root.closest("[inert]").ok().flatten().is_some());
        if event.default_prevented() || inert {
            return;
        }
        let target = event
            .target()
            .and_then(|t| wasm_bindgen::JsCast::dyn_into::<web_sys::HtmlElement>(t).ok());
        let ours = target.as_ref().is_none_or(|e| {
            matches!(e.tag_name().as_str(), "BODY" | "HTML")
                || e.closest(".welcome").ok().flatten().is_some()
        });
        if !ours {
            return;
        }
        let typing = target.as_ref().filter(|e| {
            matches!(e.tag_name().as_str(), "INPUT" | "TEXTAREA" | "SELECT")
                && e.get_attribute("type").as_deref() != Some("checkbox")
        });
        match (event.key().as_str(), typing) {
            ("ArrowRight", None) => welcome.step(1),
            ("ArrowLeft", None) => welcome.step(-1),
            ("Escape", Some(field)) => {
                let _ = field.blur();
            }
            ("Escape", None) => welcome.finish(),
            _ => return,
        }
        event.prevent_default();
    });
    on_cleanup(move || keys.remove());
    let page = move || welcome.page.get().unwrap_or(Page::Hello);
    let slide = move || {
        if welcome.toward.get() < 0 {
            "welcome-slide from-left"
        } else {
            "welcome-slide from-right"
        }
    };
    view! {
        <div class="welcome" node_ref=root>
            <button class="link quiet welcome-skip" on:click=move |_| welcome.finish()>"Skip setup"</button>
            {move || {
                let page = page();
                let body = match page {
                    Page::Hello => hello().into_any(),
                    Page::Model => view! { <Provider welcome /> }.into_any(),
                    Page::Access => view! { <Access /> }.into_any(),
                    Page::Tools => view! { <Tools /> }.into_any(),
                    Page::Ready => ready(welcome).into_any(),
                };
                view! { <div class=slide()>{body}</div> }
            }}
            <nav class="welcome-nav" aria-label="Welcome pages">
                <div class="welcome-side">
                    <Show when=move || page() != Page::Hello>
                        <button class="link quiet" on:click=move |_| welcome.step(-1)>"Back"</button>
                    </Show>
                </div>
                <div class="welcome-dots">
                    {PAGES.map(|p| view! {
                        <button class="welcome-dot" class:on=move || page() == p
                            aria-label=format!("Page {} of {}", p.index() + 1, PAGES.len())
                            aria-current=move || (page() == p).then_some("step")
                            on:click=move |_| welcome.go(p)></button>
                    })}
                </div>
                <div class="welcome-side end">{move || next(welcome, page())}</div>
            </nav>
        </div>
    }
}

/// Next, or "Skip for now" on the Model page until a provider is connected, or Start at the end.
fn next(welcome: Welcome, page: Page) -> impl IntoView {
    let (label, class) = match page {
        Page::Ready => ("Start", "primary"),
        Page::Model if welcome.connected.with(Option::is_none) => ("Skip for now", "ghost"),
        _ => ("Next", "primary"),
    };
    view! {
        <button class=class on:click=move |_| match page {
            Page::Ready => welcome.finish(),
            _ => welcome.step(1),
        }>{label}</button>
    }
}

fn hello() -> impl IntoView {
    view! {
        <div class="welcome-page center">
            <h1>"duskcode"</h1>
            <p class="muted">"An agent that works in your projects, on the model you choose."</p>
            <p class="muted small">"Three short steps: a model, how much it may do alone, and the tools."</p>
        </div>
    }
}

fn ready(welcome: Welcome) -> impl IntoView {
    let line = move || match welcome.connected.get() {
        Some(_) => "Ask for something in the chat, or open a project first.",
        None => "Connect a model any time from Settings, then ask for something in the chat.",
    };
    view! {
        <div class="welcome-page center">
            <h2>"Ready"</h2>
            <p class="muted">{line}</p>
            <p class="muted small">"Tools and voice stay in Customize."</p>
        </div>
    }
}
