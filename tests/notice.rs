//! `verify_notice` and the notice types, over payloads shaped as the router
//! builds them (`payment_notice_service.rs` and `subscription_service.rs` of
//! the router).

use chrono::{TimeZone, Utc};
use payments_sdk::model::{IntentStatus, ProviderSubscriptionStatus, SubscriptionStatus};
use payments_sdk::notice::{
    NoticeEnvelope, NoticeRejection, NoticeType, PaymentNotice, ProviderSubscriptionNotice,
    ProviderSubscriptionReason, SignedHeaders, SubscriptionEventNotice, verify_notice,
};

const SECRET: &str = "test-notice-secret-0123456789";
const SLUG: &str = "turnosqr-dev";
const PATH: &str = "/v1/payments/notice";
const BODY: &[u8] = br#"{"event_id":"7c1e0000-0000-4000-8000-000000000001","type":"payment"}"#;
/// Computed with the router guide's Python reference for `BODY` at
/// `1700000000` with nonce `0f3c9a1e`.
const SIGNATURE: &str = "82c69a9cc4a27084a716087c2d4d4efedb238a5a248530bd6e69a04c66d5a2d9";

fn headers(merchant: &str, timestamp: &str, signature: &str) -> SignedHeaders {
    SignedHeaders {
        merchant: merchant.to_owned(),
        timestamp: timestamp.to_owned(),
        nonce: "0f3c9a1e".to_owned(),
        signature: signature.to_owned(),
    }
}

fn at(seconds: i64) -> chrono::DateTime<Utc> {
    match Utc.timestamp_opt(seconds, 0).single() {
        Some(moment) => moment,
        None => panic!("timestamp {seconds}"),
    }
}

#[test]
fn a_notice_signed_by_the_router_verifies() {
    let ok = verify_notice(
        &headers(SLUG, "1700000000", SIGNATURE),
        BODY,
        PATH,
        SLUG,
        SECRET,
        at(1_700_000_100),
    );
    assert_eq!(ok, Ok(()));
}

#[test]
fn each_reason_to_refuse_is_named() {
    let now = at(1_700_000_000);
    assert_eq!(
        verify_notice(
            &headers("another-shop", "1700000000", SIGNATURE),
            BODY,
            PATH,
            SLUG,
            SECRET,
            now
        ),
        Err(NoticeRejection::OtherMerchant)
    );
    assert_eq!(
        verify_notice(
            &headers(SLUG, "1699999699", SIGNATURE),
            BODY,
            PATH,
            SLUG,
            SECRET,
            now
        ),
        Err(NoticeRejection::Stale)
    );
    assert_eq!(
        verify_notice(&headers(SLUG, "", SIGNATURE), BODY, PATH, SLUG, SECRET, now),
        Err(NoticeRejection::Stale)
    );
    assert_eq!(
        verify_notice(
            &headers(SLUG, "1700000000", SIGNATURE),
            b"{}",
            PATH,
            SLUG,
            SECRET,
            now
        ),
        Err(NoticeRejection::BadSignature)
    );
    assert_eq!(
        verify_notice(
            &headers(SLUG, "1700000000", SIGNATURE),
            BODY,
            "/other",
            SLUG,
            SECRET,
            now
        ),
        Err(NoticeRejection::BadSignature)
    );
    assert_eq!(
        verify_notice(
            &headers(SLUG, "1700000000", SIGNATURE),
            BODY,
            PATH,
            SLUG,
            "other-secret-01234",
            now
        ),
        Err(NoticeRejection::BadSignature)
    );
}

#[test]
fn the_headers_are_read_by_name_and_missing_ones_are_empty() {
    let read = SignedHeaders::read(|name| match name {
        "X-Payments-Merchant" => Some(SLUG.to_owned()),
        "X-Payments-Signature" => Some(SIGNATURE.to_owned()),
        _ => None,
    });
    assert_eq!(read.merchant, SLUG);
    assert_eq!(read.signature, SIGNATURE);
    assert_eq!(read.timestamp, "");
    assert_eq!(read.nonce, "");
}

#[test]
fn every_type_the_router_sends_is_known_and_others_are_not_refused() {
    assert_eq!(NoticeType::parse("payment"), Some(NoticeType::Payment));
    assert_eq!(
        NoticeType::parse("subscription_charge"),
        Some(NoticeType::SubscriptionCharge)
    );
    assert_eq!(
        NoticeType::parse("subscription_charge_upcoming"),
        Some(NoticeType::SubscriptionChargeUpcoming)
    );
    assert_eq!(
        NoticeType::parse("subscription_cancelled"),
        Some(NoticeType::SubscriptionCancelled)
    );
    assert_eq!(NoticeType::parse("something_new"), None);

    let envelope: NoticeEnvelope = match serde_json::from_str(
        r#"{"event_id":"7c1e0000-0000-4000-8000-000000000009","type":"something_new"}"#,
    ) {
        Ok(envelope) => envelope,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(envelope.notice_type, "something_new");
}

#[test]
fn a_provider_plan_payment_notice_carries_the_subscription_id() {
    let json = r#"{
        "event_id": "7c1e0000-0000-4000-8000-000000000001",
        "type": "payment",
        "intent_id": "3f2a0000-0000-4000-8000-000000000002",
        "external_order_id": "5b6c:plan_basic:1700000000",
        "external_product_ref": "plan_basic",
        "external_user_ref": "5b6c0000-0000-4000-8000-000000000003",
        "status": "paid",
        "amount": {"minor": 1299, "currency": "USD"},
        "provider_reference": "cs_test_1",
        "provider_transaction_id": "sub_1",
        "concept": "Basic",
        "subscription": null
    }"#;
    let notice: PaymentNotice = match serde_json::from_str(json) {
        Ok(notice) => notice,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(notice.notice_type, NoticeType::Payment);
    assert_eq!(notice.status, IntentStatus::Paid);
    assert_eq!(notice.amount.minor, 1299);
    assert_eq!(notice.provider_transaction_id.as_deref(), Some("sub_1"));
    assert!(notice.subscription.is_none());
}

#[test]
fn an_epayco_charge_notice_carries_the_subscription_after_the_charge() {
    let json = r#"{
        "event_id": "7c1e0000-0000-4000-8000-000000000004",
        "type": "subscription_charge",
        "intent_id": "3f2a0000-0000-4000-8000-000000000005",
        "external_order_id": "sub-order",
        "external_product_ref": "plan_basic_cop",
        "external_user_ref": "5b6c0000-0000-4000-8000-000000000003",
        "status": "paid",
        "amount": {"minor": 5700000, "currency": "COP"},
        "provider_reference": "1234567",
        "provider_transaction_id": "98765",
        "concept": "Plan Basic",
        "subscription": {
            "subscription_id": "9d8e0000-0000-4000-8000-000000000006",
            "status": "active",
            "cancel_reason": null,
            "external_user_ref": "5b6c0000-0000-4000-8000-000000000003",
            "external_plan_ref": "plan_basic_cop",
            "plan_name": "Plan Basic",
            "amount": {"minor": 5700000, "currency": "COP"},
            "interval_unit": "month",
            "interval_count": 1,
            "current_period_start": "2026-11-14T00:00:00Z",
            "current_period_end": "2026-12-14T00:00:00Z",
            "next_charge_at": "2026-12-14T00:00:00Z",
            "failed_charges": 0,
            "has_payment_method": true,
            "setup_expires_at": null,
            "consented_at": "2026-11-14T00:00:00Z",
            "created_at": "2026-11-14T00:00:00Z",
            "cancelled_at": null
        }
    }"#;
    let notice: PaymentNotice = match serde_json::from_str(json) {
        Ok(notice) => notice,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(notice.notice_type, NoticeType::SubscriptionCharge);
    let subscription = match notice.subscription {
        Some(subscription) => subscription,
        None => panic!("no subscription in a subscription_charge"),
    };
    assert_eq!(subscription.status, SubscriptionStatus::Active);
    assert_eq!(subscription.external_plan_ref, "plan_basic_cop");
    assert!(subscription.current_period_end.is_some());
    assert!(subscription.manage_url.is_none());
}

#[test]
fn a_cancellation_notice_names_the_subscription() {
    let json = r#"{
        "event_id": "7c1e0000-0000-4000-8000-000000000007",
        "type": "subscription_cancelled",
        "subscription_id": "9d8e0000-0000-4000-8000-000000000006",
        "external_user_ref": "5b6c0000-0000-4000-8000-000000000003",
        "cancelled_by": "customer",
        "subscription": {
            "subscription_id": "9d8e0000-0000-4000-8000-000000000006",
            "status": "cancelled",
            "cancel_reason": "customer",
            "external_user_ref": "5b6c0000-0000-4000-8000-000000000003",
            "external_plan_ref": "plan_basic_cop",
            "plan_name": "Plan Basic",
            "amount": {"minor": 5700000, "currency": "COP"},
            "interval_unit": "month",
            "interval_count": 1,
            "current_period_start": "2026-11-14T00:00:00Z",
            "current_period_end": "2026-12-14T00:00:00Z",
            "next_charge_at": null,
            "failed_charges": 0,
            "has_payment_method": true,
            "created_at": "2026-11-14T00:00:00Z",
            "cancelled_at": "2026-11-20T00:00:00Z"
        }
    }"#;
    let notice: SubscriptionEventNotice = match serde_json::from_str(json) {
        Ok(notice) => notice,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(notice.notice_type, NoticeType::SubscriptionCancelled);
    assert_eq!(notice.cancelled_by.as_deref(), Some("customer"));
    assert_eq!(notice.subscription.status, SubscriptionStatus::Cancelled);
}

const MIRROR: &str = r#"{
    "id": "9d1e0000-0000-4000-8000-000000000010",
    "provider": "stripe",
    "provider_subscription_id": "sub_1",
    "provider_customer_id": "cus_1",
    "external_user_ref": "5b6c0000-0000-4000-8000-000000000003",
    "external_order_id": "5b6c:plan_basic:1700000000",
    "external_product_ref": "plan_basic",
    "origin_intent_id": "3f2a0000-0000-4000-8000-000000000002",
    "price_ref": "price_basic",
    "metered_item_id": null,
    "status": "past_due",
    "current_period_start": "2026-11-01T00:00:00Z",
    "current_period_end": "2026-12-01T00:00:00Z",
    "cancel_at_period_end": false,
    "canceled_at": null,
    "ended_at": null,
    "trial_end": null,
    "version": 7,
    "created_at": "2026-11-01T00:00:00Z",
    "updated_at": "2026-11-15T00:00:00Z"
}"#;

#[test]
fn a_provider_subscription_notice_is_known_and_carries_the_snapshot() {
    assert_eq!(
        NoticeType::parse("provider_subscription"),
        Some(NoticeType::ProviderSubscription)
    );
    let json = format!(
        r#"{{"event_id":"7c1e0000-0000-4000-8000-000000000011","type":"provider_subscription","reason":"payment_failed","external_user_ref":"5b6c0000-0000-4000-8000-000000000003","subscription":{MIRROR},"invoice":{{"provider_invoice_id":"in_1","amount_paid":{{"minor":0,"currency":"USD"}},"period_start":"2026-11-01T00:00:00Z","period_end":"2026-12-01T00:00:00Z"}}}}"#
    );
    let notice: ProviderSubscriptionNotice = match serde_json::from_str(&json) {
        Ok(notice) => notice,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(notice.notice_type, NoticeType::ProviderSubscription);
    assert_eq!(notice.reason, ProviderSubscriptionReason::PaymentFailed);
    assert_eq!(
        notice.subscription.status,
        ProviderSubscriptionStatus::PastDue
    );
    assert!(!notice.subscription.status.is_active());
    assert_eq!(notice.subscription.version, 7);
    assert_eq!(
        notice.subscription.price_ref.as_deref(),
        Some("price_basic")
    );
    assert_eq!(
        notice.invoice.map(|invoice| invoice.provider_invoice_id),
        Some("in_1".to_owned())
    );
}

#[test]
fn a_reason_or_a_status_this_version_does_not_know_still_reads() {
    let json = format!(
        r#"{{"event_id":"7c1e0000-0000-4000-8000-000000000012","type":"provider_subscription","reason":"something_new","external_user_ref":"u","subscription":{}}}"#,
        MIRROR.replace("\"past_due\"", "\"a_new_state\"")
    );
    let notice: ProviderSubscriptionNotice = match serde_json::from_str(&json) {
        Ok(notice) => notice,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(notice.reason, ProviderSubscriptionReason::Unknown);
    assert_eq!(
        notice.subscription.status,
        ProviderSubscriptionStatus::Unknown
    );
    assert!(notice.invoice.is_none());
}

#[test]
fn a_provider_plan_payment_notice_names_the_routers_mirror() {
    let json = r#"{
        "event_id": "7c1e0000-0000-4000-8000-000000000013",
        "type": "payment",
        "intent_id": "3f2a0000-0000-4000-8000-000000000002",
        "external_order_id": "o",
        "external_product_ref": "plan_basic",
        "external_user_ref": "u",
        "status": "paid",
        "amount": {"minor": 1299, "currency": "USD"},
        "provider_transaction_id": "sub_1",
        "provider_subscription_id": "9d1e0000-0000-4000-8000-000000000010"
    }"#;
    let notice: PaymentNotice = match serde_json::from_str(json) {
        Ok(notice) => notice,
        Err(error) => panic!("{error}"),
    };
    assert_eq!(
        notice.provider_subscription_id.map(|id| id.to_string()),
        Some("9d1e0000-0000-4000-8000-000000000010".to_owned())
    );
}
