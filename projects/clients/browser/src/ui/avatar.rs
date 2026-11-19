//! A letter avatar for a connector: the first letter of its name on a tile whose color
//! the name picks (the same hash as the desktop client, so both agree).

use leptos::prelude::*;

/// How many colors the name chooses from; `.avatar.c0` to `.avatar.c7` in `style.css`.
const SHADES: u64 = 8;

fn letter(name: &str) -> String {
    name.chars()
        .find(|c| c.is_alphanumeric())
        .map_or_else(|| "?".to_owned(), |c| c.to_uppercase().collect())
}

/// FNV-1a, which gives the same answer on every run.
fn shade(name: &str) -> u64 {
    let hash = name.bytes().fold(0xcbf2_9ce4_8422_2325_u64, |hash, byte| {
        (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
    });
    hash % SHADES
}

#[component]
pub fn Avatar(#[prop(into)] name: String) -> impl IntoView {
    view! { <span class=format!("avatar c{}", shade(&name)) aria-hidden="true">{letter(&name)}</span> }
}
