//! Panels over the transcript, one file each; `frame` picks the one to draw.

mod connection;
mod context;
mod diff;
mod frame;
mod help;
mod jobs;
mod models;
pub mod overview;
mod paste;
mod plugins;
mod schedules;
mod source;
mod threads;
mod unsent;
mod update;
mod workshop;

pub use frame::{draw, fit, window};
