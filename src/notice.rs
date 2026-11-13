//! The notices the router posts to the merchant's `notify_url`
//! (`llms.txt` §7 and §8), and their verification.
//!
//! A notice is a trigger, never a source of truth: verify it with
//! [`verify_notice`], record it once per `event_id`, then re-read the intent
//! or the subscription with the client before acting.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::model::{IntentStatus, Money, Subscription};
use crate::signature::{build_message, verify, within_window};

pub const MERCHANT_HEADER: &str = "X-Payments-Merchant";
pub const TIMESTAMP_HEADER: &str = "X-Payments-Timestamp";
pub const NONCE_HEADER: &str = "X-Payments-Nonce";
pub const SIGNATURE_HEADER: &str = "X-Payments-Signature";

/// The four headers of a signed notice, as received (empty when absent).
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignedHeaders {
    pub merchant: String,
    pub timestamp: String,
    pub nonce: String,
    pub signature: String,
}

impl SignedHeaders {
    /// The headers read with `header` (for example over an HTTP request's
    /// header map); a missing header is empty.
    pub fn read(header: impl Fn(&str) -> Option<String>) -> Self {
        let value = |name: &str| header(name).unwrap_or_default();
        Self {
            merchant: value(MERCHANT_HEADER),
            timestamp: value(TIMESTAMP_HEADER),
            nonce: value(NONCE_HEADER),
            signature: value(SIGNATURE_HEADER),
        }
    }
}

/// Why a notice is not the router's: answer `401` (refused for good).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum NoticeRejection {
    /// Signed as another merchant.
    OtherMerchant,
    /// The timestamp is missing or more than 300 s away.
    Stale,
    /// The signature does not match these exact bytes.
    BadSignature,
}

/// Whether `body` (the raw bytes received) was signed by the router for the
/// merchant `merchant_slug` with `secret`, posted to `path` (the path of the
/// merchant's `notify_url`), at most 300 s from `now`.
pub fn verify_notice(
    headers: &SignedHeaders,
    body: &[u8],
    path: &str,
    merchant_slug: &str,
    secret: &str,
    now: DateTime<Utc>,
) -> Result<(), NoticeRejection> {
    if headers.merchant != merchant_slug {
        return Err(NoticeRejection::OtherMerchant);
    }
    let fresh = headers
        .timestamp
        .parse::<i64>()
        .is_ok_and(|timestamp| within_window(timestamp, now.timestamp()));
    if !fresh {
        return Err(NoticeRejection::Stale);
    }
    let message = build_message(
        &headers.timestamp,
        &headers.nonce,
        "POST",
        path,
        &headers.merchant,
        body,
    );
    if !verify(secret, &message, &headers.signature) {
        return Err(NoticeRejection::BadSignature);
    }
    Ok(())
}

/// The kinds of notice the router sends.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NoticeType {
    /// A one-off charge, or a subscription the provider bills with its own
    /// plan (`provider_plan`): then `provider_transaction_id` is `sub_…`.
    Payment,
    /// A charge of a router subscription (ePayco).
    SubscriptionCharge,
    /// A charge of a router subscription is coming (ePayco).
    SubscriptionChargeUpcoming,
    /// The customer cancelled a router subscription (ePayco).
    SubscriptionCancelled,
}

impl NoticeType {
    /// The kind named by a notice's `type`; `None` for one this version does
    /// not know (record it and answer `2xx`, never refuse it).
    pub fn parse(value: &str) -> Option<Self> {
        match value {
            "payment" => Some(Self::Payment),
            "subscription_charge" => Some(Self::SubscriptionCharge),
            "subscription_charge_upcoming" => Some(Self::SubscriptionChargeUpcoming),
            "subscription_cancelled" => Some(Self::SubscriptionCancelled),
            _ => None,
        }
    }
}

/// What every notice carries, whatever its type.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct NoticeEnvelope {
    pub event_id: Uuid,
    /// Kept as text: a type this version does not know is still a notice.
    #[serde(rename = "type")]
    pub notice_type: String,
    #[serde(default)]
    pub intent_id: Option<Uuid>,
    #[serde(default)]
    pub subscription_id: Option<Uuid>,
    #[serde(default)]
    pub status: Option<String>,
}

/// A `payment` or `subscription_charge` notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PaymentNotice {
    pub event_id: Uuid,
    #[serde(rename = "type")]
    pub notice_type: NoticeType,
    pub intent_id: Uuid,
    pub external_order_id: String,
    pub external_product_ref: String,
    pub external_user_ref: String,
    pub status: IntentStatus,
    pub amount: Money,
    #[serde(default)]
    pub provider_reference: Option<String>,
    #[serde(default)]
    pub provider_transaction_id: Option<String>,
    #[serde(default)]
    pub concept: Option<String>,
    /// For a `subscription_charge`, the subscription after that charge.
    #[serde(default)]
    pub subscription: Option<Subscription>,
}

/// A `subscription_charge_upcoming` or `subscription_cancelled` notice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubscriptionEventNotice {
    pub event_id: Uuid,
    #[serde(rename = "type")]
    pub notice_type: NoticeType,
    pub subscription_id: Uuid,
    pub external_user_ref: String,
    /// `subscription_charge_upcoming`: when the charge happens.
    #[serde(default)]
    pub charge_at: Option<DateTime<Utc>>,
    /// `subscription_charge_upcoming`: what will be charged.
    #[serde(default)]
    pub amount: Option<Money>,
    /// `subscription_charge_upcoming`: the free trial ends with this charge.
    #[serde(default)]
    pub trial_ending: Option<bool>,
    /// `subscription_cancelled`: who cancelled (`customer`).
    #[serde(default)]
    pub cancelled_by: Option<String>,
    pub subscription: Subscription,
}
