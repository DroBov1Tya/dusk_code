//! Small pieces every page uses: controls, number and time formatting, calls run in the
//! background, and the page's theme.

mod format;
mod task;
pub mod theme;
mod widgets;

pub use format::{compact, size, when};
pub use task::{run, sleep};
pub use widgets::{Row, Section, Switch, confirm};
