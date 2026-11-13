//! Client of the Payments router (`llms.txt` of the router): the requests a
//! merchant signs, the verification of the notices it receives, and the
//! types of the contract.
//!
//! - [`signature`]: the HMAC both ways.
//! - [`client`]: [`PaymentsClient`], behind the [`PaymentsGateway`] trait.
//! - [`notice`]: [`verify_notice`] and the notice types.

pub mod client;
pub mod error;
pub mod model;
pub mod notice;
pub mod signature;

pub use client::{PaymentsClient, PaymentsConfig, PaymentsGateway};
pub use error::PaymentsError;
pub use model::{
    CreateIntent, CreateSubscription, Intent, IntentStatus, Money, Plan, Subscription,
    SubscriptionStatus,
};
pub use notice::{
    NoticeEnvelope, NoticeRejection, NoticeType, PaymentNotice, SignedHeaders,
    SubscriptionEventNotice, verify_notice,
};
