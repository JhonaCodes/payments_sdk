# payments_sdk

Client of the Payments router for a product's Rust backend:

- signed requests (`X-Payments-*`, HMAC-SHA256);
- verification of the notices the router posts to your `notify_url`;
- the types of the contract.

The router decides nothing about your product. It only sends the customer to the provider's checkout (Stripe, ePayco) and tells you what happened. The full contract is the router's `llms.txt` (`https://<router>/llms.txt`).

```toml
payments_sdk = { git = "https://github.com/JhonaCodes/payments_sdk", tag = "v0.1.0" }
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

Record a type you do not know and answer `2xx`; never refuse it.

## Errors

`PaymentsError::Rejected { status, error_code, details }` keeps the router's error body. `is_retryable()` tells a transient failure (no answer, `408`, `429`, `5xx`) from a definitive one.
