//! The idle screen: after a while with no input and no work (`appearance` in the config,
//! five minutes by default), a random scene plays in a window over the app, a margin of
//! the app still showing around it. Any key or click puts the app back.
//!
//! Scenes are painted from the time alone, pixel by pixel, so they need no files or
//! network and fit any window: a taller window shows the whole picture finer, a wider one
//! more of its sides. Each scene is drawn in the look that suits it (`Scene::look`).

mod blocks;
mod cell;
mod paint;
mod playing;
mod scenes;
mod texture;

pub use playing::Playing;
