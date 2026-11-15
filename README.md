# payments_sdk

Client of the Payments router for a product's Rust backend:

- signed requests (`X-Payments-*`, HMAC-SHA256);
- verification of the notices the router posts to your `notify_url`;
- the types of the contract.

The router decides nothing about your product. It only sends the customer to the provider's checkout (Stripe, ePayco) and tells you what happened. The full contract is the router's `llms.txt` (`https://<router>/llms.txt`).

```toml
payments_sdk = { git = "https://github.com/JhonaCodes/payments_sdk", tag = "v0.3.0" }
```

## Integrate a product in five steps

1. **Merchant.** In the router's panel, create your merchant with:
   - `slug`;
   - `notify_url` (your notice route, below);
   - `return_url_prefix`;
   - an `hmac_secret` of at least 16 characters.

   Connect its provider accounts and payment methods there.

2. **Configuration.** Set three variables, all or none:
   - `PAYMENTS_BASE_URL`
   - `PAYMENTS_MERCHANT_SLUG`
   - `PAYMENTS_HMAC_SECRET`

   Build the client once:

   ```rust
   use payments_sdk::{PaymentsClient, PaymentsConfig, PaymentsGateway};

   let client = PaymentsClient::new(PaymentsConfig::new(&base_url, &slug, &secret))?;
   // Depend on `Arc<dyn PaymentsGateway>` so tests can inject a fake.
   ```

3. **Charge.** Ask for a checkout and send the customer to `checkout_url`:

   ```rust
   use payments_sdk::CreateIntent;

   // A subscription the provider bills with your own plan (Stripe Price):
   let intent = client.create_intent(&CreateIntent {
       external_order_id: format!("{company}:{plan}:{now}"),
       external_product_ref: plan.into(),
       external_user_ref: company.to_string(),
       provider: Some("stripe".into()),
       provider_plan: Some(stripe_price_id.into()),
       return_url: Some(format!("{app}/plans?billing=return")),
       ..CreateIntent::default()
   }).await?;
   ```

   A subscription the router charges itself (ePayco) is `client.create_subscription(...)`, and the customer goes to its `subscribe_url`.

4. **Notices.** Your route (public: the signature is the credential) verifies the raw body, records the notice once per `event_id`, and re-reads the intent before acting:

   ```rust
   use payments_sdk::{verify_notice, NoticeEnvelope, SignedHeaders};

   let headers = SignedHeaders::read(|name| header(name));
   verify_notice(&headers, &raw_body, "/v1/payments/notice", &slug, &secret, Utc::now())
       .map_err(|_| unauthorized())?;                 // 401: refused for good
   let envelope: NoticeEnvelope = serde_json::from_slice(&raw_body)?;
   // insert by envelope.event_id; if already processed → 200 and stop
   let intent = client.get_intent(notice.intent_id).await?;  // the truth
   ```

5. **Answer.** Your answer drives the router's retries:

   | You answer | The router |
   | --- | --- |
   | `2xx` | delivered; also answer it for a duplicate you already processed |
   | `408`, `429`, `5xx`, no answer | retries after 1 min, 5 min, 30 min, 2 h and 12 h |
   | any other `4xx` | your refusal; never retried |

   Answer `503` while your configuration or database is not ready, and a `4xx` only for a notice you will never accept.

## Notice types

| `type` | Read it as | Act on |
| --- | --- | --- |
| `payment` | `PaymentNotice` | `status` after re-reading the intent; with `provider_plan`, `provider_transaction_id` is the provider's subscription (`sub_…`) |
| `subscription_charge` | `PaymentNotice` (with `subscription`) | a router subscription's charge; access until `current_period_end` |
| `subscription_charge_upcoming` | `SubscriptionEventNotice` | tell the customer the amount and the date |
| `subscription_cancelled` | `SubscriptionEventNotice` | no more charges; access until `current_period_end` |
| `provider_subscription` | `ProviderSubscriptionNotice` | a subscription the provider manages changed (`reason`: `activated`, `renewed`, `payment_failed`, `cancel_scheduled`, `resumed`, `ended`, `updated`); apply its `subscription` snapshot unless its `version` is at or below the one you applied |

Record a type you do not know and answer `2xx`; never refuse it. `NoticeType` is `#[non_exhaustive]`: match it with a fallback arm.

## Subscriptions the provider manages

A charge with `provider_plan` opens a subscription the provider bills with the merchant's own plan (a Stripe Price). The router keeps its **mirror** (`ProviderSubscription`: the state as the provider reports it, with a growing `version`) and passes every operation on to the provider, so the product never holds the provider's keys nor its webhooks. Use the `ProviderSubscriptions` trait (implemented by `PaymentsClient`):

| Method | Router route | What the provider does |
| --- | --- | --- |
| `provider_subscription(id)` | `GET /v1/provider-subscriptions/{id}` | — (the mirror) |
| `provider_subscriptions_of(external_user_ref)` | `GET /v1/provider-subscriptions/by-user/{ref}` | — (live ones first) |
| `cancel_provider_subscription(id, at_period_end)` | `POST …/{id}/cancel` | ends it now or at the period end |
| `resume_provider_subscription(id)` | `POST …/{id}/resume` | undoes a scheduled end |
| `change_provider_price(id, price_ref, proration)` | `POST …/{id}/change-price` | moves the base item to another price |
| `provider_portal(id, return_url)` | `POST …/{id}/portal` | a page where the customer manages it (the URL signs them in: never store or log it) |
| `add_item(id, price_ref)` | `POST …/{id}/items` | adds a metered item (idempotent by price) |
| `report_usage(id, events)` | `POST …/{id}/usage` | bills metered usage; `identifier` makes a repeat count once |
| `provider_prices()` | `GET /v1/provider-catalog/prices` | the merchant's prices at the provider |
| `adopt_provider_subscription(request)` | `POST /v1/provider-subscriptions/adopt` | — (mirrors a subscription opened before the router; below) |

Every change comes back as a `provider_subscription` notice as well, so a product can act on the notice alone.

### Adopt a subscription opened before the router

A subscription the product opened at the provider directly (before it moved to the router) has no mirror, so none of the operations above can reach it. `adopt_provider_subscription` registers its mirror (`POST /v1/provider-subscriptions/adopt`); the router reads it from the merchant's own provider account and writes nothing to the provider:

```rust
use payments_sdk::{AdoptProviderSubscription, ProviderSubscriptions};

let mirror = client.adopt_provider_subscription(&AdoptProviderSubscription {
    provider: "stripe".into(),
    provider_subscription_id: stored_sub_id.clone(),   // the `sub_…` you kept
    external_user_ref: company.to_string(),
    external_order_id: stored_sub_id.clone(),
    external_product_ref: plan.into(),
}).await?;
// Apply `mirror` as you apply a notice's snapshot: live → active plan,
// ended (`status.has_ended()`) → back to your free plan.
```

- The answer is the mirror (`origin_intent_id: null`); **no notice** is sent for the adoption itself. Later changes (renewals, failed charges, the end) arrive as `provider_subscription` notices.
- Adopting again with the same `external_user_ref` answers the same mirror (idempotent); with another one, `409`.
- `404` when the subscription is not on the merchant's provider account, or its metadata names another merchant.
- An ended subscription is adopted too, so the product can take the plan down.
- Only adopt the `sub_…` you stored for that same customer: the router cannot verify `external_user_ref`.

## Errors

`PaymentsError::Rejected { status, error_code, details }` keeps the router's error body. `is_retryable()` tells a transient failure (no answer, `408`, `429`, `5xx`) from a definitive one.
