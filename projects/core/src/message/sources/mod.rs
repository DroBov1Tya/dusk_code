//! A chat's sources: long pastes and files kept in the `sources` folder of its workspace.
//! A message carries only a tag naming one. What the model reads comes in a block at the
//! end of each request: a new source until a turn with it has finished, a pinned one
//! always, and the rest by name and path, to read when needed.
//!
//! `store` keeps them (`ThreadSources`), `paths` finds picture paths in pasted text,
//! `picture` decodes and shrinks pictures, and `block` builds what a request carries.

mod block;
mod paths;
mod picture;
mod store;
#[cfg(test)]
mod tests;

pub use block::SourceBlock;
pub(crate) use picture::picture;
pub use store::ThreadSources;
pub(crate) use store::{FOLDER, clean_name};
