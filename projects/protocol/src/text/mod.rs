//! Text as every part of the system handles it the same way: bytes written as hex, text
//! cut to a number of characters.

mod clip;
mod hex;

pub use clip::clip_chars;
pub use hex::{hex, sha256_hex};
