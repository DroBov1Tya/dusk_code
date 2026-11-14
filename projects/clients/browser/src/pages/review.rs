//! A diff as the review shows it: files, their hunks and colored lines.

use dusk_protocol::project::{DiffKind, FileDiff};
use leptos::prelude::*;

#[component]
pub fn DiffView(files: RwSignal<Vec<FileDiff>>) -> impl IntoView {
    view! {
        <div class="diff">
            {move || files.get().into_iter().map(|file| view! {
                <details class="diff-file" open>
                    <summary>
                        <span>{file.path.clone()}</span>
                        <span class="added">{format!("+{}", file.added)}</span>
                        <span class="removed">{format!("-{}", file.removed)}</span>
                    </summary>
                    {if file.binary {
                        view! { <p class="muted">"Binary file."</p> }.into_any()
                    } else {
                        file.hunks.iter().map(|hunk| view! {
                            <div class="hunk">{hunk.header.clone()}</div>
                            <pre class="lines">{hunk.lines.iter().map(|line| {
                                let (class, mark) = match line.kind {
                                    DiffKind::Added => ("line add", '+'),
                                    DiffKind::Removed => ("line del", '-'),
                                    DiffKind::Context => ("line", ' '),
                                };
                                view! { <div class=class>{format!("{mark} {}", line.text)}</div> }
                            }).collect_view()}</pre>
                        }).collect_view().into_any()
                    }}
                </details>
            }).collect_view()}
        </div>
    }
}
