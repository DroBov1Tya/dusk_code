//! Blocks the agent put in the chat with the `show` tool. They are model output: every
//! text is a text node, never markup or a link, and the frame says it came from the
//! agent, so a block cannot pass for one of the app's own controls. Until a kind has a
//! view of its own it is drawn as its plain text.

use dusk_protocol::thread::Block;
use leptos::prelude::*;

#[component]
pub fn ShownBlock(block: Block) -> impl IntoView {
    let kind = match &block {
        Block::Flow(_) => "Flow",
        Block::Checks(_) => "Checks",
        Block::Choices(_) => "Choices",
    };
    view! {
        <div class="card shown" role="group" aria-label=format!("{kind} from the agent")>
            <div class="muted">"From the agent"</div>
            <pre class="tool-out">{block.text()}</pre>
        </div>
    }
}
