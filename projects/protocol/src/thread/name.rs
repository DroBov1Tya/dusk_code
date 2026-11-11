//! The name an agent's thread goes by, made from its ID, so every client shows the same one.

/// The arbiter's thread, one for the whole config.
pub const ARBITER_THREAD: &str = "arbiter";

/// The name an agent's thread goes by, made from its ID, so every client shows the same
/// one: a word and two hex digits, such as `Fable-3a`.
pub fn agent_name(thread_id: &str) -> String {
    const NAMES: [&str; 8] = [
        "Aster", "Beryl", "Cinder", "Dapple", "Ember", "Fable", "Glint", "Hollow",
    ];
    let seed = name_seed(thread_id);
    format!(
        "{}-{:02x}",
        NAMES[(seed as usize) % NAMES.len()],
        (seed >> 24) & 0xff
    )
}

pub fn name_seed(thread_id: &str) -> u64 {
    fnv1a(thread_id.as_bytes())
}

/// FNV-1a: stable across runs and builds, unlike the standard hasher. Not a defence
/// against anyone.
pub fn fnv1a(bytes: &[u8]) -> u64 {
    bytes.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    })
}
