//! What keeps threads going: the runtime clients start, the launcher that builds every
//! thread, the runner of each, its lock, and the model all threads talk to.

mod launcher;
pub mod model_choice;
// Named after its primary item, `Runtime`, as the style guide asks.
#[allow(clippy::module_inception)]
mod runtime;
mod thread_lock;
mod thread_runner;

pub use launcher::Launcher;
pub use runtime::{Runtime, ThreadHandle};
pub use thread_lock::ThreadLock;
pub use thread_runner::{ThreadEvent, ThreadInput, ThreadOutput, ThreadRunner, ThreadSetup};
