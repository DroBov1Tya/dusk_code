//! Text as every part of the system handles it the same way: bytes written as hex.

mod hex;

pub use hex::{hex, sha256_hex};
