//! Text as every part of the system handles it the same way: bytes written as hex, text
//! cut to a number of characters, and the characters that hide or reorder text.

mod clip;
mod hex;
mod hidden;

pub use clip::clip_chars;
pub use hex::{hex, sha256_hex};
pub use hidden::{is_hidden, is_plain, is_plain_line};
