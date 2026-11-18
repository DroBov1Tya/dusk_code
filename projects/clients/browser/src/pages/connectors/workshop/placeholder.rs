//! A cover's frame when there is no picture to put in it: the kind's glyph on the raised
//! surface, quieter than any real cover. Drawn here as inline SVG, so it needs no file
//! and no network. One frame stands for a listing without a cover, a cover that failed,
//! one still coming (a shimmer, no glyph) and one that could not be fetched offline.

use dusk_protocol::plugin::Kind;
use leptos::prelude::*;

/// What a cover's frame holds.
#[derive(Clone, PartialEq, Eq)]
pub enum Picture {
    /// Asked for; the answer has not come.
    Loading,
    /// A `data:` address built by the API module from bytes the core checked.
    Shown(String),
    /// No cover, or one that failed; the reason is not the card's to show.
    Missing,
    /// It failed while this browser had no network.
    Offline,
}

/// The frame for every state but `Shown`.
pub fn placeholder(kind: Kind, picture: &Picture) -> AnyView {
    match picture {
        Picture::Loading => {
            view! { <div class="ph loading" role="img" aria-label="Cover loading"></div> }
                .into_any()
        }
        Picture::Offline => view! {
            <div class="ph" role="img" aria-label="No cover (offline)">
                {glyph(kind_paths(kind))}
                <span class="off" aria-hidden="true">{glyph(CLOUD_OFF)}"Offline"</span>
            </div>
        }
        .into_any(),
        Picture::Missing | Picture::Shown(_) => {
            view! { <div class="ph" role="img" aria-label="No cover">{glyph(kind_paths(kind))}</div> }
                .into_any()
        }
    }
}

fn glyph(paths: &'static [&'static str]) -> impl IntoView {
    view! {
        <svg viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2"
            stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false">
            {paths.iter().map(|d| view! { <path d=*d /> }).collect_view()}
        </svg>
    }
}

fn kind_paths(kind: Kind) -> &'static [&'static str] {
    match kind {
        Kind::Plugin => PUZZLE,
        Kind::Pack => PACKAGE,
        Kind::Skill => BOOK,
        Kind::Agent => BOT,
    }
}

// Lucide's glyphs (ISC), the set the app's other icons come from.
const PUZZLE: &[&str] = &[
    "M19.439 7.85c-.049.322.059.648.289.878l1.568 1.568c.47.47.706 1.087.706 1.704s-.235 \
     1.233-.706 1.704l-1.611 1.611a.98.98 0 0 1-.837.276c-.47-.07-.802-.48-.968-.925a2.501 \
     2.501 0 1 0-3.214 3.214c.446.166.855.497.925.968a.979.979 0 0 1-.276.837l-1.61 \
     1.61a2.404 2.404 0 0 1-1.705.707 2.402 2.402 0 0 1-1.704-.706l-1.568-1.568a1.026 1.026 \
     0 0 0-.877-.29c-.493.074-.84.504-1.02.968a2.5 2.5 0 1 1-3.237-3.237c.464-.18.894-.527.967-1.02a1.026 \
     1.026 0 0 0-.289-.877l-1.568-1.568A2.402 2.402 0 0 1 1.998 12c0-.617.236-1.234.706-1.704L4.23 \
     8.77c.24-.24.581-.353.917-.303.515.077.877.528 1.073 1.01a2.5 2.5 0 1 0 \
     3.259-3.259c-.482-.196-.933-.558-1.01-1.073-.05-.336.062-.676.303-.917l1.525-1.525A2.402 \
     2.402 0 0 1 12 1.998c.617 0 1.234.236 1.704.706l1.568 1.568c.23.23.556.338.877.29.493-.074.84-.504 \
     1.02-.968a2.5 2.5 0 1 1 3.237 3.237c-.464.18-.894.527-.967 1.02Z",
];
const PACKAGE: &[&str] = &[
    "M11 21.73a2 2 0 0 0 2 0l7-4A2 2 0 0 0 21 16V8a2 2 0 0 0-1-1.73l-7-4a2 2 0 0 0-2 0l-7 4A2 2 \
     0 0 0 3 8v8a2 2 0 0 0 1 1.73z",
    "M12 22V12",
    "m3.3 7 7.703 4.734a2 2 0 0 0 1.994 0L20.7 7",
    "m7.5 4.27 9 5.15",
];
const BOOK: &[&str] = &[
    "M12 7v14",
    "M3 18a1 1 0 0 1-1-1V4a1 1 0 0 1 1-1h5a4 4 0 0 1 4 4 4 4 0 0 1 4-4h5a1 1 0 0 1 1 1v13a1 1 \
     0 0 1-1 1h-6a3 3 0 0 0-3 3 3 3 0 0 0-3-3z",
];
const BOT: &[&str] = &[
    "M12 8V4H8",
    "M6 8h12a2 2 0 0 1 2 2v8a2 2 0 0 1-2 2H6a2 2 0 0 1-2-2v-8a2 2 0 0 1 2-2z",
    "M2 14h2",
    "M20 14h2",
    "M15 13v2",
    "M9 13v2",
];
const CLOUD_OFF: &[&str] = &[
    "m2 2 20 20",
    "M5.782 5.782A7 7 0 0 0 9 19h8.5a4.5 4.5 0 0 0 1.307-.193",
    "M21.532 16.5A4.5 4.5 0 0 0 17.5 10h-1.79A7.008 7.008 0 0 0 10 5.07",
];
