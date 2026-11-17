//! Running one thread. `run` holds `ThreadRunner`, the loop that takes a thread's messages
//! in and runs its turns, and `ThreadSetup`, what it is built from; `handle` holds what a
//! client keeps of it, the input that feeds it and the output that tells it.

mod handle;
mod resume;
mod run;
mod take_in;
#[cfg(test)]
mod tests;

pub use handle::{ThreadInput, ThreadOutput};
pub use run::{ThreadRunner, ThreadSetup};
