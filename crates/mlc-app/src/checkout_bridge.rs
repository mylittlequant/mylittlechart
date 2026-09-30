//! Billing wire models -> checkout modal events.
//!
//! The transport (web `fetch` today, a native client later) hands back
//! `mlc_cloud` billing models; the modal speaks in `mlc-app-layout`'s
//! [`CheckoutEvent`]s. This file is the one place the two meet, so every shell
//! maps an answer the same way and the mapping is tested without a browser.
//!
//! `pay[]` is carried over exactly as the server returned it: same order, no
//! filtering, no defaults. The QR is read from the processor's own `qr_svg`.

use mlc_app_layout::ui::checkout::{
    parse_rfc3339_ms, AmountUi, CheckoutError, CheckoutEvent, CheckoutOp, HoldingsUi, InvoiceReading, PayOptionUi,
    PlanKind, PlanUi,
};
use mlc_app_layout::ui::qr_svg::QrMatrix;
use mlc_cloud::{
    AmountNote, BillingAnonCheckout, BillingChange, BillingCheckout, BillingError, BillingInterval, BillingInvoice,
    BillingMe, BillingPlanKind, BillingPlans, CloudError, PayOption,
};

/// `GET /api/billing/plans`. A plan kind this client does not know is left
/// out rather than drawn as something it is not.
pub fn plans_event(plans: BillingPlans) -> CheckoutEvent {
    let mapped = plans
        .plans
        .into_iter()
        .filter_map(|p| {
            let kind = match p.kind {
                BillingPlanKind::PaidAccount => PlanKind::PaidAccount,
                BillingPlanKind::AnonAccount => PlanKind::AnonAccount,
                BillingPlanKind::Quant => PlanKind::Quant,
                BillingPlanKind::Other => return None,
            };
            Some(PlanUi {
                id: p.id,
                kind,
                monthly: p.interval == BillingInterval::Monthly,
                price_usd: p.price_usd,
                tier: p.tier,
                quota: p.quota,
                onion_only: p.onion_only,
            })
        })
        .collect();
    CheckoutEvent::Plans { plans: mapped, onion: plans.onion, enabled: plans.checkout_enabled }
}

/// `GET /api/billing/me`: what the account already holds.
pub fn holdings_event(me: &BillingMe) -> CheckoutEvent {
    CheckoutEvent::Holdings(HoldingsUi {
        lifetime_paid: me.lifetime_paid,
        lifetime_anon: me.lifetime_anon,
        quant_plan_id: me.quant.tier.clone().filter(|_| me.quant.active),
        pending_plan_id: me
            .subscription
            .as_ref()
            .and_then(|s| s.pending_change.as_ref())
            .and_then(|c| c.plan_id.clone()),
    })
}

/// `pay[]`, in the order the server returned it.
pub fn pay_options(pay: &[PayOption]) -> Vec<PayOptionUi> {
    pay.iter()
        .map(|o| PayOptionUi {
            chain: o.chain.clone(),
            asset: o.asset.clone(),
            address: o.address.clone(),
            amount: o.amount.clone(),
            expires_at_ms: parse_rfc3339_ms(&o.expires_at),
            qr: QrMatrix::parse(&o.qr_svg),
        })
        .collect()
}

/// `POST /api/billing/checkout`.
pub fn started_event(c: &BillingCheckout) -> CheckoutEvent {
    CheckoutEvent::Started {
        invoice_id: c.invoice_id.clone(),
        plan_id: c.plan_id.clone(),
        pay: pay_options(&c.pay),
        account_key: None,
        charge_usd_micros: None,
    }
}

/// `POST /api/billing/anon-checkout`. The plan id is the one asked for (the
/// answer does not repeat it).
pub fn anon_started_event(plan_id: &str, c: BillingAnonCheckout) -> CheckoutEvent {
    CheckoutEvent::Started {
        invoice_id: c.invoice_id,
        plan_id: plan_id.to_string(),
        pay: pay_options(&c.pay),
        account_key: Some(c.account_key),
        charge_usd_micros: None,
    }
}

/// `POST /api/billing/change`: an upgrade with a charge opens the pay step, a
/// downgrade is queued for the renewal, an upgrade the credit covered is
/// already applied.
pub fn change_event(c: &BillingChange) -> CheckoutEvent {
    if c.state == "applied" {
        return CheckoutEvent::ChangeApplied;
    }
    match (&c.invoice_id, c.pay.is_empty()) {
        (Some(invoice_id), false) => CheckoutEvent::Started {
            invoice_id: invoice_id.clone(),
            plan_id: c.to_plan_id.clone().unwrap_or_default(),
            pay: pay_options(&c.pay),
            account_key: None,
            charge_usd_micros: c.charge_usd_micros,
        },
        _ => CheckoutEvent::ChangeScheduled { effective: c.pending_effective_at.clone() },
    }
}

fn amount(note: AmountNote) -> AmountUi {
    AmountUi { asset: note.asset, received: note.received, expected: note.expected, shortfall: note.shortfall }
}

/// `GET /api/billing/invoice/{id}` or `anon-invoice/{id}`.
pub fn reading(inv: &BillingInvoice) -> InvoiceReading {
    InvoiceReading {
        invoice_id: inv.invoice_id.clone(),
        status: inv.status.clone(),
        live_status: inv.live_status.clone(),
        grant_state: inv.grant_state.clone(),
        flags: inv.flags.clone(),
        partial: inv.amount_note("partial").map(amount),
        over: inv.amount_note("over").map(amount),
        expires_at_ms: inv.expires_at.as_deref().and_then(parse_rfc3339_ms),
    }
}

/// A failed request as an event. The account-key sign-in reads a `401` as "the
/// key was not accepted" (every refusal of a key is the same `401`); anywhere
/// else a `401` means "sign in".
pub fn failure_event(op: CheckoutOp, e: &CloudError) -> CheckoutEvent {
    let b = BillingError::from_cloud(e);
    let error = if op == CheckoutOp::SignIn && b.status == 401 {
        CheckoutError::InvalidKey
    } else {
        CheckoutError::from_wire(b.status, &b.code, b.detail.as_deref())
    };
    CheckoutEvent::Failed { op, error }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode<T: serde::de::DeserializeOwned>(json: &str) -> T {
        serde_json::from_str(json).expect("fixture decodes")
    }

    const QR: &str = concat!(
        r##"<svg viewBox="0 0 5 5"><rect width="5" height="5" fill="#fff"/>"##,
        r##"<path fill="#000" d="M2 1h1v1H2V1M1 2h1v1H1V2M2 2h1v1H2V2M3 2h1v1H3V2M2 3h1v1H2V3"/></svg>"##
    );

    fn checkout_json(qr: &str) -> String {
        format!(
            r#"{{"invoice_id":"grp_1","plan_id":"mlc-paid-account-v1","pay":[
              {{"chain":"tron","asset":"USDT","address":"TMock","amount":"5.000000","expires_at":"2026-09-30T12:10:00+00:00","qr_svg":{qr:?}}},
              {{"chain":"evm:1","asset":"USDC","address":"0xaa","amount":"5.000000","expires_at":"2026-09-30T12:10:00+00:00","qr_svg":"<svg/>"}}
            ]}}"#
        )
    }

    #[test]
    fn plans_keep_their_facts_and_drop_a_kind_this_client_does_not_know() {
        let plans: BillingPlans = decode(
            r#"{"onion":true,"checkout_enabled":true,"plans":[
              {"id":"paid-tor","kind":"paid_account","interval":"one_time","price_usd":10,"onion_only":true},
              {"id":"q50","kind":"quant","interval":"monthly","price_usd":50,"tier":2,"quota":300},
              {"id":"future","kind":"gift_card","interval":"one_time","price_usd":1}
            ]}"#,
        );
        let CheckoutEvent::Plans { plans, onion, enabled } = plans_event(plans) else { panic!("plans event") };
        assert!(onion && enabled);
        assert_eq!(plans.len(), 2, "the unknown kind is not drawn");
        assert_eq!((plans[0].kind, plans[0].onion_only, plans[0].monthly), (PlanKind::PaidAccount, true, false));
        assert_eq!((plans[1].kind, plans[1].tier, plans[1].quota, plans[1].monthly), (PlanKind::Quant, Some(2), Some(300), true));
    }

    #[test]
    fn holdings_read_the_running_tier_only_while_it_is_active() {
        let me: BillingMe = decode(
            r#"{"lifetime_paid":true,"quant":{"tier":"q50","active":true},
                "subscription":{"subscription_id":"s","plan_id":"q50","state":"active",
                                "pending_change":{"change_id":"c","plan_id":"q20"}}}"#,
        );
        let CheckoutEvent::Holdings(h) = holdings_event(&me) else { panic!("holdings event") };
        assert!(h.lifetime_paid && !h.lifetime_anon);
        assert_eq!(h.quant_plan_id.as_deref(), Some("q50"));
        assert_eq!(h.pending_plan_id.as_deref(), Some("q20"));

        let lapsed: BillingMe = decode(r#"{"quant":{"tier":"q50","active":false}}"#);
        let CheckoutEvent::Holdings(h) = holdings_event(&lapsed) else { panic!("holdings event") };
        assert_eq!(h.quant_plan_id, None, "a lapsed tier is not a running tier");
    }

    #[test]
    fn pay_options_keep_the_servers_order_expiry_and_qr() {
        let c: BillingCheckout = decode(&checkout_json(QR));
        let CheckoutEvent::Started { pay, account_key, charge_usd_micros, invoice_id, .. } = started_event(&c) else {
            panic!("started event")
        };
        assert_eq!(invoice_id, "grp_1");
        assert!(account_key.is_none() && charge_usd_micros.is_none());
        let chips: Vec<(&str, &str)> = pay.iter().map(|o| (o.chain.as_str(), o.asset.as_str())).collect();
        assert_eq!(chips, [("tron", "USDT"), ("evm:1", "USDC")], "no reordering, no filtering");
        assert_eq!(pay[0].expires_at_ms, Some(1_790_770_200_000));
        assert!(pay[0].qr.is_some(), "the processor's QR is read");
        assert!(pay[1].qr.is_none(), "an unreadable QR degrades to the address alone");
    }

    #[test]
    fn the_anonymous_answer_carries_the_key_and_the_plan_asked_for() {
        let c: BillingAnonCheckout = decode(r#"{"account_key":"0F4Z-KEY","invoice_id":"grp_2","pay":[]}"#);
        let CheckoutEvent::Started { account_key, plan_id, invoice_id, .. } = anon_started_event("mlc-anon-account-v1", c)
        else {
            panic!("started event")
        };
        assert_eq!(account_key.as_deref(), Some("0F4Z-KEY"));
        assert_eq!((plan_id.as_str(), invoice_id.as_str()), ("mlc-anon-account-v1", "grp_2"));
    }

    #[test]
    fn an_upgrade_opens_the_pay_step_with_its_charge_and_a_downgrade_is_queued() {
        let upgrade: BillingChange = decode(
            r#"{"change_id":"c1","state":"pending","to_plan_id":"q50","proration":"charge_now",
                "charge_usd_micros":"12000000","invoice_id":"grp_7",
                "pay":[{"chain":"evm:1","asset":"USDC","address":"0x1","amount":"12.000000"}]}"#,
        );
        let CheckoutEvent::Started { charge_usd_micros, plan_id, pay, .. } = change_event(&upgrade) else {
            panic!("started event")
        };
        assert_eq!((charge_usd_micros, plan_id.as_str(), pay.len()), (Some(12_000_000), "q50", 1));

        let downgrade: BillingChange = decode(
            r#"{"change_id":"c2","state":"pending","proration":"none",
                "pending_effective_at":"2026-10-25T00:00:00+00:00","invoice_id":null,"pay":[]}"#,
        );
        assert_eq!(
            change_event(&downgrade),
            CheckoutEvent::ChangeScheduled { effective: Some("2026-10-25T00:00:00+00:00".into()) }
        );

        let applied: BillingChange = decode(r#"{"change_id":"c3","state":"applied","proration":"charge_now","pay":[]}"#);
        assert_eq!(change_event(&applied), CheckoutEvent::ChangeApplied);
    }

    #[test]
    fn a_reading_carries_the_flags_and_the_amounts_of_a_partial_payment() {
        let inv: BillingInvoice = decode(
            r#"{"invoice_id":"grp_3","plan_id":"p","status":"new","live_status":"processing","grant_state":"none",
                "flags":["partial"],"expires_at":"2026-09-30T12:10:00+00:00",
                "detail":{"partial":{"asset":"USDT","received":"2.000000","expected":"5.000000","shortfall":"3.000000"}}}"#,
        );
        let r = reading(&inv);
        assert_eq!((r.invoice_id.as_str(), r.status.as_str(), r.live_status.as_deref()), ("grp_3", "new", Some("processing")));
        assert_eq!(r.partial.as_ref().map(|a| a.shortfall.as_str()), Some("3.000000"));
        assert_eq!(r.expires_at_ms, Some(1_790_770_200_000));
        assert_eq!(r.notes().len(), 1);
    }

    #[test]
    fn transport_errors_become_the_modals_error_codes() {
        let conflict = CloudError::Http { status: 409, body: r#"{"error":"already_owned"}"#.into() };
        assert_eq!(
            failure_event(CheckoutOp::Start, &conflict),
            CheckoutEvent::Failed { op: CheckoutOp::Start, error: CheckoutError::AlreadyOwned }
        );
        let unavailable = CloudError::Http { status: 503, body: r#"{"error":"plan_unavailable"}"#.into() };
        assert_eq!(
            failure_event(CheckoutOp::Start, &unavailable),
            CheckoutEvent::Failed { op: CheckoutOp::Start, error: CheckoutError::PlanUnavailable }
        );
        let gateway = CloudError::Http { status: 502, body: r#"{"error":"payment_gateway_unavailable"}"#.into() };
        assert_eq!(
            failure_event(CheckoutOp::Start, &gateway),
            CheckoutEvent::Failed { op: CheckoutOp::Start, error: CheckoutError::Gateway }
        );
        assert_eq!(
            failure_event(CheckoutOp::LoadPlans, &CloudError::Transport("offline".into())),
            CheckoutEvent::Failed { op: CheckoutOp::LoadPlans, error: CheckoutError::Network }
        );
        assert_eq!(
            failure_event(CheckoutOp::Start, &CloudError::Unauthorized),
            CheckoutEvent::Failed { op: CheckoutOp::Start, error: CheckoutError::SignInRequired }
        );
    }

    #[test]
    fn a_refused_account_key_is_an_invalid_key_not_a_sign_in_prompt() {
        assert_eq!(
            failure_event(CheckoutOp::SignIn, &CloudError::Unauthorized),
            CheckoutEvent::Failed { op: CheckoutOp::SignIn, error: CheckoutError::InvalidKey }
        );
        let limited = CloudError::Http { status: 429, body: r#"{"error":"rate_limited"}"#.into() };
        assert_eq!(
            failure_event(CheckoutOp::SignIn, &limited),
            CheckoutEvent::Failed { op: CheckoutOp::SignIn, error: CheckoutError::RateLimited }
        );
    }
}
