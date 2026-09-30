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
        // Public: someone with no account yet buys one. No credential goes out.
        CheckoutCmd::AnonCheckout { plan_id } => match transport.billing_anon_checkout(&plan_id).await {
            Ok(started) => vec![bridge::anon_started_event(&plan_id, started)],
            Err(e) => vec![bridge::failure_event(CheckoutOp::Start, &e)],
        },
        CheckoutCmd::PollInvoice { invoice_id, account_key } => {
            let answer = match account_key.as_deref() {
                // A pending anonymous account reads its own invoice with its key.
                Some(key) => transport.billing_anon_invoice(&invoice_id, key).await,
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

/// Key -> cookie session -> device code -> confirm -> poll -> bearer token.
async fn sign_in_with_key(transport: &FetchTransport, inner: &Rc<RefCell<Inner>>, account_key: &str) -> Vec<CheckoutEvent> {
    let confirmed = async {
        transport.account_key_sign_in(account_key).await?;
        let code = transport.device_code_create().await?;
        transport.device_code_confirm(&code.code).await?;
        transport.device_code_poll(&code.code).await
    }
    .await;
    match confirmed {
        Ok(poll @ DeviceAuthPoll::Confirmed { .. }) => {
            inner.borrow_mut().results.push_back(FlowResult::Poll(Ok(poll)));
            vec![CheckoutEvent::SignedIn]
        }
        Ok(_) => vec![bridge::failure_event(
            CheckoutOp::SignIn,
            &CloudError::Decode("the device code was not confirmed".to_string()),
        )],
        Err(e) => vec![bridge::failure_event(CheckoutOp::SignIn, &e)],
    }
}
