//! A link in a chat to a pull request of the shown project's own GitHub repository is
//! drawn as a chip, `owner/repo#n · title`, with a card on hover saying who opened it,
//! how much it changes and where its checks stand, as the desktop app has it. The core
//! gives the data (`ProjectRequest::PullStatus`); nothing is fetched here. Until it
//! answers, and for any other link, the link stays as drawn. Every word goes in as a
//! text node: the title and the author are GitHub's text, not markup.

use crate::{api, state::state};
use dusk_protocol::project::{PullState, PullStatus, pull_number};
use leptos::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, Element};

/// Longer titles are cut in the chip; the card has the whole.
const CHIP_TITLE_CHARS: usize = 48;

/// Finds the links under `root` to the shown project's own pull requests and asks the
/// core for each.
pub fn claim(root: &Element) {
    let app = state();
    let Some(project) = app.project.get_untracked() else {
        return;
    };
    let known = app.repository.with_untracked(|repository| {
        repository
            .as_ref()
            .filter(|(of, _)| *of == project)
            .map(|(_, status)| status.github.clone())
    });
    let root = root.clone();
    leptos::task::spawn_local(async move {
        // The repository is read once for the chat when no page asked for it yet.
        let github = match known {
            Some(github) => github,
            None => match api::repository_status(&project).await {
                Ok(status) => {
                    let github = status.github.clone();
                    app.repository.set(Some((project.clone(), status)));
                    github
                }
                Err(_) => None,
            },
        };
        if let Some(github) = github {
            claim_links(&root, &project, &github);
        }
    });
}

fn claim_links(root: &Element, project: &str, github: &str) {
    let Ok(links) = root.query_selector_all("a[href]") else {
        return;
    };
    let prefix = format!("https://github.com/{github}/pull/");
    for index in 0..links.length() {
        let Some(link) = links.item(index).and_then(|n| n.dyn_into::<Element>().ok()) else {
            continue;
        };
        let href = link.get_attribute("href").unwrap_or_default();
        let Some(number) = pull_number(&href, &prefix) else {
            continue;
        };
        let (project, github) = (project.to_owned(), github.to_owned());
        leptos::task::spawn_local(async move {
            // A link the core cannot tell about stays a plain link.
            if let Ok(Some(pull)) = api::pull_status(&project, number).await {
                chip(&link, &github, &pull);
            }
        });
    }
}

fn chip(link: &Element, github: &str, pull: &PullStatus) {
    let Some(document) = link.owner_document() else {
        return;
    };
    let mut title: String = pull.title.chars().take(CHIP_TITLE_CHARS).collect();
    if pull.title.chars().count() > CHIP_TITLE_CHARS {
        title.push('\u{2026}');
    }
    link.set_text_content(None);
    let _ = link.class_list().add_2("pr-chip", state_class(pull.state));
    append(&document, link, "span", "pr-dot", "");
    append(
        &document,
        link,
        "span",
        "",
        &format!("{github}#{} \u{00B7} {title}", pull.number),
    );
    let card = append(&document, link, "span", "pr-card", "");
    append(&document, &card, "b", "", &pull.title);
    let mut who = state_word(pull.state).to_owned();
    if !pull.author.is_empty() {
        who.push_str(&format!(" \u{00B7} by {}", pull.author));
    }
    if let Some(created) = pull.created {
        who.push_str(&format!(" \u{00B7} {}", crate::ui::when(created)));
    }
    append(&document, &card, "span", "muted", &who);
    let mut size = format!("+{} \u{2212}{}", pull.added, pull.removed);
    if let Some(files) = pull.files {
        size.push_str(&format!(" in {files} files"));
    }
    append(&document, &card, "span", "muted", &size);
    let checks = &pull.checks;
    if checks.total > 0 {
        let line = format!("Checks {}/{} passed", checks.passed, checks.total);
        append(&document, &card, "span", "muted", &line);
    }
}

/// A child element with `text` as a text node.
fn append(document: &Document, parent: &Element, tag: &str, class: &str, text: &str) -> Element {
    let Ok(element) = document.create_element(tag) else {
        return parent.clone();
    };
    if !class.is_empty() {
        element.set_class_name(class);
    }
    if !text.is_empty() {
        element.set_text_content(Some(text));
    }
    let _ = parent.append_child(&element);
    element
}

fn state_class(state: PullState) -> &'static str {
    match state {
        PullState::Open => "open",
        PullState::Draft => "draft",
        PullState::Merged => "merged",
        PullState::Closed => "closed",
    }
}

fn state_word(state: PullState) -> &'static str {
    match state {
        PullState::Open => "Open",
        PullState::Draft => "Draft",
        PullState::Merged => "Merged",
        PullState::Closed => "Closed",
    }
}
