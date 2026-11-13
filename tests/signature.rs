//! The HMAC of the Payments router (`llms.txt` §2 and §7): the signed message
//! is `timestamp.nonce.METHOD.path.slug.sha256_hex(body)` and the signature is
//! lowercase hex HMAC-SHA256 with the merchant's `hmac_secret`. The expected
//! values were computed with the reference implementation in Python of the
//! guide, not with this crate.

use payments_sdk::signature::{SIGNATURE_WINDOW_SECS, build_message, sign, verify, within_window};

const SECRET: &str = "test-notice-secret-0123456789";
const TIMESTAMP: &str = "1700000000";
const NONCE: &str = "0f3c9a1e";
const SLUG: &str = "turnosqr-dev";
const PATH: &str = "/v1/payments/notice";
const BODY: &[u8] = br#"{"event_id":"7c1e0000-0000-4000-8000-000000000001","type":"payment"}"#;
const EXPECTED_MESSAGE: &str = "1700000000.0f3c9a1e.POST./v1/payments/notice.turnosqr-dev.\
221ed9f9c77699d367aa56501196e83700cca16a70af4eeed51f607667fc5ca4";
const EXPECTED_SIGNATURE: &str = "82c69a9cc4a27084a716087c2d4d4efedb238a5a248530bd6e69a04c66d5a2d9";

#[test]
fn the_message_is_the_six_fields_joined_by_dots() {
    assert_eq!(
        build_message(TIMESTAMP, NONCE, "POST", PATH, SLUG, BODY),
        EXPECTED_MESSAGE
    );
}

#[test]
fn the_signature_matches_the_reference_implementation() {
    assert_eq!(
        sign(SECRET, EXPECTED_MESSAGE).as_deref(),
        Some(EXPECTED_SIGNATURE)
    );
}

#[test]
fn only_the_exact_signature_verifies() {
    assert!(verify(SECRET, EXPECTED_MESSAGE, EXPECTED_SIGNATURE));

    let other_body = build_message(TIMESTAMP, NONCE, "POST", PATH, SLUG, b"{}");
    assert!(!verify(SECRET, &other_body, EXPECTED_SIGNATURE));
    assert!(!verify(
        "another-secret-0123456789",
        EXPECTED_MESSAGE,
        EXPECTED_SIGNATURE
    ));
    assert!(!verify(SECRET, EXPECTED_MESSAGE, "not-hex"));
    assert!(!verify(SECRET, EXPECTED_MESSAGE, ""));
}

#[test]
fn the_window_is_three_hundred_seconds_each_way() {
    let now = 1_700_000_000;
    assert_eq!(SIGNATURE_WINDOW_SECS, 300);
    assert!(within_window(now - 300, now));
    assert!(within_window(now + 300, now));
    assert!(!within_window(now - 301, now));
    assert!(!within_window(now + 301, now));
}
