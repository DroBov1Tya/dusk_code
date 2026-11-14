//! Tools for working on the window itself, built only with their cargo feature and never
//! shipped: `screenshot` (`--features screenshot`) drives the window from a script and saves
//! pictures of its pages; `plugins` are the made-up plugin parts it photographs, `store`
//! the made-up listings of the Workshop.

mod plugins;
pub mod screenshot;
mod store;
