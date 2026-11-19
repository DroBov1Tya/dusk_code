//! Small pieces every page uses: controls, number and time formatting, calls run in the
//! background, and the page's theme.

mod avatar;
mod format;
mod icons;
mod task;
pub mod theme;
mod widgets;

pub use avatar::Avatar;
pub use format::{compact, size, when};
pub use icons::{Icon, IconDefinitions, IconName};
pub use task::{run, sleep};
pub use widgets::{Row, Section, Switch, confirm};
