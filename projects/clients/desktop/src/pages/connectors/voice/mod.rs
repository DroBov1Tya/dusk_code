//! Speech: the servers that turn speech into text and text into speech, set up on the
//! connectors shelf. `page` draws the shelf, `form` holds its boxes, `models` lists the
//! models that run on this machine, and `setup` saves the servers and asks for one when a
//! button needs it.

mod form;
mod models;
mod page;
mod setup;

pub use form::VoiceForm;
pub use page::render;
pub use setup::{Speech, missing};
