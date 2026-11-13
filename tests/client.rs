//! `PaymentsClient` against a one-shot local HTTP server: every request is
//! signed as the router verifies it (the server checks it with the same
//! function the router uses), the `data` of the answer is parsed, and an
//! error answer keeps the router's `error_code` and `details`.

use std::time::Duration;

use chrono::Utc;
use payments_sdk::PaymentsError;
use payments_sdk::client::{PaymentsClient, PaymentsConfig, PaymentsGateway};
use payments_sdk::model::{CreateIntent, IntentStatus};
use payments_sdk::signature::{build_message, verify, within_window};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use tokio::task::JoinHandle;
use uuid::Uuid;

const SLUG: &str = "turnosqr-dev";
const SECRET: &str = "test-merchant-secret-0123456789";

/// What the server saw: method, path, headers (lower-cased names) and body.
struct Seen {
    method: String,
    path: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

impl Seen {
    fn header(&self, name: &str) -> String {
        self.headers
            .iter()
            .find(|(key, _)| key == &name.to_lowercase())
            .map(|(_, value)| value.clone())
            .unwrap_or_default()
    }

    /// Whether the request is signed as the router checks it.
    fn is_signed(&self) -> bool {
        let timestamp = self.header("X-Payments-Timestamp");
        let fresh = timestamp
            .parse::<i64>()
            .is_ok_and(|ts| within_window(ts, Utc::now().timestamp()));
        let message = build_message(
            &timestamp,
            &self.header("X-Payments-Nonce"),
            &self.method,
            &self.path,
            &self.header("X-Payments-Merchant"),
            &self.body,
        );
        fresh
            && self.header("X-Payments-Merchant") == SLUG
            && verify(SECRET, &message, &self.header("X-Payments-Signature"))
    }
}

/// Serves one request with `status` and `body`; answers what it saw.
async fn serve_once(status: u16, body: &'static str) -> (String, JoinHandle<Seen>) {
    let listener = match TcpListener::bind("127.0.0.1:0").await {
        Ok(listener) => listener,
        Err(error) => panic!("bind: {error}"),
    };
    let address = match listener.local_addr() {
        Ok(address) => address,
        Err(error) => panic!("addr: {error}"),
    };
    let handle = tokio::spawn(async move {
        let (mut socket, _) = match listener.accept().await {
            Ok(accepted) => accepted,
            Err(error) => panic!("accept: {error}"),
        };
        let mut buffer = Vec::new();
        let mut chunk = [0u8; 4096];
        let (head_end, content_length) = loop {
            let read = match socket.read(&mut chunk).await {
                Ok(read) => read,
                Err(error) => panic!("read: {error}"),
            };
            buffer.extend_from_slice(&chunk[..read]);
            if let Some(end) = buffer.windows(4).position(|w| w == b"\r\n\r\n") {
                let head = String::from_utf8_lossy(&buffer[..end]).to_string();
                let length = head
                    .lines()
                    .find_map(|line| {
                        let (key, value) = line.split_once(':')?;
                        (key.trim().eq_ignore_ascii_case("content-length"))
                            .then(|| value.trim().parse::<usize>().ok())
                            .flatten()
                    })
                    .unwrap_or(0);
                break (end + 4, length);
            }
            if read == 0 {
                panic!("connection closed before the headers");
            }
        };
        while buffer.len() < head_end + content_length {
            let read = match socket.read(&mut chunk).await {
                Ok(read) => read,
                Err(error) => panic!("read body: {error}"),
            };
            if read == 0 {
                break;
            }
            buffer.extend_from_slice(&chunk[..read]);
        }
        let head = String::from_utf8_lossy(&buffer[..head_end - 4]).to_string();
        let mut lines = head.lines();
        let request_line = lines.next().unwrap_or_default().to_string();
        let mut parts = request_line.split_whitespace();
        let method = parts.next().unwrap_or_default().to_string();
        let path = parts.next().unwrap_or_default().to_string();
        let headers = lines
            .filter_map(|line| {
                let (key, value) = line.split_once(':')?;
                Some((key.trim().to_lowercase(), value.trim().to_string()))
            })
            .collect();
        let reply = format!(
            "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        if let Err(error) = socket.write_all(reply.as_bytes()).await {
            panic!("write: {error}");
        }
        Seen {
            method,
            path,
            headers,
            body: buffer[head_end..head_end + content_length].to_vec(),
        }
    });
    (format!("http://{address}"), handle)
}

fn client(base_url: &str) -> PaymentsClient {
    let mut config = PaymentsConfig::new(base_url, SLUG, SECRET);
    config.timeout = Duration::from_secs(5);
    match PaymentsClient::new(config) {
        Ok(client) => client,
        Err(error) => panic!("client: {error}"),
    }
}

#[tokio::test]
async fn create_intent_is_signed_over_its_exact_body_and_parses_the_data() {
    let (base, server) = serve_once(
        201,
        r#"{"data":{"intent_id":"a2af5b58-9353-4b4c-bf8e-8be1cf1592e4","status":"checkout_ready","amount":{"minor":1299,"currency":"USD"},"checkout_url":"https://checkout.stripe.com/x","provider":"stripe","provider_reference":"cs_test_1","provider_transaction_id":null},"status":"success"}"#,
    )
    .await;
    let intent = client(&base)
        .create_intent(&CreateIntent {
            external_order_id: "order-1".into(),
            external_product_ref: "plan_basic".into(),
            external_user_ref: "company-1".into(),
            provider: Some("stripe".into()),
            provider_plan: Some("price_1".into()),
            return_url: Some("https://app-dev.turnosqr.com/plans?billing=return".into()),
            ..CreateIntent::default()
        })
        .await;
    let seen = match server.await {
        Ok(seen) => seen,
        Err(error) => panic!("server: {error}"),
    };
    assert_eq!(seen.method, "POST");
    assert_eq!(seen.path, "/v1/intents");
    assert!(seen.is_signed(), "the router would refuse this signature");
    let sent: serde_json::Value = match serde_json::from_slice(&seen.body) {
        Ok(sent) => sent,
        Err(error) => panic!("body: {error}"),
    };
    assert_eq!(sent["provider_plan"], "price_1");
    assert!(sent.get("amount").is_none(), "absent fields are not sent");

    let intent = match intent {
        Ok(intent) => intent,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(intent.status, IntentStatus::CheckoutReady);
    assert_eq!(
        intent.checkout_url.as_deref(),
        Some("https://checkout.stripe.com/x")
    );
}

#[tokio::test]
async fn a_get_signs_the_empty_body() {
    let id = Uuid::new_v4();
    let (base, server) = serve_once(
        200,
        r#"{"data":{"intent_id":"a2af5b58-9353-4b4c-bf8e-8be1cf1592e4","status":"paid","provider":"stripe","provider_transaction_id":"sub_1"}}"#,
    )
    .await;
    let intent = client(&base).get_intent(id).await;
    let seen = match server.await {
        Ok(seen) => seen,
        Err(error) => panic!("server: {error}"),
    };
    assert_eq!(seen.method, "GET");
    assert_eq!(seen.path, format!("/v1/intents/{id}"));
    assert!(seen.body.is_empty());
    assert!(seen.is_signed());
    match intent {
        Ok(intent) => {
            assert_eq!(intent.status, IntentStatus::Paid);
            assert_eq!(intent.provider_transaction_id.as_deref(), Some("sub_1"));
        }
        Err(error) => panic!("{error}"),
    }
}

#[tokio::test]
async fn an_error_answer_keeps_the_routers_code_and_details() {
    let (base, server) = serve_once(
        422,
        r#"{"status":"error","code":422,"error_code":"VALIDATION_ERROR","details":"provider_plan price_x is not an active recurring price"}"#,
    )
    .await;
    let failure = client(&base).get_intent(Uuid::new_v4()).await;
    let _ = server.await;
    match failure {
        Err(PaymentsError::Rejected {
            status,
            error_code,
            details,
        }) => {
            assert_eq!(status, 422);
            assert_eq!(error_code.as_deref(), Some("VALIDATION_ERROR"));
            assert_eq!(
                details.as_deref(),
                Some("provider_plan price_x is not an active recurring price")
            );
        }
        other => panic!("expected a rejection, got {other:?}"),
    }
}

#[test]
fn only_transient_failures_are_retryable() {
    let rejected = |status| PaymentsError::Rejected {
        status,
        error_code: None,
        details: None,
    };
    assert!(PaymentsError::Unreachable("timeout".into()).is_retryable());
    assert!(rejected(503).is_retryable());
    assert!(rejected(429).is_retryable());
    assert!(!rejected(422).is_retryable());
    assert!(!rejected(401).is_retryable());
    assert!(!PaymentsError::InvalidResponse("x".into()).is_retryable());
}
