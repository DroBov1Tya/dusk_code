//! Tools for working on the window itself, built only with their cargo feature and never
//! shipped: `screenshot` (`--features screenshot`) drives the window from a script and saves
//! pictures of its pages; `plugins` are the made-up plugin parts it photographs, `store`
//! the made-up listings of the Workshop, `diagrams` a chat of diagrams, `team` a made-up team on the shown chat and `team_sizes` teams of other sizes.

mod blocks;
mod diagrams;
mod plugins;
mod polish;
pub mod screenshot;
mod store;
mod team;
mod team_sizes;
