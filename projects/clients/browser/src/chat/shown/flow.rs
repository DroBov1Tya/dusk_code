//! The Flow block: layers stacked top to bottom, cards in a grid inside each, an arrow
//! in the gap between layers. It has no control but the fold of a tall flow.

use dusk_protocol::thread::{Flow, Layer};
use leptos::{html, prelude::*};

/// Layers shown before a tall flow folds the rest.
const FIRST_LAYERS: usize = 4;
/// Height in pixels past which a flow folds.
const TALL: i32 = 640;

#[component]
pub fn FlowView(flow: Flow) -> impl IntoView {
    let layers = flow.layers().to_vec();
    let total = layers.len();
    let root = NodeRef::<html::Div>::new();
    let folded = RwSignal::new(false);
    // Measured once, when the flow is in the page: a flow does not change afterwards.
    Effect::new(move |_| {
        if let Some(root) = root.get()
            && total > FIRST_LAYERS
            && root.scroll_height() > TALL
        {
            folded.set(true);
        }
    });
    let hides = move |at: usize| folded.get() && at >= FIRST_LAYERS;
    view! {
        <div class="shown flow" role="group" aria-label=format!("Flow from the agent, {total} layers") node_ref=root>
            {layers.into_iter().enumerate().map(|(at, layer)| view! {
                {(at > 0).then(|| view! {
                    <div class="flow-arrow" class:folded=move || hides(at) aria-hidden="true">
                        <svg width="16" height="28" viewBox="0 0 16 28">
                            <path d="M8 6v12M4.5 14.5L8 18l3.5-3.5" />
                        </svg>
                    </div>
                })}
                <div class="flow-layer-slot" class:folded=move || hides(at)>
                    <LayerView layer />
                </div>
            }).collect_view()}
            <Show when=move || folded.get()>
                <button class="ghost flow-more" aria-expanded="false" on:click=move |_| folded.set(false)>
                    {format!("Show all {total} layers")}
                </button>
            </Show>
        </div>
    }
}

#[component]
fn LayerView(layer: Layer) -> impl IntoView {
    let title = layer.title.to_string();
    let described = title.clone();
    let cards = layer.cards().to_vec();
    let count = cards.len();
    let emphasis = layer.emphasis;
    view! {
        <div class="flow-layer" class:emphasis=emphasis role="group" aria-label=described>
            {emphasis.then(|| view! { <span class="sr-only">"Emphasised layer. "</span> })}
            <div class="flow-title">{title}</div>
            {layer.subtitle.map(|line| view! { <div class="flow-sub">{line.to_string()}</div> })}
            {(count > 0).then(|| view! {
                <div class=format!("flow-cards n{count}")>
                    {cards.into_iter().map(|card| view! {
                        <div class="flow-card">
                            <b>{card.title.to_string()}</b>
                            {card.line.map(|line| view! { <span>{line.to_string()}</span> })}
                        </div>
                    }).collect_view()}
                </div>
            })}
        </div>
    }
}
