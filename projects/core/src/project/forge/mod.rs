//! A project's pull requests on GitHub, through the GitHub CLI (`gh`) the user installed
//! and signed in to. The core holds no token: `gh` keeps its own. What `gh` prints is
//! untrusted JSON, parsed into fixed shapes, its text made plain and capped. The
//! repository status puts them beside the work tree, and the tracker keeps it fresh.

mod gh;
mod lookup;
mod pulls;
#[expect(dead_code, reason = "the forge host routing reads it next")]
mod remote;
mod status;
mod watch;

pub use lookup::{Look, read as read_pull};
pub use pulls::{create, list, view};
#[expect(unused_imports, reason = "the forge host routing reads it next")]
pub use remote::Remote;
pub use status::read as read_status;
pub use watch::{PERIOD as TRACK_PERIOD, Tracker};
