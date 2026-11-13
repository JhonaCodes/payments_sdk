//! The HMAC of every call between this API and the Payments router, both
//! ways (`llms.txt` §2 and §7): lowercase hex HMAC-SHA256, keyed with the
//! merchant's `hmac_secret`, over
//! `timestamp.nonce.METHOD.path.slug.sha256_hex(body)`.

use hmac::{Hmac, Mac};
use sha2::{Digest, Sha256};

type HmacSha256 = Hmac<Sha256>;

/// How far a signed timestamp may be from now, in seconds, either way.
pub const SIGNATURE_WINDOW_SECS: i64 = 300;

/// The message that gets signed. `body` is hashed as raw bytes: the bytes
/// received, before any parsing.
pub fn build_message(
    timestamp: &str,
    nonce: &str,
    method: &str,
    path: &str,
    merchant_slug: &str,
    body: &[u8],
) -> String {
    let body_hash = hex::encode(Sha256::digest(body));
    format!("{timestamp}.{nonce}.{method}.{path}.{merchant_slug}.{body_hash}")
}

/// The hex signature of `message` with `secret` (`None` only if the key is
/// unusable, which HMAC never is).
pub fn sign(secret: &str, message: &str) -> Option<String> {
    let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).ok()?;
    mac.update(message.as_bytes());
    Some(hex::encode(mac.finalize().into_bytes()))
}

/// Whether `signature_hex` signs `message` with `secret`, compared in
/// constant time (`Mac::verify_slice`).
pub fn verify(secret: &str, message: &str, signature_hex: &str) -> bool {
    let (Ok(expected), Ok(mut mac)) = (
        hex::decode(signature_hex),
        HmacSha256::new_from_slice(secret.as_bytes()),
    ) else {
        return false;
    };
    mac.update(message.as_bytes());
    mac.verify_slice(&expected).is_ok()
}

/// Whether a signed timestamp is close enough to `now` (Unix seconds).
pub fn within_window(timestamp: i64, now: i64) -> bool {
    (now - timestamp).abs() <= SIGNATURE_WINDOW_SECS
}
