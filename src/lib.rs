//! Client of the Payments router (`llms.txt` of the router): the requests a
//! merchant signs, the verification of the notices it receives, and the
//! types of the contract.
//!
//! - [`signature`]: the HMAC both ways.
//! - [`client`]: [`PaymentsClient`], behind the [`PaymentsGateway`] trait
//!   (intents, router subscriptions, plans) and the
//!   [`ProviderSubscriptions`] trait (subscriptions a provider manages).
//! - [`notice`]: [`verify_notice`] and the notice types.

pub mod client;
pub mod error;
pub mod model;
pub mod notice;
pub mod signature;

pub use client::{PaymentsClient, PaymentsConfig, PaymentsGateway, ProviderSubscriptions};
pub use error::PaymentsError;
pub use model::{
    AddedItem, AdoptProviderSubscription, CreateIntent, CreateSubscription, Intent, IntentStatus,
    Money, Plan, PortalSession, Proration, ProviderPrice, ProviderSubscription,
    ProviderSubscriptionStatus, Subscription, SubscriptionStatus, UsageEvent, UsageFailure,
    UsageReport,
};
pub use notice::{
    NoticeEnvelope, NoticeRejection, NoticeType, PaymentNotice, ProviderInvoice,
    ProviderSubscriptionNotice, ProviderSubscriptionReason, SignedHeaders, SubscriptionEventNotice,
    verify_notice,
};
