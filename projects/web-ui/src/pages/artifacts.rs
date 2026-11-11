//! The files the agents made: pictures, documents and text, with a preview where there is one.

use crate::{api, sidebar::confirm, ui::{run, size, when}};
use dusk_protocol::project::{OutputFile, OutputKind};
use leptos::prelude::*;

#[component]
pub fn Artifacts() -> impl IntoView {
    let files = RwSignal::new(Vec::<OutputFile>::new());
    let shown = RwSignal::new(None::<(String, String)>);
    let load = move || run(api::outputs(), move |list| files.set(list));
    load();
    view! {
        <div class="page">
            <h2>"Artifacts"</h2>
            <p class="muted">"Files the agents made in your workspace."</p>
            <div class="gallery">
                {move || files.get().into_iter().map(|file| {
                    let path = file.path.display().to_string();
                    let name = path.rsplit('/').next().unwrap_or(&path).to_owned();
                    let (open_path, delete_path) = (path.clone(), path.clone());
                    let image = file.kind == OutputKind::Image;
                    let text = file.kind == OutputKind::Text;
                    view! {
                        <div class="tile">
                            {image.then(|| view! { <img loading="lazy" src=format!("/api/output?path={}", encode(&path)) alt=name.clone() /> })}
                            <div class="tile-name" title=path.clone()>{name}</div>
                            <div class="muted">{format!("{} · {}", size(file.bytes), when(file.modified))}</div>
                            <div class="row-actions">
                                {text.then(|| view! {
                                    <button class="ghost" on:click=move |_| {
                                        let path = open_path.clone();
                                        run(api::read_output(path.clone()), move |body| shown.set(Some((path, body))));
                                    }>"View"</button>
                                })}
                                <button class="ghost danger" on:click=move |_| {
                                    if confirm("Delete this file from the workspace?") {
                                        run(api::delete_output(delete_path.clone()), move |()| load());
                                    }
                                }>"Delete"</button>
                            </div>
                        </div>
                    }
                }).collect_view()}
            </div>
            {move || shown.get().map(|(path, body)| view! {
                <div class="overlay" on:click=move |_| shown.set(None)>
                    <div class="window" on:click=|ev| ev.stop_propagation()>
                        <header class="window-head"><h3>{path}</h3><span class="grow"></span>
                            <button class="icon" on:click=move |_| shown.set(None)>"×"</button></header>
                        <pre class="viewer">{body}</pre>
                    </div>
                </div>
            })}
        </div>
    }
}

/// A path as a query value: everything but unreserved characters is percent-coded.
pub fn encode(text: &str) -> String {
    text.bytes()
        .map(|b| match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => (b as char).to_string(),
            _ => format!("%{b:02X}"),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::encode;

    #[test]
    fn a_path_is_percent_coded() {
        assert_eq!(encode("/a b/é.png"), "%2Fa%20b%2F%C3%A9.png");
    }
}
