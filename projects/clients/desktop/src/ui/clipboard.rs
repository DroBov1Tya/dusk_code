//! Copy with feedback: the button that was pressed shows a check mark for a moment.

use std::time::{Duration, Instant};

use gpui_kit::{component::IconName, *};

/// How long the check mark stays.
const SHOWN: Duration = Duration::from_millis(1500);

/// The button that copied last, and when.
#[derive(Default)]
struct Copied(Option<(Instant, SharedString)>);

impl Global for Copied {}

/// Puts `text` on the clipboard and marks the button `id` as copied for a moment.
pub fn copy(id: &SharedString, text: String, cx: &mut App) {
    cx.write_to_clipboard(ClipboardItem::new_string(text));
    let since = Instant::now();
    cx.default_global::<Copied>().0 = Some((since, id.clone()));
    cx.refresh_windows();
    cx.spawn(async move |cx| {
        cx.background_executor().timer(SHOWN).await;
        cx.update(|cx| {
            let copied = cx.default_global::<Copied>();
            // A newer copy has its own timer.
            if copied.0.as_ref().is_some_and(|(at, _)| *at == since) {
                copied.0 = None;
                cx.refresh_windows();
            }
        });
    })
    .detach();
}

/// The icon of a copy button: a check mark while it shows it has copied.
pub fn icon(id: &SharedString, cx: &App) -> IconName {
    match cx
        .try_global::<Copied>()
        .and_then(|copied| copied.0.as_ref())
    {
        Some((_, shown)) if shown == id => IconName::Check,
        _ => IconName::Copy,
    }
}
