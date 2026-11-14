//! Platforms: programs of plugins that bring the user in from somewhere else, such as a
//! chat bot. The core keeps each running, starts it again if it ends, and gives it a
//! thread of its own, the newest one it had or a new one.
//!
//! `running` keeps each platform's program running, `session` bridges one run of it to its
//! thread, `thread` opens that thread, and `wire` is what the two say to each other.
//!
//! Whoever reaches the platform acts as the user, so the platform decides who that is.

mod running;
mod session;
mod thread;
mod wire;

pub use running::{PlatformSpec, Platforms};
