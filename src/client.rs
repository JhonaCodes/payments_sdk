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
use crate::model::{CreateIntent, CreateSubscription, Intent, Plan, Subscription};
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
