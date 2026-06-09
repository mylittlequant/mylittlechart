//! Top-level `Managers` struct (K2).
//!
//! Owner mandate (verbatim 2026-06-10):
//!   "топ лвл менеджер стракт живет в млч-кор очевидно его же будут
//!    потреблять все менеджеры, чартапп — это просто сборка приложения
//!    именно хедфул приложения зачастую (но даже если хедлесс)".
//!
//! Architecture model:
//!
//! - `Managers` lives here in mlc-core so EVERY host (mlc-app headful,
//!   future mlc-headless, future mlc-app-wasm) composes the same struct.
//! - `DataBubble` is a pure state machine (data slices + subscription
//!   key).  It does NOT own manager instances — that was the legacy
//!   mistake.
//! - Each manager is a global service.  It finds its target by
//!   `bubble_id` (or by chart/panel id through bubble.members) and
//!   mutates the bubble's data slice (e.g. `bubble.signals`,
//!   `bubble.primitives`).
//! - Chart panel views are pure consumers — they hold `bubble_id` and
//!   read what the managers wrote.
//!
//! ## Population schedule
//!
//! Fields are added one per sub-commit so each step ships a passing
//! cargo check:
//!
//! - K2.3 — `signal:    SignalManager` (lifted from `DataBubble.signal_manager`)
//! - K2.4 — `trade:     TradeManager`  (lifted from `DataBubble.trade_manager`)
//! - K2.5 — `marker:    MarkerManager` (lifted from `DataBubble.marker_manager`)
//! - K2.6 — `indicator: IndicatorManager` (moved from `ChartApp`)
//! - K2.7 — `alert:     AlertManager`     (moved from `ChartApp`)
//! - K2.8 — `bubble:    BubbleStore`      (moved from `ChartPanelApp.tag_manager`)
//! - K3   — `drawing:   DrawingManager`   (lifted to global, was per-canvas)
//!
//! Until K2.3 lands the struct is empty — it exists so call sites that
//! want to depend on `mlc_core::managers::Managers` can do so without
//! waiting for the lift cascade.

/// Top-level container for every backend manager service.
///
/// Hosted by the application (e.g. `ChartApp.managers` in mlc-app).
/// Headless services compose the same struct directly.
#[derive(Default)]
pub struct Managers {
    // Fields added incrementally by K2.3-K2.8 + K3.
}

impl Managers {
    /// Construct a fresh top-level manager container.
    pub fn new() -> Self {
        Self::default()
    }
}

impl std::fmt::Debug for Managers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Managers").finish_non_exhaustive()
    }
}
