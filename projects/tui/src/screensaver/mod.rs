//! The idle screen: after a while with no input and no work (`appearance` in the config,
//! five minutes by default), a random scene plays in a window over the app, a margin of
//! the app still showing around it. Any key or click puts the app back.
//!
//! Scenes are painted from the time alone, dot by dot, so they need no files or network
//! and fit any window: a taller window shows the whole picture finer, a wider one more of
//! its sides.

mod halftone;
mod paint;
mod playing;
mod scenes;

pub use playing::Playing;
