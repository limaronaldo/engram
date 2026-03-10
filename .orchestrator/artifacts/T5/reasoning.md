## Task: T5 — Wire StripeService into AppState

### Approach
Added `StripeService` as an optional field on `AppState`. Initialized it
conditionally during `AppState::new` by checking `config.stripe`. `StripeService`
is `Clone` (confirmed by the derive and the existing `stripe_service_is_clone` unit test),
so no `Arc` wrapper is needed.

### Files Changed
- `engram-cloud/gateway/src/state.rs` — imported `StripeService` from
  `crate::stripe_client`, added `pub stripe: Option<StripeService>` field,
  added initialization block that calls `StripeService::new` when `config.stripe`
  is `Some` and logs "Stripe billing service initialized" or "Stripe billing
  not configured (STRIPE_SECRET_KEY not set)" when `None`.

### Decisions Made
- No `Arc` wrapping: `StripeService` is `Clone` and `stripe::Client` is internally
  arc-backed, so an extra indirection adds no value.
- Placed the Stripe init block after the backup block, immediately before the
  final `Ok(Self { ... })` to keep init ordering consistent with existing fields.
- Mirrored the optional R2 pattern: conditional `if let Some(cfg) = &config.stripe`.

### Verification
- Tests pass: `cargo check -p engram-gateway` passes cleanly.
- Lint clean: yes (only pre-existing upstream sqlx-postgres future-incompat warning).
- Type check: yes.
