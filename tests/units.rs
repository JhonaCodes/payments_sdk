//! Unit pricing as the router computes it: `base + unit_amount × max(0,
//! units − included_units)`, and the plan and subscription fields of a
//! router that does not send them yet.

use payments_sdk::model::{Money, Plan, Subscription, period_amount};

fn cop(minor: i64) -> Money {
    Money {
        minor,
        currency: "COP".into(),
    }
}

fn corporativo() -> Plan {
    match serde_json::from_str(
        r#"{"external_plan_ref":"corporativo","name":"Corporativo","amount":{"minor":69000000,"currency":"COP"},"interval_unit":"month","interval_count":1,"trial_days":0,"active":true,"unit_amount":{"minor":69000,"currency":"COP"},"included_units":1000}"#,
    ) {
        Ok(plan) => plan,
        Err(error) => panic!("plan: {error}"),
    }
}

#[test]
fn units_above_the_included_ones_are_charged_at_the_unit_price() {
    let plan = corporativo();
    assert_eq!(plan.period_amount(1_200), Some(cop(82_800_000)));
    assert_eq!(plan.period_amount(1_000), Some(cop(69_000_000)));
    assert_eq!(plan.period_amount(3), Some(cop(69_000_000)));
}

#[test]
fn without_a_unit_price_the_base_amount_is_charged() {
    assert_eq!(period_amount(&cop(5_900_000), None, 0, 999), Some(cop(5_900_000)));
}

#[test]
fn a_unit_price_in_another_currency_or_an_overflow_has_no_amount() {
    let usd = Money {
        minor: 100,
        currency: "USD".into(),
    };
    assert_eq!(period_amount(&cop(100), Some(&usd), 0, 1), None);
    assert_eq!(period_amount(&cop(i64::MAX), Some(&cop(1)), 0, 1), None);
}

#[test]
fn a_router_without_unit_pricing_still_parses() {
    let plan: Plan = match serde_json::from_str(
        r#"{"external_plan_ref":"micro","name":"Micro","amount":{"minor":5900000,"currency":"COP"},"interval_unit":"month","interval_count":1,"active":true}"#,
    ) {
        Ok(plan) => plan,
        Err(error) => panic!("plan: {error}"),
    };
    assert_eq!(plan.unit_amount, None);
    assert_eq!(plan.included_units, 0);

    let subscription: Subscription = match serde_json::from_str(
        r#"{"subscription_id":"a2af5b58-9353-4b4c-bf8e-8be1cf1592e4","status":"active","external_user_ref":"c","external_plan_ref":"micro","amount":{"minor":5900000,"currency":"COP"},"interval_unit":"month","interval_count":1}"#,
    ) {
        Ok(subscription) => subscription,
        Err(error) => panic!("subscription: {error}"),
    };
    assert_eq!(subscription.units, 0);
    assert_eq!(subscription.base_amount, None);
}
