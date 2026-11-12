//! The keys and commands.

use ratatui::text::{Line, Span};

use crate::ui::windows::theme::{TEXT_SOFT, fg};

pub const KEYS: [(&str, &str); 40] = [
    (
        "Enter",
        "send; while the agent works it waits above the box",
    ),
    ("Alt+Enter", "send now, interrupting the agent"),
    ("Esc", "stop the agent; queued messages return to the box"),
    ("Shift+Enter", "new line; Ctrl+J or \\ then Enter too"),
    ("Ctrl+E, click", "edit a pasted text in its own window"),
    (
        "Shift+arrows",
        "select typed text; Ctrl+A all, drag works too",
    ),
    ("Ctrl+C/X", "copy or cut the selected typed text"),
    ("Up / Down", "earlier messages you sent"),
    ("Tab", "switch the mode: main, plan, orchestrator"),
    ("Shift+Tab", "walk thinking, tools and summaries"),
    ("Enter / Space", "fold or unfold the selected one"),
    ("Left click", "fold or unfold thinking, tools and summaries"),
    ("Drag", "select text and copy it; Esc clears"),
    ("Hover", "copy a message, edit one you sent"),
    ("Ctrl+R", "fold or unfold all thinking"),
    ("Ctrl+B", "send the running command to the background"),
    ("PgUp / PgDn", "scroll the transcript; wheel works too"),
    ("Shift+PgUp/Dn", "scroll subagent cards"),
    ("Alt+PgUp/Dn", "scroll MCP tools"),
    (
        "Ctrl+T /resume /sessions",
        "threads, with a preview of each",
    ),
    (
        "/model",
        "choose the model; /model provider/name uses one the server does not list",
    ),
    ("/connect", "add a provider URL and optional API key"),
    (
        "/schedules",
        "the scheduled tasks: run now, switch on or off, remove",
    ),
    (
        "/schedule",
        "a task in this folder's project: /schedule daily 09:00 <prompt>",
    ),
    ("/settings", "concurrent agent limit, tools and skills"),
    (
        "/resend",
        "view saved local messages; Enter explicitly sends one",
    ),
    ("/plan", "read and edit this chat's pinned plan"),
    ("/files", "the files this chat's commands changed"),
    ("/diff", "review the changes in this chat's folder"),
    (
        "/disconnect",
        "forget a provider and its key: /disconnect name",
    ),
    (
        "/me",
        "what @me says about you; its files are set in the GUI",
    ),
    ("/snippet", "write a snippet for @name: /snippet name"),
    (
        "/access",
        "this chat's access: ask, sandbox, arbiter, full, default",
    ),
    ("Ctrl+N", "new thread"),
    ("Ctrl+O", "settings categories"),
    (
        "F3, card click",
        "open a subagent: its chat, actions and numbers",
    ),
    ("F2", "show or hide the statistics sidebar"),
    ("/compact", "summarize the context to free it"),
    ("/context", "what the context holds, part by part"),
    ("Ctrl+C", "clear the box, or copy a selection; twice quits"),
];

pub fn help() -> Vec<Line<'static>> {
    KEYS.iter()
        .map(|(keys, what)| {
            Line::from(vec![
                Span::styled(format!(" {keys:>14}  "), fg(TEXT_SOFT)),
                Span::raw(*what),
            ])
        })
        .collect()
}
