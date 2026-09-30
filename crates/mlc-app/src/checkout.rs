//! Checkout modal — the `ChartApp` side.
//!
//! The pure state machine and the drawing live in `mlc-app-layout`
//! (`ui::checkout`, `layout::modals::checkout`). This file is the seam a shell
//! drives:
//!
//! * [`ChartApp::open_checkout`] — the entry-point buttons call it (through
//!   the `checkout:open` widget id);
//! * [`ChartApp::checkout_tick`] once per frame, [`ChartApp::checkout_take_cmd`]
//!   for the next request to send, [`ChartApp::checkout_apply`] for its answer;
//! * [`ChartApp::checkout_take_entitlements_dirty`] — a payment landed, ask the
//!   entitlements again.
//!
//! A shell never writes the modal's fields itself; it goes through these
//! methods. The modal's own clicks arrive here from `dispatch_panel_click`.

use mlc_app_layout::ui::checkout::{
    entry_points_enabled, CheckoutCmd, CheckoutEffect, CheckoutEvent, CheckoutStep,
};
use mlc_app_layout::ui::modal_settings::UserSettingsTab;
use uzor::WidgetId;

use crate::ChartApp;

/// Wall clock in unix milliseconds (`web_time` works on every target).
fn now_ms() -> u64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

impl ChartApp {
    /// Open the checkout modal on the plans. Refused (logged) while the shell
    /// has no checkout driver, so a shell without one never opens a modal that
    /// would sit on "Loading plans" forever.
    pub fn open_checkout(&mut self) {
        if !entry_points_enabled() {
            log::warn!("[ChartApp] open_checkout refused: this shell has no checkout driver");
            return;
        }
        // The sign-in overlay sits above the modal; whoever came from it wants
        // the checkout, not both.
        #[cfg(feature = "cloud")]
        self.panel_app.cloud_login_state.close();
        let signed_in = self.panel_app.user_settings_state.account_signed_in;
        self.panel_app.checkout_state.open(signed_in);
    }

    /// Whether the checkout modal is open.
    pub fn checkout_is_open(&self) -> bool {
        self.panel_app.checkout_state.is_open
    }

    /// Per-frame: mirror the account, advance the clock (closing a finished
    /// purchase after its success hold).
    pub fn checkout_tick(&mut self, signed_in: bool, now_ms: u64) {
        let st = &mut self.panel_app.checkout_state;
        st.set_signed_in(signed_in);
        st.tick(now_ms);
        if !(st.is_open && st.step == CheckoutStep::KeyEntry) {
            self.release_key_field();
        }
    }

    /// The account-key box is a credential's hiding place: whenever the
    /// key-entry page is not up it gives back the keyboard and forgets its
    /// text.
    fn release_key_field(&mut self) {
        let wid = WidgetId::from(crate::text_input::CHECKOUT_KEY_INPUT);
        let mut coord = self.input_coordinator.borrow_mut();
        if coord.text_fields().focused() == Some(&wid) {
            coord.text_fields_mut().blur();
        }
        if !coord.text_fields().text(&wid).is_empty() {
            coord.text_fields_mut().clear(&wid);
        }
    }

    /// The next request the shell should send, if any.
    pub fn checkout_take_cmd(&mut self, now_ms: u64) -> Option<CheckoutCmd> {
        self.panel_app.checkout_state.take_cmd(now_ms)
    }

    /// Hand back the answer to a request taken with [`Self::checkout_take_cmd`].
    pub fn checkout_apply(&mut self, event: CheckoutEvent, now_ms: u64) {
        self.panel_app.checkout_state.apply(event, now_ms);
    }

    /// Whether a payment landed since the last call (one-shot): the shell
    /// re-asks the entitlements and the account view.
    pub fn checkout_take_entitlements_dirty(&mut self) -> bool {
        let dirty = self.panel_app.checkout_state.take_entitlements_dirty();
        // The tier / role line reads the forum identity answer: ask it again.
        #[cfg(feature = "cloud")]
        if dirty {
            self.panel_app.community_state.forum2.invalidate_me();
        }
        dirty
    }

    /// A click on a widget the checkout modal (or an entry point) registered;
    /// `id` is the part after `checkout:`.
    pub(crate) fn on_checkout_click(&mut self, id: &str, x: f64, y: f64) {
        if id == "open" {
            self.open_checkout();
            return;
        }
        if id == "key_input" {
            // Click-to-caret in the account-key box.
            self.input_coordinator.borrow_mut().text_fields_mut().on_drag_start(x, y);
            return;
        }
        match self.panel_app.checkout_state.click(id, now_ms()) {
            CheckoutEffect::None => {}
            CheckoutEffect::Copy(text) => self.clipboard_text = Some(text),
            CheckoutEffect::OpenRedeem => self.open_redeem_field(),
            CheckoutEffect::FocusKeyField => {
                let wid = WidgetId::from(crate::text_input::CHECKOUT_KEY_INPUT);
                self.input_coordinator.borrow_mut().text_fields_mut().focus(wid);
            }
            CheckoutEffect::SubmitKey => self.checkout_submit_key(),
        }
    }

    /// Sign in with the key typed in the account-key box.
    pub(crate) fn checkout_submit_key(&mut self) {
        let wid = WidgetId::from(crate::text_input::CHECKOUT_KEY_INPUT);
        let typed = self.input_coordinator.borrow().text_fields().text(&wid).to_string();
        self.panel_app.checkout_state.submit_key(&typed);
    }

    /// Escape in the account-key box.
    pub(crate) fn checkout_dismiss(&mut self) {
        self.panel_app.checkout_state.dismiss();
    }

    /// "Have a code?": leave the checkout for the existing gift-code field on
    /// Settings → Account. Signed out, the sign-in overlay opens instead (a
    /// code is spent on an account).
    pub(crate) fn open_redeem_field(&mut self) {
        self.panel_app.checkout_state.close();
        if !self.panel_app.user_settings_state.account_signed_in {
            #[cfg(feature = "cloud")]
            self.panel_app.cloud_login_state.open();
            return;
        }
        self.panel_app.user_settings_state.open();
        self.panel_app.user_settings_state.set_tab(UserSettingsTab::Account);
        let wid = uzor::WidgetId::from(crate::text_input::REDEEM_CODE);
        self.input_coordinator.borrow_mut().text_fields_mut().focus(wid);
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use mlc_app_layout::ui::checkout::{
        set_entry_points_enabled, AmountUi, CheckoutError, CheckoutOp, CheckoutStep, HoldingsUi, InvoicePhase,
        InvoiceReading, PayOptionUi, PlanKind, PlanUi, SignInPhase, POLL_INTERVAL_MS, SUCCESS_HOLD_MS,
    };
    use mlc_backend_data_bridge::DataBridge;

    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    async fn app() -> ChartApp {
        let (bridge, _live_rx, _ready_rx) = DataBridge::new().await;
        let core = mlc_backend::BackendCore::new(Arc::new(bridge), None);
        ChartApp::new_empty(core)
    }

    fn plan(id: &str, kind: PlanKind, price: u32) -> PlanUi {
        PlanUi {
            id: id.to_string(),
            kind,
            monthly: kind == PlanKind::Quant,
            price_usd: price,
            tier: None,
            quota: None,
            onion_only: false,
        }
    }

    fn pay() -> Vec<PayOptionUi> {
        vec![
            PayOptionUi {
                chain: "evm:1".into(),
                asset: "USDC".into(),
                address: "0xaa".into(),
                amount: "5.000000".into(),
                expires_at_ms: Some(NOW as i64 + 600_000),
                qr: None,
            },
            PayOptionUi {
                chain: "tron".into(),
                asset: "USDT".into(),
                address: "TMock".into(),
                amount: "5.000000".into(),
                expires_at_ms: Some(NOW as i64 + 600_000),
                qr: None,
            },
        ]
    }

    /// An app with the checkout open on a loaded plan list.
    async fn opened(signed_in: bool) -> ChartApp {
        set_entry_points_enabled(true);
        let mut app = app().await;
        app.panel_app.user_settings_state.account_signed_in = signed_in;
        app.open_checkout();
        assert_eq!(app.checkout_take_cmd(NOW), Some(CheckoutCmd::LoadPlans));
        app.checkout_apply(
            CheckoutEvent::Plans {
                plans: vec![
                    plan("paid", PlanKind::PaidAccount, 5),
                    plan("anon", PlanKind::AnonAccount, 10),
                    plan("q20", PlanKind::Quant, 20),
                ],
                onion: false,
                enabled: true,
            },
            NOW,
        );
        app
    }

    fn started(plan_id: &str, key: Option<&str>) -> CheckoutEvent {
        CheckoutEvent::Started {
            invoice_id: "grp_1".into(),
            plan_id: plan_id.into(),
            pay: pay(),
            account_key: key.map(str::to_string),
            charge_usd_micros: None,
        }
    }

    fn reading(status: &str, live: &str) -> CheckoutEvent {
        CheckoutEvent::Invoice(InvoiceReading {
            invoice_id: "grp_1".into(),
            status: status.into(),
            live_status: Some(live.into()),
            grant_state: if status == "settled" { "granted" } else { "none" }.into(),
            ..InvoiceReading::default()
        })
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_entry_point_widget_opens_the_modal_and_asks_for_the_plans() {
        set_entry_points_enabled(true);
        let mut app = app().await;
        assert!(!app.checkout_is_open());
        app.dispatch_panel_click("checkout:open", 0.0, 0.0);
        assert!(app.checkout_is_open());
        assert!(app.any_blocking_modal_open(), "the chart underneath is inert while paying");
        assert_eq!(app.checkout_take_cmd(NOW), Some(CheckoutCmd::LoadPlans));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn choosing_a_plan_polling_and_paying_closes_the_modal_and_refreshes_entitlements() {
        let mut app = opened(true).await;
        app.dispatch_panel_click("checkout:plan:paid", 0.0, 0.0);
        assert_eq!(app.checkout_take_cmd(NOW), Some(CheckoutCmd::Checkout { plan_id: "paid".into() }));
        app.checkout_apply(started("paid", None), NOW);
        assert_eq!(app.panel_app.checkout_state.step, CheckoutStep::Pay);

        // Nothing is due before the first beat; then one poll per 3 s.
        assert_eq!(app.checkout_take_cmd(NOW + 1_000), None);
        let poll = app.checkout_take_cmd(NOW + POLL_INTERVAL_MS);
        assert_eq!(poll, Some(CheckoutCmd::PollInvoice { invoice_id: "grp_1".into(), account_key: None }));
        app.checkout_apply(reading("new", "processing"), NOW + POLL_INTERVAL_MS + 100);
        assert_eq!(
            app.panel_app.checkout_state.invoice.as_ref().map(|i| i.phase),
            Some(InvoicePhase::Processing)
        );

        let next = app.checkout_take_cmd(NOW + 2 * POLL_INTERVAL_MS);
        assert!(matches!(next, Some(CheckoutCmd::PollInvoice { .. })));
        let paid_at = NOW + 2 * POLL_INTERVAL_MS + 100;
        app.checkout_apply(reading("settled", "settled"), paid_at);
        assert_eq!(app.panel_app.checkout_state.step, CheckoutStep::Paid);
        assert!(app.checkout_is_open(), "the success state is shown first");
        assert!(app.checkout_take_entitlements_dirty());

        app.checkout_tick(true, paid_at + SUCCESS_HOLD_MS);
        assert!(!app.checkout_is_open(), "then the modal closes itself");
        assert!(!app.any_blocking_modal_open());
        assert!(!app.checkout_take_entitlements_dirty(), "one-shot");
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn an_expired_invoice_offers_a_new_one_through_the_button() {
        let mut app = opened(true).await;
        app.dispatch_panel_click("checkout:plan:paid", 0.0, 0.0);
        app.checkout_take_cmd(NOW);
        app.checkout_apply(started("paid", None), NOW);
        app.checkout_take_cmd(NOW + POLL_INTERVAL_MS);
        app.checkout_apply(reading("new", "expired"), NOW + 4_000);
        assert_eq!(app.panel_app.checkout_state.invoice.as_ref().map(|i| i.phase), Some(InvoicePhase::Expired));

        app.dispatch_panel_click("checkout:new_invoice", 0.0, 0.0);
        assert_eq!(app.checkout_take_cmd(NOW + 5_000), Some(CheckoutCmd::Checkout { plan_id: "paid".into() }));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_anonymous_gate_holds_until_the_box_is_ticked_then_the_key_signs_the_buyer_in() {
        let mut app = opened(false).await;
        app.dispatch_panel_click("checkout:plan:anon", 0.0, 0.0);
        assert_eq!(app.checkout_take_cmd(NOW), Some(CheckoutCmd::AnonCheckout { plan_id: "anon".into() }));
        app.checkout_apply(started("anon", Some("0F4Z-KEY")), NOW);
        assert_eq!(app.panel_app.checkout_state.step, CheckoutStep::KeyGate);

        app.dispatch_panel_click("checkout:copy_key", 0.0, 0.0);
        assert_eq!(app.clipboard_text.as_deref(), Some("0F4Z-KEY"), "the key goes out through the shared clipboard path");

        app.dispatch_panel_click("checkout:key_continue", 0.0, 0.0);
        assert_eq!(app.panel_app.checkout_state.step, CheckoutStep::KeyGate, "the gate holds");
        app.dispatch_panel_click("checkout:key_saved", 0.0, 0.0);
        app.dispatch_panel_click("checkout:key_continue", 0.0, 0.0);
        assert_eq!(app.panel_app.checkout_state.step, CheckoutStep::Pay);

        let at = now_ms() + 10_000;
        let poll = app.checkout_take_cmd(at);
        assert_eq!(
            poll,
            Some(CheckoutCmd::PollInvoice { invoice_id: "grp_1".into(), account_key: Some("0F4Z-KEY".into()) })
        );
        app.checkout_apply(reading("settled", "settled"), at + 100);
        assert_eq!(app.checkout_take_cmd(at + 100), Some(CheckoutCmd::SignInWithKey { account_key: "0F4Z-KEY".into() }));
        assert_eq!(app.panel_app.checkout_state.anon.as_ref().map(|a| a.sign_in), Some(SignInPhase::Working));

        app.checkout_apply(CheckoutEvent::SignedIn, at + 200);
        app.checkout_tick(true, at + 200 + SUCCESS_HOLD_MS);
        assert!(!app.checkout_is_open());
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn the_copy_buttons_use_the_shared_clipboard_path() {
        let mut app = opened(true).await;
        app.dispatch_panel_click("checkout:plan:paid", 0.0, 0.0);
        app.checkout_take_cmd(NOW);
        app.checkout_apply(started("paid", None), NOW);
        app.dispatch_panel_click("checkout:chip:1", 0.0, 0.0);
        app.dispatch_panel_click("checkout:copy_address", 0.0, 0.0);
        assert_eq!(app.clipboard_text.take().as_deref(), Some("TMock"));
        app.dispatch_panel_click("checkout:copy_amount", 0.0, 0.0);
        assert_eq!(app.clipboard_text.take().as_deref(), Some("5.000000"));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn have_a_code_leaves_for_the_redeem_field_on_the_account_tab() {
        let mut app = opened(true).await;
        app.dispatch_panel_click("checkout:have_code", 0.0, 0.0);
        assert!(!app.checkout_is_open());
        assert!(app.panel_app.user_settings_state.is_open);
        assert_eq!(app.panel_app.user_settings_state.active_tab, UserSettingsTab::Account);
        let focused = app.input_coordinator.borrow().text_fields().focused().cloned();
        assert_eq!(focused.as_ref().map(|w| w.as_str()), Some(crate::text_input::REDEEM_CODE));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_typed_account_key_signs_a_signed_out_visitor_in_and_the_box_forgets_it() {
        let mut app = opened(false).await;
        app.dispatch_panel_click("checkout:have_key", 0.0, 0.0);
        assert_eq!(app.panel_app.checkout_state.step, CheckoutStep::KeyEntry);
        let wid = uzor::WidgetId::from(crate::text_input::CHECKOUT_KEY_INPUT);
        assert_eq!(app.input_coordinator.borrow().text_fields().focused(), Some(&wid));

        app.input_coordinator.borrow_mut().text_fields_mut().set_text(&wid, "  0F4Z-TYPED-KEY ");
        app.dispatch_panel_click("checkout:key_submit", 0.0, 0.0);
        assert_eq!(
            app.checkout_take_cmd(NOW),
            Some(CheckoutCmd::SignInWithKey { account_key: "0F4Z-TYPED-KEY".into() })
        );

        app.checkout_apply(CheckoutEvent::SignedIn, NOW + 10);
        assert!(!app.checkout_is_open());
        app.checkout_tick(true, NOW + 20);
        let coord = app.input_coordinator.borrow();
        assert_eq!(coord.text_fields().text(&wid), "", "the credential does not linger in the box");
        assert_ne!(coord.text_fields().focused(), Some(&wid));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn escape_and_an_outside_click_keep_an_account_key_the_buyer_still_needs() {
        let mut app = opened(true).await;
        app.dispatch_panel_click("checkout:plan:anon", 0.0, 0.0);
        app.checkout_take_cmd(NOW);
        app.checkout_apply(started("anon", Some("KEEP-ME")), NOW);
        app.dispatch_panel_click("checkout:key_saved", 0.0, 0.0);
        app.dispatch_panel_click("checkout:key_continue", 0.0, 0.0);
        app.checkout_apply(reading("settled", "settled"), NOW + 10);
        assert_eq!(app.panel_app.checkout_state.step, CheckoutStep::Paid);

        app.checkout_dismiss();
        assert!(app.checkout_is_open(), "a signed-in buyer's key is not thrown away by a stray click");
        app.dispatch_panel_click("checkout:done", 0.0, 0.0);
        assert!(!app.checkout_is_open());
    }

    #[cfg(feature = "cloud")]
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn have_a_code_signed_out_opens_the_sign_in_overlay_instead() {
        let mut app = opened(false).await;
        app.dispatch_panel_click("checkout:have_code", 0.0, 0.0);
        assert!(!app.checkout_is_open());
        assert!(app.panel_app.cloud_login_state.is_open);
        assert!(!app.panel_app.user_settings_state.is_open);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn server_error_codes_reach_the_modal_as_the_user_facing_sentence() {
        let mut app = opened(true).await;
        app.dispatch_panel_click("checkout:plan:paid", 0.0, 0.0);
        app.checkout_take_cmd(NOW);
        app.checkout_apply(
            CheckoutEvent::Failed { op: CheckoutOp::Start, error: CheckoutError::from_wire(409, "already_owned", None) },
            NOW,
        );
        let message = app.panel_app.checkout_state.error.as_ref().map(|e| e.message(mlc_core::Language::En));
        assert_eq!(message.as_deref(), Some("You already own this plan."));

        app.dispatch_panel_click("checkout:plan:paid", 0.0, 0.0);
        app.checkout_take_cmd(NOW);
        app.checkout_apply(
            CheckoutEvent::Failed { op: CheckoutOp::Start, error: CheckoutError::from_wire(503, "plan_unavailable", None) },
            NOW,
        );
        let message = app.panel_app.checkout_state.error.as_ref().map(|e| e.message(mlc_core::Language::En));
        assert_eq!(message.as_deref(), Some("The payment service does not have this plan yet. Try again later."));
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn owned_plans_never_start_a_purchase() {
        let mut app = opened(true).await;
        app.checkout_apply(
            CheckoutEvent::Holdings(HoldingsUi { lifetime_paid: true, ..HoldingsUi::default() }),
            NOW,
        );
        app.dispatch_panel_click("checkout:plan:paid", 0.0, 0.0);
        assert_eq!(app.checkout_take_cmd(NOW), None);
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn a_partial_payment_note_survives_to_the_modal_state() {
        let mut app = opened(true).await;
        app.dispatch_panel_click("checkout:plan:paid", 0.0, 0.0);
        app.checkout_take_cmd(NOW);
        app.checkout_apply(started("paid", None), NOW);
        app.checkout_apply(
            CheckoutEvent::Invoice(InvoiceReading {
                invoice_id: "grp_1".into(),
                status: "new".into(),
                live_status: Some("processing".into()),
                grant_state: "none".into(),
                flags: vec!["partial".into()],
                partial: Some(AmountUi {
                    asset: "USDT".into(),
                    received: "2".into(),
                    expected: "5".into(),
                    shortfall: "3".into(),
                }),
                ..InvoiceReading::default()
            }),
            NOW + 100,
        );
        let notes = app.panel_app.checkout_state.invoice.as_ref().map(|i| i.notes.len());
        assert_eq!(notes, Some(1));
    }
}
