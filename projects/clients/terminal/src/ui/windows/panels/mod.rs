//! Panels over the transcript, one file each; `frame` picks the one to draw.

mod connection;
mod context;
mod diff;
mod frame;
mod help;
mod jobs;
mod models;
mod paste;
mod plugins;
mod schedules;
mod threads;
mod unsent;
mod update;

pub use frame::{draw, window};
