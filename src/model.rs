//! The types of the router's contract: money, intents, subscriptions, plans.

use std::fmt;

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

/// The states of a subscription the provider manages itself (a Stripe
/// subscription opened with `provider_plan`), as the provider reports them.
/// A state this version does not know reads as [`Self::Unknown`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProviderSubscriptionStatus {
    Trialing,
    Active,
    PastDue,
    Unpaid,
    Canceled,
    Incomplete,
    IncompleteExpired,
    Paused,
    #[serde(other)]
    Unknown,
}

impl ProviderSubscriptionStatus {
    /// Paid up (or in its free trial): the plan applies.
    pub fn is_active(&self) -> bool {
        matches!(self, Self::Trialing | Self::Active)
    }

    /// Over for good: the provider charges it no more.
    pub fn has_ended(&self) -> bool {
        matches!(self, Self::Canceled | Self::IncompleteExpired)
    }
}

/// The router's mirror of a subscription the provider manages: the state
/// the provider reports, kept by the router and sent in every
/// `provider_subscription` notice. `version` grows with every change: a
/// snapshot with a `version` at or below the one applied is older.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderSubscription {
    /// The router's id of the mirror.
    pub id: Uuid,
    pub provider: String,
    /// The provider's id (`sub_…`).
    pub provider_subscription_id: String,
    /// The provider's customer (`cus_…`).
    #[serde(default)]
    pub provider_customer_id: Option<String>,
    pub external_user_ref: String,
    #[serde(default)]
    pub external_order_id: Option<String>,
    #[serde(default)]
    pub external_product_ref: Option<String>,
    /// The intent whose checkout opened it.
    #[serde(default)]
    pub origin_intent_id: Option<Uuid>,
    /// The price of the base item (not the metered one).
    #[serde(default)]
    pub price_ref: Option<String>,
    /// The metered item added with [`crate::ProviderSubscriptions::add_item`].
    #[serde(default)]
    pub metered_item_id: Option<String>,
    pub status: ProviderSubscriptionStatus,
    #[serde(default)]
    pub current_period_start: Option<DateTime<Utc>>,
    #[serde(default)]
    pub current_period_end: Option<DateTime<Utc>>,
    /// Ends at `current_period_end` instead of renewing.
    #[serde(default)]
    pub cancel_at_period_end: bool,
    #[serde(default)]
    pub canceled_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub ended_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub trial_end: Option<DateTime<Utc>>,
    pub version: i64,
    #[serde(default)]
    pub created_at: Option<DateTime<Utc>>,
    #[serde(default)]
    pub updated_at: Option<DateTime<Utc>>,
}

/// `POST /v1/provider-subscriptions/adopt`: registers the mirror of a
/// subscription that already exists on the merchant's own provider account
/// (one opened before the router). The router writes nothing to the
/// provider. Adopting it again with the same `external_user_ref` answers the
/// same mirror; with another one, a `409`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdoptProviderSubscription {
    /// The provider (`"stripe"`).
    pub provider: String,
    /// The provider's id (`sub_…`).
    pub provider_subscription_id: String,
    pub external_user_ref: String,
    pub external_order_id: String,
    pub external_product_ref: String,
}

/// How a price change is billed at the provider.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Proration {
    /// Prorated on the next invoice.
    CreateProrations,
    /// Prorated and invoiced now.
    AlwaysInvoice,
    /// The new price from the next period, nothing prorated.
    None,
}

/// A metered usage the provider bills: `identifier` is the provider's
/// idempotency key, so the same usage sent twice counts once.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageEvent {
    pub meter_event: String,
    /// Never negative: the router refuses a negative usage.
    pub value: u64,
    pub identifier: String,
}

/// What the provider took of a batch of [`UsageEvent`]s.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageReport {
    pub accepted: u32,
    #[serde(default)]
    pub failed: Vec<UsageFailure>,
}

/// A usage the provider refused, by its `identifier`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct UsageFailure {
    pub identifier: String,
    pub error: String,
}

/// A price of the merchant's own catalog at the provider.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderPrice {
    pub price_ref: String,
    #[serde(default)]
    pub product_ref: Option<String>,
    #[serde(default)]
    pub unit_amount_minor: Option<i64>,
    pub currency: String,
    /// `month`, `year`, …; none for a one-off price.
    #[serde(default)]
    pub interval: Option<String>,
    #[serde(default)]
    pub interval_count: Option<i32>,
    /// `licensed` or `metered`.
    #[serde(default)]
    pub usage_type: Option<String>,
    pub active: bool,
    #[serde(default)]
    pub metadata: Value,
}

/// Where the customer manages the subscription at the provider. The URL
/// signs the customer in: it never shows in a `Debug` rendering.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PortalSession {
    pub url: String,
}

impl fmt::Debug for PortalSession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PortalSession")
            .field("url", &"[redacted]")
            .finish()
    }
}

/// The item [`crate::ProviderSubscriptions::add_item`] added (or found).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AddedItem {
    pub item_id: String,
}
