//! Wire models for `/api/billing/*` and the account-key sign-in
//! (`POST /api/auth/account-key`).
//!
//! Mirrors `mylittlechart-server/crates/server/src/routes/billing.rs` and
//! `routes/anon_account.rs`. The browser talks to the MLC server only; the
//! server talks to the payment processor. `pay[]` is passed through exactly as
//! the processor issued it, so nothing here filters or reorders it.
//!
//! Error bodies are `{"error": "<code>"}` (plus `detail` for a processor
//! refusal); [`BillingError::from_cloud`] turns a [`CloudError`] into that
//! triple so the UI can pick a sentence without parsing JSON itself.

use serde::{Deserialize, Deserializer};
use serde_json::Value;

use crate::CloudError;

// =============================================================================
// GET /api/billing/plans
// =============================================================================

/// What a plan grants. `Other` keeps a server that learned a new kind from
/// breaking the whole plan list: the UI simply does not draw a plan it cannot
/// name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingPlanKind {
    /// Lifetime paid account (OAuth identity).
    PaidAccount,
    /// Lifetime anonymous account (no OAuth, the account key is the credential).
    AnonAccount,
    /// A monthly quant tier.
    Quant,
    /// A kind this client does not know.
    #[serde(other)]
    Other,
}

/// How a plan is billed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BillingInterval {
    /// Settles once.
    OneTime,
    /// Renews every period.
    Monthly,
    /// An interval this client does not know.
    #[serde(other)]
    Other,
}

/// One purchasable plan from `GET /api/billing/plans`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingPlan {
    /// Processor plan id — what `checkout` / `change` take back.
    pub id: String,
    pub kind: BillingPlanKind,
    pub interval: BillingInterval,
    /// Display price in whole USD. The amount actually owed is `pay[].amount`.
    pub price_usd: u32,
    /// Position among the quant tiers, 1 = cheapest.
    #[serde(default)]
    pub tier: Option<u8>,
    /// Operator-configured quant quota, when set.
    #[serde(default)]
    pub quota: Option<u64>,
    /// A Tor-variant account plan (offered on the onion host only).
    #[serde(default)]
    pub onion_only: bool,
}

/// Response body of `GET /api/billing/plans`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingPlans {
    /// The request came through the onion vhost.
    #[serde(default)]
    pub onion: bool,
    /// The server holds a processor key, so `checkout` can succeed.
    #[serde(default)]
    pub checkout_enabled: bool,
    pub plans: Vec<BillingPlan>,
}

// =============================================================================
// GET /api/billing/me
// =============================================================================

/// The quant part of `GET /api/billing/me`.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
pub struct BillingQuant {
    /// Plan id of the running tier.
    #[serde(default)]
    pub tier: Option<String>,
    #[serde(default)]
    pub active: bool,
    #[serde(default)]
    pub active_until: Option<String>,
    #[serde(default)]
    pub quota: Option<u64>,
}

/// A change the subscription has queued for its next renewal.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingPendingChange {
    pub change_id: String,
    #[serde(default)]
    pub plan_id: Option<String>,
}

/// The account's running subscription.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingSubscription {
    pub subscription_id: String,
    pub plan_id: String,
    /// `creating | open | active | past_due | cancelled | paid`.
    pub state: String,
    #[serde(default)]
    pub active_until: Option<String>,
    #[serde(default)]
    pub pending_change: Option<BillingPendingChange>,
}

/// One unpaid invoice of the account.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingOpenInvoice {
    pub invoice_id: String,
    pub plan_id: String,
    /// `initial | renewal | proration`.
    #[serde(default)]
    pub kind: String,
    #[serde(default)]
    pub expires_at: Option<String>,
}

/// Response body of `GET /api/billing/me`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingMe {
    #[serde(default)]
    pub paid: bool,
    #[serde(default)]
    pub paid_until: Option<String>,
    #[serde(default)]
    pub lifetime_paid: bool,
    #[serde(default)]
    pub lifetime_anon: bool,
    #[serde(default)]
    pub quant: BillingQuant,
    #[serde(default, deserialize_with = "micros_or_zero")]
    pub credit_usd_micros: i64,
    #[serde(default)]
    pub subscription: Option<BillingSubscription>,
    #[serde(default)]
    pub open_invoices: Vec<BillingOpenInvoice>,
}

// =============================================================================
// checkout / anon-checkout / change / invoice
// =============================================================================

/// One way to pay an invoice, exactly as the processor issued it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct PayOption {
    /// Network id, e.g. `evm:1`, `tron`.
    pub chain: String,
    /// Asset symbol, e.g. `USDC`.
    pub asset: String,
    /// Recipient address.
    pub address: String,
    /// Decimal amount owed, as text.
    pub amount: String,
    /// RFC 3339 instant this option stops accepting payment.
    #[serde(default)]
    pub expires_at: String,
    /// SVG document of the QR code.
    #[serde(default)]
    pub qr_svg: String,
}

/// Response body of `POST /api/billing/checkout`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingCheckout {
    pub invoice_id: String,
    pub plan_id: String,
    pub pay: Vec<PayOption>,
    /// Hosted checkout page of the processor's widget, when the server passes it.
    #[serde(default)]
    pub checkout_url: Option<String>,
}

/// Response body of `POST /api/billing/anon-checkout` (`201`). It carries no
/// key: the client drew the key itself and sent only the credential derived
/// from it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingAnonCheckout {
    pub invoice_id: String,
    pub pay: Vec<PayOption>,
    #[serde(default)]
    pub checkout_url: Option<String>,
}

// =============================================================================
// Request bodies that carry the credential
// =============================================================================

/// Whether `text` is a login credential: 64 hex characters (`hex(C)`).
pub fn is_credential(text: &str) -> bool {
    text.len() == 64 && text.bytes().all(|b| b.is_ascii_hexdigit())
}

fn checked_credential(credential: &str) -> Result<&str, CloudError> {
    if is_credential(credential) {
        Ok(credential)
    } else {
        Err(CloudError::Decode("the credential must be 64 hex characters".to_string()))
    }
}

fn json_body(value: serde_json::Value) -> Result<String, CloudError> {
    serde_json::to_string(&value).map_err(|e| CloudError::Decode(e.to_string()))
}

/// Body of `POST /api/billing/anon-checkout`: the plan and `hex(C)`, nothing else.
pub fn anon_checkout_body(plan_id: &str, credential: &str) -> Result<String, CloudError> {
    json_body(serde_json::json!({ "plan_id": plan_id, "credential": checked_credential(credential)? }))
}

/// Body of `POST /api/auth/account-key` (and of `POST /api/account/key/rotate`):
/// `hex(C)`, nothing else.
pub fn credential_body(credential: &str) -> Result<String, CloudError> {
    json_body(serde_json::json!({ "credential": checked_credential(credential)? }))
}

/// Body of `POST /api/auth/invite/redeem`: the invite code and `hex(C)`.
pub fn invite_redeem_body(invite_code: &str, credential: &str) -> Result<String, CloudError> {
    json_body(serde_json::json!({ "invite_code": invite_code, "credential": checked_credential(credential)? }))
}

/// Value of the `Authorization` header of `GET /api/billing/anon-invoice/{id}`.
pub fn account_key_authorization(credential: &str) -> Result<String, CloudError> {
    Ok(format!("AccountKey {}", checked_credential(credential)?))
}

/// The validity window of a paid quant period.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingPeriod {
    pub from: String,
    pub to: String,
}

/// Amounts the processor reported for a partial or over payment
/// (`detail.partial` / `detail.over`). Every field is text: the server sends
/// decimals as strings, and a number is accepted and stringified.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AmountNote {
    pub asset: String,
    pub chain: String,
    pub received: String,
    pub expected: String,
    pub shortfall: String,
    pub excess: String,
}

/// Response body of `GET /api/billing/invoice/{id}` and
/// `GET /api/billing/anon-invoice/{id}`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingInvoice {
    pub invoice_id: String,
    pub plan_id: String,
    /// `initial | renewal | proration`.
    #[serde(default)]
    pub kind: String,
    /// MLC's own record: `new | settled | reverted | refunded`.
    pub status: String,
    /// The processor's live status line, when it could be read:
    /// `new | processing | settled | complete | expired | invalid`.
    #[serde(default)]
    pub live_status: Option<String>,
    /// `none | granted | reverted | revoked`.
    #[serde(default)]
    pub grant_state: String,
    /// Integrity flags: `partial`, `over`, `late`, `failed`, `no_show`,
    /// `funds_pending`, `anomaly`.
    #[serde(default)]
    pub flags: Vec<String>,
    /// Structured notes keyed by flag.
    #[serde(default)]
    pub detail: Value,
    #[serde(default)]
    pub period: Option<BillingPeriod>,
    #[serde(default)]
    pub expires_at: Option<String>,
    #[serde(default)]
    pub pay: Vec<PayOption>,
}

impl BillingInvoice {
    /// Whether the integrity flag `flag` is set on this invoice.
    pub fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    /// The amounts recorded under `detail.<key>` (`partial` / `over`), when
    /// the server sent an object there.
    pub fn amount_note(&self, key: &str) -> Option<AmountNote> {
        let obj = self.detail.get(key)?;
        if !obj.is_object() {
            return None;
        }
        Some(AmountNote {
            asset: text_field(obj, "asset"),
            chain: text_field(obj, "chain"),
            received: text_field(obj, "received"),
            expected: text_field(obj, "expected"),
            shortfall: text_field(obj, "shortfall"),
            excess: text_field(obj, "excess"),
        })
    }
}

/// Response body of `POST /api/billing/change`.
#[derive(Debug, Clone, PartialEq, Deserialize)]
pub struct BillingChange {
    pub change_id: String,
    /// `pending | applied | ...` — the processor's word.
    #[serde(default)]
    pub state: String,
    #[serde(default)]
    pub from_plan_id: Option<String>,
    #[serde(default)]
    pub to_plan_id: Option<String>,
    /// `charge_now` for an upgrade, `none` for a downgrade at renewal.
    #[serde(default)]
    pub proration: String,
    /// The upgrade charge in millionths of a USD (sent as text or a number).
    #[serde(default, deserialize_with = "micros_opt")]
    pub charge_usd_micros: Option<i64>,
    /// When a downgrade takes effect (RFC 3339).
    #[serde(default)]
    pub pending_effective_at: Option<String>,
    /// Invoice of the upgrade charge.
    #[serde(default)]
    pub invoice_id: Option<String>,
    #[serde(default)]
    pub pay: Vec<PayOption>,
}

// =============================================================================
// POST /api/auth/account-key
// =============================================================================

/// The session an account key mints — the shape the other sign-ins answer with.
///
/// The `session_id` also arrives as an `HttpOnly` cookie; nothing here keeps
/// it, and the client completes the sign-in through the existing device-code
/// confirmation (`POST /api/auth/confirm`) which turns that cookie into the
/// bearer token every other request uses.
#[derive(Clone, PartialEq, Deserialize)]
pub struct AccountKeySession {
    pub status: String,
    pub user_id: i64,
    #[serde(default)]
    pub public_id: String,
    #[serde(default)]
    pub session_id: String,
}

impl std::fmt::Debug for AccountKeySession {
    /// The session id is a credential: it must never reach a log line.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AccountKeySession")
            .field("status", &self.status)
            .field("user_id", &self.user_id)
            .field("public_id", &self.public_id)
            .field("session_id", &"<redacted>")
            .finish()
    }
}

// =============================================================================
// Errors
// =============================================================================

/// A billing failure reduced to what the UI needs to pick a sentence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillingError {
    /// HTTP status, `0` for a transport or decode failure.
    pub status: u16,
    /// The server's `error` code (`already_owned`, `plan_unavailable`, ...),
    /// or `unauthorized` / `forbidden` / `network` / `decode` when no body
    /// code exists.
    pub code: String,
    /// The processor's own explanation (`gateway_refused` only).
    pub detail: Option<String>,
}

impl BillingError {
    /// Reduce a transport error. The body is parsed leniently: anything that
    /// is not `{"error": "..."}` keeps the status and an empty code.
    pub fn from_cloud(e: &CloudError) -> Self {
        match e {
            CloudError::Unauthorized => Self {
                status: 401,
                code: "unauthorized".to_string(),
                detail: None,
            },
            CloudError::Forbidden(body) => Self::from_body(403, "forbidden", body),
            CloudError::Http { status, body } => Self::from_body(*status, "", body),
            CloudError::Transport(_) => Self {
                status: 0,
                code: "network".to_string(),
                detail: None,
            },
            CloudError::Decode(_) => Self {
                status: 0,
                code: "decode".to_string(),
                detail: None,
            },
            CloudError::LoginChallenge(_) => Self {
                status: 0,
                code: "login_challenge".to_string(),
                detail: None,
            },
        }
    }

    fn from_body(status: u16, fallback: &str, body: &str) -> Self {
        let parsed: Option<Value> = serde_json::from_str(body).ok();
        let code = parsed
            .as_ref()
            .and_then(|v| v.get("error"))
            .and_then(Value::as_str)
            .unwrap_or(fallback)
            .to_string();
        let detail = parsed
            .as_ref()
            .and_then(|v| v.get("detail"))
            .and_then(Value::as_str)
            .map(str::to_string);
        Self { status, code, detail }
    }
}

// =============================================================================
// Helpers
// =============================================================================

fn text_field(v: &Value, key: &str) -> String {
    match v.get(key) {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Number(n)) => n.to_string(),
        _ => String::new(),
    }
}

/// `charge_usd_micros`-style fields arrive as a decimal string (`"12000000"`)
/// or a bare number; anything else, including `null`, is `None`.
fn micros_opt<'de, D: Deserializer<'de>>(d: D) -> Result<Option<i64>, D::Error> {
    let raw = Option::<Value>::deserialize(d)?;
    Ok(match raw {
        Some(Value::Number(n)) => n.as_i64(),
        Some(Value::String(s)) => s.trim().parse::<i64>().ok(),
        _ => None,
    })
}

fn micros_or_zero<'de, D: Deserializer<'de>>(d: D) -> Result<i64, D::Error> {
    Ok(micros_opt(d)?.unwrap_or(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plans_decode_with_every_kind_and_the_onion_flag() {
        let body = r#"{
            "onion": false, "checkout_enabled": true,
            "plans": [
              {"id":"mlc-paid-account-v1","kind":"paid_account","interval":"one_time","price_usd":5,"tier":null,"quota":null,"onion_only":false},
              {"id":"mlc-anon-account-v1","kind":"anon_account","interval":"one_time","price_usd":10,"tier":null,"quota":null,"onion_only":false},
              {"id":"mlc-monthly-50-v1","kind":"quant","interval":"monthly","price_usd":50,"tier":2,"quota":300,"onion_only":false},
              {"id":"mlc-future-v1","kind":"something_new","interval":"weekly","price_usd":1}
            ]}"#;
        let plans: BillingPlans = serde_json::from_str(body).expect("plans decode");
        assert!(plans.checkout_enabled);
        assert!(!plans.onion);
        assert_eq!(plans.plans.len(), 4);
        assert_eq!(plans.plans[0].kind, BillingPlanKind::PaidAccount);
        assert_eq!(plans.plans[1].kind, BillingPlanKind::AnonAccount);
        assert_eq!(plans.plans[2].kind, BillingPlanKind::Quant);
        assert_eq!(plans.plans[2].interval, BillingInterval::Monthly);
        assert_eq!(plans.plans[2].tier, Some(2));
        assert_eq!(plans.plans[2].quota, Some(300));
        assert_eq!(plans.plans[3].kind, BillingPlanKind::Other, "an unknown kind must not fail the list");
        assert_eq!(plans.plans[3].interval, BillingInterval::Other);
    }

    #[test]
    fn me_decodes_credit_as_number_or_text_and_the_open_invoices() {
        let body = r#"{
            "paid": true, "paid_until": null, "lifetime_paid": false, "lifetime_anon": true,
            "quant": {"tier":"mlc-monthly-50-v1","active":true,"active_until":"2026-10-25T00:00:00+00:00","quota":300},
            "credit_usd_micros": "2500000",
            "subscription": {"subscription_id":"sub_1","plan_id":"mlc-monthly-50-v1","state":"active",
                             "active_until":"2026-10-25T00:00:00+00:00",
                             "pending_change":{"change_id":"chg_1","plan_id":"mlc-monthly-20-v1"}},
            "open_invoices": [{"invoice_id":"grp_1","plan_id":"mlc-monthly-100-v1","kind":"initial","expires_at":"2026-09-30T12:00:00+00:00"}]
        }"#;
        let me: BillingMe = serde_json::from_str(body).expect("me decode");
        assert!(me.paid && me.lifetime_anon && !me.lifetime_paid);
        assert_eq!(me.quant.tier.as_deref(), Some("mlc-monthly-50-v1"));
        assert!(me.quant.active);
        assert_eq!(me.credit_usd_micros, 2_500_000);
        let sub = me.subscription.expect("subscription");
        assert_eq!(sub.state, "active");
        assert_eq!(sub.pending_change.and_then(|c| c.plan_id).as_deref(), Some("mlc-monthly-20-v1"));
        assert_eq!(me.open_invoices.len(), 1);

        let bare: BillingMe = serde_json::from_str(r#"{"paid":false,"credit_usd_micros":0}"#).expect("bare me");
        assert_eq!(bare.credit_usd_micros, 0);
        assert!(bare.subscription.is_none());
        assert!(bare.open_invoices.is_empty());
    }

    #[test]
    fn checkout_keeps_pay_options_as_issued_and_the_optional_checkout_url() {
        let body = r#"{"invoice_id":"grp_9","plan_id":"mlc-monthly-20-v1",
            "checkout_url":"https://pay.example/v1/checkout/abc",
            "pay":[
              {"chain":"evm:1","asset":"USDC","address":"0xaa","amount":"20.000000","expires_at":"2026-09-30T12:00:00+00:00","qr_svg":"<svg/>"},
              {"chain":"tron","asset":"USDT","address":"TMock","amount":"20.000000","expires_at":"2026-09-30T12:00:00+00:00","qr_svg":"<svg/>"}
            ]}"#;
        let started: BillingCheckout = serde_json::from_str(body).expect("checkout decode");
        assert_eq!(started.pay.len(), 2);
        assert_eq!(started.pay[0].chain, "evm:1");
        assert_eq!(started.pay[1].asset, "USDT");
        assert_eq!(started.checkout_url.as_deref(), Some("https://pay.example/v1/checkout/abc"));

        let plain = r#"{"invoice_id":"grp_1","plan_id":"p","pay":[]}"#;
        let started: BillingCheckout = serde_json::from_str(plain).expect("plain checkout");
        assert!(started.checkout_url.is_none());
    }

    const CRED: &str = "b94a08ef8cf6f09519d070ee44d3118a7d6e0dcc27968fb6885cd6ff7fa16302";

    #[test]
    fn anon_checkout_answer_carries_no_key() {
        let body = r#"{"invoice_id":"grp_2","pay":[],"checkout_url":"https://pay.example/c"}"#;
        let started: BillingAnonCheckout = serde_json::from_str(body).expect("anon decode");
        assert_eq!(started.invoice_id, "grp_2");
        assert_eq!(started.checkout_url.as_deref(), Some("https://pay.example/c"));
    }

    #[test]
    fn only_the_credential_goes_into_request_bodies() {
        let anon: serde_json::Value = serde_json::from_str(&anon_checkout_body("plan", CRED).expect("anon")).expect("json");
        assert_eq!(anon, serde_json::json!({"plan_id": "plan", "credential": CRED}));
        let key: serde_json::Value = serde_json::from_str(&credential_body(CRED).expect("key")).expect("json");
        assert_eq!(key, serde_json::json!({"credential": CRED}));
        let invite: serde_json::Value =
            serde_json::from_str(&invite_redeem_body("CODE-1", CRED).expect("invite")).expect("json");
        assert_eq!(invite, serde_json::json!({"invite_code": "CODE-1", "credential": CRED}));
        assert_eq!(account_key_authorization(CRED).expect("header"), format!("AccountKey {CRED}"));
    }

    #[test]
    fn anything_that_is_not_a_credential_never_reaches_a_body() {
        // The displayed key `K` (Crockford groups), a short hex, a non-hex string.
        for bad in [
            "000G-40R4-0M30-E209-185G-R38E-1W81-24GK-2GAH-C5RR-34D1-P70X-3RFG-ZHQJ",
            &CRED[..63],
            &CRED.replace('b', "z"),
            "",
        ] {
            assert!(anon_checkout_body("p", bad).is_err(), "{bad:?}");
            assert!(credential_body(bad).is_err(), "{bad:?}");
            assert!(invite_redeem_body("c", bad).is_err(), "{bad:?}");
            assert!(account_key_authorization(bad).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn invoice_decodes_flags_and_partial_amounts() {
        let body = r#"{
            "invoice_id":"grp_3","plan_id":"mlc-paid-account-v1","kind":"initial","status":"new",
            "live_status":"processing","grant_state":"none","flags":["partial","late"],
            "detail":{"partial":{"asset":"USDT","chain":"tron","value":"2.000000","received":"2.000000","expected":"5.000000","shortfall":"3.000000"}},
            "period":null,"expires_at":"2026-09-30T12:00:00+00:00","pay":[]}"#;
        let inv: BillingInvoice = serde_json::from_str(body).expect("invoice decode");
        assert_eq!(inv.status, "new");
        assert_eq!(inv.live_status.as_deref(), Some("processing"));
        assert!(inv.has_flag("partial") && inv.has_flag("late") && !inv.has_flag("over"));
        let partial = inv.amount_note("partial").expect("partial note");
        assert_eq!(partial.received, "2.000000");
        assert_eq!(partial.expected, "5.000000");
        assert_eq!(partial.shortfall, "3.000000");
        assert_eq!(partial.asset, "USDT");
        assert!(inv.amount_note("over").is_none());
    }

    #[test]
    fn invoice_amounts_accept_numbers_and_a_missing_detail() {
        let body = r#"{"invoice_id":"g","plan_id":"p","status":"settled","grant_state":"granted",
            "flags":["over"],"detail":{"over":{"received":12.5,"expected":10}}}"#;
        let inv: BillingInvoice = serde_json::from_str(body).expect("invoice decode");
        let over = inv.amount_note("over").expect("over note");
        assert_eq!(over.received, "12.5");
        assert_eq!(over.expected, "10");

        let bare = r#"{"invoice_id":"g","plan_id":"p","status":"new"}"#;
        let inv: BillingInvoice = serde_json::from_str(bare).expect("bare invoice");
        assert!(inv.flags.is_empty());
        assert!(inv.amount_note("partial").is_none());
        assert!(inv.live_status.is_none());
    }

    #[test]
    fn change_decodes_the_upgrade_charge_from_text_and_the_downgrade_date() {
        let upgrade = r#"{"change_id":"chg_1","state":"pending","from_plan_id":"a","to_plan_id":"b",
            "proration":"charge_now","charge_usd_micros":"12000000","pending_effective_at":null,
            "invoice_id":"grp_7","pay":[{"chain":"evm:1","asset":"USDC","address":"0x1","amount":"12.000000"}]}"#;
        let change: BillingChange = serde_json::from_str(upgrade).expect("upgrade decode");
        assert_eq!(change.proration, "charge_now");
        assert_eq!(change.charge_usd_micros, Some(12_000_000));
        assert_eq!(change.invoice_id.as_deref(), Some("grp_7"));
        assert_eq!(change.pay.len(), 1);

        let downgrade = r#"{"change_id":"chg_2","state":"pending","proration":"none","charge_usd_micros":null,
            "pending_effective_at":"2026-10-25T00:00:00+00:00","invoice_id":null,"pay":[]}"#;
        let change: BillingChange = serde_json::from_str(downgrade).expect("downgrade decode");
        assert_eq!(change.charge_usd_micros, None);
        assert_eq!(change.pending_effective_at.as_deref(), Some("2026-10-25T00:00:00+00:00"));
        assert!(change.invoice_id.is_none());
    }

    #[test]
    fn account_key_session_decodes_and_redacts_the_session_id() {
        let body = r#"{"status":"confirmed","user_id":42,"public_id":"abc","session_id":"sess-secret"}"#;
        let session: AccountKeySession = serde_json::from_str(body).expect("session decode");
        assert_eq!(session.status, "confirmed");
        assert_eq!(session.user_id, 42);
        assert!(!format!("{session:?}").contains("sess-secret"));
    }

    #[test]
    fn error_codes_survive_the_transport_error_types() {
        let conflict = CloudError::Http { status: 409, body: r#"{"error":"already_owned"}"#.to_string() };
        let e = BillingError::from_cloud(&conflict);
        assert_eq!((e.status, e.code.as_str()), (409, "already_owned"));

        let refused = CloudError::Http {
            status: 400,
            body: r#"{"error":"gateway_refused","detail":"plan cannot change"}"#.to_string(),
        };
        let e = BillingError::from_cloud(&refused);
        assert_eq!(e.code, "gateway_refused");
        assert_eq!(e.detail.as_deref(), Some("plan cannot change"));

        let unavailable = CloudError::Http { status: 503, body: r#"{"error":"plan_unavailable"}"#.to_string() };
        assert_eq!(BillingError::from_cloud(&unavailable).code, "plan_unavailable");

        let forbidden = CloudError::Forbidden(r#"{"error":"not_an_anonymous_account"}"#.to_string());
        let e = BillingError::from_cloud(&forbidden);
        assert_eq!((e.status, e.code.as_str()), (403, "not_an_anonymous_account"));

        let html = CloudError::Http { status: 502, body: "<html>Bad gateway</html>".to_string() };
        let e = BillingError::from_cloud(&html);
        assert_eq!((e.status, e.code.as_str()), (502, ""));

        assert_eq!(BillingError::from_cloud(&CloudError::Unauthorized).code, "unauthorized");
        assert_eq!(BillingError::from_cloud(&CloudError::Transport("offline".into())).code, "network");
        assert_eq!(BillingError::from_cloud(&CloudError::Decode("x".into())).code, "decode");
    }
}
