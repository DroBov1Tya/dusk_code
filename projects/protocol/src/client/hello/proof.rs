//! The proofs of the handshake: HMAC-SHA256 over a fixed label, the claim's id and both
//! nonces. The labels differ, so the core's proof can never be replayed as the client's.

use hmac::{Hmac, KeyInit, Mac};
use sha2::{Digest, Sha256};
use subtle::ConstantTimeEq;

const CORE_LABEL: &str = "duskcode-core\n";
const CLIENT_LABEL: &str = "duskcode-client\n";

/// A fresh nonce: 32 bytes from the OS random source, lowercase hex.
#[cfg(not(target_arch = "wasm32"))]
pub fn try_nonce() -> Result<String, getrandom::Error> {
    let mut bytes = [0u8; 32];
    getrandom::fill(&mut bytes)?;
    Ok(hex(&bytes))
}

/// `try_nonce` for callers that cannot go on without one.
///
/// Panics when the OS gives no random bytes: a predictable nonce would let a recorded
/// handshake be replayed.
#[cfg(not(target_arch = "wasm32"))]
pub fn nonce() -> String {
    match try_nonce() {
        Ok(nonce) => nonce,
        Err(error) => panic!("the system gave no random bytes: {error}"),
    }
}

/// Whether `s` has the shape of a nonce: exactly 64 lowercase hex characters.
///
/// Handshake code checks both nonces with this before computing a proof: the proofs
/// concatenate id and nonces without separators, which is only unambiguous when the
/// nonces have a fixed length.
pub fn nonce_is_valid(s: &str) -> bool {
    s.len() == 64 && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// The core's proof that it holds `k`, sent first.
pub fn core_proof(k: &[u8], id: &str, nc: &str, ns: &str) -> String {
    labelled(CORE_LABEL, k, id, nc, ns)
}

/// The client's proof that it holds `k`, sent only after the core's checked out.
pub fn client_proof(k: &[u8], id: &str, nc: &str, ns: &str) -> String {
    labelled(CLIENT_LABEL, k, id, nc, ns)
}

/// Whether `given` is the `expected` proof, in time that does not depend on where they
/// differ.
pub fn proof_matches(expected: &str, given: &str) -> bool {
    bool::from(expected.as_bytes().ct_eq(given.as_bytes()))
}

/// The key a token's holder proves with: SHA-256 of its secret, which is all the core keeps.
pub fn token_key(secret: &str) -> [u8; 32] {
    Sha256::digest(secret.as_bytes()).into()
}

fn labelled(label: &str, k: &[u8], id: &str, nc: &str, ns: &str) -> String {
    let message = [label, id, nc, ns].concat();
    hex(&hmac_sha256(k, message.as_bytes()))
}

pub(super) fn hmac_sha256(key: &[u8], message: &[u8]) -> [u8; 32] {
    // HMAC takes a key of any length, so this never fails.
    let mut mac =
        <Hmac<Sha256> as KeyInit>::new_from_slice(key).expect("HMAC takes a key of any length");
    mac.update(message);
    mac.finalize().into_bytes().into()
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
