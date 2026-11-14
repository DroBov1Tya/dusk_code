//! The duskcode web client: the same chats and settings as the desktop app, drawn in a
//! browser from what the core sends over one WebSocket.

mod api;
mod app;
mod chat;
mod link;
mod markdown;
mod pages;
mod session;
mod sidebar;
mod state;
mod theme;
mod ui;
mod voice;

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}
