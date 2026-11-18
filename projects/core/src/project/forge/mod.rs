//! A project's pull requests on GitHub, through the GitHub CLI (`gh`) the user installed
//! and signed in to. The core holds no token: `gh` keeps its own. What `gh` prints is
//! untrusted JSON, parsed into fixed shapes, its text made plain and capped.

mod gh;
mod pulls;

pub use pulls::{create, list, view};
