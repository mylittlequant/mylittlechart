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

use crate::drawing::SignalManager;
use crate::drawing::TradeManager;

/// Top-level container for every backend manager service.
///
/// Hosted by the application (e.g. `ChartApp.managers` in mlc-app).
/// Headless services compose the same struct directly.
pub struct Managers {
    /// Global signal annotation service (lifted from `DataBubble.signal_manager` in K2.3).
    ///
    /// Stateless dispatcher — all signal data lives on the bubble data slice
    /// (`bubble.signals`, `bubble.signal_next_id`, `bubble.signal_strategies`,
    /// `bubble.signals_visible`).
    pub signal: SignalManager,

    /// Global trade visualization service (lifted from `DataBubble.trade_manager` in K2.4).
    ///
    /// Stateless dispatcher — all trade data lives on the bubble data slice
    /// (`bubble.trades`, `bubble.trade_next_id`, `bubble.trades_visible`,
    /// `bubble.trade_show_lines`).
    pub trade: TradeManager,
    // K2.5 — marker:    MarkerManager
    // K2.6 — indicator: IndicatorManager
    // K2.7 — alert:     AlertManager
    // K2.8 — bubble:    BubbleStore
    // K3   — drawing:   DrawingManager
}

impl Managers {
    /// Construct a fresh top-level manager container.
    pub fn new() -> Self {
        Self {
            signal: SignalManager::new(),
            trade: TradeManager::new(),
        }
    }
}

impl Default for Managers {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Managers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Managers").finish_non_exhaustive()
    }
}
