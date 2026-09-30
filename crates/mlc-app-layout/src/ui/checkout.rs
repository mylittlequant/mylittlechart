//! State machine of the in-app checkout modal.
//!
//! One modal opens on a plan and closes itself on confirmation; there is no
//! redirect. This module is the pure half — no fetch, no clock, no drawing:
//!
//! * the shell asks [`CheckoutState::take_cmd`] what to send, sends it, and
//!   hands the answer back as a [`CheckoutEvent`] through
//!   [`CheckoutState::apply`];
//! * a click on a drawn widget arrives as [`CheckoutState::click`];
//! * `layout::modals::checkout` draws whatever the state says.
//!
//! The steps are [`CheckoutStep::Plans`], the anonymous-account
//! [`CheckoutStep::KeyGate`], [`CheckoutStep::Pay`], the closing
//! [`CheckoutStep::Paid`] and the queued-downgrade [`CheckoutStep::Scheduled`].
//! The pay step is one component (`draw_pay` in the modal); the processor's
//! hosted widget can replace it later without touching this state.
//!
//! Data comes from the MLC server only (`/api/billing/*`); the browser never
//! talks to the processor. `pay[]` is kept exactly as the server returned it.
//!
//! The anonymous account key is a credential: it lives in memory only, is
//! redacted from every `Debug` impl here, and is never written to storage.

use std::sync::atomic::{AtomicBool, Ordering};

use mlc_core::i18n::{CheckoutKey, Language};

use crate::ui::qr_svg::QrMatrix;

/// Gap between two invoice polls while the pay step is open.
pub const POLL_INTERVAL_MS: u64 = 3_000;
/// How long the success state stays up before the modal closes itself.
pub const SUCCESS_HOLD_MS: u64 = 2_500;
/// How long a "Copied" flash stays on its button.
pub const COPIED_FLASH_MS: u64 = 1_800;
/// Past `expires_at` by this much with no processor status to say otherwise,
/// a still-new invoice is called expired. The processor's own `expired`
/// normally arrives first and is the only word trusted when it is present.
const EXPIRY_GRACE_MS: i64 = 15_000;

static ENTRY_POINTS: AtomicBool = AtomicBool::new(false);

/// Whether the shell can run the checkout at all. Read by every surface that
/// offers an entry point (the Account tab, the Messages notice, the sign-in
/// overlay) so a shell without a driver never draws a dead button. Process
/// wide because three unrelated states read it; the shell sets it once.
pub fn set_entry_points_enabled(enabled: bool) {
    ENTRY_POINTS.store(enabled, Ordering::Relaxed);
}

/// See [`set_entry_points_enabled`].
pub fn entry_points_enabled() -> bool {
    ENTRY_POINTS.load(Ordering::Relaxed)
}

// =============================================================================
// Plans and holdings
// =============================================================================

/// What a plan grants.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanKind {
    /// Lifetime paid account.
    PaidAccount,
    /// Lifetime anonymous account.
    AnonAccount,
    /// Monthly quant tier.
    Quant,
}

/// One plan as the server listed it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PlanUi {
    pub id: String,
    pub kind: PlanKind,
    pub monthly: bool,
    pub price_usd: u32,
    pub tier: Option<u8>,
    pub quota: Option<u64>,
    pub onion_only: bool,
}

/// What the signed-in account already holds (`GET /api/billing/me`).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct HoldingsUi {
    pub lifetime_paid: bool,
    pub lifetime_anon: bool,
    /// Plan id of the running quant tier.
    pub quant_plan_id: Option<String>,
    /// Plan id a queued change moves to.
    pub pending_plan_id: Option<String>,
}

/// How a plan card reads for the current account.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PlanStatus {
    /// Can be bought.
    Available,
    /// Already held (lifetime plans).
    Owned,
    /// The running quant tier.
    Current,
    /// A higher quant tier than the running one.
    Upgrade,
    /// A lower quant tier than the running one.
    Downgrade,
    /// The tier a queued change moves to.
    Scheduled,
}

impl PlanStatus {
    /// Whether clicking the card starts anything.
    pub fn is_actionable(self) -> bool {
        matches!(self, PlanStatus::Available | PlanStatus::Upgrade | PlanStatus::Downgrade)
    }
}

// =============================================================================
// Pay step data
// =============================================================================

/// One way to pay, exactly as `pay[]` returned it.
#[derive(Clone, Debug, PartialEq)]
pub struct PayOptionUi {
    pub chain: String,
    pub asset: String,
    pub address: String,
    pub amount: String,
    /// Unix ms this option stops accepting payment.
    pub expires_at_ms: Option<i64>,
    /// The processor's QR, read into modules; `None` when it could not be read.
    pub qr: Option<QrMatrix>,
}

/// Where an invoice stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InvoicePhase {
    /// Nothing seen on the network yet.
    Waiting,
    /// A payment is on the network, not yet confirmed.
    Processing,
    /// MLC recorded the payment.
    Paid,
    /// The invoice can no longer be paid.
    Expired,
    /// The processor called it invalid.
    Failed,
}

/// Amounts the processor reported for a partial or over payment.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct AmountUi {
    pub asset: String,
    pub received: String,
    pub expected: String,
    pub shortfall: String,
}

/// One line the pay step prints under the status.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InvoiceNote {
    Partial(AmountUi),
    Over(AmountUi),
    Late,
    Failed,
    FundsPending,
    Reverted,
    Refunded,
}

/// One `GET /api/billing/invoice/{id}` answer, reduced to what the state reads.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InvoiceReading {
    pub invoice_id: String,
    /// MLC's own record: `new | settled | reverted | refunded`.
    pub status: String,
    /// The processor's live line, when it could be read.
    pub live_status: Option<String>,
    /// `none | granted | reverted | revoked`.
    pub grant_state: String,
    pub flags: Vec<String>,
    pub partial: Option<AmountUi>,
    pub over: Option<AmountUi>,
    pub expires_at_ms: Option<i64>,
}

impl InvoiceReading {
    fn has_flag(&self, flag: &str) -> bool {
        self.flags.iter().any(|f| f == flag)
    }

    /// Phase this reading puts the invoice in.
    pub fn phase(&self, now_ms: i64) -> InvoicePhase {
        if self.status == "settled" || self.grant_state == "granted" {
            return InvoicePhase::Paid;
        }
        let live = self.live_status.as_deref();
        match live {
            Some("expired") => return InvoicePhase::Expired,
            Some("invalid") => return InvoicePhase::Failed,
            Some("processing") | Some("settled") | Some("complete") => return InvoicePhase::Processing,
            _ => {}
        }
        // No processor word: fall back to the clock, with a grace so a
        // slightly fast local clock cannot expire a live invoice.
        if live.is_none() && self.status == "new" {
            if let Some(exp) = self.expires_at_ms {
                if now_ms > exp + EXPIRY_GRACE_MS {
                    return InvoicePhase::Expired;
                }
            }
        }
        InvoicePhase::Waiting
    }

    /// The lines to print for this reading, in a stable order.
    pub fn notes(&self) -> Vec<InvoiceNote> {
        let mut notes = Vec::new();
        if self.has_flag("partial") {
            notes.push(InvoiceNote::Partial(self.partial.clone().unwrap_or_default()));
        }
        if self.has_flag("over") {
            notes.push(InvoiceNote::Over(self.over.clone().unwrap_or_default()));
        }
        if self.has_flag("late") {
            notes.push(InvoiceNote::Late);
        }
        if self.has_flag("failed") || self.has_flag("no_show") || self.has_flag("anomaly") {
            notes.push(InvoiceNote::Failed);
        }
        if self.has_flag("funds_pending") {
            notes.push(InvoiceNote::FundsPending);
        }
        if self.status == "reverted" || self.grant_state == "reverted" {
            notes.push(InvoiceNote::Reverted);
        }
        if self.status == "refunded" || self.grant_state == "revoked" {
            notes.push(InvoiceNote::Refunded);
        }
        notes
    }
}

/// The invoice the pay step is showing.
#[derive(Clone, Debug, PartialEq)]
pub struct InvoiceSession {
    pub invoice_id: String,
    pub plan_id: String,
    pub pay: Vec<PayOptionUi>,
    /// Index of the selected chip.
    pub selected: usize,
    pub phase: InvoicePhase,
    pub notes: Vec<InvoiceNote>,
    /// Prorated charge of a tier upgrade, in millionths of a USD.
    pub charge_usd_micros: Option<i64>,
}

/// Where the sign-in with the account key stands.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SignInPhase {
    /// Not started (the invoice is not paid yet, or this buyer keeps their session).
    Idle,
    Working,
    Done,
    Failed,
}

/// The anonymous account being bought.
#[derive(Clone, PartialEq)]
pub struct AnonSession {
    /// The whole credential. Shown once; never logged, never stored.
    pub account_key: String,
    /// The buyer ticked "I saved my account key".
    pub saved: bool,
    /// Sign in with the key once paid (the buyer held no session).
    pub auto_sign_in: bool,
    pub sign_in: SignInPhase,
}

impl std::fmt::Debug for AnonSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnonSession")
            .field("account_key", &"<redacted>")
            .field("saved", &self.saved)
            .field("auto_sign_in", &self.auto_sign_in)
            .field("sign_in", &self.sign_in)
            .finish()
    }
}

// =============================================================================
// Steps, commands, events
// =============================================================================

/// The modal's page.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckoutStep {
    Plans,
    /// The anonymous account key, shown once, before any payment detail.
    KeyGate,
    Pay,
    /// Confirmed; the modal closes itself shortly.
    Paid,
    /// A downgrade was queued for the next renewal.
    Scheduled,
    /// Signed out: type the key of an anonymous account bought earlier.
    KeyEntry,
}

/// What the shell must send next.
#[derive(Clone, PartialEq)]
pub enum CheckoutCmd {
    /// `GET /api/billing/plans`, and `GET /api/billing/me` when signed in.
    LoadPlans,
    /// `POST /api/billing/checkout`.
    Checkout { plan_id: String },
    /// `POST /api/billing/change`.
    Change { plan_id: String },
    /// `POST /api/billing/anon-checkout` (public).
    AnonCheckout { plan_id: String },
    /// `GET /api/billing/invoice/{id}`, or `anon-invoice` with the key.
    PollInvoice { invoice_id: String, account_key: Option<String> },
    /// `POST /api/auth/account-key`, then the device-code confirmation.
    SignInWithKey { account_key: String },
}

impl std::fmt::Debug for CheckoutCmd {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckoutCmd::LoadPlans => write!(f, "LoadPlans"),
            CheckoutCmd::Checkout { plan_id } => write!(f, "Checkout({plan_id})"),
            CheckoutCmd::Change { plan_id } => write!(f, "Change({plan_id})"),
            CheckoutCmd::AnonCheckout { plan_id } => write!(f, "AnonCheckout({plan_id})"),
            CheckoutCmd::PollInvoice { invoice_id, account_key } => {
                write!(f, "PollInvoice({invoice_id}, key={})", if account_key.is_some() { "<redacted>" } else { "none" })
            }
            CheckoutCmd::SignInWithKey { .. } => write!(f, "SignInWithKey(<redacted>)"),
        }
    }
}

/// Which request an answer belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CheckoutOp {
    LoadPlans,
    Start,
    Poll,
    SignIn,
}

/// An answer from the shell.
#[derive(Clone, PartialEq)]
pub enum CheckoutEvent {
    /// `GET /api/billing/plans`.
    Plans { plans: Vec<PlanUi>, onion: bool, enabled: bool },
    /// `GET /api/billing/me`.
    Holdings(HoldingsUi),
    /// A purchase started: `checkout`, `anon-checkout` (with the key) or an
    /// upgrade `change` that returned a charge.
    Started {
        invoice_id: String,
        plan_id: String,
        pay: Vec<PayOptionUi>,
        account_key: Option<String>,
        charge_usd_micros: Option<i64>,
    },
    /// A downgrade was queued; `effective` is the renewal date.
    ChangeScheduled { effective: Option<String> },
    /// An upgrade was applied on the spot (credit covered the charge).
    ChangeApplied,
    /// A poll answer.
    Invoice(InvoiceReading),
    /// The sign-in with the account key completed.
    SignedIn,
    /// A request failed.
    Failed { op: CheckoutOp, error: CheckoutError },
}

impl std::fmt::Debug for CheckoutEvent {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            CheckoutEvent::Plans { plans, onion, enabled } => {
                write!(f, "Plans({} plans, onion={onion}, enabled={enabled})", plans.len())
            }
            CheckoutEvent::Holdings(h) => write!(f, "Holdings({h:?})"),
            CheckoutEvent::Started { invoice_id, plan_id, pay, account_key, .. } => write!(
                f,
                "Started({invoice_id}, {plan_id}, {} options, key={})",
                pay.len(),
                if account_key.is_some() { "<redacted>" } else { "none" }
            ),
            CheckoutEvent::ChangeScheduled { effective } => write!(f, "ChangeScheduled({effective:?})"),
            CheckoutEvent::ChangeApplied => write!(f, "ChangeApplied"),
            CheckoutEvent::Invoice(r) => write!(f, "Invoice({})", r.invoice_id),
            CheckoutEvent::SignedIn => write!(f, "SignedIn"),
            CheckoutEvent::Failed { op, error } => write!(f, "Failed({op:?}, {error:?})"),
        }
    }
}

/// What a click asks the app (not the state) to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckoutEffect {
    None,
    /// Put this text on the clipboard.
    Copy(String),
    /// Close the modal and open the gift-code field.
    OpenRedeem,
    /// The key-entry page opened: give its text field the keyboard.
    FocusKeyField,
    /// "Sign in" on the key-entry page: read the field and submit its text
    /// with [`CheckoutState::submit_key`].
    SubmitKey,
}

// =============================================================================
// Errors
// =============================================================================

/// A failure the modal words for the person.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CheckoutError {
    PlanNotAvailable,
    AlreadyOwned,
    SubscriptionExists,
    PlanUnavailable,
    NotConfigured,
    Gateway,
    RateLimited,
    NoSubscription,
    SamePlan,
    ChangePending,
    /// The processor refused; its own explanation rides along.
    GatewayRefused(Option<String>),
    SignInRequired,
    Network,
    InvalidKey,
    Other,
}

impl CheckoutError {
    /// Map the server's answer: HTTP `status` (`0` for no answer) and the
    /// `error` code of `{"error": "<code>"}`.
    pub fn from_wire(status: u16, code: &str, detail: Option<&str>) -> Self {
        match code {
            "plan_not_available" | "anonymous_only" => Self::PlanNotAvailable,
            "already_owned" => Self::AlreadyOwned,
            "subscription_exists" => Self::SubscriptionExists,
            "plan_unavailable" => Self::PlanUnavailable,
            "billing_not_configured" => Self::NotConfigured,
            "payment_gateway_unavailable" => Self::Gateway,
            "rate_limited" => Self::RateLimited,
            "no_active_subscription" => Self::NoSubscription,
            "same_plan" => Self::SamePlan,
            "change_pending" => Self::ChangePending,
            "gateway_refused" => Self::GatewayRefused(detail.map(str::to_string)),
            "unauthorized" => Self::SignInRequired,
            "network" => Self::Network,
            "invalid account key" => Self::InvalidKey,
            _ => match status {
                0 => Self::Network,
                401 => Self::SignInRequired,
                429 => Self::RateLimited,
                502..=504 => Self::Gateway,
                _ => Self::Other,
            },
        }
    }

    /// The sentence's translation key.
    pub fn key(&self) -> CheckoutKey {
        match self {
            Self::PlanNotAvailable => CheckoutKey::ErrPlanNotAvailable,
            Self::AlreadyOwned => CheckoutKey::ErrAlreadyOwned,
            Self::SubscriptionExists => CheckoutKey::ErrSubscriptionExists,
            Self::PlanUnavailable => CheckoutKey::ErrPlanUnavailable,
            Self::NotConfigured => CheckoutKey::ErrNotConfigured,
            Self::Gateway => CheckoutKey::ErrGateway,
            Self::RateLimited => CheckoutKey::ErrRateLimited,
            Self::NoSubscription => CheckoutKey::ErrNoSubscription,
            Self::SamePlan => CheckoutKey::ErrSamePlan,
            Self::ChangePending => CheckoutKey::ErrChangePending,
            Self::GatewayRefused(_) => CheckoutKey::ErrGatewayRefused,
            Self::SignInRequired => CheckoutKey::ErrSignInRequired,
            Self::Network => CheckoutKey::ErrNetwork,
            Self::InvalidKey => CheckoutKey::ErrInvalidKey,
            Self::Other => CheckoutKey::ErrGeneric,
        }
    }

    /// The sentence in `lang`, followed by the processor's own explanation
    /// for a refusal.
    pub fn message(&self, lang: Language) -> String {
        let base = self.key().get(lang);
        match self {
            Self::GatewayRefused(Some(detail)) if !detail.is_empty() => format!("{base} {detail}"),
            _ => base.to_string(),
        }
    }
}

// =============================================================================
// The state
// =============================================================================

/// What a copy button copies (for the "Copied" flash).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CopyTarget {
    Amount,
    Address,
    Key,
}

/// The command that started the current invoice, kept so "New invoice"
/// repeats it.
#[derive(Clone, Debug, PartialEq, Eq)]
enum StartOp {
    Checkout(String),
    Change(String),
    Anon(String),
}

impl StartOp {
    fn cmd(&self) -> CheckoutCmd {
        match self {
            StartOp::Checkout(plan_id) => CheckoutCmd::Checkout { plan_id: plan_id.clone() },
            StartOp::Change(plan_id) => CheckoutCmd::Change { plan_id: plan_id.clone() },
            StartOp::Anon(plan_id) => CheckoutCmd::AnonCheckout { plan_id: plan_id.clone() },
        }
    }
}

/// The whole modal.
#[derive(Clone, Debug)]
pub struct CheckoutState {
    pub is_open: bool,
    pub step: CheckoutStep,
    /// Mirrors the account: signed in through a stored token.
    pub signed_in: bool,
    pub plans: Vec<PlanUi>,
    pub plans_loaded: bool,
    pub plans_loading: bool,
    pub onion: bool,
    /// The server holds a processor key.
    pub checkout_enabled: bool,
    pub holdings: HoldingsUi,
    /// Plan whose purchase request is in flight.
    pub starting: Option<String>,
    pub error: Option<CheckoutError>,
    pub invoice: Option<InvoiceSession>,
    pub anon: Option<AnonSession>,
    /// Renewal date of a queued downgrade.
    pub scheduled_on: Option<String>,
    /// A sign-in with a typed account key is in flight.
    pub key_working: bool,
    /// Last clock the shell handed in (unix ms); the countdown reads it.
    pub now_ms: u64,
    cmd: Option<CheckoutCmd>,
    cmd_in_flight: bool,
    next_poll_ms: u64,
    paid_at_ms: Option<u64>,
    entitlements_dirty: bool,
    copied: Option<(CopyTarget, u64)>,
    last_start: Option<StartOp>,
}

impl Default for CheckoutState {
    fn default() -> Self {
        Self {
            is_open: false,
            step: CheckoutStep::Plans,
            signed_in: false,
            plans: Vec::new(),
            plans_loaded: false,
            plans_loading: false,
            onion: false,
            checkout_enabled: true,
            holdings: HoldingsUi::default(),
            starting: None,
            error: None,
            invoice: None,
            anon: None,
            scheduled_on: None,
            key_working: false,
            now_ms: 0,
            cmd: None,
            cmd_in_flight: false,
            next_poll_ms: 0,
            paid_at_ms: None,
            entitlements_dirty: false,
            copied: None,
            last_start: None,
        }
    }
}

impl CheckoutState {
    /// Open the modal. A purchase that was under way resumes where it was;
    /// otherwise it opens on the plans. The plan list and holdings are always
    /// asked for again.
    pub fn open(&mut self, signed_in: bool) {
        self.is_open = true;
        self.signed_in = signed_in;
        self.error = None;
        // A request that was in flight when the modal closed may never answer
        // (a dropped connection); it must not leave the modal unable to send.
        // If its answer does arrive it is applied as usual.
        self.cmd_in_flight = false;
        self.starting = None;
        self.key_working = false;
        if self.invoice.is_none() {
            self.step = CheckoutStep::Plans;
        }
        self.request_plans();
    }

    /// Escape and a click outside the frame. Like [`Self::close`], except that
    /// it leaves the modal up while it still holds an account key the buyer
    /// may not have copied — only the Close button or the ✕ ends that.
    pub fn dismiss(&mut self) {
        if self.step == CheckoutStep::Paid && self.keeps_open_when_paid() {
            return;
        }
        self.close();
    }

    /// Close the modal. A finished purchase is forgotten; an unfinished one is
    /// kept so reopening resumes it (the invoice stays payable on the server).
    pub fn close(&mut self) {
        self.is_open = false;
        self.error = None;
        if matches!(self.step, CheckoutStep::Paid | CheckoutStep::Scheduled) {
            self.forget_session();
        }
    }

    fn forget_session(&mut self) {
        self.invoice = None;
        self.anon = None;
        self.scheduled_on = None;
        self.paid_at_ms = None;
        self.last_start = None;
        self.starting = None;
        self.step = CheckoutStep::Plans;
    }

    /// The shell mirrors the account here every frame. A different identity
    /// answers the holdings differently, so they are dropped and re-asked.
    pub fn set_signed_in(&mut self, signed_in: bool) {
        if self.signed_in == signed_in {
            return;
        }
        self.signed_in = signed_in;
        self.holdings = HoldingsUi::default();
        if self.is_open {
            self.request_plans();
        }
    }

    fn request_plans(&mut self) {
        self.plans_loading = true;
        self.cmd = Some(CheckoutCmd::LoadPlans);
    }

    /// Plans to draw: accounts first, then tiers by price. On an onion host
    /// the clearnet account plans are dropped and off it the Tor ones are.
    pub fn visible_plans(&self) -> Vec<&PlanUi> {
        let mut plans: Vec<&PlanUi> = self
            .plans
            .iter()
            .filter(|p| p.kind == PlanKind::Quant || p.onion_only == self.onion)
            .collect();
        plans.sort_by_key(|p| {
            let rank = match p.kind {
                PlanKind::PaidAccount => 0,
                PlanKind::AnonAccount => 1,
                PlanKind::Quant => 2,
            };
            (rank, p.price_usd)
        });
        plans
    }

    /// How `plan` reads for the current account.
    pub fn plan_status(&self, plan: &PlanUi) -> PlanStatus {
        let h = &self.holdings;
        match plan.kind {
            PlanKind::PaidAccount => {
                if h.lifetime_paid || h.lifetime_anon {
                    PlanStatus::Owned
                } else {
                    PlanStatus::Available
                }
            }
            PlanKind::AnonAccount => {
                if self.signed_in && h.lifetime_anon {
                    PlanStatus::Owned
                } else {
                    PlanStatus::Available
                }
            }
            PlanKind::Quant => {
                let current = h.quant_plan_id.as_deref();
                if current == Some(plan.id.as_str()) {
                    return PlanStatus::Current;
                }
                if h.pending_plan_id.as_deref() == Some(plan.id.as_str()) {
                    return PlanStatus::Scheduled;
                }
                let Some(current_price) = current
                    .and_then(|id| self.plans.iter().find(|p| p.id == id))
                    .map(|p| p.price_usd)
                else {
                    return PlanStatus::Available;
                };
                if plan.price_usd > current_price {
                    PlanStatus::Upgrade
                } else if plan.price_usd < current_price {
                    PlanStatus::Downgrade
                } else {
                    PlanStatus::Available
                }
            }
        }
    }

    /// The selected pay option, when the pay step has any.
    pub fn selected_option(&self) -> Option<&PayOptionUi> {
        let inv = self.invoice.as_ref()?;
        inv.pay.get(inv.selected)
    }

    /// Seconds until the selected option expires (negative once past).
    pub fn remaining_secs(&self) -> Option<i64> {
        let exp = self.selected_option()?.expires_at_ms?;
        Some((exp - self.now_ms as i64).div_euclid(1000))
    }

    /// Whether `target`'s button is still flashing "Copied".
    pub fn is_copied(&self, target: CopyTarget) -> bool {
        self.copied.is_some_and(|(t, at)| t == target && self.now_ms < at + COPIED_FLASH_MS)
    }

    /// Whether the entitlements answer should be re-asked (a payment landed).
    /// One-shot.
    pub fn take_entitlements_dirty(&mut self) -> bool {
        std::mem::take(&mut self.entitlements_dirty)
    }

    /// Whether the modal has to stay up after a payment: an anonymous account
    /// bought by someone who keeps their own session, or a failed key sign-in.
    fn keeps_open_when_paid(&self) -> bool {
        match &self.anon {
            None => false,
            Some(a) => !(a.auto_sign_in && a.sign_in == SignInPhase::Done),
        }
    }

    // ---- commands ----------------------------------------------------------

    /// The next request the shell should send, if any. Explicit commands come
    /// first; otherwise an open pay step polls its invoice every
    /// [`POLL_INTERVAL_MS`] while the invoice can still change.
    pub fn take_cmd(&mut self, now_ms: u64) -> Option<CheckoutCmd> {
        if !self.is_open || self.cmd_in_flight {
            return None;
        }
        if let Some(cmd) = self.cmd.take() {
            self.cmd_in_flight = true;
            return Some(cmd);
        }
        if self.step == CheckoutStep::Pay && now_ms >= self.next_poll_ms {
            if let Some(inv) = &self.invoice {
                if matches!(inv.phase, InvoicePhase::Waiting | InvoicePhase::Processing) {
                    self.cmd_in_flight = true;
                    self.next_poll_ms = now_ms + POLL_INTERVAL_MS;
                    return Some(CheckoutCmd::PollInvoice {
                        invoice_id: inv.invoice_id.clone(),
                        account_key: self.anon.as_ref().map(|a| a.account_key.clone()),
                    });
                }
            }
        }
        None
    }

    // ---- shell answers -----------------------------------------------------

    /// Apply one answer from the shell.
    pub fn apply(&mut self, event: CheckoutEvent, now_ms: u64) {
        self.now_ms = now_ms;
        match event {
            CheckoutEvent::Plans { plans, onion, enabled } => {
                self.cmd_in_flight = false;
                self.plans_loading = false;
                self.plans_loaded = true;
                self.plans = plans;
                self.onion = onion;
                self.checkout_enabled = enabled;
                if !enabled {
                    self.error = Some(CheckoutError::NotConfigured);
                } else if self.error == Some(CheckoutError::NotConfigured) {
                    self.error = None;
                }
            }
            CheckoutEvent::Holdings(h) => {
                self.holdings = h;
            }
            CheckoutEvent::Started { invoice_id, plan_id, pay, account_key, charge_usd_micros } => {
                self.cmd_in_flight = false;
                self.starting = None;
                self.error = None;
                self.paid_at_ms = None;
                self.next_poll_ms = now_ms + POLL_INTERVAL_MS;
                self.invoice = Some(InvoiceSession {
                    invoice_id,
                    plan_id,
                    pay,
                    selected: 0,
                    phase: InvoicePhase::Waiting,
                    notes: Vec::new(),
                    charge_usd_micros,
                });
                match account_key {
                    Some(account_key) => {
                        self.anon = Some(AnonSession {
                            account_key,
                            saved: false,
                            auto_sign_in: !self.signed_in,
                            sign_in: SignInPhase::Idle,
                        });
                        self.step = CheckoutStep::KeyGate;
                    }
                    None => {
                        self.anon = None;
                        self.step = CheckoutStep::Pay;
                    }
                }
            }
            CheckoutEvent::ChangeScheduled { effective } => {
                self.cmd_in_flight = false;
                self.starting = None;
                self.error = None;
                self.scheduled_on = effective;
                self.step = CheckoutStep::Scheduled;
                self.entitlements_dirty = true;
            }
            CheckoutEvent::ChangeApplied => {
                self.cmd_in_flight = false;
                self.starting = None;
                self.error = None;
                self.invoice = None;
                self.anon = None;
                self.enter_paid(now_ms);
            }
            CheckoutEvent::Invoice(reading) => self.apply_reading(reading, now_ms),
            CheckoutEvent::SignedIn if self.step == CheckoutStep::KeyEntry => {
                // A typed key signed the visitor in: nothing left to do here.
                self.cmd_in_flight = false;
                self.key_working = false;
                self.entitlements_dirty = true;
                self.close();
            }
            CheckoutEvent::SignedIn => {
                self.cmd_in_flight = false;
                self.error = None;
                if let Some(a) = self.anon.as_mut() {
                    a.sign_in = SignInPhase::Done;
                }
                self.paid_at_ms = Some(now_ms);
                self.entitlements_dirty = true;
            }
            CheckoutEvent::Failed { op, error } => self.apply_failure(op, error),
        }
    }

    fn apply_reading(&mut self, reading: InvoiceReading, now_ms: u64) {
        self.cmd_in_flight = false;
        let Some(inv) = self.invoice.as_mut() else { return };
        if inv.invoice_id != reading.invoice_id {
            // A late answer for an invoice the modal has moved on from.
            return;
        }
        let phase = reading.phase(now_ms as i64);
        inv.notes = reading.notes();
        let was_paid = inv.phase == InvoicePhase::Paid;
        inv.phase = phase;
        if phase == InvoicePhase::Paid && !was_paid {
            self.enter_paid(now_ms);
        }
    }

    fn enter_paid(&mut self, now_ms: u64) {
        if let Some(inv) = self.invoice.as_mut() {
            inv.phase = InvoicePhase::Paid;
        }
        self.step = CheckoutStep::Paid;
        self.paid_at_ms = Some(now_ms);
        self.entitlements_dirty = true;
        if let Some(a) = self.anon.as_mut() {
            if a.auto_sign_in && a.sign_in == SignInPhase::Idle {
                a.sign_in = SignInPhase::Working;
                self.cmd = Some(CheckoutCmd::SignInWithKey { account_key: a.account_key.clone() });
            }
        }
    }

    fn apply_failure(&mut self, op: CheckoutOp, error: CheckoutError) {
        self.cmd_in_flight = false;
        match op {
            CheckoutOp::LoadPlans => {
                self.plans_loading = false;
                self.error = Some(error);
            }
            CheckoutOp::Start => {
                self.starting = None;
                self.error = Some(error);
            }
            CheckoutOp::Poll => {
                // A dropped poll is not news: the next one goes out on the
                // usual beat. Only an answer that says the invoice is not
                // ours to read is worth showing.
                if !matches!(error, CheckoutError::Network | CheckoutError::Gateway | CheckoutError::Other) {
                    self.error = Some(error);
                }
            }
            CheckoutOp::SignIn => {
                self.key_working = false;
                if let Some(a) = self.anon.as_mut() {
                    a.sign_in = SignInPhase::Failed;
                }
                self.error = Some(error);
            }
        }
    }

    /// Advance the clock: closes the modal once the success state has been up
    /// long enough (unless the buyer still has a key to keep), and ends a
    /// "Copied" flash.
    pub fn tick(&mut self, now_ms: u64) {
        self.now_ms = now_ms;
        if let Some((_, at)) = self.copied {
            if now_ms >= at + COPIED_FLASH_MS {
                self.copied = None;
            }
        }
        if let Some(at) = self.paid_at_ms {
            if self.is_open
                && self.step == CheckoutStep::Paid
                && now_ms >= at + SUCCESS_HOLD_MS
                && !self.keeps_open_when_paid()
            {
                self.close();
            }
        }
    }

    // ---- clicks ------------------------------------------------------------

    /// A click on a widget drawn by the modal; `id` is the part after
    /// `checkout:`.
    pub fn click(&mut self, id: &str, now_ms: u64) -> CheckoutEffect {
        self.now_ms = now_ms;
        if let Some(plan_id) = id.strip_prefix("plan:") {
            self.select_plan(plan_id);
            return CheckoutEffect::None;
        }
        if let Some(n) = id.strip_prefix("chip:") {
            if let (Ok(n), Some(inv)) = (n.parse::<usize>(), self.invoice.as_mut()) {
                if n < inv.pay.len() {
                    inv.selected = n;
                }
            }
            return CheckoutEffect::None;
        }
        match id {
            "copy_amount" => self.copy(CopyTarget::Amount, now_ms),
            "copy_address" => self.copy(CopyTarget::Address, now_ms),
            "copy_key" => self.copy(CopyTarget::Key, now_ms),
            "key_saved" => {
                if let Some(a) = self.anon.as_mut() {
                    a.saved = !a.saved;
                }
                CheckoutEffect::None
            }
            "key_continue" => {
                if self.anon.as_ref().is_some_and(|a| a.saved) && self.step == CheckoutStep::KeyGate {
                    self.step = CheckoutStep::Pay;
                    self.next_poll_ms = now_ms;
                }
                CheckoutEffect::None
            }
            "back" => {
                self.back();
                CheckoutEffect::None
            }
            "new_invoice" => {
                self.new_invoice();
                CheckoutEffect::None
            }
            "retry" => {
                self.retry();
                CheckoutEffect::None
            }
            "have_code" => CheckoutEffect::OpenRedeem,
            "have_key" => {
                if self.signed_in {
                    return CheckoutEffect::None;
                }
                self.error = None;
                self.key_working = false;
                self.step = CheckoutStep::KeyEntry;
                CheckoutEffect::FocusKeyField
            }
            "key_submit" => CheckoutEffect::SubmitKey,
            "close" | "done" => {
                self.close();
                CheckoutEffect::None
            }
            _ => CheckoutEffect::None,
        }
    }

    fn copy(&mut self, target: CopyTarget, now_ms: u64) -> CheckoutEffect {
        let text = match target {
            CopyTarget::Amount => self.selected_option().map(|o| o.amount.clone()),
            CopyTarget::Address => self.selected_option().map(|o| o.address.clone()),
            CopyTarget::Key => self.anon.as_ref().map(|a| a.account_key.clone()),
        };
        match text {
            Some(text) if !text.is_empty() => {
                self.copied = Some((target, now_ms));
                CheckoutEffect::Copy(text)
            }
            _ => CheckoutEffect::None,
        }
    }

    fn select_plan(&mut self, plan_id: &str) {
        if self.starting.is_some() || self.cmd_in_flight || !self.checkout_enabled {
            return;
        }
        let Some(plan) = self.plans.iter().find(|p| p.id == plan_id).cloned() else { return };
        let status = self.plan_status(&plan);
        if !status.is_actionable() {
            return;
        }
        self.error = None;
        let op = match plan.kind {
            PlanKind::AnonAccount => StartOp::Anon(plan.id.clone()),
            PlanKind::PaidAccount | PlanKind::Quant if !self.signed_in => {
                self.error = Some(CheckoutError::SignInRequired);
                return;
            }
            PlanKind::Quant if self.holdings.quant_plan_id.is_some() => StartOp::Change(plan.id.clone()),
            _ => StartOp::Checkout(plan.id.clone()),
        };
        self.start(op);
    }

    fn start(&mut self, op: StartOp) {
        let plan_id = match &op {
            StartOp::Checkout(id) | StartOp::Change(id) | StartOp::Anon(id) => id.clone(),
        };
        self.starting = Some(plan_id);
        self.cmd = Some(op.cmd());
        self.last_start = Some(op);
    }

    /// Sign in with an account key typed on the key-entry page.
    pub fn submit_key(&mut self, key: &str) {
        // An answer still owed to an earlier request (the plan list) does not
        // stop the sign-in: it is queued and goes out as soon as the modal is
        // free to send.
        if self.step != CheckoutStep::KeyEntry || self.key_working {
            return;
        }
        let key = key.trim();
        if key.is_empty() {
            self.error = Some(CheckoutError::InvalidKey);
            return;
        }
        self.error = None;
        self.key_working = true;
        self.cmd = Some(CheckoutCmd::SignInWithKey { account_key: key.to_string() });
    }

    fn back(&mut self) {
        match self.step {
            CheckoutStep::KeyEntry => {
                self.key_working = false;
                self.step = CheckoutStep::Plans;
            }
            CheckoutStep::KeyGate => {
                // Nothing was paid yet; the pending account is discarded
                // server-side.
                self.invoice = None;
                self.anon = None;
                self.step = CheckoutStep::Plans;
            }
            CheckoutStep::Pay if self.anon.is_none() => {
                self.invoice = None;
                self.step = CheckoutStep::Plans;
            }
            _ => {}
        }
        self.error = None;
    }

    fn new_invoice(&mut self) {
        let Some(op) = self.last_start.clone() else { return };
        if self.starting.is_some() || self.cmd_in_flight {
            return;
        }
        if matches!(op, StartOp::Anon(_)) {
            // A fresh anonymous purchase is a fresh account and a fresh key.
            self.anon = None;
        }
        self.error = None;
        self.start(op);
    }

    fn retry(&mut self) {
        self.error = None;
        if let Some(a) = self.anon.as_mut() {
            if a.sign_in == SignInPhase::Failed && self.step == CheckoutStep::Paid {
                a.sign_in = SignInPhase::Working;
                self.cmd = Some(CheckoutCmd::SignInWithKey { account_key: a.account_key.clone() });
                return;
            }
        }
        if !self.plans_loaded || self.plans.is_empty() {
            self.request_plans();
        }
    }
}

// =============================================================================
// Text helpers
// =============================================================================

/// `text` with every `{name}` replaced by its value.
pub fn fill(text: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = text.to_string();
    for (name, value) in pairs {
        out = out.replace(&format!("{{{name}}}"), value);
    }
    out
}

/// `mm:ss`, or `h:mm:ss` from an hour up; negative counts as zero.
pub fn format_countdown(secs: i64) -> String {
    let s = secs.max(0);
    let (h, m, sec) = (s / 3600, (s % 3600) / 60, s % 60);
    if h > 0 {
        format!("{h}:{m:02}:{sec:02}")
    } else {
        format!("{m:02}:{sec:02}")
    }
}

/// Millionths of a USD as `12.00` (rounded down to cents).
pub fn format_usd_micros(micros: i64) -> String {
    let cents = micros.max(0) / 10_000;
    format!("{}.{:02}", cents / 100, cents % 100)
}

/// Unix ms of an RFC 3339 instant (`2026-09-30T12:00:00Z`,
/// `...:00.250+03:00`). `None` for anything else.
pub fn parse_rfc3339_ms(s: &str) -> Option<i64> {
    let s = s.trim();
    let bytes = s.as_bytes();
    if bytes.len() < 19 || !s.is_char_boundary(19) {
        return None;
    }
    let num = |from: usize, to: usize| -> Option<i64> { s.get(from..to)?.parse::<i64>().ok() };
    if bytes[4] != b'-' || bytes[7] != b'-' || !(bytes[10] == b'T' || bytes[10] == b' ')
        || bytes[13] != b':' || bytes[16] != b':'
    {
        return None;
    }
    let (year, month, day) = (num(0, 4)?, num(5, 7)?, num(8, 10)?);
    let (hour, minute, second) = (num(11, 13)?, num(14, 16)?, num(17, 19)?);
    if !(1..=12).contains(&month) || !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let mut rest = &s[19..];
    let mut millis = 0_i64;
    if let Some(frac) = rest.strip_prefix('.') {
        let digits: String = frac.chars().take_while(char::is_ascii_digit).collect();
        if digits.is_empty() {
            return None;
        }
        let three: String = digits.chars().chain("000".chars()).take(3).collect();
        millis = three.parse::<i64>().ok()?;
        rest = &frac[digits.len()..];
    }
    let offset_min = match rest {
        "Z" | "z" => 0,
        _ => {
            let sign = match rest.as_bytes().first()? {
                b'+' => 1,
                b'-' => -1,
                _ => return None,
            };
            let hh = rest.get(1..3)?.parse::<i64>().ok()?;
            let mm = rest.get(4..6)?.parse::<i64>().ok()?;
            if rest.len() != 6 || rest.as_bytes()[3] != b':' {
                return None;
            }
            sign * (hh * 60 + mm)
        }
    };
    let days = days_from_civil(year, month, day);
    let secs = days * 86_400 + hour * 3600 + minute * 60 + second - offset_min * 60;
    Some(secs * 1000 + millis)
}

/// Days since 1970-01-01 of a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe - 719_468
}

#[cfg(test)]
mod tests {
    use super::*;

    const NOW: u64 = 1_800_000_000_000;

    fn plan(id: &str, kind: PlanKind, price: u32, onion_only: bool) -> PlanUi {
        PlanUi {
            id: id.to_string(),
            kind,
            monthly: kind == PlanKind::Quant,
            price_usd: price,
            tier: None,
            quota: None,
            onion_only,
        }
    }

    fn catalog() -> Vec<PlanUi> {
        vec![
            plan("mlc-monthly-50-v1", PlanKind::Quant, 50, false),
            plan("mlc-anon-account-v1", PlanKind::AnonAccount, 10, false),
            plan("mlc-monthly-20-v1", PlanKind::Quant, 20, false),
            plan("mlc-paid-account-v1", PlanKind::PaidAccount, 5, false),
            plan("mlc-paid-account-tor-v1", PlanKind::PaidAccount, 10, true),
            plan("mlc-anon-account-tor-v1", PlanKind::AnonAccount, 20, true),
            plan("mlc-monthly-100-v1", PlanKind::Quant, 100, false),
        ]
    }

    fn pay_option(chain: &str, asset: &str) -> PayOptionUi {
        PayOptionUi {
            chain: chain.to_string(),
            asset: asset.to_string(),
            address: format!("addr-{chain}"),
            amount: "20.000000".to_string(),
            expires_at_ms: Some(NOW as i64 + 600_000),
            qr: None,
        }
    }

    fn open_signed_in() -> CheckoutState {
        let mut s = CheckoutState::default();
        s.open(true);
        assert_eq!(s.take_cmd(NOW), Some(CheckoutCmd::LoadPlans));
        s.apply(CheckoutEvent::Plans { plans: catalog(), onion: false, enabled: true }, NOW);
        s
    }

    fn started(plan_id: &str, key: Option<&str>) -> CheckoutEvent {
        CheckoutEvent::Started {
            invoice_id: "grp_1".to_string(),
            plan_id: plan_id.to_string(),
            pay: vec![pay_option("evm:1", "USDC"), pay_option("tron", "USDT")],
            account_key: key.map(str::to_string),
            charge_usd_micros: None,
        }
    }

    fn reading(status: &str, live: Option<&str>) -> InvoiceReading {
        InvoiceReading {
            invoice_id: "grp_1".to_string(),
            status: status.to_string(),
            live_status: live.map(str::to_string),
            grant_state: if status == "settled" { "granted" } else { "none" }.to_string(),
            ..InvoiceReading::default()
        }
    }

    // ---- plan list ---------------------------------------------------------

    #[test]
    fn plans_are_sorted_accounts_first_then_tiers_by_price() {
        let s = open_signed_in();
        let ids: Vec<&str> = s.visible_plans().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "mlc-paid-account-v1",
                "mlc-anon-account-v1",
                "mlc-monthly-20-v1",
                "mlc-monthly-50-v1",
                "mlc-monthly-100-v1"
            ]
        );
    }

    #[test]
    fn an_onion_host_shows_only_the_tor_account_plans() {
        let mut s = open_signed_in();
        s.apply(CheckoutEvent::Plans { plans: catalog(), onion: true, enabled: true }, NOW);
        let ids: Vec<&str> = s.visible_plans().iter().map(|p| p.id.as_str()).collect();
        assert_eq!(
            ids,
            [
                "mlc-paid-account-tor-v1",
                "mlc-anon-account-tor-v1",
                "mlc-monthly-20-v1",
                "mlc-monthly-50-v1",
                "mlc-monthly-100-v1"
            ]
        );
    }

    #[test]
    fn owned_plans_follow_the_holdings() {
        let mut s = open_signed_in();
        let paid = catalog()[3].clone();
        let anon = catalog()[1].clone();
        assert_eq!(s.plan_status(&paid), PlanStatus::Available);

        s.apply(CheckoutEvent::Holdings(HoldingsUi { lifetime_paid: true, ..HoldingsUi::default() }), NOW);
        assert_eq!(s.plan_status(&paid), PlanStatus::Owned);
        assert_eq!(s.plan_status(&anon), PlanStatus::Available, "a paid account can still buy an anonymous one");

        s.apply(CheckoutEvent::Holdings(HoldingsUi { lifetime_anon: true, ..HoldingsUi::default() }), NOW);
        assert_eq!(s.plan_status(&paid), PlanStatus::Owned, "an anonymous account is paid too");
        assert_eq!(s.plan_status(&anon), PlanStatus::Owned);
    }

    #[test]
    fn quant_tiers_read_as_current_upgrade_downgrade_and_scheduled() {
        let mut s = open_signed_in();
        s.apply(
            CheckoutEvent::Holdings(HoldingsUi {
                quant_plan_id: Some("mlc-monthly-50-v1".to_string()),
                pending_plan_id: Some("mlc-monthly-20-v1".to_string()),
                ..HoldingsUi::default()
            }),
            NOW,
        );
        let by_id = |id: &str| catalog().into_iter().find(|p| p.id == id).expect("plan");
        assert_eq!(s.plan_status(&by_id("mlc-monthly-50-v1")), PlanStatus::Current);
        assert_eq!(s.plan_status(&by_id("mlc-monthly-100-v1")), PlanStatus::Upgrade);
        assert_eq!(s.plan_status(&by_id("mlc-monthly-20-v1")), PlanStatus::Scheduled);
        s.apply(
            CheckoutEvent::Holdings(HoldingsUi {
                quant_plan_id: Some("mlc-monthly-100-v1".to_string()),
                ..HoldingsUi::default()
            }),
            NOW,
        );
        assert_eq!(s.plan_status(&by_id("mlc-monthly-20-v1")), PlanStatus::Downgrade);
    }

    // ---- choosing a plan ---------------------------------------------------

    #[test]
    fn choosing_a_plan_queues_the_matching_request() {
        let mut s = open_signed_in();
        s.click("plan:mlc-paid-account-v1", NOW);
        assert_eq!(s.take_cmd(NOW), Some(CheckoutCmd::Checkout { plan_id: "mlc-paid-account-v1".into() }));

        let mut s = open_signed_in();
        s.click("plan:mlc-anon-account-v1", NOW);
        assert_eq!(s.take_cmd(NOW), Some(CheckoutCmd::AnonCheckout { plan_id: "mlc-anon-account-v1".into() }));

        let mut s = open_signed_in();
        s.apply(
            CheckoutEvent::Holdings(HoldingsUi {
                quant_plan_id: Some("mlc-monthly-20-v1".to_string()),
                ..HoldingsUi::default()
            }),
            NOW,
        );
        s.click("plan:mlc-monthly-50-v1", NOW);
        assert_eq!(s.take_cmd(NOW), Some(CheckoutCmd::Change { plan_id: "mlc-monthly-50-v1".into() }));
    }

    #[test]
    fn an_owned_or_current_plan_starts_nothing() {
        let mut s = open_signed_in();
        s.apply(CheckoutEvent::Holdings(HoldingsUi { lifetime_paid: true, ..HoldingsUi::default() }), NOW);
        s.click("plan:mlc-paid-account-v1", NOW);
        assert_eq!(s.take_cmd(NOW), None);
        assert!(s.starting.is_none());
    }

    #[test]
    fn a_signed_out_visitor_can_buy_only_the_anonymous_account() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.take_cmd(NOW);
        s.apply(CheckoutEvent::Plans { plans: catalog(), onion: false, enabled: true }, NOW);

        s.click("plan:mlc-paid-account-v1", NOW);
        assert_eq!(s.take_cmd(NOW), None);
        assert_eq!(s.error, Some(CheckoutError::SignInRequired));

        s.click("plan:mlc-anon-account-v1", NOW);
        assert_eq!(s.error, None);
        assert_eq!(s.take_cmd(NOW), Some(CheckoutCmd::AnonCheckout { plan_id: "mlc-anon-account-v1".into() }));
    }

    #[test]
    fn a_server_without_a_processor_key_disables_every_card() {
        let mut s = open_signed_in();
        s.apply(CheckoutEvent::Plans { plans: catalog(), onion: false, enabled: false }, NOW);
        assert_eq!(s.error, Some(CheckoutError::NotConfigured));
        s.click("plan:mlc-paid-account-v1", NOW);
        assert_eq!(s.take_cmd(NOW), None);
    }

    // ---- pay step ----------------------------------------------------------

    #[test]
    fn a_started_purchase_shows_a_chip_per_pay_option_as_returned() {
        let mut s = open_signed_in();
        s.click("plan:mlc-paid-account-v1", NOW);
        s.take_cmd(NOW);
        s.apply(started("mlc-paid-account-v1", None), NOW);
        assert_eq!(s.step, CheckoutStep::Pay);
        let inv = s.invoice.as_ref().expect("invoice");
        let chips: Vec<(&str, &str)> = inv.pay.iter().map(|o| (o.chain.as_str(), o.asset.as_str())).collect();
        assert_eq!(chips, [("evm:1", "USDC"), ("tron", "USDT")]);
        assert_eq!(inv.selected, 0);

        s.click("chip:1", NOW);
        assert_eq!(s.selected_option().map(|o| o.chain.as_str()), Some("tron"));
        s.click("chip:9", NOW);
        assert_eq!(s.selected_option().map(|o| o.chain.as_str()), Some("tron"), "an out-of-range chip is ignored");
    }

    #[test]
    fn the_countdown_reads_the_selected_options_expiry() {
        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        s.tick(NOW + 60_000);
        assert_eq!(s.remaining_secs(), Some(540));
        s.tick(NOW + 700_000);
        assert_eq!(s.remaining_secs(), Some(-100));
        assert_eq!(format_countdown(540), "09:00");
        assert_eq!(format_countdown(-5), "00:00");
        assert_eq!(format_countdown(3725), "1:02:05");
    }

    #[test]
    fn copy_buttons_hand_back_the_selected_options_text_and_flash() {
        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        s.click("chip:1", NOW);
        assert_eq!(s.click("copy_address", NOW), CheckoutEffect::Copy("addr-tron".to_string()));
        assert!(s.is_copied(CopyTarget::Address));
        assert!(!s.is_copied(CopyTarget::Amount));
        assert_eq!(s.click("copy_amount", NOW), CheckoutEffect::Copy("20.000000".to_string()));
        s.tick(NOW + COPIED_FLASH_MS + 1);
        assert!(!s.is_copied(CopyTarget::Amount));
        assert_eq!(s.click("copy_key", NOW), CheckoutEffect::None, "no key, nothing to copy");
    }

    #[test]
    fn the_invoice_is_polled_every_three_seconds_while_it_can_still_change() {
        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        assert_eq!(s.take_cmd(NOW + 1_000), None, "not due yet");
        let due = s.take_cmd(NOW + POLL_INTERVAL_MS);
        assert_eq!(
            due,
            Some(CheckoutCmd::PollInvoice { invoice_id: "grp_1".into(), account_key: None })
        );
        assert_eq!(s.take_cmd(NOW + POLL_INTERVAL_MS + 10), None, "one poll in flight at a time");
        s.apply(CheckoutEvent::Invoice(reading("new", Some("new"))), NOW + POLL_INTERVAL_MS + 200);
        assert_eq!(s.take_cmd(NOW + 2 * POLL_INTERVAL_MS - 1), None);
        assert!(s.take_cmd(NOW + 2 * POLL_INTERVAL_MS).is_some());
    }

    #[test]
    fn a_closed_modal_does_not_poll() {
        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        s.close();
        assert_eq!(s.take_cmd(NOW + 10 * POLL_INTERVAL_MS), None);
        s.open(true);
        assert_eq!(s.step, CheckoutStep::Pay, "reopening resumes the unfinished purchase");
    }

    // ---- outcomes ----------------------------------------------------------

    #[test]
    fn a_paid_reading_shows_success_then_the_modal_closes_and_entitlements_refresh() {
        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        s.take_cmd(NOW + POLL_INTERVAL_MS);
        let at = NOW + POLL_INTERVAL_MS + 100;
        s.apply(CheckoutEvent::Invoice(reading("settled", Some("settled"))), at);
        assert_eq!(s.step, CheckoutStep::Paid);
        assert!(s.is_open);
        assert!(s.take_entitlements_dirty());
        assert!(!s.take_entitlements_dirty(), "one-shot");

        s.tick(at + SUCCESS_HOLD_MS - 1);
        assert!(s.is_open, "the success state is held");
        s.tick(at + SUCCESS_HOLD_MS);
        assert!(!s.is_open, "the modal closes itself");
        assert_eq!(s.step, CheckoutStep::Plans);
        assert!(s.invoice.is_none(), "a finished purchase is forgotten");
    }

    #[test]
    fn a_settled_processor_line_alone_is_not_yet_paid() {
        let mut r = reading("new", Some("settled"));
        assert_eq!(r.phase(NOW as i64), InvoicePhase::Processing);
        r.status = "settled".to_string();
        assert_eq!(r.phase(NOW as i64), InvoicePhase::Paid);
    }

    #[test]
    fn an_expired_invoice_offers_a_new_one_that_repeats_the_same_request() {
        let mut s = open_signed_in();
        s.click("plan:mlc-paid-account-v1", NOW);
        s.take_cmd(NOW);
        s.apply(started("mlc-paid-account-v1", None), NOW);
        s.take_cmd(NOW + POLL_INTERVAL_MS);
        s.apply(CheckoutEvent::Invoice(reading("new", Some("expired"))), NOW + 4_000);
        assert_eq!(s.invoice.as_ref().map(|i| i.phase), Some(InvoicePhase::Expired));
        assert_eq!(s.take_cmd(NOW + 60_000), None, "an expired invoice is not polled");

        s.click("new_invoice", NOW + 61_000);
        assert_eq!(s.take_cmd(NOW + 61_000), Some(CheckoutCmd::Checkout { plan_id: "mlc-paid-account-v1".into() }));
    }

    #[test]
    fn without_a_processor_word_the_clock_expires_a_new_invoice_only_after_a_grace() {
        let mut r = reading("new", None);
        r.expires_at_ms = Some(NOW as i64);
        assert_eq!(r.phase(NOW as i64 + 1_000), InvoicePhase::Waiting);
        assert_eq!(r.phase(NOW as i64 + EXPIRY_GRACE_MS + 1), InvoicePhase::Expired);
        r.live_status = Some("new".to_string());
        assert_eq!(r.phase(NOW as i64 + 3_600_000), InvoicePhase::Waiting, "the processor's word wins");
    }

    #[test]
    fn partial_over_and_late_readings_carry_the_servers_amounts() {
        let mut r = reading("new", Some("processing"));
        r.flags = vec!["partial".into(), "late".into(), "over".into()];
        r.partial = Some(AmountUi {
            asset: "USDT".into(),
            received: "2.000000".into(),
            expected: "5.000000".into(),
            shortfall: "3.000000".into(),
        });
        r.over = Some(AmountUi { asset: "USDC".into(), received: "12.5".into(), expected: "10".into(), ..AmountUi::default() });
        let notes = r.notes();
        assert_eq!(notes.len(), 3);
        assert!(matches!(&notes[0], InvoiceNote::Partial(a) if a.shortfall == "3.000000"));
        assert!(matches!(&notes[1], InvoiceNote::Over(a) if a.received == "12.5"));
        assert_eq!(notes[2], InvoiceNote::Late);

        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        s.apply(CheckoutEvent::Invoice(r), NOW + 1);
        assert_eq!(s.invoice.as_ref().map(|i| i.notes.len()), Some(3));
        assert_eq!(s.invoice.as_ref().map(|i| i.phase), Some(InvoicePhase::Processing));
    }

    #[test]
    fn a_reverted_or_refunded_record_prints_its_own_note() {
        let mut r = reading("reverted", Some("processing"));
        r.grant_state = "reverted".to_string();
        assert_eq!(r.notes(), vec![InvoiceNote::Reverted]);
        let mut r = reading("refunded", None);
        r.grant_state = "revoked".to_string();
        assert_eq!(r.notes(), vec![InvoiceNote::Refunded]);
    }

    #[test]
    fn a_stale_reading_for_another_invoice_is_ignored() {
        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        let mut r = reading("settled", Some("settled"));
        r.invoice_id = "grp_other".to_string();
        s.apply(CheckoutEvent::Invoice(r), NOW + 5);
        assert_eq!(s.step, CheckoutStep::Pay);
    }

    #[test]
    fn a_dropped_poll_is_silent_but_a_refused_one_is_shown() {
        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        s.take_cmd(NOW + POLL_INTERVAL_MS);
        s.apply(CheckoutEvent::Failed { op: CheckoutOp::Poll, error: CheckoutError::Network }, NOW + 4_000);
        assert_eq!(s.error, None);
        assert!(s.take_cmd(NOW + 2 * POLL_INTERVAL_MS).is_some(), "polling goes on");
        s.apply(CheckoutEvent::Failed { op: CheckoutOp::Poll, error: CheckoutError::SignInRequired }, NOW + 7_000);
        assert_eq!(s.error, Some(CheckoutError::SignInRequired));
    }

    #[test]
    fn a_downgrade_is_queued_not_paid() {
        let mut s = open_signed_in();
        s.apply(CheckoutEvent::ChangeScheduled { effective: Some("2026-10-25T00:00:00+00:00".into()) }, NOW);
        assert_eq!(s.step, CheckoutStep::Scheduled);
        assert_eq!(s.scheduled_on.as_deref(), Some("2026-10-25T00:00:00+00:00"));
        s.click("done", NOW);
        assert!(!s.is_open);
        assert_eq!(s.step, CheckoutStep::Plans);
    }

    #[test]
    fn an_upgrade_returns_a_charge_and_the_pay_step_keeps_it() {
        let mut s = open_signed_in();
        s.apply(
            CheckoutEvent::Started {
                invoice_id: "grp_1".into(),
                plan_id: "mlc-monthly-50-v1".into(),
                pay: vec![pay_option("evm:1", "USDC")],
                account_key: None,
                charge_usd_micros: Some(12_000_000),
            },
            NOW,
        );
        assert_eq!(s.invoice.as_ref().and_then(|i| i.charge_usd_micros), Some(12_000_000));
        assert_eq!(format_usd_micros(12_000_000), "12.00");
        assert_eq!(format_usd_micros(12_345_678), "12.34");
        assert_eq!(format_usd_micros(-5), "0.00");
    }

    #[test]
    fn an_upgrade_applied_on_the_spot_goes_straight_to_success() {
        let mut s = open_signed_in();
        s.apply(CheckoutEvent::ChangeApplied, NOW);
        assert_eq!(s.step, CheckoutStep::Paid);
        assert!(s.take_entitlements_dirty());
        s.tick(NOW + SUCCESS_HOLD_MS);
        assert!(!s.is_open);
    }

    // ---- anonymous flow ----------------------------------------------------

    #[test]
    fn the_anonymous_key_is_gated_behind_the_saved_box_before_any_payment_detail() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.take_cmd(NOW);
        s.apply(CheckoutEvent::Plans { plans: catalog(), onion: false, enabled: true }, NOW);
        s.click("plan:mlc-anon-account-v1", NOW);
        s.take_cmd(NOW);
        s.apply(started("mlc-anon-account-v1", Some("0F4Z-AAAA-BBBB")), NOW);

        assert_eq!(s.step, CheckoutStep::KeyGate);
        assert_eq!(s.take_cmd(NOW + 10 * POLL_INTERVAL_MS), None, "no polling behind the gate");

        s.click("key_continue", NOW);
        assert_eq!(s.step, CheckoutStep::KeyGate, "continue is dead until the box is ticked");

        assert_eq!(s.click("copy_key", NOW), CheckoutEffect::Copy("0F4Z-AAAA-BBBB".to_string()));
        s.click("key_saved", NOW);
        s.click("key_continue", NOW + 5);
        assert_eq!(s.step, CheckoutStep::Pay);
        assert_eq!(
            s.take_cmd(NOW + 5),
            Some(CheckoutCmd::PollInvoice { invoice_id: "grp_1".into(), account_key: Some("0F4Z-AAAA-BBBB".into()) })
        );
    }

    #[test]
    fn going_back_from_the_gate_drops_the_pending_account_and_its_key() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.apply(started("mlc-anon-account-v1", Some("KEY")), NOW);
        s.click("back", NOW);
        assert_eq!(s.step, CheckoutStep::Plans);
        assert!(s.anon.is_none() && s.invoice.is_none());
    }

    #[test]
    fn after_payment_the_signed_out_buyer_is_signed_in_with_the_key_and_the_modal_closes() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.take_cmd(NOW);
        s.apply(started("mlc-anon-account-v1", Some("KEY-1")), NOW);
        s.click("key_saved", NOW);
        s.click("key_continue", NOW);
        s.take_cmd(NOW + POLL_INTERVAL_MS);
        let at = NOW + POLL_INTERVAL_MS + 50;
        s.apply(CheckoutEvent::Invoice(reading("settled", Some("settled"))), at);

        assert_eq!(s.step, CheckoutStep::Paid);
        assert_eq!(s.take_cmd(at), Some(CheckoutCmd::SignInWithKey { account_key: "KEY-1".into() }));
        s.tick(at + SUCCESS_HOLD_MS + 10);
        assert!(s.is_open, "the modal waits for the sign-in");

        s.apply(CheckoutEvent::SignedIn, at + 500);
        s.tick(at + 500 + SUCCESS_HOLD_MS - 1);
        assert!(s.is_open);
        s.tick(at + 500 + SUCCESS_HOLD_MS);
        assert!(!s.is_open);
        assert!(s.anon.is_none(), "the key does not outlive the purchase");
    }

    #[test]
    fn a_failed_key_sign_in_keeps_the_modal_and_offers_a_retry() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.apply(started("mlc-anon-account-v1", Some("KEY-2")), NOW);
        s.click("key_saved", NOW);
        s.click("key_continue", NOW);
        s.apply(CheckoutEvent::Invoice(reading("settled", Some("settled"))), NOW + 10);
        s.take_cmd(NOW + 10);
        s.apply(CheckoutEvent::Failed { op: CheckoutOp::SignIn, error: CheckoutError::InvalidKey }, NOW + 20);
        s.tick(NOW + 60_000);
        assert!(s.is_open);
        assert_eq!(s.anon.as_ref().map(|a| a.sign_in), Some(SignInPhase::Failed));
        s.click("retry", NOW + 60_001);
        assert_eq!(s.take_cmd(NOW + 60_001), Some(CheckoutCmd::SignInWithKey { account_key: "KEY-2".into() }));
    }

    #[test]
    fn a_signed_in_buyer_of_an_anonymous_account_keeps_their_session_and_the_modal() {
        let mut s = open_signed_in();
        s.click("plan:mlc-anon-account-v1", NOW);
        s.take_cmd(NOW);
        s.apply(started("mlc-anon-account-v1", Some("KEY-3")), NOW);
        s.click("key_saved", NOW);
        s.click("key_continue", NOW);
        s.apply(CheckoutEvent::Invoice(reading("settled", Some("settled"))), NOW + 10);
        assert_eq!(s.take_cmd(NOW + 10), None, "no silent swap of the account they are in");
        s.tick(NOW + 10 * SUCCESS_HOLD_MS);
        assert!(s.is_open, "the key is still to be copied");
        assert_eq!(s.click("copy_key", NOW + 20), CheckoutEffect::Copy("KEY-3".to_string()));
        s.click("done", NOW + 30);
        assert!(!s.is_open);
        assert!(s.anon.is_none());
    }

    #[test]
    fn a_new_anonymous_invoice_is_a_new_account_and_a_new_key() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.apply(CheckoutEvent::Plans { plans: catalog(), onion: false, enabled: true }, NOW);
        s.click("plan:mlc-anon-account-v1", NOW);
        s.take_cmd(NOW);
        s.apply(started("mlc-anon-account-v1", Some("OLD")), NOW);
        s.click("key_saved", NOW);
        s.click("key_continue", NOW);
        s.take_cmd(NOW + POLL_INTERVAL_MS);
        s.apply(CheckoutEvent::Invoice(reading("new", Some("expired"))), NOW + 3_500);
        s.click("new_invoice", NOW + 4_000);
        assert!(s.anon.is_none(), "the old key is dropped");
        assert_eq!(s.take_cmd(NOW + 4_000), Some(CheckoutCmd::AnonCheckout { plan_id: "mlc-anon-account-v1".into() }));
        s.apply(started("mlc-anon-account-v1", Some("NEW")), NOW + 4_100);
        assert_eq!(s.step, CheckoutStep::KeyGate);
        assert_eq!(s.anon.as_ref().map(|a| (a.account_key.as_str(), a.saved)), Some(("NEW", false)));
    }

    #[test]
    fn nothing_prints_the_account_key() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.apply(started("mlc-anon-account-v1", Some("SECRET-KEY-VALUE")), NOW);
        assert!(!format!("{s:?}").contains("SECRET-KEY-VALUE"));
        let cmd = CheckoutCmd::SignInWithKey { account_key: "SECRET-KEY-VALUE".into() };
        assert!(!format!("{cmd:?}").contains("SECRET-KEY-VALUE"));
        let poll = CheckoutCmd::PollInvoice { invoice_id: "x".into(), account_key: Some("SECRET-KEY-VALUE".into()) };
        assert!(!format!("{poll:?}").contains("SECRET-KEY-VALUE"));
        let ev = started("p", Some("SECRET-KEY-VALUE"));
        assert!(!format!("{ev:?}").contains("SECRET-KEY-VALUE"));
    }

    // ---- errors ------------------------------------------------------------

    #[test]
    fn every_documented_error_code_maps_to_a_sentence_in_every_language() {
        let cases: [(u16, &str, CheckoutError); 12] = [
            (400, "plan_not_available", CheckoutError::PlanNotAvailable),
            (409, "already_owned", CheckoutError::AlreadyOwned),
            (409, "subscription_exists", CheckoutError::SubscriptionExists),
            (503, "plan_unavailable", CheckoutError::PlanUnavailable),
            (503, "billing_not_configured", CheckoutError::NotConfigured),
            (502, "payment_gateway_unavailable", CheckoutError::Gateway),
            (429, "rate_limited", CheckoutError::RateLimited),
            (409, "no_active_subscription", CheckoutError::NoSubscription),
            (400, "same_plan", CheckoutError::SamePlan),
            (409, "change_pending", CheckoutError::ChangePending),
            (401, "unauthorized", CheckoutError::SignInRequired),
            (0, "network", CheckoutError::Network),
        ];
        for (status, code, expected) in cases {
            let got = CheckoutError::from_wire(status, code, None);
            assert_eq!(got, expected, "{code}");
            for &lang in Language::all() {
                assert!(!got.message(lang).is_empty(), "{code} in {lang:?}");
            }
        }
        // The two English sentences the owner reads first.
        assert_eq!(
            CheckoutError::from_wire(409, "already_owned", None).message(Language::En),
            "You already own this plan."
        );
        assert_eq!(
            CheckoutError::from_wire(503, "plan_unavailable", None).message(Language::En),
            "The payment service does not have this plan yet. Try again later."
        );
    }

    #[test]
    fn an_unknown_code_falls_back_on_the_status() {
        assert_eq!(CheckoutError::from_wire(502, "", None), CheckoutError::Gateway);
        assert_eq!(CheckoutError::from_wire(429, "whatever", None), CheckoutError::RateLimited);
        assert_eq!(CheckoutError::from_wire(500, "boom", None), CheckoutError::Other);
        assert_eq!(CheckoutError::from_wire(401, "invalid account key", None), CheckoutError::InvalidKey);
    }

    #[test]
    fn a_gateway_refusal_appends_the_processors_own_words() {
        let e = CheckoutError::from_wire(400, "gateway_refused", Some("plan cannot change"));
        assert_eq!(e.message(Language::En), "The payment service refused this request. plan cannot change");
    }

    #[test]
    fn a_failed_start_clears_the_busy_card_and_shows_the_sentence() {
        let mut s = open_signed_in();
        s.click("plan:mlc-paid-account-v1", NOW);
        assert_eq!(s.starting.as_deref(), Some("mlc-paid-account-v1"));
        s.take_cmd(NOW);
        s.apply(
            CheckoutEvent::Failed { op: CheckoutOp::Start, error: CheckoutError::from_wire(409, "already_owned", None) },
            NOW,
        );
        assert!(s.starting.is_none());
        assert_eq!(s.error, Some(CheckoutError::AlreadyOwned));
        // A second click is possible again.
        s.click("plan:mlc-paid-account-v1", NOW);
        assert!(s.take_cmd(NOW).is_some());
    }

    #[test]
    fn a_plan_list_failure_offers_a_retry_that_asks_again() {
        let mut s = CheckoutState::default();
        s.open(true);
        s.take_cmd(NOW);
        s.apply(CheckoutEvent::Failed { op: CheckoutOp::LoadPlans, error: CheckoutError::Network }, NOW);
        assert!(!s.plans_loading);
        assert_eq!(s.error, Some(CheckoutError::Network));
        s.click("retry", NOW);
        assert_eq!(s.error, None);
        assert_eq!(s.take_cmd(NOW), Some(CheckoutCmd::LoadPlans));
    }

    #[test]
    fn have_a_code_asks_the_app_to_open_the_redeem_field() {
        let mut s = open_signed_in();
        assert_eq!(s.click("have_code", NOW), CheckoutEffect::OpenRedeem);
    }

    // ---- helpers -----------------------------------------------------------

    #[test]
    fn rfc3339_parses_zulu_offsets_and_fractions() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:01.250Z"), Some(1_250));
        assert_eq!(parse_rfc3339_ms("2026-09-30T12:00:00+00:00"), Some(1_790_769_600_000));
        assert_eq!(
            parse_rfc3339_ms("2026-09-30T15:00:00+03:00"),
            parse_rfc3339_ms("2026-09-30T12:00:00Z")
        );
        assert_eq!(parse_rfc3339_ms("2026-09-30T12:00:00.5-05:30"), Some(1_790_769_600_000 + 19_800_500));
        assert_eq!(parse_rfc3339_ms("2000-02-29T00:00:00Z"), Some(951_782_400_000));
    }

    #[test]
    fn rfc3339_refuses_garbage() {
        for bad in ["", "yesterday", "2026-13-01T00:00:00Z", "2026-09-30", "2026-09-30T12:00:00", "2026-09-30T12:00:00+0300"] {
            assert_eq!(parse_rfc3339_ms(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn fill_substitutes_named_placeholders() {
        assert_eq!(fill("Expires in {time}", &[("time", "09:00")]), "Expires in 09:00");
        assert_eq!(
            fill("{received} of {expected} {asset}", &[("received", "2"), ("expected", "5"), ("asset", "USDT")]),
            "2 of 5 USDT"
        );
    }

    // ---- signing in with a typed key ----------------------------------------

    #[test]
    fn a_signed_out_visitor_can_type_a_saved_key_and_is_signed_in() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.take_cmd(NOW);
        s.apply(CheckoutEvent::Plans { plans: catalog(), onion: false, enabled: true }, NOW);

        assert_eq!(s.click("have_key", NOW), CheckoutEffect::FocusKeyField);
        assert_eq!(s.step, CheckoutStep::KeyEntry);
        assert_eq!(s.click("key_submit", NOW), CheckoutEffect::SubmitKey);

        s.submit_key("  0F4Z-AAAA-BBBB  ");
        assert!(s.key_working);
        assert_eq!(s.take_cmd(NOW), Some(CheckoutCmd::SignInWithKey { account_key: "0F4Z-AAAA-BBBB".into() }));
        s.submit_key("second click while working");
        assert_eq!(s.take_cmd(NOW), None, "one sign-in at a time");

        s.apply(CheckoutEvent::SignedIn, NOW + 10);
        assert!(!s.is_open, "signed in: nothing left to do in the modal");
        assert!(s.take_entitlements_dirty());
        assert!(!s.key_working);
    }

    #[test]
    fn a_refused_typed_key_says_so_and_can_be_retyped() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.take_cmd(NOW);
        s.apply(CheckoutEvent::Plans { plans: catalog(), onion: false, enabled: true }, NOW);
        s.click("have_key", NOW);
        s.submit_key("");
        assert_eq!(s.error, Some(CheckoutError::InvalidKey), "an empty box is refused before any request");
        assert_eq!(s.take_cmd(NOW), None);

        s.submit_key("WRONG");
        s.take_cmd(NOW);
        s.apply(CheckoutEvent::Failed { op: CheckoutOp::SignIn, error: CheckoutError::InvalidKey }, NOW + 5);
        assert!(!s.key_working);
        assert_eq!(s.error, Some(CheckoutError::InvalidKey));
        assert_eq!(s.step, CheckoutStep::KeyEntry, "the page stays for another try");
        s.submit_key("RIGHT");
        assert_eq!(s.take_cmd(NOW + 6), Some(CheckoutCmd::SignInWithKey { account_key: "RIGHT".into() }));
    }

    #[test]
    fn the_key_page_is_signed_out_only_and_back_returns_to_the_plans() {
        let mut s = open_signed_in();
        assert_eq!(s.click("have_key", NOW), CheckoutEffect::None);
        assert_eq!(s.step, CheckoutStep::Plans);

        let mut s = CheckoutState::default();
        s.open(false);
        s.click("have_key", NOW);
        s.click("back", NOW);
        assert_eq!(s.step, CheckoutStep::Plans);
    }

    #[test]
    fn nothing_prints_a_typed_key() {
        let mut s = CheckoutState::default();
        s.open(false);
        s.click("have_key", NOW);
        s.submit_key("TYPED-SECRET");
        assert!(!format!("{s:?}").contains("TYPED-SECRET"));
    }

    // ---- dismissal and reopening --------------------------------------------

    #[test]
    fn escape_and_an_outside_click_never_drop_an_account_key_still_to_be_copied() {
        let mut s = open_signed_in();
        s.click("plan:mlc-anon-account-v1", NOW);
        s.take_cmd(NOW);
        s.apply(started("mlc-anon-account-v1", Some("KEY-9")), NOW);
        s.click("key_saved", NOW);
        s.click("key_continue", NOW);
        s.apply(CheckoutEvent::Invoice(reading("settled", Some("settled"))), NOW + 10);

        s.dismiss();
        assert!(s.is_open, "the buyer keeps their own session, so the key is all they get");
        assert!(s.anon.is_some());
        s.click("done", NOW + 20);
        assert!(!s.is_open);
    }

    #[test]
    fn dismiss_closes_an_ordinary_modal_and_a_plain_success() {
        let mut s = open_signed_in();
        s.dismiss();
        assert!(!s.is_open);

        let mut s = open_signed_in();
        s.apply(started("mlc-paid-account-v1", None), NOW);
        s.apply(CheckoutEvent::Invoice(reading("settled", Some("settled"))), NOW + 10);
        s.dismiss();
        assert!(!s.is_open);
    }

    #[test]
    fn reopening_frees_a_request_that_never_answered() {
        let mut s = open_signed_in();
        s.click("plan:mlc-paid-account-v1", NOW);
        assert!(s.take_cmd(NOW).is_some());
        s.close();
        s.open(true);
        assert!(s.starting.is_none());
        assert_eq!(s.take_cmd(NOW + 1), Some(CheckoutCmd::LoadPlans), "the modal can send again");
    }

    #[test]
    fn entry_points_default_off_and_follow_the_switch() {
        // Process-wide; restore so no other test sees a changed default.
        let before = entry_points_enabled();
        set_entry_points_enabled(true);
        assert!(entry_points_enabled());
        set_entry_points_enabled(before);
    }
}
