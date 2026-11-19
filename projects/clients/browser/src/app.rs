//! The page: sign-in first, then the workspace.

use crate::{
    auth,
    chat::subagent::SubagentWindow,
    columns::Workspace,
    pages::{
        artifacts::Artifacts,
        connectors::Connectors,
        project::ProjectPage,
        settings::SettingsPage,
        welcome::{self, ChatOrWelcome, Step},
    },
    state::{AppState, View, state},
    ui::{IconDefinitions, theme},
};
use leptos::{prelude::*, task::spawn_local};

#[derive(Clone, PartialEq)]
enum Auth {
    Checking,
    SignedOut(Option<String>),
    SignedIn,
}

#[component]
pub fn App() -> impl IntoView {
    let auth = RwSignal::new(Auth::Checking);
    reload_on_new_address();
    spawn_local(async move {
        let result = match auth::take_fragment_token() {
            Some(token) => auth::sign_in(&token).await.map(|()| true),
            None => Ok(auth::is_signed_in().await),
        };
        auth.set(match result {
            Ok(true) => Auth::SignedIn,
            Ok(false) => Auth::SignedOut(None),
            Err(error) => Auth::SignedOut(Some(error)),
        });
    });
    move || match auth.get() {
        Auth::Checking => view! { <div class="boot">"duskcode"</div> }.into_any(),
        Auth::SignedOut(note) => view! { <SignIn note auth /> }.into_any(),
        Auth::SignedIn => view! { <Shell auth /> }.into_any(),
    }
}

/// A new token in the address bar (the page was not reloaded) starts the page over.
fn reload_on_new_address() {
    let Some(window) = web_sys::window() else {
        return;
    };
    let reload = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(|| {
        if let Some(window) = web_sys::window() {
            let _ = window.location().reload();
        }
    });
    window.set_onhashchange(Some(wasm_bindgen::JsCast::unchecked_ref(reload.as_ref())));
    reload.forget();
}

#[component]
fn SignIn(note: Option<String>, auth: RwSignal<Auth>) -> impl IntoView {
    let token = RwSignal::new(String::new());
    let note = RwSignal::new(note);
    let submit = move || {
        let value = token.get_untracked();
        spawn_local(async move {
            match auth::sign_in(&value).await {
                Ok(()) => auth.set(Auth::SignedIn),
                Err(error) => note.set(Some(error)),
            }
        });
    };
    view! {
        <div class="signin">
            <h1><img class="brand-emblem" src="mark.png" alt="" width="36" height="36" />"duskcode"</h1>
            <p>"Paste the token this run of duskcode printed, or open the address it showed."</p>
            <input
                type="password"
                autocomplete="off"
                placeholder="token"
                prop:value=move || token.get()
                on:input=move |ev| token.set(event_target_value(&ev))
                on:keydown=move |ev| if ev.key() == "Enter" { submit() }
            />
            <button class="primary" on:click=move |_| submit()>"Sign in"</button>
            {move || note.get().map(|text| view! { <p class="error">{text}</p> })}
        </div>
    }
}

#[component]
fn Shell(auth: RwSignal<Auth>) -> impl IntoView {
    let app = AppState::new();
    crate::state::install(app);
    app.connect(move || {
        auth.set(Auth::SignedOut(Some(
            "The session ended. Sign in again.".into(),
        )))
    });
    Effect::new(move |_| theme::apply(&app.theme.get()));
    let welcome = RwSignal::new(Step::Hidden);
    welcome::ask(welcome);
    view! {
        <IconDefinitions />
        <Workspace>
            <main class="main">{move || match state().view.get() {
                View::Chat(id) => view! { <ChatOrWelcome id step=welcome /> }.into_any(),
                View::Settings(page) => view! { <SettingsPage page /> }.into_any(),
                View::Connectors(tab) => view! { <Connectors tab /> }.into_any(),
                View::Artifacts => view! { <Artifacts /> }.into_any(),
                View::Schedules => view! { <crate::pages::schedules::Schedules /> }.into_any(),
                View::Project(id) => view! { <ProjectPage id /> }.into_any(),
            }}</main>
        </Workspace>
        <SubagentWindow />
        <Toast />
    }
}

#[component]
fn Toast() -> impl IntoView {
    let toast = state().toast;
    view! {
        <Show when=move || toast.get().is_some()>
            <div class="toast" role="alert" on:click=move |_| toast.set(None)>
                {move || toast.get().unwrap_or_default()}
            </div>
        </Show>
    }
}
