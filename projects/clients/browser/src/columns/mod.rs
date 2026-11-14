//! The columns beside the main view. The left one lists projects, chats and pages; the
//! right one belongs to a chat and lives in `chat::inspector`.

mod sidebar;

pub use sidebar::{Sidebar, chat_title};
