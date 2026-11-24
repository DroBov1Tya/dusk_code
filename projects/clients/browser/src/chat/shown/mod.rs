//! Blocks the agent put in the chat with `show` or `draw_diagram`. They are model output:
//! every text is a text node, never markup or a link, and the frame says it came from the
//! agent, so a block cannot pass for one of the app's own controls.

mod checks;
mod choices;
mod diagram;
mod flow;

pub use choices::{ReplyRow, plain, question_labels};

use crate::state::Chat;
use dusk_protocol::thread::Block;
use leptos::prelude::*;

/// One block of the transcript. `id` is its place in the chat, which says whether it is
/// still the last thing there (reply buttons are live only then).
#[component]
pub fn ShownBlock(
    block: Block,
    id: u64,
    chat: Chat,
    thread: Option<String>,
    read_only: bool,
) -> impl IntoView {
    match block {
        Block::Flow(flow) => view! { <flow::FlowView flow /> }.into_any(),
        Block::Checks(checks) => view! { <checks::ChecksView checks /> }.into_any(),
        Block::Choices(choices) => {
            view! { <choices::ChoicesView choices id chat thread read_only /> }.into_any()
        }
        Block::Diagram(diagram) => view! { <diagram::DiagramView diagram /> }.into_any(),
    }
}
