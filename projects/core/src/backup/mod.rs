//! Moving duskcode between machines and taking it off one: `/migrate` archives, their
//! offline restore with its startup guard, and the guard of `duskcode remove`.

pub mod archive;
mod uninstall;

pub use uninstall::Uninstall;
