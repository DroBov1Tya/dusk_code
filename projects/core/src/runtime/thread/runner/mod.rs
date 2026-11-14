//! Running one thread. `run` holds `ThreadRunner`, the loop that takes a thread's messages
//! in and runs its turns, and `ThreadSetup`, what it is built from; `handle` holds what a
//! client keeps of it, the input that feeds it and the output that tells it.

mod handle;
mod run;
#[cfg(test)]
mod tests;

pub use handle::{ThreadInput, ThreadOutput};
pub use run::{ThreadRunner, ThreadSetup};
