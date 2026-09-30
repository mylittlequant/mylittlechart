//! Checkout driver (cloud builds only).
//!
//! The modal's state machine lives in `mlc-app-layout`; `mlc-app` exposes it
//! to a shell as `ChartApp::checkout_*`. This file is the web half of that
//! seam: once a frame it hands the modal its answers, asks it what to send
//! next, and sends it with the shared `fetch` transport. The mapping between
//! the wire models and the modal's events is `mlc_app::checkout_bridge`, so
//! nothing here decides how an answer reads.
//!
//! One request runs at a time per command; its answers land in a mailbox and
//! are applied on the next frame (the same pattern the rest of this driver
//! uses). The browser talks to the MLC server only.
//!
//! Signing in with an account key: `POST /api/auth/account-key` mints a cookie
//! session, which is turned into the bearer token every other request uses by
//! the existing device-code confirmation (`create code -> confirm -> poll`).
//! The poll's `Confirmed` answer is queued as the ordinary sign-in result, so
//! the token, the profile link and the "Signed in" event all take the path any
//! other sign-in takes.

use std::cell::RefCell;
use std::rc::Rc;

use wasm_bindgen_futures::spawn_local;

use mlc_app::checkout_bridge as bridge;
use mlc_app::ChartApp;
use mlc_app_layout::ui::checkout::{CheckoutCmd, CheckoutEvent, CheckoutOp};
use mlc_cloud::{CloudError, DeviceAuthPoll, FetchTransport};

use super::{mark_unauthorized, CloudSession, FlowResult, Inner};

impl CloudSession {
    /// Per-frame checkout step: apply answers, advance the modal's clock,
    /// refresh what a payment changed, send the next request.
    pub(super) fn drive_checkout(&self, chart: &mut ChartApp, now_ms: f64) {
        let now = now_ms as u64;
        let events: Vec<CheckoutEvent> = self.inner.borrow_mut().checkout_events.drain(..).collect();
        for event in events {
            chart.checkout_apply(event, now);
        }
        chart.checkout_tick(self.is_signed_in(), now);
        if chart.checkout_take_entitlements_dirty() {
            // A payment landed: the paid answer is stale, ask again now.
            self.inner.borrow_mut().next_entitlements_ms = 0.0;
        }
        if let Some(cmd) = chart.checkout_take_cmd(now) {
            self.fire_checkout(cmd);
        }
    }

    fn fire_checkout(&self, cmd: CheckoutCmd) {
        let inner = self.inner.clone();
        let transport = self.inner.borrow().transport.snapshot();
        let signed_in = self.is_signed_in();
        spawn_local(async move {
            let events = run_cmd(&transport, &inner, cmd, signed_in).await;
            inner.borrow_mut().checkout_events.extend(events);
        });
    }
}

/// Send one command and return the events its answer produces.
async fn run_cmd(
    transport: &FetchTransport,
    inner: &Rc<RefCell<Inner>>,
    cmd: CheckoutCmd,
    signed_in: bool,
) -> Vec<CheckoutEvent> {
    match cmd {
        CheckoutCmd::LoadPlans => load_plans(transport, inner, signed_in).await,
        CheckoutCmd::Checkout { plan_id } => match transport.billing_checkout(&plan_id).await {
            Ok(started) => vec![bridge::started_event(&started)],
            Err(e) => {
                mark_unauthorized(inner, &e);
                vec![bridge::failure_event(CheckoutOp::Start, &e)]
            }
        },
        CheckoutCmd::Change { plan_id } => match transport.billing_change(&plan_id).await {
            Ok(change) => vec![bridge::change_event(&change)],
            Err(e) => {
                mark_unauthorized(inner, &e);
                vec![bridge::failure_event(CheckoutOp::Start, &e)]
            }
        },
        // Public: someone with no account yet buys one. The account key is
        // drawn here, on this device; only the credential derived from it goes
        // out, and the key itself goes to the modal to be shown once.
        CheckoutCmd::AnonCheckout { plan_id } => {
            let key = mlc_vault::account_key::AccountKey::generate();
            match transport.billing_anon_checkout(&plan_id, &key.credential_hex()).await {
                Ok(started) => vec![bridge::anon_started_event(&plan_id, key.display(), started)],
                Err(e) => vec![bridge::failure_event(CheckoutOp::Start, &e)],
            }
        }
        CheckoutCmd::PollInvoice { invoice_id, account_key } => {
            let answer = match account_key.as_deref() {
                // A pending anonymous account reads its own invoice with the
                // credential derived from its key.
                Some(typed) => match bridge::key_material(typed) {
                    Ok(material) => transport.billing_anon_invoice(&invoice_id, &material.credential).await,
                    Err(error) => return vec![CheckoutEvent::Failed { op: CheckoutOp::Poll, error }],
                },
                None => transport.billing_invoice(&invoice_id).await,
            };
            match answer {
                Ok(invoice) => vec![CheckoutEvent::Invoice(bridge::reading(&invoice))],
                Err(e) => {
                    if account_key.is_none() {
                        mark_unauthorized(inner, &e);
                    }
                    vec![bridge::failure_event(CheckoutOp::Poll, &e)]
                }
            }
        }
        CheckoutCmd::SignInWithKey { account_key } => sign_in_with_key(transport, inner, &account_key).await,
        CheckoutCmd::RedeemInvite { invite_code, account_key } => {
            redeem_invite(transport, inner, &invite_code, &account_key).await
        }
    }
}

/// The plan list, then (signed in) what the account already holds.
async fn load_plans(transport: &FetchTransport, inner: &Rc<RefCell<Inner>>, signed_in: bool) -> Vec<CheckoutEvent> {
    let plans = match transport.billing_plans().await {
        Ok(plans) => plans,
        Err(e) => {
            mark_unauthorized(inner, &e);
            return vec![bridge::failure_event(CheckoutOp::LoadPlans, &e)];
        }
    };
    let mut events = vec![bridge::plans_event(plans)];
    if signed_in {
        match transport.billing_me().await {
            Ok(me) => events.push(bridge::holdings_event(&me)),
            Err(e) => {
                mark_unauthorized(inner, &e);
                events.push(bridge::failure_event(CheckoutOp::LoadPlans, &e));
            }
        }
    }
    events
}

/// Key -> credential -> cookie session -> device code -> confirm -> poll ->
/// bearer token. Only the credential derived from the typed key is sent; the
/// vault key derived beside it is kept for the app.
async fn sign_in_with_key(transport: &FetchTransport, inner: &Rc<RefCell<Inner>>, typed_key: &str) -> Vec<CheckoutEvent> {
    let material = match bridge::key_material(typed_key) {
        Ok(material) => material,
        Err(error) => return vec![CheckoutEvent::Failed { op: CheckoutOp::SignIn, error }],
    };
    if let Err(e) = transport.account_key_sign_in(&material.credential).await {
        return vec![bridge::failure_event(CheckoutOp::SignIn, &e)];
    }
    finish_sign_in(transport, inner, material.vault_key).await
}

/// Invite code + the new key's credential -> new key account and cookie
/// session -> the same device-code confirmation as any key sign-in.
async fn redeem_invite(
    transport: &FetchTransport,
    inner: &Rc<RefCell<Inner>>,
    invite_code: &str,
    typed_key: &str,
) -> Vec<CheckoutEvent> {
    let material = match bridge::key_material(typed_key) {
        Ok(material) => material,
        Err(error) => return vec![CheckoutEvent::Failed { op: CheckoutOp::SignIn, error }],
    };
    if let Err(e) = transport.invite_redeem(invite_code, &material.credential).await {
        return vec![bridge::invite_failure_event(&e)];
    }
    finish_sign_in(transport, inner, material.vault_key).await
}

/// The cookie session exists: turn it into the bearer token every other
/// request uses (`create code -> confirm -> poll`), queue that as the ordinary
/// sign-in result, and hand the vault key to the app.
async fn finish_sign_in(
    transport: &FetchTransport,
    inner: &Rc<RefCell<Inner>>,
    vault_key: mlc_vault::crypto::VaultKey,
) -> Vec<CheckoutEvent> {
    let confirmed = async {
        let code = transport.device_code_create().await?;
        transport.device_code_confirm(&code.code).await?;
        transport.device_code_poll(&code.code).await
    }
    .await;
    match confirmed {
        Ok(poll @ DeviceAuthPoll::Confirmed { .. }) => {
            let mut state = inner.borrow_mut();
            state.pending_vault_key = Some(zeroize::Zeroizing::new(vault_key));
            state.results.push_back(FlowResult::Poll(Ok(poll)));
            vec![CheckoutEvent::SignedIn]
        }
        Ok(_) => vec![bridge::failure_event(
            CheckoutOp::SignIn,
            &CloudError::Decode("the device code was not confirmed".to_string()),
        )],
        Err(e) => vec![bridge::failure_event(CheckoutOp::SignIn, &e)],
    }
}
