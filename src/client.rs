//! The router over HTTPS: every request signed as the merchant
//! (`llms.txt` §2). Depend on [`PaymentsGateway`] to swap it for a fake in
//! tests.

use std::fmt;
use std::time::Duration;

use async_trait::async_trait;
use chrono::Utc;
use reqwest::Method;
use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;
use uuid::Uuid;

use crate::error::PaymentsError;
use crate::model::{
    AddedItem, AdoptProviderSubscription, CreateIntent, CreateSubscription, Intent, Plan,
    PortalSession, Proration, ProviderPrice, ProviderSubscription, Subscription, UsageEvent,
    UsageReport,
};
use crate::notice::{MERCHANT_HEADER, NONCE_HEADER, SIGNATURE_HEADER, TIMESTAMP_HEADER};
use crate::signature::{build_message, sign};

/// Who the client signs as, and where the router is.
#[derive(Clone, PartialEq, Eq)]
pub struct PaymentsConfig {
    /// Base URL of the router, without a trailing `/`.
    pub base_url: String,
    /// The merchant's slug at the router.
    pub merchant_slug: String,
    /// The merchant's `hmac_secret`.
    pub hmac_secret: String,
    /// Per-request timeout.
    pub timeout: Duration,
}

impl PaymentsConfig {
    pub fn new(base_url: &str, merchant_slug: &str, hmac_secret: &str) -> Self {
        Self {
            base_url: base_url.trim_end_matches('/').to_owned(),
            merchant_slug: merchant_slug.trim().to_owned(),
            hmac_secret: hmac_secret.to_owned(),
            timeout: Duration::from_secs(15),
        }
    }
}

impl fmt::Debug for PaymentsConfig {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PaymentsConfig")
            .field("base_url", &self.base_url)
            .field("merchant_slug", &self.merchant_slug)
            .field("hmac_secret", &"[redacted]")
            .finish()
    }
}

/// What a merchant asks the router.
#[async_trait]
pub trait PaymentsGateway: Send + Sync {
    /// Creates a charge (`POST /v1/intents`).
    async fn create_intent(&self, intent: &CreateIntent) -> Result<Intent, PaymentsError>;
    /// The intent as the router has it now (the only source of truth).
    async fn get_intent(&self, intent_id: Uuid) -> Result<Intent, PaymentsError>;
    /// Opens a router subscription (`POST /v1/subscriptions`, ePayco).
    async fn create_subscription(
        &self,
        subscription: &CreateSubscription,
    ) -> Result<Subscription, PaymentsError>;
    /// A router subscription as it is now.
    async fn get_subscription(&self, subscription_id: Uuid) -> Result<Subscription, PaymentsError>;
    /// Stops a router subscription's charges; paid until its period end.
    async fn cancel_subscription(
        &self,
        subscription_id: Uuid,
    ) -> Result<Subscription, PaymentsError>;
    /// The merchant's active plans (`GET /v1/plans`).
    async fn list_plans(&self) -> Result<Vec<Plan>, PaymentsError>;
}

/// The router over HTTPS.
pub struct PaymentsClient {
    http: reqwest::Client,
    config: PaymentsConfig,
}

impl PaymentsClient {
    pub fn new(config: PaymentsConfig) -> Result<Self, PaymentsError> {
        let http = reqwest::Client::builder()
            .timeout(config.timeout)
            .build()
            .map_err(|failure| PaymentsError::Config(failure.to_string()))?;
        Ok(Self { http, config })
    }

    /// `method path` with `body`, signed; the `data` of the answer.
    async fn call<B: Serialize + ?Sized, T: DeserializeOwned>(
        &self,
        method: Method,
        path: &str,
        body: Option<&B>,
    ) -> Result<T, PaymentsError> {
        let bytes = match body {
            Some(body) => serde_json::to_vec(body)
                .map_err(|failure| PaymentsError::Config(failure.to_string()))?,
            None => Vec::new(),
        };
        let timestamp = Utc::now().timestamp().to_string();
        let nonce = Uuid::new_v4().simple().to_string();
        let message = build_message(
            &timestamp,
            &nonce,
            method.as_str(),
            path,
            &self.config.merchant_slug,
            &bytes,
        );
        let signature = sign(&self.config.hmac_secret, &message)
            .ok_or_else(|| PaymentsError::Config("unusable hmac key".to_owned()))?;

        let mut request = self
            .http
            .request(method, format!("{}{path}", self.config.base_url))
            .header(MERCHANT_HEADER, &self.config.merchant_slug)
            .header(TIMESTAMP_HEADER, timestamp)
            .header(NONCE_HEADER, nonce)
            .header(SIGNATURE_HEADER, signature);
        if body.is_some() {
            request = request
                .header(reqwest::header::CONTENT_TYPE, "application/json")
                .body(bytes);
        }
        let response = request
            .send()
            .await
            .map_err(|failure| PaymentsError::Unreachable(failure.to_string()))?;
        let status = response.status();
        let text = response
            .text()
            .await
            .map_err(|failure| PaymentsError::Unreachable(failure.to_string()))?;
        if !status.is_success() {
            return Err(rejection(status.as_u16(), &text));
        }
        let envelope: Value = serde_json::from_str(&text)
            .map_err(|failure| PaymentsError::InvalidResponse(failure.to_string()))?;
        let data = envelope
            .get("data")
            .cloned()
            .ok_or_else(|| PaymentsError::InvalidResponse("no `data` in the answer".to_owned()))?;
        serde_json::from_value(data)
            .map_err(|failure| PaymentsError::InvalidResponse(failure.to_string()))
    }
}

/// The error of an answer: the router's JSON error body when there is one
/// (`error_code`, `details`), else the plain text as details.
fn rejection(status: u16, text: &str) -> PaymentsError {
    let body: Option<Value> = serde_json::from_str(text).ok();
    let field = |name: &str| {
        body.as_ref()
            .and_then(|body| body.get(name))
            .and_then(Value::as_str)
            .map(str::to_owned)
    };
    let details = field("details")
        .or_else(|| field("message"))
        .or_else(|| (!text.trim().is_empty()).then(|| text.trim().to_owned()));
    PaymentsError::Rejected {
        status,
        error_code: field("error_code"),
        details,
    }
}

#[async_trait]
impl PaymentsGateway for PaymentsClient {
    async fn create_intent(&self, intent: &CreateIntent) -> Result<Intent, PaymentsError> {
        self.call(Method::POST, "/v1/intents", Some(intent)).await
    }

    async fn get_intent(&self, intent_id: Uuid) -> Result<Intent, PaymentsError> {
        self.call::<(), _>(Method::GET, &format!("/v1/intents/{intent_id}"), None)
            .await
    }

    async fn create_subscription(
        &self,
        subscription: &CreateSubscription,
    ) -> Result<Subscription, PaymentsError> {
        self.call(Method::POST, "/v1/subscriptions", Some(subscription))
            .await
    }

    async fn get_subscription(&self, subscription_id: Uuid) -> Result<Subscription, PaymentsError> {
        self.call::<(), _>(
            Method::GET,
            &format!("/v1/subscriptions/{subscription_id}"),
            None,
        )
        .await
    }

    async fn cancel_subscription(
        &self,
        subscription_id: Uuid,
    ) -> Result<Subscription, PaymentsError> {
        self.call::<(), _>(
            Method::POST,
            &format!("/v1/subscriptions/{subscription_id}/cancel"),
            None,
        )
        .await
    }

    async fn list_plans(&self) -> Result<Vec<Plan>, PaymentsError> {
        self.call::<(), _>(Method::GET, "/v1/plans", None).await
    }
}

/// What a merchant asks the router about the subscriptions a provider
/// manages itself (Stripe, with `provider_plan`): the router keeps their
/// mirror and passes each operation on to the provider, so the merchant
/// never holds the provider's keys. Its own trait, so a fake of
/// [`PaymentsGateway`] keeps compiling.
#[async_trait]
pub trait ProviderSubscriptions: Send + Sync {
    /// The mirror as the router has it now.
    async fn provider_subscription(&self, id: Uuid) -> Result<ProviderSubscription, PaymentsError>;
    /// The mirrors of one customer, live ones first.
    async fn provider_subscriptions_of(
        &self,
        external_user_ref: &str,
    ) -> Result<Vec<ProviderSubscription>, PaymentsError>;
    /// Ends it now, or at `current_period_end` when `at_period_end`.
    async fn cancel_provider_subscription(
        &self,
        id: Uuid,
        at_period_end: bool,
    ) -> Result<ProviderSubscription, PaymentsError>;
    /// Undoes a scheduled end.
    async fn resume_provider_subscription(
        &self,
        id: Uuid,
    ) -> Result<ProviderSubscription, PaymentsError>;
    /// Moves the base item (never the metered one) to `price_ref`.
    async fn change_provider_price(
        &self,
        id: Uuid,
        price_ref: &str,
        proration: Proration,
    ) -> Result<ProviderSubscription, PaymentsError>;
    /// A provider page where the customer manages it; back to `return_url`.
    async fn provider_portal(
        &self,
        id: Uuid,
        return_url: &str,
    ) -> Result<PortalSession, PaymentsError>;
    /// Adds an item of `price_ref` (a metered price); the one already there
    /// when it has it.
    async fn add_item(&self, id: Uuid, price_ref: &str) -> Result<AddedItem, PaymentsError>;
    /// Reports metered usage (1 to 100 events).
    async fn report_usage(
        &self,
        id: Uuid,
        events: &[UsageEvent],
    ) -> Result<UsageReport, PaymentsError>;
    /// The merchant's prices at the provider.
    async fn provider_prices(&self) -> Result<Vec<ProviderPrice>, PaymentsError>;
    /// Registers the mirror of a subscription opened on the merchant's own
    /// provider account before the router; answers it (`origin_intent_id`
    /// null). Idempotent for the same `external_user_ref`.
    async fn adopt_provider_subscription(
        &self,
        request: &AdoptProviderSubscription,
    ) -> Result<ProviderSubscription, PaymentsError>;
}

/// `value` as one path segment: every byte but the unreserved ones
/// (`A-Z a-z 0-9 - . _ ~`) percent-encoded, so the router signs and reads
/// the same path.
fn path_segment(value: &str) -> String {
    value
        .bytes()
        .map(|byte| match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                char::from(byte).to_string()
            }
            other => format!("%{other:02X}"),
        })
        .collect()
}

/// The path of the mirror `id`, with `action` after it when there is one.
fn mirror_path(id: Uuid, action: Option<&str>) -> String {
    match action {
        Some(action) => format!("/v1/provider-subscriptions/{id}/{action}"),
        None => format!("/v1/provider-subscriptions/{id}"),
    }
}

#[derive(Serialize)]
struct CancelBody {
    at_period_end: bool,
}

#[derive(Serialize)]
struct ChangePriceBody<'a> {
    price_ref: &'a str,
    proration: Proration,
}

#[derive(Serialize)]
struct PortalBody<'a> {
    return_url: &'a str,
}

#[derive(Serialize)]
struct ItemBody<'a> {
    price_ref: &'a str,
}

#[derive(Serialize)]
struct UsageBody<'a> {
    events: &'a [UsageEvent],
}

#[derive(Serialize)]
struct EmptyBody {}

#[async_trait]
impl ProviderSubscriptions for PaymentsClient {
    async fn provider_subscription(&self, id: Uuid) -> Result<ProviderSubscription, PaymentsError> {
        self.call::<(), _>(Method::GET, &mirror_path(id, None), None)
            .await
    }

    async fn provider_subscriptions_of(
        &self,
        external_user_ref: &str,
    ) -> Result<Vec<ProviderSubscription>, PaymentsError> {
        let path = format!(
            "/v1/provider-subscriptions/by-user/{}",
            path_segment(external_user_ref)
        );
        self.call::<(), _>(Method::GET, &path, None).await
    }

    async fn cancel_provider_subscription(
        &self,
        id: Uuid,
        at_period_end: bool,
    ) -> Result<ProviderSubscription, PaymentsError> {
        self.call(
            Method::POST,
            &mirror_path(id, Some("cancel")),
            Some(&CancelBody { at_period_end }),
        )
        .await
    }

    async fn resume_provider_subscription(
        &self,
        id: Uuid,
    ) -> Result<ProviderSubscription, PaymentsError> {
        self.call(
            Method::POST,
            &mirror_path(id, Some("resume")),
            Some(&EmptyBody {}),
        )
        .await
    }

    async fn change_provider_price(
        &self,
        id: Uuid,
        price_ref: &str,
        proration: Proration,
    ) -> Result<ProviderSubscription, PaymentsError> {
        self.call(
            Method::POST,
            &mirror_path(id, Some("change-price")),
            Some(&ChangePriceBody {
                price_ref,
                proration,
            }),
        )
        .await
    }

    async fn provider_portal(
        &self,
        id: Uuid,
        return_url: &str,
    ) -> Result<PortalSession, PaymentsError> {
        self.call(
            Method::POST,
            &mirror_path(id, Some("portal")),
            Some(&PortalBody { return_url }),
        )
        .await
    }

    async fn add_item(&self, id: Uuid, price_ref: &str) -> Result<AddedItem, PaymentsError> {
        self.call(
            Method::POST,
            &mirror_path(id, Some("items")),
            Some(&ItemBody { price_ref }),
        )
        .await
    }

    async fn report_usage(
        &self,
        id: Uuid,
        events: &[UsageEvent],
    ) -> Result<UsageReport, PaymentsError> {
        self.call(
            Method::POST,
            &mirror_path(id, Some("usage")),
            Some(&UsageBody { events }),
        )
        .await
    }

    async fn provider_prices(&self) -> Result<Vec<ProviderPrice>, PaymentsError> {
        self.call::<(), _>(Method::GET, "/v1/provider-catalog/prices", None)
            .await
    }

    async fn adopt_provider_subscription(
        &self,
        request: &AdoptProviderSubscription,
    ) -> Result<ProviderSubscription, PaymentsError> {
        self.call(
            Method::POST,
            "/v1/provider-subscriptions/adopt",
            Some(request),
        )
        .await
    }
}
