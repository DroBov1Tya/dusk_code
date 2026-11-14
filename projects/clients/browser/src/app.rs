//! The page: sign-in first, then the sidebar, the main view and the inspector.

use crate::{
    chat::{Chat, inspector::Inspector, subagent::SubagentWindow},
    pages::{
        artifacts::Artifacts, connectors::Connectors, project::ProjectPage, settings::SettingsPage,
    },
    session,
    sidebar::Sidebar,
    state::{AppState, View, state},
    theme,
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
        let result = match session::take_fragment_token() {
            Some(token) => session::sign_in(&token).await.map(|()| true),
            None => Ok(session::is_signed_in().await),
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
            match session::sign_in(&value).await {
                Ok(()) => auth.set(Auth::SignedIn),
                Err(error) => note.set(Some(error)),
            }
        });
    };
    view! {
        <div class="signin">
            <h1>"duskcode"</h1>
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
    view! {
        <div class="app">
            <Sidebar />
            <main class="main">{move || match state().view.get() {
                View::Chat(id) => view! { <Chat id /> }.into_any(),
                View::Settings(page) => view! { <SettingsPage page /> }.into_any(),
                View::Connectors(tab) => view! { <Connectors tab /> }.into_any(),
                View::Artifacts => view! { <Artifacts /> }.into_any(),
                View::Schedules => view! { <crate::pages::schedules::Schedules /> }.into_any(),
                View::Project(id) => view! { <ProjectPage id /> }.into_any(),
            }}</main>
            <Inspector />
            <SubagentWindow />
            <Toast />
        </div>
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
