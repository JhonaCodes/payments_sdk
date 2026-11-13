//! The types of the router's contract: money, intents, subscriptions, plans.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use uuid::Uuid;

/// An amount in minor units of an ISO 4217 currency (`USD 12.99` =
/// `{minor: 1299, currency: "USD"}`).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Money {
    pub minor: i64,
    pub currency: String,
}

/// The states of an intent. `Paid` is final.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IntentStatus {
    Created,
    CheckoutReady,
    PendingConfirmation,
    Paid,
    Rejected,
    Expired,
    ManualReview,
}

/// `POST /v1/intents`. Exactly one of `price_ref`, `provider_plan` or
/// `amount` (+ `concept`) prices the charge.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateIntent {
    pub external_order_id: String,
    pub external_product_ref: String,
    pub external_user_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub price_ref: Option<String>,
    /// The merchant's own recurring plan at the provider (a Stripe Price);
    /// requires `provider`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider_plan: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub provider: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub amount: Option<Money>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub concept: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub metadata: Option<Value>,
    /// Must start with the merchant's `return_url_prefix`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_url: Option<String>,
}

/// An intent as the router has it (`POST /v1/intents`, `GET /v1/intents/{id}`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Intent {
    pub intent_id: Uuid,
    pub status: IntentStatus,
    #[serde(default)]
    pub amount: Option<Money>,
    /// Where the customer pays (the provider's checkout, or the router's
    /// choice page).
    #[serde(default)]
    pub checkout_url: Option<String>,
    #[serde(default)]
    pub concept: Option<String>,
    #[serde(default)]
    pub provider: Option<String>,
    #[serde(default)]
    pub provider_reference: Option<String>,
    /// For a `provider_plan` charge, the provider's subscription (`sub_…`).
    #[serde(default)]
    pub provider_transaction_id: Option<String>,
    #[serde(default)]
    pub expires_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
}

/// `POST /v1/subscriptions`: a subscription the router charges itself
/// (ePayco) on one of the merchant's plans.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct CreateSubscription {
    pub external_plan_ref: String,
    pub external_user_ref: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub return_url: Option<String>,
}

/// The states of a router subscription.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubscriptionStatus {
    /// No card yet, or the first charge is not approved yet.
    Incomplete,
    /// Up to date; covers until `current_period_end`.
    Active,
    /// A renewal failed; a retry is set at `next_charge_at`.
    PastDue,
    /// No more charges; paid until `current_period_end`.
    Cancelled,
}

/// A router subscription. In a notice the two links are left out.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Subscription {
    pub subscription_id: Uuid,
    pub status: SubscriptionStatus,
    #[serde(default)]
    pub cancel_reason: Option<String>,
    pub external_user_ref: String,
    pub external_plan_ref: String,
    #[serde(default)]
    pub plan_name: Option<String>,
    pub amount: Money,
    pub interval_unit: String,
    pub interval_count: i32,
    #[serde(default)]
    pub current_period_start: Option<DateTime<Utc>>,
    #[serde(default)]
    pub current_period_end: Option<DateTime<Utc>>,
    #[serde(default)]
    pub next_charge_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub failed_charges: i32,
    #[serde(default)]
    pub has_payment_method: bool,
    /// Where the customer registers the card (valid 24 h).
    #[serde(default)]
    pub subscribe_url: Option<String>,
    /// Where the customer sees and cancels the subscription.
    #[serde(default)]
    pub manage_url: Option<String>,
    #[serde(default)]
    pub cancelled_at: Option<DateTime<Utc>>,
}

/// A plan of the merchant at the router (`GET /v1/plans`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Plan {
    pub external_plan_ref: String,
    pub name: String,
    pub amount: Money,
    pub interval_unit: String,
    pub interval_count: i32,
    #[serde(default)]
    pub trial_days: i32,
    pub active: bool,
}
