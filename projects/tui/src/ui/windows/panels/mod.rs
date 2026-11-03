//! Panels over the transcript, one file each; `frame` picks the one to draw.

mod connection;
mod context;
mod frame;
mod help;
mod jobs;
mod models;
mod paste;
mod plugins;
mod threads;
mod unsent;
mod update;

pub use frame::{draw, window};
