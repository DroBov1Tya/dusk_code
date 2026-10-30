//! The database file: opening it, its migrations, one per table, and bringing databases
//! made by older versions up to them.

mod legacy;
// Named after its primary item, `State`, as the style guide asks.
#[allow(clippy::module_inception)]
mod state;

pub use state::{State, timestamp_ms};
