//! chart-app — standalone chart application crate
//!
//! A minimal, self-contained chart application that owns one `ChartPanelApp`
//! (which itself holds one `ChartContainer` via `ChartPanelGrid`) plus an
//! `InputCoordinator` for widget routing and a `DefaultChartInputHandler`
//! for chart canvas input.
//!
//! # Usage
//!
//! ```ignore
//! let mut app = ChartApp::new("BTCUSDT");
//! app.resize(1280, 800);
//! app.render(&mut ctx, current_time_ms);
//! app.on_click(x, y);
//! ```

pub mod active_series;
pub mod agent;

// On wasm32, gate4agent is absent from the dep graph. Re-alias our stub
// module as `gate4agent` so that all `gate4agent::Foo` path references in
// submodules resolve to the stub without touching every call site.
#[cfg(target_arch = "wasm32")]
pub(crate) use agent as gate4agent;

pub mod chart_container;
pub mod chart_container_leaf;
pub mod chart_panel_bridge;
pub mod dom_settings_modal;
pub mod chart_panel_state;
pub mod chart_state;
pub mod focus;
pub mod frame_lifecycle;
pub mod input;
pub mod chart_panel_app;
pub mod panel_grid;
pub mod panels_render;
pub mod panels_store;
pub mod scroll_dispatch;
pub mod share_viewer;
pub mod standalone_panel;
pub mod system_events;
pub mod tag_ui;
pub mod text_input;
pub mod trading_center_chart;
pub mod ui_container;
pub mod user_manager;
use uzor::input::TextFieldConfig;
use uzor::WidgetId;
pub mod persistence;
pub mod watchlist_ui;
pub mod workspace;

pub use panels_store::TradingPanelsStore;
pub use share_viewer::{PendingReopenShare, PresetId, SharedChartMeta, StrippedIndicator};

pub use input::KeyPress;
pub use input::VaultSetupOutcome;
pub use digdigdig3::ExchangeId;
pub use mlc_backend::watchlist::WatchlistManager;
pub use mlc_backend::template::TemplateManager;
pub use mlc_core::instrument::InstrumentAddress;
pub use mlc_backend_indicator_engine::RecalcMode;
pub use mlc_core::{Language, set_language};

// ── Debug log gate (MLC_PERF_LOG) ────────────────────────────────────────────
// Checked once at first use via OnceLock; hot path reads a single bool.
static DEBUG_LOG: std::sync::OnceLock<bool> = std::sync::OnceLock::new();

pub fn debug_log_enabled() -> bool {
    *DEBUG_LOG.get_or_init(|| std::env::var("MLC_PERF_LOG").is_ok())
}

/// Diagnostic log — gated by `MLC_PERF_LOG` env var.
/// Set `MLC_PERF_LOG=1` (any non-empty value) to enable.
macro_rules! dlog {
    ($($arg:tt)*) => {
        if $crate::debug_log_enabled() {
            log::debug!($($arg)*);
        }
    };
}
pub(crate) use dlog;

/// Wasm-aware diagnostic log: native goes to stderr via `eprintln!`, wasm
/// goes to the browser console via `web_sys::console::log_1` because wasm
/// stderr is dropped on the floor.  Always emits — no env-var gate.  Use
/// for short-lived diagnostic dumps that need to surface in DevTools.
macro_rules! sglog {
    ($($arg:tt)*) => {{
        #[cfg(target_arch = "wasm32")]
        {
            ::web_sys::console::log_1(&format!($($arg)*).into());
        }
        #[cfg(not(target_arch = "wasm32"))]
        {
            log::debug!($($arg)*);
        }
    }};
}
pub(crate) use sglog;

/// Register every text field this app owns on the coordinator's
/// `TextFieldStore`.
///
/// One function, called by all three `ChartApp` constructors, because this
/// table used to be copy-pasted into each of them and a field added to one
/// copy simply did not exist in the other two. `TextFieldStore::on_char`
/// looks the field state up by id and returns `TextAction::None` when there
/// is none — so an unregistered field draws, takes a click zone, publishes
/// geometry, and still swallows every keystroke. It has now caused that
/// exact defect twice: the profile-manager inputs under skeleton mode
/// (which builds through `new_window`, not `new`), and the account
/// overlay's gift-code box (owner, 2026-09-04, on a build where it was
/// registered in `new` alone).
fn register_all_text_fields(coord: &mut uzor::input::InputCoordinator) {
    let tf = coord.text_fields_mut();
    tf.register(text_input::HEX_COLOR, TextFieldConfig::text()
        .with_filter(|c| c == '#' || c.is_ascii_hexdigit())
        .with_max_len(9));
    tf.register(text_input::AGENT_PTY, TextFieldConfig::raw());
    tf.register(text_input::AGENT_CHAT, TextFieldConfig::text());
    // Bar-mode parameter: digits and decimal point only, max 12 chars.
    tf.register(text_input::BAR_MODE_PARAM, TextFieldConfig::text()
        .with_filter(|c| c.is_ascii_digit() || c == '.')
        .with_max_len(12));
    tf.register(text_input::WATERMARK_TEXT, TextFieldConfig::text().with_max_len(64));
    // Granularity favorites (kusok 1): custom TF name (letters+digits,
    // e.g. "7m") and custom numeric param (digits+dot) input rows.
    tf.register(text_input::TF_FAVORITE_INPUT, TextFieldConfig::text()
        .with_filter(|c| c.is_ascii_alphanumeric())
        .with_max_len(8));
    tf.register(text_input::PARAM_FAVORITE_INPUT, TextFieldConfig::text()
        .with_filter(|c| c.is_ascii_digit() || c == '.')
        .with_max_len(16));
    // Wave 1 search/dialog fields.
    tf.register(text_input::SYMBOL_SEARCH, TextFieldConfig::text());
    tf.register(text_input::INDICATOR_SEARCH, TextFieldConfig::text());
    tf.register(text_input::COMPARE_SEARCH, TextFieldConfig::text());
    // Symbol fast-search dropdown input (symbol-search-compare arc,
    // Wave 2). Registration is MANDATORY, not cosmetic: an
    // unregistered id can still be focus()ed (the store only tracks
    // the id), but `on_char` looks the field state up and returns
    // `TextAction::None` when missing — live repro 2026-07-16:
    // caret rendered, keystrokes silently dropped.
    tf.register(text_input::SYMBOL_QUICK_SEARCH, TextFieldConfig::text());
    tf.register(text_input::CHART_BROWSER_SEARCH, TextFieldConfig::text());
    tf.register(text_input::WATCHLIST_SEARCH, TextFieldConfig::text());
    tf.register(text_input::WATCHLIST_GROUP_NAME, TextFieldConfig::text());
    tf.register(text_input::PRESET_NAME_INPUT, TextFieldConfig::text());
    // Wave 2a template-name overlay fields.
    tf.register(text_input::PRIMITIVE_SETTINGS_TEMPLATE_NAME, TextFieldConfig::text());
    tf.register(text_input::INDICATOR_SETTINGS_TEMPLATE_NAME, TextFieldConfig::text());
    tf.register(text_input::COMPARE_SETTINGS_TEMPLATE_NAME, TextFieldConfig::text());
    tf.register(text_input::CHART_SETTINGS_TEMPLATE_NAME, TextFieldConfig::text());
    // Wave 2b inline sub-field editor (one WidgetId per modal, discriminant in active_field_id).
    tf.register(text_input::PRIMITIVE_SETTINGS_FIELD, TextFieldConfig::text());
    tf.register(text_input::INDICATOR_SETTINGS_FIELD, TextFieldConfig::text());
    tf.register(text_input::CHART_SETTINGS_FIELD, TextFieldConfig::text());
    tf.register(text_input::COMPARE_SETTINGS_FIELD, TextFieldConfig::text());
    // Wave 3 profile management + alert token fields.
    tf.register(text_input::NEW_PROFILE_NAME, TextFieldConfig::text());
    tf.register(text_input::PROFILE_RENAME, TextFieldConfig::text());
    tf.register(text_input::NEW_PASSPHRASE, TextFieldConfig::password());
    tf.register(text_input::CONFIRM_PASSPHRASE, TextFieldConfig::password());
    tf.register(text_input::RECOVERY_KEY, TextFieldConfig::text());
    // The reveal box's own field, separate from the entry field above: the
    // two co-exist on a bidirectional ShowRecoveryKey <-> ConfirmRecoveryKey
    // hop (recovery-key-confirm-by-typing arc) — sharing one id would let a
    // mistyped confirm attempt overwrite what the reveal box shows on
    // "Back". `read_only()` still permits Copy/SelectAll (see its own doc)
    // and mouse drag-select, just never typing.
    tf.register(text_input::RECOVERY_KEY_DISPLAY, TextFieldConfig::read_only());
    // Account nickname: 3-24 chars per `validate_nick`'s own rule; the box
    // accepts a little more so a pasted overlong value is still editable
    // rather than silently truncated before the person even sees the error.
    tf.register(text_input::ACCOUNT_NICK, TextFieldConfig::text().with_max_len(32));
    tf.register(text_input::ALERT_TG_TOKEN, TextFieldConfig::text());
    // Credentials modal: webhook URL edit + "add key" form fields.
    tf.register(text_input::CRED_WEBHOOK_URL, TextFieldConfig::text());
    tf.register(text_input::CRED_ADD_EXCHANGE, TextFieldConfig::text());
    tf.register(text_input::CRED_ADD_LABEL, TextFieldConfig::text());
    tf.register(text_input::CRED_ADD_API_KEY, TextFieldConfig::text());
    tf.register(text_input::CRED_ADD_API_SECRET, TextFieldConfig::password());
    tf.register(text_input::CRED_ADD_PASSPHRASE, TextFieldConfig::password());
    // Server tab's agent-API key manager "label" field (agent-api-auth-and-
    // secret-store plan, piece 4).
    tf.register(text_input::KEY_LABEL_INPUT, TextFieldConfig::text().with_max_len(64));
    // Account overlay gift/trial code. See SYMBOL_QUICK_SEARCH above
    // for why this line is the difference between a field and a
    // drawing of one. Codes are ten uppercase alphanumerics; the box
    // accepts a little more so a pasted code with a stray character
    // is still editable rather than silently truncated, and the
    // server normalises what finally arrives.
    tf.register(text_input::REDEEM_CODE, TextFieldConfig::text().with_max_len(32));
    // Forum compose/reply fields (paid-only tab). Max lengths generous
    // above the server's own bound (title 120, body 8192 — see
    // `mlc_app_layout::ui::modal_settings::{validate_forum_title,
    // validate_forum_body}`) so a pasted overlong value is still editable
    // rather than silently truncated before the person sees the local
    // validation error.
    tf.register(text_input::FORUM_TITLE, TextFieldConfig::text().with_max_len(160));
    tf.register(text_input::FORUM_BODY, TextFieldConfig::text().with_max_len(4000));
    tf.register(text_input::FORUM_REPLY, TextFieldConfig::text().with_max_len(4000));
    // Forum v2's search box and DM's draft/peer boxes were added to
    // `TextFieldStore` geometry (`update_field`, `lib.rs`'s own per-frame
    // pass) and dispatch (`text_input::dispatch::text_action_forum_search`/
    // `text_action_dm_draft`/`text_action_dm_peer`) but never here — and
    // `register`/`update_field`/`focus` all use `get`/`get_mut` against the
    // field map, never `entry().or_insert_with()`, so a field that is never
    // registered can never hold a rect, never be focused, and never accept
    // a typed character. That silently broke Board search (found in review
    // of the shipped W1 build, owner 2026-09-09) and would have broken DM's
    // draft/peer boxes identically the first time anyone tried them.
    tf.register(text_input::FORUM_SEARCH, TextFieldConfig::text().with_max_len(200));
    tf.register(text_input::DM_DRAFT, TextFieldConfig::text().with_max_len(4000));
    tf.register(text_input::DM_PEER, TextFieldConfig::text().with_max_len(128));
}

use std::cell::RefCell;

use mlc_core::{
    ToolbarConfig,
    LayoutRect,
    ScaleCornerHitZones,
    ToolbarState,
    render::RenderContext,
    DefaultChartInputHandler, ChartOutputAction,
    Command,
    data_provider::SharedDataProvider,
    ScaleMode,
    ChartId,
};
use mlc_app_layout::layout::{
    ChartPanelRenderData, render_full_chart_panel,
    ChartModalLayout, ChartModalRenderResult,
    ChartRenderConfig,
    ContextMenuResult,
    render_chart_crosshair_layer,
};
use mlc_app_layout::panel_app::ChartPanelLayout;
use mlc_app_layout::layout::modals::context_menu::render_context_menu;
use mlc_app_layout::layout::modals::indicator_overlay::render_indicator_overlay;
use mlc_app_layout::layout::modals::search_overlay::render_search_overlay;
use mlc_app_layout::layout::ModalSearchResult;
use mlc_app_layout::layout::render_ui::IndicatorOverlayInfo;
use mlc_core::indicator_source::{IndicatorSource, AlertRenderData, AlertRenderStatus};
use mlc_core::state::modal_state::{ModalState, IndicatorCatalogItem};
use mlc_app_layout::ui::modal_settings::ChartScreenArea;
use mlc_app_layout::layout::modals::watchlist_modal::{render_watchlist_modal, WatchlistEntry, WatchlistGroupInfo, render_wl_group_name_input, WlGroupNameInputResult};
use mlc_app_layout::layout::modals::clock_popup::render_clock_popup;
use mlc_backend_data_bridge::{DataBridge, LiveDataProvider};

use crate::chart_panel_app::ChartPanelApp;
use uzor::input::{InputCoordinator, InputState};

/// Every exchange whose connector is warmed up at startup (in small batches from
/// tick(), spread across the skeleton/loading screen so dials don't burst).
const ALL_EXCHANGE_IDS: &[digdigdig3::ExchangeId] = &[
    digdigdig3::ExchangeId::Binance, digdigdig3::ExchangeId::Bybit,
    digdigdig3::ExchangeId::OKX, digdigdig3::ExchangeId::KuCoin,
    digdigdig3::ExchangeId::GateIO, digdigdig3::ExchangeId::Bitget,
    digdigdig3::ExchangeId::MEXC, digdigdig3::ExchangeId::HTX,
    digdigdig3::ExchangeId::Kraken, digdigdig3::ExchangeId::Coinbase,
    digdigdig3::ExchangeId::BingX, digdigdig3::ExchangeId::Bitfinex,
    digdigdig3::ExchangeId::Bitstamp, digdigdig3::ExchangeId::Gemini,
    digdigdig3::ExchangeId::CryptoCom, digdigdig3::ExchangeId::Lighter,
    digdigdig3::ExchangeId::Upbit, digdigdig3::ExchangeId::Deribit,
    digdigdig3::ExchangeId::HyperLiquid, digdigdig3::ExchangeId::Dydx,
    digdigdig3::ExchangeId::Moex,
];


// =============================================================================
// Account type helpers
// =============================================================================

/// Convert a short account-type label (e.g. `"S"`, `"FC"`) into the
/// `digdigdig3::AccountType` enum.
///
/// Falls back to `AccountType::Spot` for unknown or empty labels so that
/// existing serialised data without an `account_type` field continues to
/// work correctly.
///
/// Delegates to `mlc_core::instrument::AccountType` — the single canonical
/// label parser (see `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1C)
/// — and converts across the mlc-core/dig3 boundary via
/// `mlc_backend_data_bridge::account_type_to_dig3`.
pub fn account_type_from_label(label: &str) -> digdigdig3::AccountType {
    let core_at: mlc_core::instrument::AccountType = label.parse().unwrap_or_default();
    mlc_backend_data_bridge::account_type_to_dig3(core_at)
}

/// Convert an [`AccountType`] back to the short label used in persisted
/// state, bubble metadata, and every map key. Inverse of
/// [`account_type_from_label`].
///
/// Delegates to `mlc_core::instrument::AccountType::as_label` — the single
/// canonical label formatter, converted across the mlc-core/dig3 boundary
/// via `mlc_backend_data_bridge::account_type_from_dig3`.
pub fn account_type_to_label(at: digdigdig3::AccountType) -> &'static str {
    mlc_backend_data_bridge::account_type_from_dig3(at).as_label()
}

// =============================================================================
// Timeframe parsing
// =============================================================================

/// Parse a timeframe name string (e.g. "1m", "5m", "1H", "4h", "1D") into a
/// [`Timeframe`].  Case-insensitive for the suffix letter (except the
/// `m`-minute / `M`-month pair, which stays case-sensitive by design).
///
/// Thin wrapper over [`mlc_core::state::Timeframe::parse`] (consolidation,
/// user-granularity-templates-inner arc, кусок 1) — this used to be an
/// independent closed match table (one of three hand-maintained copies of
/// "TF name -> duration", alongside `ChangeTimeframe`'s dispatch and
/// `ALL_TF_IDS`); it is now a generic parser so any `<count><unit>` name
/// (not just the 17 builtins) resolves here too. Callers are unaffected —
/// same signature, same `None` on unknown input.
fn parse_timeframe_name(name: &str) -> Option<mlc_core::state::Timeframe> {
    mlc_core::state::Timeframe::parse(name)
}

/// Flatten the trading container's live geometry into the Trading Container
/// modal's MAP cards (owner spec 2026-07-28). Rects are container-local
/// (`v1_layout_rects` at origin, sized by the last rendered container size);
/// the bottom strip becomes 3 tab segments (tab bar + content merged) with
/// the active one highlighted and clickable-to-switch.
fn tc_modal_map_panels(
    tc: &mlc_app_panels::trading::trading::trading_container::TradingContainerState,
) -> (Vec<mlc_app_layout::layout::modals::TcMapPanel>, (f64, f64)) {
    use mlc_app_layout::layout::modals::TcMapPanel;
    use mlc_app_panels::trading::trading::trading_container::BottomTab;

    let (mut w, mut h) = tc.last_size.get();
    if w <= 0.0 || h <= 0.0 {
        // Container not rendered yet this session (modal opened before the
        // Trading tab painted) — a plausible default aspect.
        w = 900.0;
        h = 640.0;
    }

    // Free mode (owner spec 2026-07-29): the MAP reflects the LIVE docking
    // tree — one card per leaf at its real rect; a tab stack becomes one
    // segmented card whose tab buttons activate that stack's tabs.
    if tc.layout_mode == mlc_app_panels::trading::trading::trading_container::TcLayoutMode::Free {
        use mlc_app_panels::trading::trading::trading_container::TcLeaf;
        let mut panels: Vec<TcMapPanel> = Vec::new();
        if let Some(dock) = &tc.free_dock {
            let ds = dock.0.borrow();
            let area = ds.layout_area();
            let count_for = |p: &TcLeaf| match p {
                TcLeaf::Positions => Some(tc.position_manager.positions.len()),
                TcLeaf::Orders => Some(tc.orders_log.open_orders.len()),
                TcLeaf::Trades => Some(tc.trade_log.trades.len()),
                _ => None,
            };
            let label_for = |p: &TcLeaf| -> String {
                let t = uzor::panels::DockPanel::title(p).to_string();
                match count_for(p) {
                    Some(n) => format!("{} ({})", t, n),
                    None => t,
                }
            };
            for l in ds.tree().leaves() {
                let Some(r) = ds.panel_rects().get(&l.id) else { continue };
                let rect = (
                    (r.x - area.x) as f64,
                    (r.y - area.y) as f64,
                    r.width as f64,
                    r.height as f64,
                );
                let id = format!("leaf{}", l.id.0);
                if l.tab_count() > 1 {
                    let mut card = TcMapPanel::plain(&id, String::new(), String::new(), rect, false);
                    card.tabs = l
                        .panels
                        .iter()
                        .enumerate()
                        .map(|(i, p)| (p.wire_id().to_string(), label_for(p), i == l.active_tab))
                        .collect();
                    panels.push(card);
                } else if let Some(p) = l.active_panel() {
                    let sub = if matches!(p, TcLeaf::Chart | TcLeaf::Dom) {
                        tc.symbol.clone()
                    } else {
                        String::new()
                    };
                    panels.push(TcMapPanel::plain(&id, label_for(p), sub, rect, false));
                }
            }
        }
        return (panels, (w, h));
    }

    let r = tc.v1_layout_rects(0.0, 0.0, w, h);

    // VIRTUAL vertical remap (owner spec 2026-07-28): the real 22px strips
    // are unreadable at map scale. Keep real horizontal proportions (x/w),
    // but give the bottom strip + metrics + tape generous fixed virtual
    // heights and compress the top region to whatever remains — so every
    // card's label fits. Widths/x are taken straight from `r`.
    const V_TOP: f64 = 260.0;
    const V_BOTTOM: f64 = 74.0;
    const V_STRIP: f64 = 34.0;

    let dom_col_w = r.dom.2;
    let right_x = r.order_entry.0;
    let right_w = r.order_entry.2;
    let main_h = r.dom.3.max(1.0);
    let oe_frac = (r.order_entry.3 / main_h).clamp(0.0, 1.0);
    let oe_h = V_TOP * oe_frac;

    let bottom_y = V_TOP;
    let metrics_y = bottom_y + V_BOTTOM;
    let tape_y = metrics_y + V_STRIP;

    // Center card — DOM/Chart, carries the fast-swap button.
    let mut center = TcMapPanel::plain(
        "center",
        tc.center_kind.label().to_string(),
        tc.symbol.clone(),
        (0.0, 0.0, dom_col_w, V_TOP),
        true,
    );
    center.swap = true;

    // Bottom strip — ONE card, three tab segments (active highlighted).
    let mut bottom = TcMapPanel::plain(
        "bottom",
        String::new(),
        String::new(),
        (0.0, bottom_y, w, V_BOTTOM),
        false,
    );
    bottom.tabs = [
        (BottomTab::Positions, "positions", "Positions", tc.position_manager.positions.len()),
        (BottomTab::Orders, "orders", "Orders", tc.orders_log.open_orders.len()),
        (BottomTab::Trades, "trades", "Trades", tc.trade_log.trades.len()),
    ]
    .into_iter()
    .map(|(tab, wire, label, count)| {
        (wire.to_string(), format!("{} ({})", label, count), tc.active_tab == tab)
    })
    .collect();

    let panels = vec![
        center,
        TcMapPanel::plain("order_entry", "Order Entry".to_string(), String::new(), (right_x, 0.0, right_w, oe_h), false),
        TcMapPanel::plain("risk_calculator", "Risk Calc".to_string(), String::new(), (right_x, oe_h, right_w, V_TOP - oe_h), false),
        bottom,
        TcMapPanel::plain("metrics", "Metrics".to_string(), String::new(), (0.0, metrics_y, w, V_STRIP), false),
        TcMapPanel::plain("tape", "Tape".to_string(), String::new(), (0.0, tape_y, w, V_STRIP), false),
    ];

    (panels, (w, h))
}

// =============================================================================
// Cross-drag overlay rendering
// =============================================================================

/// Draw the ghost rect + compass drop-zone indicator for an active cross-drag.
///
// MiniTickerData lives in mlc-backend (see `mlc_backend::MiniTickerData`).
// It's re-exported below as `crate::MiniTickerData` so existing references in
// this crate continue to work without rewriting every call site.
pub use mlc_backend::MiniTickerData;

// =============================================================================
// ChartApp
// =============================================================================

/// Standalone chart application.
///
/// Owns a single `ChartPanelApp` (which holds exactly one `ChartContainer` and
/// its own modal state structs), an `InputCoordinator` for widget routing,
/// and a `DefaultChartInputHandler` for chart canvas input.
///
/// There is no sync bridge — with only one window the state flows directly.
/// A compare-symbol add that is waiting for its bars to arrive from the bridge.
///
/// Created when `ChartContainer::add_compare_symbol` returns `false` (cache miss).
/// Consumed in `react_to_bubble_event / BarsLoaded` when matching bars arrive.
pub(crate) struct PendingCompareAdd {
    /// Target chart container that should receive the compare series.
    pub chart_id: mlc_core::ChartId,
    /// The compare symbol (e.g. "ETHUSDT").
    pub symbol: String,
    /// Exchange the compare symbol belongs to.
    pub exchange: digdigdig3::ExchangeId,
    /// Account type label (e.g. "S", "F").
    pub account_type_label: String,
    /// Remaining frames before this entry is evicted (~600 at 60 fps = 10 s).
    pub deadline_frames: u32,
}

pub struct PendingColdApply {
    pub preset: mlc_core::preset::preset::ChartPreset,
    pub stashed_at: web_time::Instant,
    /// Timers carried from the synchronous portion of the `LoadPreset`
    /// handler (warm-check / autosave / park / unpark / clone), forwarded
    /// to `apply_preset_cold` so the trailing log line keeps measuring
    /// "click → fully restored" the same way.
    pub timings: ColdApplyTimings,
}

#[derive(Debug, Clone, Copy)]
pub struct ColdApplyTimings {
    pub lp_total: web_time::Instant,
    pub lp_materialized_us: u128,
    pub lp_autosave_us: u128,
    pub lp_park_us: u128,
    pub lp_unpark_us: u128,
    pub lp_clone_us: u128,
}

/// One venue's request budget for the watchlist warm-up sweep (feature:
/// "warm the watchlist", owner 2026-08-30). Token bucket: `tokens` grows
/// continuously toward `capacity` at `capacity` tokens/minute and is spent
/// one per background `request_kline` call — see
/// `ChartApp::drain_watchlist_warmup` (`frame_lifecycle/react.rs`).
/// Per-venue rather than one shared budget: a single BitMEX-scale ceiling
/// (~30 req/min) would throttle every other venue's warm-up too, and a
/// generous shared budget would let one hot venue starve BitMEX's own.
struct WarmupBudget {
    tokens: f64,
    capacity: f64,
    last_refill: web_time::Instant,
}

impl WarmupBudget {
    /// Starts full — the sweep that just discovered this venue (a
    /// connector coming up, or a fresh watchlist row) is exactly the kind
    /// of burst a token bucket is meant to allow immediately.
    fn new(requests_per_minute: u32, now: web_time::Instant) -> Self {
        let capacity = requests_per_minute.max(1) as f64;
        Self { tokens: capacity, capacity, last_refill: now }
    }

    /// Add back whatever accrued since the last refill, capped at capacity.
    fn refill(&mut self, now: web_time::Instant) {
        let elapsed_secs = now.duration_since(self.last_refill).as_secs_f64();
        if elapsed_secs <= 0.0 {
            return;
        }
        let rate_per_sec = self.capacity / 60.0;
        self.tokens = (self.tokens + elapsed_secs * rate_per_sec).min(self.capacity);
        self.last_refill = now;
    }

    /// Spend one token if available.
    fn take(&mut self) -> bool {
        if self.tokens >= 1.0 {
            self.tokens -= 1.0;
            true
        } else {
            false
        }
    }
}

pub struct ChartApp {
    /// Live feeds each bubble currently holds on behalf of its indicators
    /// (see `ensure_bubble_indicator_feeds`). RAM-only bookkeeping: the
    /// durable truth is the set of indicator instances on the bubble; this
    /// only records what was already subscribed so the reconcile can diff
    /// instead of re-subscribing every tick.
    pub(crate) bubble_feeds: std::collections::HashMap<
        mlc_core::ids::BubbleId,
        std::collections::HashSet<mlc_backend_indicator_engine::StreamKind>,
    >,

    /// Per-(bubble, venue) read position in the shared liquidation rings.
    /// RAM-only: the rings hold the prints, this only records how far this
    /// bubble's tape has consumed them, so a frame that never ran is caught
    /// up by the next one instead of losing what arrived in between.
    pub(crate) liq_cursors: std::collections::HashMap<(mlc_core::ids::BubbleId, String), i64>,

    /// The (symbol, aggregate, venues, armed-at-ms) the liquidation overlay
    /// last armed feeds for, per bubble. RAM-only, and it exists so the
    /// arming runs on a CHANGE — plus a throttled repair while a venue is
    /// missing — rather than every frame: `subscribe_liquidations` is
    /// idempotent, but calling it per venue per frame would take the
    /// bridge's gate mutex hundreds of times a second to learn nothing.
    ///
    /// The venue list is PART of the key: narrowing the set changes what has
    /// to be live, and a key that only watched the flag would leave the old
    /// subscriptions armed and make the picker look like it did nothing.
    pub(crate) liq_armed:
        std::collections::HashMap<mlc_core::ids::BubbleId, (String, bool, Vec<String>, i64)>,

    /// The (symbol, aggregate, venues, requested-at-ms) the projection
    /// overlay last pulled open-interest history for. RAM-only. Open
    /// interest moves on the venue's own ladder — 5m at the finest — so
    /// re-pulling faster than the refresh tick only burns rate limit.
    pub(crate) oi_hist_armed:
        std::collections::HashMap<mlc_core::ids::BubbleId, (String, bool, Vec<String>, i64)>,

    /// Bars of OTHER venues, for the profiles that fold them. Keyed
    /// (symbol, timeframe name, venue) — deliberately not on a bubble: a
    /// bubble is one instrument's series, and these are other instruments'
    /// series that exist only as profile input. RAM-only, and pruned to what
    /// some live roster still asks for.
    #[allow(clippy::type_complexity)]
    pub profile_bars: std::collections::HashMap<
        (String, String, digdigdig3::ExchangeId),
        Vec<mlc_core::Bar>,
    >,

    /// The (symbol, timeframe, venues, armed-at-ms) the profile folds last
    /// subscribed klines for, per bubble. Same contract as `liq_armed`.
    #[allow(clippy::type_complexity)]
    pub(crate) profile_klines_armed: std::collections::HashMap<
        mlc_core::ids::BubbleId,
        (String, String, Vec<String>, i64),
    >,

    /// The (symbol, market, venues) whose ORDER BOOKS the DOM Heatmap
    /// overlay currently holds, per bubble. RAM-only, and it is the roster's
    /// memory: without it the reconcile could not tell a book it opened from
    /// one it never had, which is the difference between releasing a
    /// subscription and leaking it.
    #[allow(clippy::type_complexity)]
    pub(crate) dom_books_armed: std::collections::HashMap<
        mlc_core::ids::BubbleId,
        (String, digdigdig3::AccountType, Vec<digdigdig3::ExchangeId>),
    >,

    /// The chart panel — owns ChartContainer, toolbar state, split grid, and modal state.
    pub panel_app: ChartPanelApp,

    /// Central widget router — registers hit-zones each frame, processes clicks.
    ///
    /// Uses `RefCell` so that `render_to_scene(&self)` can register widgets via
    /// interior mutability without requiring `&mut self`.
    pub input_coordinator: RefCell<InputCoordinator>,

    /// Chart input handler — processes drag/scroll/click actions.
    pub input_handler: DefaultChartInputHandler,

    /// Screen dimensions (pixels)
    pub width: u32,
    pub height: u32,

    /// Hit-zone results from last render (used for click dispatch)
    pub(crate) frame_result: Option<ChartModalRenderResult>,
    /// Per-leaf scale-corner hit zones, populated each frame.
    /// Single-window mode stores one entry for the active leaf;
    /// split mode stores one entry per visible leaf.
    pub(crate) scale_corner_zones_by_leaf: std::collections::HashMap<mlc_core::LeafId, ScaleCornerHitZones>,

    /// Per-leaf loading-spinner geometry from the LAST frame: `(cx, cy, radius)`
    /// for every leaf that rendered the loading skeleton (bars empty, no
    /// data_error). Empty when nothing is loading.
    ///
    /// Read by the host renderer (`scene_builder.rs` in app-vello) AFTER
    /// `pw.chart.render()` to composite a spinner overlay layer on top of
    /// chart-scene. "scene-ready" is implicit: non-empty → empty between
    /// adjacent frames is the transition that retires the spinner, no extra
    /// flag needed.
    pub last_loading_spinners: Vec<(mlc_core::LeafId, (f64, f64, f64))>,

    /// Search overlay render result from last frame (used for rect-based hit
    /// testing) — `pub`, same as `last_sidebar_result` below, so
    /// `mlc-app-input-shell::dispatch` can hit-test the indicator-modal
    /// category rail for tooltips the same way it already does the right
    /// sidebar.
    pub search_modal_result: Option<ModalSearchResult>,

    /// Context menu render result from last frame (used for rect-based hit testing).
    pub(crate) context_menu_result: Option<ContextMenuResult>,

    /// Currently hovered context menu item id (for hover highlight rendering).
    pub(crate) hovered_context_menu_item_id: Option<String>,

    /// Which physical gesture (double-click vs. right-click) opens the
    /// wave-gesture popups (`ChartApp::try_wave_gesture`) — a settings
    /// field, not a hardcoded match, so the owner's still-open question
    /// ("на даталклик или на ПКМ, дискуссионно", 2026-08-13) is a one-field
    /// flip later rather than a code change. See
    /// `mlc_core::state::gestures`'s own module doc.
    pub(crate) gesture_bindings: mlc_core::state::GestureBindings,

    /// Mouse position from the last `on_mouse_move` call
    pub(crate) last_mouse_pos: (f64, f64),

    /// Chart content rect from last render — toolbar-offset area used for
    /// coordinate transforms (bar index, price) in drawing tool hit testing.
    pub(crate) content_rect: LayoutRect,

    /// Inline floating bar rect from last render — used for drag hit-testing.
    pub(crate) last_inline_bar_rect: Option<LayoutRect>,

    /// Favorites quick-pick bar rect from last render (favorite-primitives
    /// arc) — used for drag hit-testing, same role as `last_inline_bar_rect`.
    pub(crate) last_favorites_bar_rect: Option<LayoutRect>,

    /// Favorites-bar item drag-out-to-remove (favorite-primitives arc):
    /// press on a `fav:{tool_id}` item is recorded as pending `(tool_id,
    /// press_x, press_y)` and promoted to `fav_item_drag` once the pointer
    /// moves past the shell's 5px click threshold (same pending/promote
    /// pattern as `granularity_drag_pending`/`granularity_drag`).
    /// `fav_item_drag` carries `(tool_id, cursor_x, cursor_y)` — the cursor
    /// feeds the render's floating icon via `FavoritesDragRender`.
    pub(crate) fav_item_drag_pending: Option<(String, f64, f64)>,
    pub(crate) fav_item_drag: Option<(String, f64, f64)>,

    /// Drag-in-to-add from a left-toolbar drawing-tool group button
    /// (favorite-primitives arc): pending `(button_id, press_x, press_y)`,
    /// promoted to `fav_group_drag` (the button id) past 5px. While
    /// `fav_group_drag` is `Some`, the button's click activation is
    /// suppressed (a drag that leaves the button must not activate the tool).
    pub(crate) fav_group_drag_pending: Option<(String, f64, f64)>,
    pub(crate) fav_group_drag: Option<String>,

    /// Modal state for search overlays (indicator search, symbol search).
    modal_state: ModalState,

    /// Pending screenshot request — set by context menu, drained by the renderer.
    pending_screenshot: bool,

    /// True while the render pass whose output a shell is about to capture
    /// as a screenshot is running. Owner ruling (screenshot-is-chart-only
    /// arc, 2026-09-05): the captured PNG is the chart and nothing else —
    /// no tooltip, context menu, dropdown or modal may appear in it.
    /// `is_capturing_screenshot()` reads `pending_screenshot` (true) OR this
    /// field: the web shell drains `pending_screenshot` only AFTER its
    /// (synchronous, single-pass) render call, so the plain peek is enough
    /// there and needs no shell change. The native vello shell pipelines
    /// scene-build one frame ahead of GPU capture and already converts
    /// `pending_screenshot` into its own shell-side flag BEFORE the
    /// captured scene is built, so `pending_screenshot` reads false by the
    /// time `render()` runs for that scene — the vello shell instead calls
    /// `set_capturing_screenshot()` around the specific build pass whose
    /// output becomes the captured frame (see `mlc-app-shell-vello/src/
    /// render/scene_builder.rs` and `main.rs`'s `screenshot_armed` handoff).
    capturing_screenshot: bool,

    /// Pending live-share request — set by the toolbar "share_chart" button
    /// (via `ChartOutEvent::ShareLiveChart`), drained by the shell, which
    /// owns the cloud session/token. Mirror of `pending_screenshot`.
    pending_share_live: bool,

    /// When set, the frame loop should clear in-memory bars and re-fetch.
    pub pending_reset_cache: bool,
    /// When set, the frame loop should delete the .bin file, clear in-memory bars, and re-fetch.
    pub pending_reset_storage: bool,

    /// Points of the primitive captured at drag start, keyed by primitive index.
    ///
    /// Set in `StartPrimitiveDrag` so that `EndPrimitiveDrag` can compare old
    /// vs new points and push a `MovePrimitive` command to history.
    drag_start_points: Option<(u64, Vec<(i64, f64)>)>,

    /// Active color picker L2 drag state.
    ///
    /// Set in `on_drag_start` when the user begins dragging in the SV square or
    /// hue bar of an L2 color picker.  Cleared in `on_drag_end`.
    color_picker_drag: Option<ColorPickerDragState>,

    /// Compare-series presentation as it stood when a slider drag STARTED.
    ///
    /// A width drag writes live on every pixel, so the value the stack must
    /// restore is not the previous write — it is where the drag began. Set on
    /// drag start, consumed on drag end.
    compare_style_before_drag: Option<(String, mlc_core::chart::CompareSeriesStyle)>,

    /// Monotonic stamp handed to every recorded command.
    ///
    /// Chart edits land in TWO histories — the sync group's (primitives,
    /// indicators) and the window's (compare series) — and undo has to walk
    /// them in the order the user acted, not stack by stack. One counter for
    /// the whole app is what makes the two comparable.
    undo_seq: u64,

    /// A primitive's whole state as it stood when a slider drag STARTED.
    ///
    /// Same reason as `compare_style_before_drag`: the drag writes on every
    /// pixel, so the value to restore is where the gesture began.
    primitive_json_before_drag: Option<(u64, String, String)>,

    /// Nesting depth of `edit_*_recorded`.
    ///
    /// The colour picker's apply runs INSIDE the settings-modal click
    /// handler, and both are wrapped — without this one colour pick recorded
    /// two identical commands and took two Ctrl+Z to undo. Only the outermost
    /// wrapper records; it spans the whole edit anyway.
    edit_recording_depth: u32,

    /// True while a drag is active on a UI element (modal, slider, scrollbar, color picker).
    /// Set in `on_drag_start` when `is_over_ui()` is true; cleared in `on_drag_end`.
    /// Used to suppress crosshair during UI drags without enumerating every drag type.
    pub(crate) ui_drag_active: bool,

    /// True when `on_drag_start` dismissed a color-picker popup.
    /// While set, `on_drag_move` and `on_drag_end` are swallowed so the chart
    /// doesn't receive a spurious pan/draw drag.  Cleared in `on_drag_end`.
    pub(crate) drag_dismissed_popup: bool,

    /// In-progress separator drag state when the user is dragging a split-panel divider.
    /// Set in `on_drag_start`, updated in `on_drag_move`, cleared in `on_drag_end`.
    pub(crate) split_separator_drag: Option<SplitSeparatorDragState>,

    /// Stored overlay-tab hit zones per leaf (from last render frame).
    /// In single mode, only one entry with the active leaf id.
    pub(crate) leaf_tab_hit_zones: std::collections::HashMap<mlc_core::LeafId, mlc_app_layout::layout::panel_overlay::LeafTabHitZones>,

    /// Currently hovered overlay-tab zone (shared across all tabs — only one can be hovered at a time).
    pub(crate) leaf_tab_hover: mlc_app_layout::layout::panel_overlay::LeafTabHoverZone,

    /// LeafId of the tab currently being hovered (if any).
    pub(crate) leaf_tab_hovered_leaf: Option<mlc_core::LeafId>,

    /// Toolbar render result from the last frame — used to read chevron rects and
    /// max_scroll values when dispatching chevron clicks.
    pub(crate) last_toolbar_result: Option<mlc_core::ChartToolbarRenderResult>,

    /// Triggered alert events waiting for delivery (Telegram, toast, etc.)
    pub pending_delivery_events: Vec<mlc_alert_delivery::DeliveryEvent>,

    /// Whether an alert-triggered screenshot capture is needed.
    /// Set by tick() when an alert fires. Cleared by the renderer after capture.
    pub pending_alert_screenshot: bool,

    /// Sidebar panel state — tracks which right-side panel is open and its content.
    pub sidebar_state: mlc_app_sidebar_content::state::SidebarState,

    /// System event log (system-event-bus arc, 2026-09-05, owner scope
    /// `docs/mlc/plans/users-paid-features-scope-2026-09-05.md` §8) — one
    /// durable per-profile record of everything the system told the user.
    /// `ChartApp` is the single writer (doctrine): shells report facts
    /// through [`Self::push_system_event`] (see `system_events.rs`), never
    /// through this field directly — `pub(crate)` so persistence code in
    /// this crate can read/restore it, but the shell↔app boundary stays
    /// method-only.
    pub(crate) system_events: mlc_core::SystemEventLog,

    /// Currently visible toasts, drained from `system_events` each frame by
    /// [`Self::refresh_active_toasts`] (toast restyle-and-anchor follow-up
    /// to §8, owner ask 2026-09-09). RAM-only, like `active_toasts` in the
    /// shells this replaced (native's own `Vec<mlc_alert_delivery::
    /// ToastNotification>`, the web shell's own `Vec<(SystemEvent, f64)>`)
    /// — a toast on screen when the app closes is simply gone on relaunch.
    pub(crate) active_toasts: Vec<system_events::ToastItem>,
    /// Toast body drag in flight — `(toast_id, x, y)`, the CURRENT pointer
    /// position. Toasts do not visually follow the drag (there is nothing
    /// to paint mid-drag); only the drop point matters, read by
    /// [`Self::set_toast_anchor_from_drop`] at drag-end.
    pub(crate) toast_drag: Option<(u64, f64, f64)>,

    /// Raw stored workspace focus (chart canvas vs. the Trading tab's
    /// container) — read via [`Self::workspace_focus`], never directly.
    /// See `focus.rs` module docs for the write sites and the self-heal
    /// read-time fallback.
    pub(crate) workspace_focus: focus::WorkspaceFocus,

    /// Watchlist UI/presentation state — active list tab, column config,
    /// group color/collapsed, sort-order snapshot. Doctrine 1B split from
    /// `WatchlistManager` (backend owns domain: lists/groups/color_flags).
    pub watchlist_ui: watchlist_ui::WatchlistUiState,

    /// Symbol the watchlist market-counters block (funding rate + OI Δ%,
    /// owner spec 2026-08-01) is currently subscribed for, across every
    /// `KNOWN_EXCHANGES` venue — `None` when there is no active chart
    /// instrument. RAM-only (never persisted): re-derived from the active
    /// container every frame in `prepare_frame`, which also drives the
    /// subscribe/unsubscribe transition when this changes.
    pub(crate) counters_symbol: Option<String>,

    /// Right sidebar render result from the last frame (hit zones for click dispatch).
    pub last_sidebar_result: Option<mlc_app_sidebar_content::render::RightSidebarResult>,

    /// True while the user is dragging the right-sidebar separator to resize it.
    ///
    /// Set in `on_drag_start` when the drag begins on `"right_sidebar_separator"`,
    /// cleared in `on_drag_end`.
    pub(crate) sidebar_separator_drag_active: bool,

    /// When true, split operations create the new window as an independent
    /// auto-created group instead of inheriting the source window's tag.
    pub split_without_group: bool,

    /// Left edge X of the right toolbar from the last render.
    ///
    /// Used during sidebar separator drag to compute the new sidebar width as
    /// `right_toolbar_left_x - mouse_x`.
    pub(crate) right_toolbar_left_x: f64,

    /// Currently selected indicator id (for selection marker rendering).
    ///
    /// Set when the user left-clicks on an indicator line; cleared when the user
    /// clicks on a primitive or on empty chart background.
    pub selected_indicator_id: Option<u64>,

    /// Standalone outer-dock panel registry (Step 10 doctrine cutover) —
    /// owns the watchlist overlay modal singletons. See
    /// `standalone_panel::StandalonePanelRegistry` module docs for why the
    /// tag-following multi-instance case (Dom/L2Tape/Footprint/...) is NOT
    /// here — it's already correctly owned by
    /// `mlc_backend::tag::TagManager::panel_members`.
    pub standalone_panels: standalone_panel::StandalonePanelRegistry,

    /// Render result from the last watchlist modal frame (hit zones for click/scroll dispatch).
    pub(crate) last_watchlist_modal_result: Option<mlc_app_layout::layout::modals::watchlist_modal::WatchlistModalResult>,

    /// Render result from the last watchlist group name input modal frame (hit zones for drag dispatch).
    pub(crate) last_wl_group_name_result: Option<WlGroupNameInputResult>,

    /// Set `true` when a preset was restored at startup before the first
    /// `resize()`.  On the first resize (when `chart_width` becomes known)
    /// the viewport is repositioned so the last bar sits with 5 empty bars
    /// of right margin.
    pub needs_initial_viewport_fit: bool,

    /// Shared backend data plane — bridge, mini-ticker cache, exchange symbol
    /// catalogue, trading manager. One per process; every `ChartApp` instance
    /// holds an `Arc::clone`. See `backend_core.rs` for the migration plan
    /// (more fields move here in subsequent passes).
    pub core: std::sync::Arc<mlc_backend::BackendCore>,

    /// App-level tag presentation sidecar.  Domain tag data (color,
    /// auto_created, members, member_overrides, member routing, synced
    /// panels) lives in `core.tags()` / `core.tags_mut()`
    /// (`mlc_backend::tag::TagManager`, doctrine 1B). This registry holds
    /// only which windows display a tag (`ui_container_ids`,
    /// `standalone_panel_ids`) and which tag the active chart currently
    /// shows (`active_chart_tag`) — presentation, not persisted.
    ///
    /// All tag lifecycle goes through [`Self::create_tagged_bubble`] /
    /// [`Self::create_auto_bubble`] / [`Self::remove_tag`] /
    /// [`Self::connect_member`] / [`Self::disconnect_member`] so that
    /// the bubble + tag pair stays in sync.
    pub tag_ui: tag_ui::TagUiRegistry,

    /// Currently active exchange.
    pub active_exchange: digdigdig3::ExchangeId,

    /// Unique identifier for this OS window (e.g. "win_1728503941").
    ///
    /// Used when performing a coordinated multi-window save so that each
    /// window's tab/preset state is stored under its own slot in
    /// [`UserProfile::windows`].
    pub window_id: String,

    /// Last known OS window position/size — updated by the windowing layer
    /// (main.rs) so that `build_window_state()` can include them without
    /// requiring access to the winit `Window` handle.
    pub window_x: Option<i32>,
    pub window_y: Option<i32>,
    pub window_width: Option<u32>,
    pub window_height: Option<u32>,

    /// Set to true when this window's tab/preset state changed and needs saving.
    /// Checked and cleared by App each frame.
    pub profile_dirty: bool,
    /// Shift held — 8-compass / square constrain while placing drawings.
    pub(crate) shift_held: bool,
    /// Live preset state for open tabs (except the active one).
    /// Key = preset id. On tab switch: park active → cache, unpark target → active.
    live_preset_cache: std::collections::HashMap<String, persistence::live_cache::LivePresetState>,
    /// Live | Loading { target_id, .. }. While `Loading`, autosave is gated
    /// (`flush_pending_autosave` early-returns, `PresetAction::Upsert` drain
    /// skips) and `render_full_chart_panel` forces the loading skeleton —
    /// the `7a9d1d2` disaster path (placeholder grid serialised under the
    /// NEW preset id) is impossible. See `persistence::load_state`.
    pub load_state: crate::persistence::LoadState,
    /// Set to true when window geometry (position/size) changed.
    /// Triggers local save but NOT cloud sync.
    pub profile_geometry_dirty: bool,
    /// Set to true when watchlist data changed and needs saving.
    /// Checked and cleared by App each frame.
    pub watchlists_dirty: bool,

    /// Set to true whenever [`Self::persist_alerts`] runs — i.e. the
    /// account-wide alert store changed and `alerts.json` was just
    /// rewritten. Checked and cleared by App each frame, same drain
    /// idiom as `profile_dirty`/`watchlists_dirty` above, except this one
    /// does not gate a disk write (alerts.json is already written
    /// synchronously by `persist_alerts` itself) — it only tells the shell
    /// to push a `SyncPushChanged(["alerts"])` command to the updater so a
    /// changed/deleted alert reaches the server without waiting on the
    /// next unrelated sync trigger.
    pub alerts_sync_dirty: bool,

    /// Queued watchlist mutations — drained by App each frame and applied to AppState.
    /// Windows never mutate watchlist directly; they push actions here.
    pub watchlist_actions: Vec<WatchlistAction>,

    /// Queued connector mutations — drained by App each frame and applied to AppState.
    /// Windows never mutate connector state directly; they push actions here.
    pub connector_actions: Vec<ConnectorAction>,

    /// Queued preset mutations — drained by App each frame and applied to AppState.
    pub preset_actions: Vec<PresetAction>,

    /// Pending autosave target. `Some(preset_id)` means a callsite has
    /// requested an autosave for that exact preset; `None` means no autosave
    /// is queued. Drained by `flush_pending_autosave` once per frame.
    ///
    /// CRITICAL: this captures `active_preset_id` AT THE MOMENT OF
    /// `autosave_snapshot()` call. A previous version used a bare `bool` +
    /// re-read `active_preset_id` at flush time — but on a preset switch
    /// the handler advances `active_preset_id` to the NEW id BEFORE the
    /// end-of-tick flush runs, so flush would persist the new preset
    /// instead of the outgoing one. The outgoing preset's user edits
    /// (viewport pan, scale, drawings made just before the switch) were
    /// silently lost — visible to owner as "data disappears on switch".
    pub pending_autosave: Option<String>,

    /// Preset ids currently open as read-only shared-chart tabs (native
    /// share-viewer arc, 2026-09-06) — set by `share_viewer::open_shared_preset`.
    /// Per-preset-id, NOT a shell-global flag: native holds several tabs at
    /// once and only the ones opened from a share link are read-only.
    /// Checked by every mutation site that writes drawings, indicators, or
    /// preset state before it writes — see
    /// [`share_viewer`]'s `ChartApp::is_readonly_tab`.
    pub(crate) readonly_preset_ids: std::collections::HashSet<String>,

    /// Metadata (owner label, rights, expiry) for every id in
    /// `readonly_preset_ids` — the tab chrome reads this for the label and
    /// the "Clone" action. An entry here always implies membership there;
    /// kept as a separate map (not one enum-valued map) because
    /// `is_readonly_tab` is a hot-path check (autosave, edit recording) and
    /// this metadata is cold, chrome-render-only.
    pub(crate) shared_chart_meta: std::collections::HashMap<String, share_viewer::SharedChartMeta>,

    /// Pending "open this share" request — set by
    /// `share_viewer::ChartApp::request_open_share`, drained by the shell,
    /// which owns the HTTP fetch (`ChartApp` does no network IO — SWC crate
    /// contract). Mirror of `pending_share_live`'s create-side mailbox.
    pub(crate) pending_open_share: Option<String>,

    /// Pending "clone this share" request — mirror of `pending_open_share`.
    pub(crate) pending_clone_share: Option<String>,

    /// Boot-time re-fetch queue for shared tabs that were open when the
    /// profile was last saved (shared-tab-reload-anchor arc, 2026-09-23).
    /// `new_window`'s restore path seeds one entry per placeholder it
    /// installs via `share_viewer::ChartApp::install_shared_tab_
    /// placeholders`; the shell drains this once via
    /// [`Self::take_pending_reopen_shares`] and re-fetches each token
    /// through whatever transport it already uses for a fresh
    /// `open_shared_preset`, then reports the outcome back via
    /// `fill_shared_placeholder`/`share_reopen_gone`/
    /// `share_reopen_transient`.
    pub(crate) pending_reopen_shares: Vec<share_viewer::PendingReopenShare>,

    /// `placeholder preset id -> share token` for every boot-time
    /// placeholder still waiting on its re-fetch outcome. `shared_chart_meta`
    /// only gains an entry once the real preset lands (`open_shared_preset`/
    /// `fill_shared_placeholder`), so a still-pending placeholder has no
    /// other record of which token it came from — needed by
    /// `share_viewer::ChartApp::collect_open_shared_tabs` so a save that
    /// lands mid-fetch still persists the tab's slot instead of losing it.
    pub(crate) pending_share_tokens: std::collections::HashMap<String, String>,

    /// Shared tabs this session gave up re-fetching after a TRANSIENT
    /// failure (offline / 5xx — item 2 of the shared-tab-reload-anchor
    /// arc): the placeholder tab is dropped for the rest of THIS session
    /// (no retry loop), but the token + saved index are kept here so the
    /// NEXT boot tries again, unlike a GONE (404/410/403) outcome, which
    /// drops the entry for good. Folded into `open_shared_tabs` at save
    /// time by `collect_open_shared_tabs` in addition to whatever is still
    /// live in `open_tabs`.
    pub(crate) deferred_share_reopens: Vec<mlc_core::user_profile::profile::PersistedSharedTab>,

    /// Queued settings snapshot mutations — drained by App each frame and applied to AppState.
    pub snapshot_actions: Vec<SnapshotAction>,

    /// Queued template mutations — drained by App each frame and applied to AppState.
    pub template_actions: Vec<TemplateAction>,

    /// Performance control actions queued by sidebar clicks.
    pub perf_actions: Vec<PerfAction>,

    /// Set when the user switches theme preset in this window.
    /// Drained by App in about_to_wait(); App then updates AppState.theme_preset
    /// and syncs the new preset to all windows.
    pub theme_changed: Option<String>,

    /// Set when the user selects a new RecalcMode in the User Settings modal.
    /// Drained by App in about_to_wait(); App then updates AppState.recalc_mode
    /// and syncs the new mode to all windows.
    pub recalc_mode_changed: Option<String>,

    /// Set when the user selects a new language in the User Settings General tab.
    /// Drained by App in about_to_wait(); App then applies set_language(), propagates
    /// to all windows, and saves to the active profile.
    pub language_changed: Option<String>,

    /// Signal: server enabled/disabled changed. Consumed by main.rs.
    pub server_enabled_changed: Option<bool>,

    /// Signal: API key was changed/generated. Consumed by main.rs.
    pub local_agent_key_changed: Option<String>,

    /// Signal: text to copy to clipboard. Consumed by main.rs.
    pub clipboard_text: Option<String>,

    /// Rendered-frame counter, bumped once per `render_to_scene`.
    ///
    /// Drives the toolbar clock's hover animation, which advances one step
    /// per frame so it runs at the app's own FPS cadence rather than on a
    /// wall-clock quantum of its own (owner's ruling 2026-09-02). Wraps;
    /// every consumer only cares that consecutive frames differ.
    pub anim_frame: u64,

    /// Signal: user clicked "Create" in the key manager. Consumed by main.rs
    /// which generates the key, calls `agent_state.add_key()`, and writes the
    /// raw key back into `user_settings_state.last_created_key`.
    pub key_create_request: Option<(String, String)>, // (label, tier)

    /// Signal: user clicked delete [X] for a managed key. Consumed by main.rs
    /// which calls `agent_state.remove_key()` and refreshes `managed_keys`.
    pub key_delete_request: Option<String>, // label to delete

    /// Signal: request to refresh `managed_keys` from AgentState. Set after
    /// create/delete so main.rs re-reads the list on the next about_to_wait().
    pub key_list_refresh: bool,

    /// Signal: open a URL in the system browser. Set by click handlers in input.rs,
    /// drained by main.rs which calls `open::that()`.
    pub pending_open_url: Option<String>,

    /// Signal: send a command to the OTA updater. Set by click handlers in input.rs,
    /// drained by main.rs which forwards it to `updater_handle.cmd_tx`.
    pub pending_updater_cmd: Option<String>, // "logout" | "start_oauth:{provider}"

    /// Cure work deferred from LoadPreset's warm path. The handler swaps
    /// preset state in ~3ms (park + unpark + active id), then stashes the
    /// per-window cure tuples here and returns IMMEDIATELY. The host drains
    /// this queue AFTER the first post-swap frame is presented so the user
    /// sees loading skeletons + spinners while the 100-220ms `request_bars`
    /// loop runs, instead of staring at the old chart for that long.
    ///
    /// Tuple: (exchange_id, symbol, timeframe, account_type, force).
    pub pending_cure_after_preset_load: Vec<(
        digdigdig3::ExchangeId,
        String,
        mlc_core::state::Timeframe,
        digdigdig3::AccountType,
        bool,
    )>,

    /// Cold-path preset apply, deferred by one frame.
    ///
    /// The `LoadPreset` handler stashes the cloned `ChartPreset` here and
    /// flips `load_state = Loading`. The next frame paints the skeleton (no
    /// chart yet because `panel_grid` is `placeholder()`), and only on the
    /// frame AFTER that does the host call `drain_pending_cold_apply`,
    /// which runs the actual 1.3-1.8s `apply_preset_cold` body and clears
    /// `load_state` back to `Live`.
    ///
    /// The intermediate skeleton frame is what owner sees instead of a
    /// 2-second freeze.
    pub pending_cold_apply: Option<PendingColdApply>,

    /// Background pre-warm queue. Populated once `load_state == Live` for
    /// the first time post-startup with every `open_tabs` entry that is
    /// NOT the currently active preset and NOT already in
    /// `live_preset_cache`. Drained one entry per frame by the host —
    /// each entry: park current Live state → cold-apply target → park
    /// target into cache → unpark original. All within one event-loop
    /// tick so no intermediate frame renders. Cold body on warmed
    /// wire_symbol cache is ~1-5ms per preset → ~6-50ms total spread
    /// across as many frames as there are tabs. Owner gets warm cache
    /// hits on every tab click going forward (the chrome-bar visible
    /// set was the explicit ask).
    pub pending_background_warm: std::collections::VecDeque<String>,
    /// Set to true once `pending_background_warm` has been seeded from
    /// `open_tabs` (done one-shot the first time we observe
    /// `load_state == Live` with an active preset). Prevents re-seeding
    /// on every subsequent frame.
    pub background_warm_seeded: bool,

    /// Count of broadcast lag events since startup (for performance monitoring).
    pub lag_event_count: u64,

    /// Number of indicator recalculations performed since the last log flush.
    recalc_count: u32,

    /// Timer controlling the periodic RecalcMode diagnostic log (every 5 s).
    recalc_log_timer: web_time::Instant,

    /// Timer gating the idle-parked-preset release sweep — see
    /// [`frame_lifecycle::tick::IDLE_RELEASE_SWEEP_INTERVAL`] and
    /// [`Self::release_idle_parked_presets`].
    idle_release_sweep_timer: web_time::Instant,

    /// Number of TradeUpdate events received since the last log flush.
    trade_count: u32,

    /// Whether the periodic RecalcMode diagnostic log is enabled.
    /// Controlled via the Performance tab toggle in User Settings.
    pub diagnostics_enabled: bool,

    /// Sidebar data needs rebuild — set by mutations, cleared after sidebar populate.
    ///
    /// When `false` the sidebar population block is skipped entirely, saving the
    /// per-frame cost of iterating primitives, indicators, watchlist entries, and
    /// the connector registry. Set to `true` by `tick()` whenever a `LiveUpdate`
    /// message is received, and by sidebar panel toggle handlers in `input.rs`.
    ///
    /// Also read by `chart-app-vello` to propagate the dirty state to the
    /// per-window `sidebar_dirty_scene` flag for scene caching.
    pub sidebar_data_dirty: bool,

    /// Wall-clock of the last `sidebar_data_dirty = true` bump triggered by
    /// a MiniTickerUpdated event. Used by the rate-limit in tick.rs so the
    /// watchlist refreshes ~4 Hz instead of once per per-symbol tick.
    pub sidebar_ticker_mark: Option<web_time::Instant>,

    /// Tracks the last active leaf so we can detect leaf switches and
    /// mark sidebar_data_dirty automatically.
    last_active_leaf: Option<mlc_core::LeafId>,

    /// Last render timing breakdown in microseconds (for PERF diagnostics).
    /// Tuple: (chart_us, toolbar_us, sidebar_us, setup_us)
    /// - chart_us:   time spent in the chart render block (split or single)
    /// - toolbar_us: time spent rendering toolbars + registering toolbar hit zones
    /// - sidebar_us: time spent rendering sidebar, watchlist modal, and other modals
    /// - setup_us:   time spent in layout setup before chart render starts
    pub render_timing_us: (u64, u64, u64, u64),

    // ── Sub-pane height restore ───────────────────────────────────────────────
    /// Per-window sub-pane height ratios pending application on the next
    /// `sync_sub_panes_from_manager` call.  Populated during `LoadPreset` from
    /// the saved `ChartWindowSnapshot::sub_pane_height_ratios`.
    ///
    /// Format: `window_id → (instance_id → height_ratio)`.  Cleared after the
    /// ratios are applied.
    pub(crate) pending_sub_pane_ratios: std::collections::HashMap<u64, std::collections::HashMap<u64, f32>>,
    /// Instance ids already given a one-shot heal recalc (tick.rs indicator
    /// heal). Prevents per-tick recalc storms for indicators that
    /// legitimately produce no values.
    pub(crate) indicator_heal_attempted: std::collections::HashSet<u64>,
    /// Per-window set of sub-pane instance_ids that should have `above_main = true`
    /// after the next `sync_sub_panes_from_manager` call.  Populated during
    /// `LoadPreset` from `ChartWindowSnapshot::sub_pane_above_main`.
    ///
    /// Format: `window_id → HashSet<instance_id>`.  Cleared after applied.
    pub(crate) pending_sub_pane_above_main: std::collections::HashMap<u64, std::collections::HashSet<u64>>,
    /// Per-window ordered list of sub-pane instance_ids to restore the saved
    /// Vec order (above-main first, then below-main).  Populated during
    /// `LoadPreset` from `ChartWindowSnapshot::sub_pane_order`.
    ///
    /// Format: `window_id → Vec<instance_id>`.  Cleared after applied.
    pub(crate) pending_sub_pane_order: std::collections::HashMap<u64, Vec<u64>>,
    /// Per-window set of sub-pane instance_ids that should be hidden after
    /// the next `sync_sub_panes_from_manager` call.  Populated during
    /// `LoadPreset` from `ChartWindowSnapshot::sub_pane_hidden`.
    ///
    /// Format: `window_id → HashSet<instance_id>`.  Cleared after applied.
    pub(crate) pending_sub_pane_hidden: std::collections::HashMap<u64, std::collections::HashSet<u64>>,

    /// Per-window maximized sub-pane state.
    ///
    /// When a sub-pane is expanded (maximized), this map stores:
    /// - key: `window.id.0` (ChartId)
    /// - value: `(maximized_instance_id, pre_maximize_proportions)`
    ///
    /// The pre-maximize proportions are the `inner_docking` root proportions
    /// before the maximize, used to restore the original layout on Restore.
    /// Runtime-only — not persisted to disk.
    pub(crate) maximized_sub_panes: std::collections::HashMap<u64, (u64, Vec<f64>)>,

    /// Agent session manager — owns PTY and pipe sessions.
    ///
    /// Call `drain_events` each frame (in `tick`) to process incoming terminal
    /// output. Call `snapshot` to read rendering state without OS handles.
    pub agent: agent::AgentSessionManager,

    /// True when the PTY panel received focus via hover rather than a click.
    ///
    /// Hover-focus is weaker than click-focus: it is cleared when the cursor
    /// leaves the PTY area, whereas click-focus persists until an explicit
    /// blur.
    pub agent_pty_hover_focused: bool,

    /// True while a host-side PTY text-selection drag is in progress.
    pub agent_pty_drag_active: bool,

    /// True while a host-side chat text-selection drag is in progress.
    pub agent_chat_drag_active: bool,

    /// Set to `true` after `autostart_all` has been called once.
    ///
    /// The autostart fires on the first frame of the event loop so that a
    /// Tokio runtime is guaranteed to be available via `core.native_rt()`.
    pub agent_autostarted: bool,

    // ── Leak / backpressure diagnostics (snapshotted each tick) ──────────────
    /// Pending events in the broadcast queue after this tick's drain (backpressure).
    pub diag_queue_len: usize,
    /// Live `mini_ticker_cache` map size (should be bounded by watchlist size).
    pub diag_mini_ticker: usize,

    // ── Internal CPU profiling timers (updated each tick, read by sidebar) ────
    /// Total time spent in the last tick() call, in microseconds.
    pub last_tick_us: u64,
    /// Time spent in indicator recalculation (calculate_for_window) in last tick.
    pub last_indicator_recalc_us: u64,
    /// Time spent processing LiveUpdate events in the last tick.
    pub last_event_process_us: u64,
    /// Accumulated time in calc_auto_scale() calls during the last tick.
    pub last_auto_scale_us: u64,

    /// Scene composite time (scene.reset + scene.append for chart/overlay/sidebar/toolbar)
    /// measured in the last build_window_scene call, in microseconds.
    /// Written by scene_builder.rs, read by the PERF populate block in main.rs.
    pub last_composite_us: u64,

    /// Accumulated time in orderbook panel .tick() calls (DOM + L2Tape + LiquidityHeatmap)
    /// for OrderbookSnapshot and OrderbookDelta events during the last tick.
    pub last_orderbook_panel_us: u64,
    /// Per-panel breakdown: DOM .tick() time (μs) in last tick.
    pub last_dom_panel_us: u64,
    /// Per-panel breakdown: L2Tape .tick() time (μs) in last tick.
    pub last_l2_panel_us: u64,
    /// Per-panel breakdown: LiquidityHeatmap .tick() time (μs) in last tick.
    pub last_heatmap_panel_us: u64,
    /// Accumulated time in trade panel .tick() calls (BigTrades + VolumeProfile + Footprint + TradeTape)
    /// for TradeUpdate events during the last tick.
    pub last_trade_panel_us: u64,
    /// Accumulated time in bar apply operations (BarsLoaded + BackfillComplete + ScrollBarsLoaded + TradeUpdate bar update)
    /// during the last tick.
    pub last_bar_apply_us: u64,
    /// Number of OrderbookSnapshot + OrderbookDelta events drained during the last tick.
    pub last_ob_event_count: u32,
    /// Number of TradeUpdate + BarUpdate events drained during the last tick.
    pub last_trade_event_count: u32,

    /// Heavy state for all trading panels docked into the free-slot sidebars.
    ///
    /// Keyed by `PanelId`. The matching `Panel` leaves in each slot's
    /// `DockingManager<Panel>` carry only the `PanelId`, keeping
    /// `sidebar-content` free of `zengeld-panels`.
    pub panels_store: panels_store::TradingPanelsStore,

    /// Active agent-panel separator drag.
    ///
    /// `(separator_index, start_mouse_pos, total_available_size)` — the total
    /// available size (width for vertical separators, height for horizontal) is
    /// captured at drag-start so that `drag_separator` can convert pixel deltas
    /// to proportions correctly even as the window resizes.
    pub(crate) agent_sep_drag: Option<(usize, f64, f32)>,

    /// Active free-slot separator drag.
    ///
    /// `(slot_index, separator_index, start_mouse_pos, total_available_size)`.
    pub(crate) slot_sep_drag: Option<(usize, usize, f64, f32)>,

    /// Active sub-pane separator drag (between main chart and sub-pane, or
    /// between two sub-panes).  Mirrors slot_sep_drag pattern.
    ///
    /// `(instance_id, start_mouse_y, available_height)`.
    /// Active sub-pane separator drag:
    /// `(instance_id, start_cursor_y, content_w, content_h, _, _)`.
    /// All math goes through `ChartContainer::drag_sub_pane_separator_via_uzor`
    /// which delegates to uzor's `inner_docking.drag_separator` -- the same
    /// API the sidebar/agent/outer-grid separators already use. Last two
    /// tuple slots are reserved for forward-compat and unused today.
    pub(crate) sub_pane_sep_drag: Option<(u64, f64, f64, f64, f64, Option<u64>)>,

    /// Active coordinator-routed panel drag (DOM, L2Tape, Footprint, BigTrades, LiquidityHeatmap, VolumeProfile).
    ///
    /// `(panel, local_id, last_x, last_y)` — replaces per-panel fields.
    pub(crate) active_drag_panel: Option<(mlc_core::state::Panel, String, f64, f64)>,

    /// Active TradeTape drag-to-scroll.
    ///
    /// `(panel_id, last_x, last_y)`.
    pub(crate) slot_tradetape_drag: Option<(mlc_core::state::PanelId, f64, f64)>,

    /// Active-leaf (or single-window) chart geometry for the current frame.
    ///
    /// PRODUCED once per frame by the layout phase (`sync_sub_pane_geometry`),
    /// CONSUMED read-only by input handlers and `process_output_actions`.
    /// Plain mouse movement never changes geometry, so it is built in the layout
    /// phase rather than rebuilt per mouse event (was a 1500+ calls/sec hotspot).
    pub(crate) active_frame_layout: Option<mlc_core::ExtendedFrameLayout>,
    /// Per-leaf chart geometry for split mode, keyed by LeafId. Same lifecycle.
    pub(crate) frame_layouts: std::collections::HashMap<mlc_core::LeafId, mlc_core::ExtendedFrameLayout>,

    /// Compare symbols waiting for their first bar delivery.
    ///
    /// Populated when `add_compare_symbol` returns `false` because the
    /// `LiveDataProvider` cache missed and fired an async `request_bars`.
    /// Drained in `react_to_bubble_event / BarsLoaded` once the data arrives.
    /// Each entry carries a `deadline_frames` counter (decremented per tick)
    /// so stale entries are evicted after ~10 s without a server response.
    pub(crate) pending_compare: Vec<PendingCompareAdd>,

    /// Diagnostic counters for compare-add attempts.  Updated in
    /// `OpenModal::CompareSearch` click handler in `modals.rs`.
    pub(crate) compare_add_attempts: u32,
    pub(crate) compare_add_success: u32,
    pub(crate) compare_add_queued: u32,

    /// Transient state for the `timeframe_selector` dropdown's visible
    /// inline granularity editor (user-granularity-templates-inner arc,
    /// кусок 2). `None` when no "+ Add"/edit editor is active.
    pub(crate) granularity_edit: Option<GranularityEdit>,

    /// Pending granularity row drag (owner spec 2026-07-16 items 2+3) — set
    /// on mousedown over a reorderable FAVORITES/CUSTOM row, promoted to
    /// `granularity_drag` past the ~4px threshold. `None` outside a
    /// just-started possible drag.
    pub(crate) granularity_drag_pending: Option<GranularityDragPending>,
    /// Active granularity row drag-to-reorder. `None` when no row is being
    /// dragged.
    pub(crate) granularity_drag: Option<GranularityDrag>,

    /// Transient state for the `symbol_selector` toolbar dropdown's inline
    /// fast-search editor (symbol-search-compare-inner arc, Wave 2, кусок 4
    /// — owner spec 2026-07-16, `ProfileUiSettings::symbol_quick_search`).
    /// `None` when the dropdown isn't showing the fast-search input (i.e.
    /// `symbol_quick_search == false`, dropdown closed, or the full-screen
    /// modal is open instead). Mirrors `granularity_edit`'s lifecycle:
    /// created on dropdown open with an empty query, cleared on dropdown
    /// close / Escape / apply.
    pub(crate) symbol_quick_edit: Option<SymbolQuickEdit>,

    /// `Some(pid)` while the Trading Container header-tab's TF favorites
    /// popup is open for that container (trading-header-tf arc, 2026-07-25)
    /// — lists `GranularityFavorites::starred_tfs` (the SAME store the
    /// topbar's `timeframe_selector` dropdown reads/writes) anchored under
    /// the header's TF button. `None` when closed. Toggled by
    /// `"trading_header:tf_button:{pid}"`, cleared on row pick / Escape /
    /// click-away.
 
    /// Toolbar UI state for the trading container's EMBEDDED header toolbar
    /// (owner spec 2026-07-25: the container header is an alternative top
    /// toolbar). Separate from `panel_app.toolbar_state` so its open
    /// dropdown / hover never collide with the main topbar's; dispatch
    /// enforces mutual exclusion between the two.
    pub(crate) trading_header_tb: mlc_core::state::ChartToolbarState,
    /// Last render result of the embedded header toolbar — item rects for
    /// `tch:{id}` hit-zone registration and popup anchoring.
    pub(crate) last_trading_header_result: Option<mlc_core::ChartToolbarRenderResult>,

    /// In-RAM clipboard for Ctrl+C/Ctrl+V on a selected drawing primitive.
    /// `None` when nothing has been copied — never persisted, never synced
    /// across windows (process-local, cleared on restart).
    pub(crate) primitive_clipboard: Option<PrimitiveClipboard>,

    /// Active inline on-chart text edit of a placed text primitive
    /// (double-click arc, 2026-08-26) — see [`InlinePrimTextEdit`]. RAM-only,
    /// never persisted: an in-progress edit belongs to the live frame.
    pub(crate) inline_prim_text_edit: Option<InlinePrimTextEdit>,

    /// Embedded chart in the trading container's CENTER region
    /// (chart-in-container arc, 2026-07-26): PanelId of the trading
    /// container → ChartId of its aux `ChartContainer`. The aux container
    /// lives IN `panel_grid.containers` (so every reactive sweep, park/
    /// unpark, and BarsLoaded reconcile handles it for free) but has NO
    /// docking leaf / `leaf_to_chart` entry — the grid never lays it out,
    /// the preset snapshot never captures it. RAM-only: identity re-derives
    /// from the container's symbol key; `replace_containers` (preset
    /// cold-load) wipes the map, so entries here are valid ONLY while the
    /// map still holds the id (see `ensure_trading_center_charts`).
    pub(crate) trading_center_charts: std::collections::HashMap<mlc_core::state::PanelId, mlc_core::state::ChartId>,
    /// Last rendered center-region rect per trading container — cached by
    /// the render pass, consumed by the next tick's
    /// `ensure_trading_center_charts` to `resize_geometry` the aux chart
    /// (mirrors how the grid resizes per-leaf viewports in prepare).
    /// `None` while the Trading sidebar is closed or center shows DOM.
    pub(crate) trading_center_rect: Option<(mlc_core::state::PanelId, mlc_app_layout::layout::LayoutRect)>,
    /// Dedicated chart input handler for the embedded center chart —
    /// SECOND `DefaultChartInputHandler` instance so the aux chart gets
    /// the FULL engine mechanics (chart pan, price/time scale drags,
    /// wheel zoom by hit zone, double-click resets) with its own drag
    /// state, never colliding with the main grid's `input_handler`.
    pub(crate) trading_center_input: DefaultChartInputHandler,
    /// True while a drag gesture is owned by the embedded center chart
    /// (any drag mode: chart pan, price scale, time scale).
    pub(crate) trading_center_drag_active: bool,
    /// Scale-corner button rects from the center chart's last render —
    /// aux counterpart of `scale_corner_zones_by_leaf` (which is
    /// leaf-keyed and can never hold the leafless aux chart).
    pub(crate) trading_center_corner_zones: Option<ScaleCornerHitZones>,
    /// Active sub-pane separator drag on the center chart: the pane's
    /// indicator `instance_id` (aux counterpart of `sub_pane_sep_drag`).
    pub(crate) trading_center_sep_drag: Option<u64>,
    /// Indicator legend overlay open/closed state for the center chart —
    /// aux counterpart of the leaf-keyed `indicator_overlay_states`.
    pub(crate) trading_center_ind_overlay: mlc_app_layout::ui::modal_settings::IndicatorOverlayState,
    /// Trading Container modal (gear on the container header, owner spec
    /// 2026-07-28): card MAP of the hosted panels + setup section.
    pub(crate) tc_modal: mlc_app_layout::layout::modals::TradingContainerModalState,
    /// Last rendered Trading Container modal geometry — header drag reads
    /// the header rect off this between frames.
    pub(crate) tc_modal_result: Option<mlc_app_layout::layout::modals::TcModalResult>,
    /// Risk Calculator modal (gauge chip on the Order Entry header —
    /// overlay hosting, owner spec 2026-07-29). Standard modal state
    /// shape reused; the `section` field is unused.
    pub(crate) rc_modal: mlc_app_layout::layout::modals::TradingContainerModalState,
    /// Last rendered Risk Calculator modal geometry (header drag anchor).
    pub(crate) rc_modal_result: Option<mlc_app_layout::layout::modals::TcModalResult>,
    /// DOM settings modal (Stage B) — opened by the DOM section header gear.
    pub(crate) dom_modal: mlc_app_layout::layout::modals::TradingContainerModalState,
    /// Open standalone panel-settings surface (overlay gear, owner spec
    /// 2026-07-28): the target panel + the dropdown anchor. `None` = closed.
    pub(crate) panel_settings_open: Option<(mlc_core::state::Panel, (f64, f64))>,
    /// Last rendered panel-settings surface geometry.
    pub(crate) panel_settings_result: Option<mlc_app_layout::layout::modals::PanelSettingsPopupResult>,

    /// FIFO of watchlist rows still waiting for their background warm-up
    /// `request_kline` (feature: "warm the watchlist", owner 2026-08-30).
    /// Populated by the `ConnectorReady` reactor (`frame_lifecycle/react.rs`)
    /// for every row on the venue that just came up, at the active chart's
    /// timeframe; drained under `watchlist_warmup_budget`'s per-venue token
    /// bucket by `drain_watchlist_warmup`, called once per tick.
    pub(crate) watchlist_warmup_queue: std::collections::VecDeque<(digdigdig3::ExchangeId, mlc_core::instrument::InstrumentAddress, mlc_core::state::Timeframe)>,
    /// Per-venue token bucket gating `watchlist_warmup_queue`'s drain rate.
    /// See [`WarmupBudget`].
    pub(crate) watchlist_warmup_budget: std::collections::HashMap<digdigdig3::ExchangeId, WarmupBudget>,
    /// Every `(instrument, timeframe)` this session has already sent a
    /// background warm-up `request_kline` for (feature: "warm the
    /// watchlist"). The stopping condition for the warm-up sweep — some
    /// venues (dYdX, Gemini: REST-poll, no kline WS stream) never register
    /// a live kline subscriber, so the bridge's own dedup (which the
    /// per-open-chart-window kline refetch below relies on) does NOT skip a
    /// repeat non-force `request_kline` for them; without this set a
    /// `ConnectorReady` on such a venue re-warms every row on every fire
    /// (owner-measured 2026-08-30: ~1.3 req/s sustained against dYdX with
    /// nothing clicked). A key is warmed AT MOST ONCE and is only removed —
    /// re-arming a future warm-up — when the row itself is removed from the
    /// watchlist (`WatchlistAction::Remove` / `Toggle`-off, below). A
    /// timeframe change does not need explicit invalidation: the key
    /// includes the `Timeframe`, so a new active TF is simply a new,
    /// not-yet-warmed key.
    pub(crate) watchlist_warmed: std::collections::HashSet<(mlc_core::instrument::InstrumentAddress, mlc_core::state::Timeframe)>,

    /// Handle to the native host's RAM-only agent-key `SecretStore` —
    /// unlocked/sealed off the SAME `VaultKey` as the main vault, so every
    /// seal/unlock call-out in `input/credentials.rs` that already derives
    /// or drops one also keeps this store in lockstep (see
    /// [`Self::set_agent_key_store`]'s doc). `None` on any shell that never
    /// wires an Agent API — today, `mlc-app-shell-web`'s wasm build (see
    /// that shell's own `mlc-app-agent-api` doc note).
    pub(crate) agent_key_store: Option<std::sync::Arc<mlc_vault::SecretStore>>,

    /// Handle to the native host's RAM-only Telegram/webhook delivery
    /// `SecretStore` sidecar (`alert_secrets.enc`) — same shape and same
    /// seal/unlock lockstep as [`Self::agent_key_store`] (see
    /// `input/credentials.rs`'s `vault_key_sidecars`), just holding
    /// [`mlc_alert_delivery::NotificationSecrets`] instead of agent-key
    /// records. `None` on any shell that never wires native delivery —
    /// today, `mlc-app-shell-web`'s wasm build.
    pub(crate) alert_secrets_store: Option<std::sync::Arc<mlc_vault::SecretStore>>,
}

/// An action that mutates the app-level watchlist.
/// Queued by windows, drained and applied by App.
#[derive(Debug, Clone)]
pub enum WatchlistAction {
    /// Toggle symbol in/out of watchlist (star button in search overlay).
    Toggle { symbol: String, exchange: String, account_type: String },
    /// Remove a specific symbol from watchlist.
    Remove { symbol: String, exchange: String, account_type: String },
    /// Reorder a symbol within the active list (drag & drop).
    Reorder { from_idx: usize, to_idx: usize },
    /// Create a new watchlist with given name.
    CreateList { name: String },
    /// Rename an existing watchlist.
    RenameList { id: u64, new_name: String },
    /// Delete a watchlist by id.
    DeleteList { id: u64 },
    /// Switch to a different active watchlist.
    SetActiveList { id: u64 },
    /// Clear the order snapshot on the active list.
    ClearOrderSnapshot,
    /// Set a color flag on a symbol.
    SetColorFlag { symbol: String, exchange: String, account_type: String, color: Option<String> },
    /// Move symbol into a group.
    MoveToGroup { symbol: String, exchange: String, account_type: String, group_name: String },
    /// Remove symbol from its group (back to ungrouped).
    RemoveFromGroup { symbol: String, exchange: String, account_type: String },
    /// Set column separator offsets.
    SetSeparatorOffsets { offsets: Vec<f64> },
    /// Reset column separator offsets to equal widths.
    ResetSeparatorOffsets,
    /// Set a single column separator offset by index.
    SetSeparatorOffset { index: usize, value: f64 },
    /// Toggle visibility of a watchlist column.
    ToggleColumnVisibility { column: String },
    /// Cycle sort mode: 0 → 1 → 2 → 0.
    SortCycle,
    /// Reset sort mode to 0 (unsorted).
    ResetSort,
}

/// An action that mutates app-level connector state.
/// Queued by windows, drained and applied by App.
#[derive(Debug, Clone)]
pub enum ConnectorAction {
    /// Toggle a connector's enabled/disabled state.
    ToggleEnabled { exchange_id: String },
}

/// An action that mutates app-level preset state.
/// Queued by windows, drained and applied by App each frame.
#[derive(Debug, Clone)]
pub enum PresetAction {
    /// Upsert a preset — stores a fully-collected ChartPreset into AppState.
    Upsert(mlc_core::preset::preset::ChartPreset),
    /// Delete a preset by id (removes from memory and disk).
    Delete { id: String },
    /// Rename a preset in-place.
    Rename { id: String, new_name: String },
}

/// An action that mutates the app-level settings snapshots.
/// Queued by windows, drained and applied by App each frame.
#[derive(Debug, Clone)]
pub enum SnapshotAction {
    /// Update the chart settings snapshot.
    ChartSettings(serde_json::Value),
    /// Update the primitive settings snapshot for a given type_id.
    PrimitiveSettings { type_id: String, data: serde_json::Value },
    /// Update the indicator settings snapshot for a given type_id.
    IndicatorSettings { type_id: String, data: serde_json::Value },
    /// Update the compare overlay settings snapshot.
    CompareSettings(serde_json::Value),
    /// Update the last-used drawing style for a given primitive `type_id`.
    ///
    /// Persisted globally so the next primitive of the same type is
    /// pre-populated with the user's last-used color, width, line style, and
    /// extended style properties.
    DrawingStyle { type_id: String, data: serde_json::Value },
}

/// An action that mutates app-level template state.
/// Queued by windows, drained and applied by App each frame.
#[derive(Debug, Clone)]
pub enum TemplateAction {
    /// Add a primitive style template.
    AddPrimitive(mlc_core::templates::primitive_template::PrimitiveTemplate),
    /// Remove a primitive template by id.
    RemovePrimitive { id: String },
    /// Rename a primitive template by id (Template menu's own per-row
    /// pencil action, item 4, 2026-08-08 review).
    RenamePrimitive { id: String, name: String },
    /// Replace the Template menu's merged system-preset + user-template
    /// display order wholesale (the menu's own drag-to-reorder commit,
    /// item 4, 2026-08-08 review).
    ReorderPrimitiveTemplates { order: Vec<String> },
    /// Add an indicator template.
    AddIndicator(mlc_core::templates::indicator_template::IndicatorTemplate),
    /// Remove an indicator template by id.
    RemoveIndicator { id: String },
    /// Add a compare overlay template.
    AddCompare(mlc_core::templates::compare_template::CompareTemplate),
    /// Remove a compare template by id.
    RemoveCompare { id: String },
    /// Add a chart settings template.
    AddChart(mlc_core::templates::chart_template::ChartTemplate),
    /// Remove a chart template by id.
    RemoveChart { id: String },
    /// Add an indicator set.
    AddIndicatorSet(mlc_core::templates::indicator_set::IndicatorSet),
    /// Remove an indicator set by id.
    RemoveIndicatorSet { id: String },
}

impl ChartApp {
    /// Apply everything the UI queued onto the shared template store and empty
    /// the queue. Returns whether anything was applied, which is the only part
    /// a shell has an opinion about — the native one uses it to mark the store
    /// dirty for its disk writer.
    ///
    /// This lives on `ChartApp`, not in a shell, because `ChartApp` already
    /// holds the same `Arc<BackendCore>` the actions are destined for. It used
    /// to be a ten-arm match inside the native shell's frame loop and nowhere
    /// else, so every template saved in the browser was queued and silently
    /// dropped — the queue grew for the life of the tab and nothing ever read
    /// it. One implementation, called by both shells, is the shape that cannot
    /// go missing from one of them again.
    pub fn drain_template_actions(&mut self) -> bool {
        if self.template_actions.is_empty() {
            return false;
        }
        for action in self.template_actions.drain(..) {
            let mut templates = self.core.templates_mut();
            match action {
                TemplateAction::AddPrimitive(t) => {
                    let _ = templates.add_primitive_template(t);
                }
                TemplateAction::RemovePrimitive { id } => {
                    let _ = templates.remove_primitive_template(&id);
                }
                TemplateAction::RenamePrimitive { id, name } => {
                    let _ = templates.rename_primitive_template(&id, &name);
                }
                TemplateAction::ReorderPrimitiveTemplates { order } => {
                    let _ = templates.set_primitive_template_order(order);
                }
                TemplateAction::AddIndicator(t) => {
                    let _ = templates.add_indicator_template(t);
                }
                TemplateAction::RemoveIndicator { id } => {
                    let _ = templates.remove_indicator_template(&id);
                }
                TemplateAction::AddCompare(t) => {
                    let _ = templates.add_compare_template(t);
                }
                TemplateAction::RemoveCompare { id } => {
                    let _ = templates.remove_compare_template(&id);
                }
                TemplateAction::AddChart(t) => {
                    let _ = templates.add_chart_template(t);
                }
                TemplateAction::RemoveChart { id } => {
                    let _ = templates.remove_chart_template(&id);
                }
                TemplateAction::AddIndicatorSet(s) => {
                    let _ = templates.add_indicator_set(s);
                }
                TemplateAction::RemoveIndicatorSet { id } => {
                    let _ = templates.remove_indicator_set(&id);
                }
            }
        }
        true
    }
}

/// An action that changes a performance control setting.
/// Queued by the Performance panel, drained and applied by App.
#[derive(Debug, Clone)]
pub enum PerfAction {
    SetFpsLimit(u32),
    SetMsaa(u8),
    SetRecalcMode(String),
    TogglePerfLog,
    SetBackend(String),
    ToggleVsync,
}

/// Identifies which region of a color picker is being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ColorPickerDragArea {
    /// Saturation/Value square (x → saturation, y → value)
    SVSquare,
    /// Vertical hue bar (y → hue)
    HueBar,
    /// Horizontal opacity slider (x → opacity 0..1)
    OpacitySlider,
}

/// State for an in-progress color picker drag.
#[derive(Debug, Clone)]
pub(crate) struct ColorPickerDragState {
    /// Which part of the picker is being dragged
    pub area: ColorPickerDragArea,
    /// Which settings modal owns the picker ("primitive", "indicator", "chart")
    pub source: String,
    /// SV square: (x, y, width, height)
    pub sv_rect: (f64, f64, f64, f64),
    /// Hue bar: (x, y, width, height)
    pub hue_rect: (f64, f64, f64, f64),
    /// Opacity slider track: (x, y, width, height)
    pub opacity_rect: (f64, f64, f64, f64),
}

/// State for an in-progress split-panel separator drag.
#[derive(Debug, Clone)]
pub(crate) struct SplitSeparatorDragState {
    /// Index into `docking.separators()`
    pub separator_idx: usize,
    /// Orientation of the separator being dragged
    pub orientation: mlc_core::SeparatorOrientation,
    /// Screen X at drag start
    pub start_x: f64,
    /// Screen Y at drag start
    pub start_y: f64,
}

/// Which granularity value the visible inline editor in the
/// `timeframe_selector` dropdown's "+ Add" row slot is currently editing
/// (user-granularity-templates-inner arc, кусок 2 — owner spec 2026-07-16
/// replaced the invisible custom-input-row UX with a visible inline editor).
#[derive(Debug, Clone, PartialEq)]
pub enum GranularityEditKind {
    /// Creating a new custom timeframe (TF-flavor dropdown).
    AddTf,
    /// Creating a new custom numeric param (Param-flavor dropdown).
    AddParam,
    /// Renaming an existing custom TF; `original` is the pre-edit name so
    /// commit can remove it (and re-add the star, if it had one) before
    /// inserting the new name.
    EditTf { original: String },
    /// Editing an existing custom numeric param's value; `original` is the
    /// pre-edit value.
    EditParam { original: f64 },
    /// Creating a new custom footprint bucket interval (`Interval`-flavor
    /// dropdown — per-family scoping arc 2026-07-16).
    AddInterval,
    /// Renaming an existing custom bucket interval; `original` is the
    /// pre-edit name, same rationale as `EditTf`.
    EditInterval { original: String },
}

/// Transient RAM-only state for the `timeframe_selector` dropdown's visible
/// inline granularity editor. `None` when no "+ Add"/edit editor is active.
/// Cleared on commit, cancel (Escape), and dropdown close — never persisted.
#[derive(Debug, Clone, PartialEq)]
pub struct GranularityEdit {
    pub kind: GranularityEditKind,
}

/// Which UI surface a live [`SymbolQuickEdit`] belongs to (trading-header-
/// symbol-edit arc, 2026-07-25 — generalizes the 2026-07-17 topbar-only
/// editor so the Trading Container's header-tab inline symbol editor can
/// reuse the exact same state/matching machinery instead of a parallel
/// copy). `SYMBOL_QUICK_SEARCH` stays the ONE `TextFieldStore` field either
/// surface focuses — only one of them can be live at a time, so sharing the
/// field id is safe and avoids minting a second one per trading panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SymbolQuickEditTarget {
    /// The topbar `symbol_selector` button-hosted editor — the original
    /// 2026-07-17 surface, commits via `apply_symbol` against the active
    /// chart.
    TopbarChart,
    /// A `TradingContainer`'s header-tab inline editor — commits via
    /// `commit_symbol_search_to_trading_panel` (group-propagating rebind),
    /// never `apply_symbol`.
    TradingPanel(mlc_core::state::PanelId),
    /// A data-section header editor (global-docking Stage B, 2026-07-30) —
    /// the DOM / Tape / L2 / BigTrades sidebar sections' symbol system.
    /// Commits via `commit_symbol_search_to_section_panel` (group
    /// instrument write + apply-key rebind).
    SectionPanel(mlc_core::state::PanelId),
    /// The Multi-DOM header's own symbol+exchange picker (owner spec
    /// 2026-07-30): the dropdown lists the already-added roster columns
    /// first (click removes), then search results — a same-symbol row
    /// click ADDS that exchange as a column; a different-symbol row click
    /// switches the base symbol. No chart apply, no tag-group coupling.
    MultiDomRoster(mlc_core::state::PanelId),
    /// A multi-tape section header's roster picker (M/TT · M/L2 · M/BT,
    /// owner order 2026-07-31) — same contract as `MultiDomRoster`, the
    /// panel id resolves to whichever tape store holds it.
    MultiTapeRoster(mlc_core::state::PanelId),
}

impl Default for SymbolQuickEditTarget {
    fn default() -> Self { Self::TopbarChart }
}

/// Transient RAM-only state for a live symbol-search inline editor
/// (symbol-search-compare-inner arc, Wave 2, кусок 4; generalized past the
/// topbar-only `symbol_selector` surface by the trading-header-symbol-edit
/// arc, 2026-07-25 — see [`SymbolQuickEditTarget`]). Mirrors
/// `GranularityEdit`'s shape/lifecycle: created empty (or seeded) on open,
/// live-updated on every keystroke (drives `ChartApp::
/// build_symbol_search_results` each frame — no separate results cache
/// field, same "recompute is cheap enough" contract `ModalState::
/// symbol_search_results` already relies on for the full-screen modal),
/// cleared on Escape / apply / close.
#[derive(Debug, Clone, PartialEq)]
pub struct SymbolQuickEdit {
    /// Which surface opened this editor — decides the commit path and
    /// where the dropdown/results popup renders.
    pub target: SymbolQuickEditTarget,
    /// Live typed query text.
    pub text: String,
    /// Cursor position (character index).
    pub cursor: usize,
    /// Selection range (start, end) — `None` when nothing selected.
    pub selection: Option<(usize, usize)>,
}

/// In-RAM clipboard payload for a single copied drawing primitive
/// (Ctrl+C / Ctrl+V hotkeys). `paste_count` tracks how many times this
/// entry has been pasted so repeated Ctrl+V stacks the same +20px offset
/// used by the right-click "clone" action instead of pasting every copy
/// on top of the last.
#[derive(Debug, Clone)]
pub(crate) struct PrimitiveClipboard {
    /// Full serialized state of the copied primitive — restored through
    /// `PrimitiveRegistry::from_json` on paste so type-specific fields
    /// outside `PrimitiveData` (an Elliott wave's own degree/link/follows)
    /// survive the round trip, not just the ones `data` below carries.
    pub snapshot: mlc_core::drawing::PrimitiveSnapshot,
    pub points: Vec<(i64, f64)>,
    pub data: mlc_core::drawing::PrimitiveData,
    pub paste_count: u32,
}

/// Active inline on-chart text edit of a placed text primitive
/// (inline-prim-text arc, owner ask 2026-08-26) — started by double-click,
/// committed by Enter / click-away, cancelled by Escape. The editable text
/// itself lives in the shared `TextFieldStore` under
/// `text_input::PRIM_INLINE_TEXT`; this is only the identity of what is
/// being edited. `pane` is `None` for the main chart, `Some(instance_id)`
/// for a sub-pane (same convention as `DrawingManager::current_pane`).
#[derive(Debug, Clone, Copy)]
pub(crate) struct InlinePrimTextEdit {
    pub bid: mlc_core::BubbleId,
    pub prim_id: u64,
    pub pane: Option<u64>,
    /// `Some` = Fib/Gann (or other ladder) VALUE edit of this stored
    /// level, seeded with `level_value_label` and committed via
    /// `parse_level_value`. `None` = `data.text` edit.
    pub level: Option<f64>,
}

/// Pending granularity row drag — set on mousedown over a reorderable row,
/// promoted to [`GranularityDrag`] only once the pointer has moved past the
/// ~4px threshold (owner spec 2026-07-16 items 2+3; mirrors
/// `WatchlistModalState::drag_reorder_pending`'s promote-or-fall-through-to-
/// click contract exactly). Never promoted -> the mouseup still reaches the
/// normal click routing (apply/star/edit) unchanged.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GranularityDragPending {
    pub family: mlc_core::chart_type::GranularityFamily,
    pub section: mlc_core::chart_type::GranularitySection,
    pub from_index: usize,
    pub start_x: f64,
    pub start_y: f64,
}

/// Active granularity row drag-to-reorder (owner spec 2026-07-16 items
/// 2+3). `None` when no row is being dragged. Cleared on mouseup (after
/// dispatching `ChartOutEvent::ReorderGranularity` if the drop index moved)
/// and on Escape/dropdown close (cancels without reordering).
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct GranularityDrag {
    pub family: mlc_core::chart_type::GranularityFamily,
    pub section: mlc_core::chart_type::GranularitySection,
    pub from_index: usize,
    /// Current drop index under the cursor — `None` until the first
    /// `on_drag_move` after promotion computes one from row geometry.
    pub drop_index: Option<usize>,
}

// =============================================================================
// RenderOutput
// =============================================================================

/// Cached results produced by one call to [`ChartApp::render_to_scene`].
///
/// `render_to_scene` is `&self` — it stores computed layout and result data
/// here instead of mutating `ChartApp` fields directly.  The caller passes
/// this struct to [`ChartApp::apply_render_output`] to persist the values.
#[derive(Default)]
pub struct RenderOutput {
    /// Per-leaf scale-corner hit zones computed during chart content rendering.
    pub scale_corner_zones_by_leaf: std::collections::HashMap<mlc_core::LeafId, ScaleCornerHitZones>,
    /// Toolbar render result (hit zones, dropdown state) for this frame.
    pub last_toolbar_result: Option<mlc_core::ChartToolbarRenderResult>,
    /// Modal render result (settings panels, color pickers).
    pub frame_result: Option<ChartModalRenderResult>,
    /// Search-overlay render result (indicator / symbol search).
    pub search_modal_result: Option<mlc_app_layout::layout::ModalSearchResult>,
    /// Context-menu render result.
    pub context_menu_result: Option<ContextMenuResult>,
    /// Right-sidebar render result.
    pub last_sidebar_result: Option<mlc_app_sidebar_content::render::RightSidebarResult>,
    /// Trading container's embedded header toolbar render result.
    pub trading_header_result: Option<mlc_core::ChartToolbarRenderResult>,
    /// Watchlist-modal render result.
    pub last_watchlist_modal_result: Option<mlc_app_layout::layout::modals::watchlist_modal::WatchlistModalResult>,
    /// Watchlist group-name input result.
    pub last_wl_group_name_result: Option<WlGroupNameInputResult>,
    /// Per-leaf tab hit zones, rebuilt every frame.
    pub leaf_tab_hit_zones: std::collections::HashMap<mlc_core::LeafId, mlc_app_layout::layout::panel_overlay::LeafTabHitZones>,
    /// Frame timing breakdown (chart_us, toolbar_us, sidebar_us, setup_us).
    pub render_timing_us: (u64, u64, u64, u64),
    /// Content rect (chart area minus sidebar) computed this frame.
    pub content_rect: LayoutRect,
    /// Left edge of the right toolbar (for sidebar separator drag math).
    pub right_toolbar_left_x: f64,
    /// Bounding rect of the floating inline config bar, if visible.
    pub last_inline_bar_rect: Option<LayoutRect>,
    /// Bounding rect of the floating favorites quick-pick bar, if visible
    /// (favorite-primitives arc) — used for drag hit-testing.
    pub last_favorites_bar_rect: Option<LayoutRect>,
    /// If `Some(v)`, sets `toolbar_state.open_submenu_id = v` after render.
    pub open_submenu_update: Option<Option<String>>,
    /// Sub-pane range writebacks: `(leaf_id, pane_index, min, max)`.
    ///
    /// Populated by the render path after `render_sub_pane` returns the
    /// final (symmetrized + padded) range.  Applied in `apply_render_output`
    /// so that stored values always match what was displayed.
    pub sub_pane_range_writebacks: Vec<(mlc_core::LeafId, usize, f64, f64)>,
    /// Sub-pane overlay result writebacks: `(leaf_id, overlay_results)`.
    ///
    /// Produced each frame by `render_full_chart_panel`.  Applied in
    /// `apply_render_output` so the hit tester can read button rects next frame.
    pub sub_pane_overlay_writebacks: Vec<(mlc_core::LeafId, Vec<mlc_core::SubPaneOverlayResult>)>,
    /// Per-leaf loading-spinner geometry: `(leaf_id, (cx, cy, radius))` for
    /// every panel that rendered the loading skeleton (bars empty + no
    /// data_error) this frame. Empty when nothing is loading.
    ///
    /// The host (`scene_builder.rs`) reads this AFTER `pw.chart.render()` to
    /// composite the spinner as an OVERLAY layer ON TOP of chart-scene — so
    /// the spinner animates independently of chart-scene composition and the
    /// "scene-ready" transition is just non-empty → empty across two frames.
    pub loading_spinners: Vec<(mlc_core::LeafId, (f64, f64, f64))>,
}

impl ChartApp {
    // -------------------------------------------------------------------------
    // Tag lifecycle facade (Phase 2B-2 of the Tag/Bubble/UiContainer cutover;
    // promoted to backend in Step 3.6b; single-write since Step 6).
    //
    // Every operation that creates / removes a `DataBubble` or connects /
    // disconnects a member goes through these helpers so that the paired
    // `core.tags()` entry stays in sync with the bubble. `color`,
    // `auto_created`, `members`, `member_overrides` live solely on `Tag`
    // (`core.tags()`/`core.tags_mut()`) — `DataBubble` no longer carries
    // them (Step 6 doctrine cutover,
    // `docs/mlc/plans/doctrine-cutover-2026-06-27.md`).
    // -------------------------------------------------------------------------

    /// Create a user-tagged bubble + paired tag and return its id.
    pub fn create_tagged_bubble(
        &mut self,
        color: [f32; 4],
        symbol: String,
        timeframe: mlc_core::state::Timeframe,
    ) -> mlc_core::ids::BubbleId {
        let kline_cap = self.kline_layer_capacity();
        let bid = self
            .core
            .bubbles_mut()
            .create_bubble(symbol, timeframe);
        self.core.set_layer_capacities(bid, kline_cap);
        self.core.tags_mut().insert(mlc_core::tag::Tag::new(bid, color));
        bid
    }

    /// Create an invisible (auto-created) bubble + paired tag.
    pub fn create_auto_bubble(
        &mut self,
        color: [f32; 4],
        symbol: String,
        timeframe: mlc_core::state::Timeframe,
    ) -> mlc_core::ids::BubbleId {
        let kline_cap = self.kline_layer_capacity();
        let bid = self
            .core
            .bubbles_mut()
            .create_bubble_auto(symbol, timeframe);
        self.core.set_layer_capacities(bid, kline_cap);
        self.core.tags_mut().insert(mlc_core::tag::Tag::new_auto(bid, color));
        bid
    }

    /// Kline raw-layer ring capacity from the active profile
    /// (`UserProfile.domain.data_load.kline_layer_capacity`), applied to
    /// every freshly-created bubble (unified-data-plane Stage Э1). The
    /// trade half is GONE (owner rule 2026-07-15) — raw trades are bounded
    /// by the day window (`data_load.trade_raw_window_days`), not a count
    /// ring. `user()` is a leaf lock — read independently, never nested
    /// under a held `bubbles_mut()` guard.
    fn kline_layer_capacity(&self) -> usize {
        let user = self.core.user();
        user.profile().domain.data_load.kline_layer_capacity as usize
    }

    /// Remove a bubble + its paired tag.
    pub fn remove_tag(&mut self, bid: mlc_core::ids::BubbleId) {
        self.core.bubbles_mut().remove_bubble(bid);
        // §6.7 cross-manager gap: indicator instances live in IndicatorManager,
        // not on DataBubble, so removing the bubble doesn't automatically drop
        // its indicators the way alerts/primitives (bubble data) do.
        self.core.indicators_mut().purge_for_bubble(bid);
        self.core.tags_mut().remove(bid);
        self.tag_ui.remove_tag(bid);
    }

    /// Connect any sync member to a bubble's paired tag.  If the member
    /// is already in another tag it is disconnected first.
    pub fn connect_member(
        &mut self,
        member: mlc_core::ids::SyncMemberId,
        bid: mlc_core::ids::BubbleId,
    ) {
        let _ = self.core.tags_mut().connect(member, bid);
    }

    /// Disconnect a member from its current tag.  Returns the tag id it was
    /// in, if any.
    pub fn disconnect_member(
        &mut self,
        member: mlc_core::ids::SyncMemberId,
    ) -> Option<mlc_core::ids::BubbleId> {
        self.core.tags_mut().disconnect(member)
    }

    /// Attach a chart window to a bubble's paired tag.
    pub fn attach_chart_to_tag(
        &mut self,
        chart_id: mlc_core::state::ChartId,
        bid: mlc_core::ids::BubbleId,
    ) {
        let _ = self.core.tags_mut().attach_chart_to_tag(chart_id, bid);
    }

    /// Detach a chart window from its current tag.
    pub fn detach_chart_from_tag(
        &mut self,
        chart_id: mlc_core::state::ChartId,
    ) -> Option<mlc_core::ids::BubbleId> {
        self.core.tags_mut().detach_chart_from_tag(chart_id)
    }

    /// Move a panel out of the Synced set and into its own private
    /// auto-created bubble.
    pub fn set_panel_auto(
        &mut self,
        panel_member: mlc_core::ids::SyncMemberId,
        private_bubble_id: mlc_core::ids::BubbleId,
    ) {
        self.core.tags_mut().set_auto_group(panel_member, private_bubble_id);
    }

    /// Put a panel in Synced state and connect to a bubble's paired tag.
    pub fn set_panel_synced(
        &mut self,
        panel_member: mlc_core::ids::SyncMemberId,
        bid: mlc_core::ids::BubbleId,
    ) {
        self.core.tags_mut().set_synced(panel_member, bid);
    }

    /// Fully deregister a panel: disconnect from its tag and remove
    /// from the synced set.  Use when permanently destroying a panel
    /// (closed by user).
    pub fn remove_panel_completely(
        &mut self,
        panel_member: mlc_core::ids::SyncMemberId,
    ) {
        let _ = self.core.tags_mut().disconnect(panel_member);
        self.core.tags_mut().unmark_synced(panel_member);
    }

    /// Promote an auto-created tag to a user-visible (color-tagged) one.
    /// Returns the newly assigned color, or `None` if the tag does not
    /// exist or was already user-tagged.
    pub fn promote_to_tagged(
        &mut self,
        bid: mlc_core::ids::BubbleId,
    ) -> Option<[f32; 4]> {
        let color = self.core.tags().next_unused_color();
        self.core.tags_mut().promote_to_tagged(bid, color).then_some(color)
    }

    // -------------------------------------------------------------------------
    // BackendCore-backed accessor helpers (Session 2 of the cutover).
    //
    // These return owned values (`String` / `Option<BubbleId>` etc) instead of
    // `&str` references into the `RwLock` guard. That avoids the "guard
    // dropped while borrowed" class of errors (E0515/E0716) at call sites,
    // and keeps the lock held only for the duration of the helper call.
    //
    // The lock is reader-preferring and is taken once per call. In the
    // single-threaded embedded path lock contention is zero — these are RAII
    // guard acquisitions, not waits.
    // -------------------------------------------------------------------------

    /// Resolve the `BubbleId` attached to this chart, if any.
    #[inline]
    pub fn bubble_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<mlc_core::ids::BubbleId> {
        self.core.tags().tag_for_chart(chart_id)
    }

    /// Catalog id of the one overlay promoted to a real `IndicatorInstance`
    /// (owner doctrine 2026-08-01: an on-chart overlay renders ONLY through
    /// an explicitly added indicator instance — see
    /// [`Self::liq_overlay_active`]).
    pub const LIQ_HEATMAP_TYPE_ID: &'static str = "overlay_liquidation_heatmap";

    /// Is the Liquidation Heatmap overlay live for `bid`'s bubble?
    ///
    /// Owner doctrine (2026-08-01): overlays render ONLY through an
    /// explicit `IndicatorInstance`, never through an invisible per-window
    /// bool. So this is DERIVED on every read — the presence of a VISIBLE
    /// `overlay_liquidation_heatmap` instance on the bubble IS the on/off
    /// state. There is deliberately no stored `liq_heatmap_show` field to
    /// drift out of sync: adding the indicator turns the overlay on,
    /// hiding it with the eye icon turns the render off, deleting the row
    /// turns it off and drops the subscription.
    ///
    /// The remaining four opaque overlays (Volume Profile / TPO Profile /
    /// OI Delta / Funding Rate) still ride their own window bools — they
    /// are converted one at a time (plan item G, "add + stand-test ONE
    /// overlay at a time"). DOM Heatmap was converted second, alongside
    /// re-keying its grid to the chart's bars.
    pub fn liq_overlay_active(&self, bid: mlc_core::ids::BubbleId) -> bool {
        self.core.indicators().instances_for_bubble(bid)
            .into_iter()
            .any(|inst| inst.type_id == Self::LIQ_HEATMAP_TYPE_ID && inst.visible)
    }

    /// [`Self::liq_overlay_active`] keyed by chart instead of bubble.
    pub fn liq_overlay_active_for_chart(&self, chart_id: mlc_core::state::ChartId) -> bool {
        self.bubble_for_chart(chart_id)
            .map(|bid| self.liq_overlay_active(bid))
            .unwrap_or(false)
    }

    /// How the realized-liquidation layer reads, from the
    /// `overlay_liquidation_heatmap` instance bound to `bid`'s bubble.
    /// Catalog defaults when the overlay was never added — the same rule
    /// [`Self::dom_heatmap_ink`] follows, so neither layer carries a
    /// rendering constant the owner cannot reach.
    #[inline]
    pub fn liq_ink(&self, bid: mlc_core::ids::BubbleId) -> mlc_core::chart::render_data::LiquidationInk {
        let d = mlc_core::chart::render_data::LiquidationInk::default();
        let indicators = self.core.indicators();
        let Some(inst) = indicators
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::LIQ_HEATMAP_TYPE_ID)
        else { return d };
        let f = |k: &str, fallback: f64| inst.params.get(k).and_then(|v| v.as_float()).unwrap_or(fallback);
        let b = |k: &str, fallback: bool| inst.params.get(k).and_then(|v| v.as_bool()).unwrap_or(fallback);
        mlc_core::chart::render_data::LiquidationInk {
            sensitivity: f("sensitivity", d.sensitivity),
            row_pct: f("row_pct", d.row_pct),
            min_usd: f("min_usd", d.min_usd),
            show_levels: b("show_levels", d.show_levels),
            show_dots: b("show_dots", d.show_dots),
            dot_scale: f("dot_scale", d.dot_scale),
            span_bars: f("span_bars", d.span_bars),
            opacity: f("opacity", d.opacity),
        }
    }

    /// Whether the projected-liquidation overlay folds every venue that
    /// serves open-interest history, or reads only the chart's own. Default
    /// ON, and for the same reason the liquidation fold is: price is one
    /// market, so a projection from a single venue understates every level
    /// by whatever share the others hold.
    #[inline]
    pub fn liq_projection_aggregate(&self, bid: mlc_core::ids::BubbleId) -> bool {
        self.core.indicators().instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::LIQ_PROJECTION_TYPE_ID)
            .and_then(|inst| inst.params.get("aggregate").and_then(|v| v.as_bool()))
            .unwrap_or(true)
    }

    /// Whether the liquidation overlay on `bid` folds every venue the
    /// workspace streams for this symbol, or only its own — same contract as
    /// [`Self::dom_heatmap_aggregate`], and the same reason: aggregation is a
    /// READ of rings other panels opened, never a subscription.
    #[inline]
    pub fn liq_aggregate(&self, bid: mlc_core::ids::BubbleId) -> bool {
        self.core.indicators().instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::LIQ_HEATMAP_TYPE_ID)
            .and_then(|inst| inst.params.get("aggregate").and_then(|v| v.as_bool()))
            .unwrap_or(true)
    }

    /// Catalog id of the projected-liquidation overlay.
    pub const LIQ_PROJECTION_TYPE_ID: &'static str = "overlay_liquidation_projection";

    /// Is the projected-liquidation overlay live for `bid`'s bubble? Derived
    /// from the instance, same doctrine as [`Self::liq_overlay_active`].
    pub fn liq_projection_active(&self, bid: mlc_core::ids::BubbleId) -> bool {
        self.core.indicators().instances_for_bubble(bid)
            .into_iter()
            .any(|inst| inst.type_id == Self::LIQ_PROJECTION_TYPE_ID && inst.visible)
    }

    /// How the projected layer reads, from its instance params.
    #[inline]
    pub fn liq_projection_ink(
        &self,
        bid: mlc_core::ids::BubbleId,
    ) -> mlc_core::chart::render_data::LiquidationProjectionInk {
        let d = mlc_core::chart::render_data::LiquidationProjectionInk::default();
        let indicators = self.core.indicators();
        let Some(inst) = indicators
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::LIQ_PROJECTION_TYPE_ID)
        else { return d };
        let f = |k: &str, fallback: f64| inst.params.get(k).and_then(|v| v.as_float()).unwrap_or(fallback);
        let b = |k: &str, fallback: bool| inst.params.get(k).and_then(|v| v.as_bool()).unwrap_or(fallback);
        mlc_core::chart::render_data::LiquidationProjectionInk {
            sensitivity: f("sensitivity", d.sensitivity),
            row_pct: f("row_pct", d.row_pct),
            opacity: f("opacity", d.opacity),
            show_consumed: b("show_consumed", d.show_consumed),
            future_bars: f("future_bars", d.future_bars),
            fade: b("fade", d.fade),
        }
    }

    /// The model's inputs, from the same instance. The leverage
    /// distribution is five weights rather than one preset: it is the knob
    /// the whole picture is tuned with, and a venue's clientele is not a
    /// constant we get to bake in.
    pub fn liq_projection_params(
        &self,
        bid: mlc_core::ids::BubbleId,
    ) -> mlc_core::chart::liquidation_projection::ProjectionParams {
        use mlc_core::chart::liquidation_projection::{LeverageTier, ProjectionParams};
        let d = ProjectionParams::default();
        let indicators = self.core.indicators();
        let Some(inst) = indicators
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::LIQ_PROJECTION_TYPE_ID)
        else { return d };
        let f = |k: &str, fallback: f64| inst.params.get(k).and_then(|v| v.as_float()).unwrap_or(fallback);
        let w = |k: &str, i: usize| f(k, d.tiers.get(i).map(|t| t.weight).unwrap_or(0.0));
        ProjectionParams {
            tiers: vec![
                LeverageTier { leverage: 5.0, weight: w("lev_5", 0) },
                LeverageTier { leverage: 10.0, weight: w("lev_10", 1) },
                LeverageTier { leverage: 25.0, weight: w("lev_25", 2) },
                LeverageTier { leverage: 50.0, weight: w("lev_50", 3) },
                LeverageTier { leverage: 100.0, weight: w("lev_100", 4) },
            ],
            mmr: f("mmr", d.mmr) / 100.0,
            row_pct: f("row_pct", d.row_pct),
            long_share: f("long_share", d.long_share),
            min_usd: f("min_usd", d.min_usd),
        }
    }

    /// Catalog id of the DOM Heatmap overlay indicator.
    /// The last four opaque entries, converted 2026-08-03 (plan item G).
    ///
    /// Until then they rode per-window bools and their catalog ids were
    /// INTERCEPTED before `IndicatorManager::add` (`adc5506d`, 2026-07-04),
    /// so picking one from the indicator list never produced an indicator:
    /// no legend row, no object-tree node, no settings, and — for the two
    /// sub-panes — no pane either, just a strip carved out of the price
    /// plot. They are now ordinary instances like every other overlay.
    pub const VOLUME_PROFILE_TYPE_ID: &'static str = "overlay_volume_profile";
    pub const TPO_PROFILE_TYPE_ID: &'static str = "overlay_tpo_profile";
    pub const OI_DELTA_TYPE_ID: &'static str = "overlay_oi_delta";
    pub const FUNDING_RATE_TYPE_ID: &'static str = "overlay_funding_rate";

    /// Every catalog id in this file that is host-rendered rather than
    /// computed. Kept as one list so a new one cannot be added without
    /// deciding where it draws.
    pub const HOST_RENDERED_TYPE_IDS: &'static [&'static str] = &[
        Self::VOLUME_PROFILE_TYPE_ID,
        Self::TPO_PROFILE_TYPE_ID,
        Self::OI_DELTA_TYPE_ID,
        Self::FUNDING_RATE_TYPE_ID,
        Self::DOM_HEATMAP_TYPE_ID,
        Self::LIQ_HEATMAP_TYPE_ID,
        Self::LIQ_PROJECTION_TYPE_ID,
    ];

    /// Whether a visible instance of `type_id` sits on `bid`'s bubble —
    /// the one gate shape every converted overlay uses.
    pub fn overlay_instance_active(&self, bid: mlc_core::ids::BubbleId, type_id: &str) -> bool {
        self.core.indicators().instances_for_bubble(bid)
            .into_iter()
            .any(|inst| inst.type_id == type_id && inst.visible)
    }

    /// [`Self::overlay_instance_active`] keyed by chart.
    pub fn overlay_instance_active_for_chart(
        &self,
        chart_id: mlc_core::state::ChartId,
        type_id: &str,
    ) -> bool {
        self.bubble_for_chart(chart_id)
            .map(|bid| self.overlay_instance_active(bid, type_id))
            .unwrap_or(false)
    }

    /// How a profile strip reads, from its instance's params.
    ///
    /// One resolver for both profiles: they differ in what they count, not
    /// in how the histogram draws, so a second copy would only be a second
    /// place for the two to drift apart.
    pub fn profile_ink(
        &self,
        bid: mlc_core::ids::BubbleId,
        type_id: &str,
    ) -> mlc_core::chart::render_data::ProfileInk {
        let d = mlc_core::chart::render_data::ProfileInk::default();
        let indicators = self.core.indicators();
        let Some(inst) = indicators
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == type_id)
        else {
            return d;
        };
        let f = |k: &str, fallback: f64| {
            inst.params.get(k).and_then(|v| v.as_float()).unwrap_or(fallback)
        };
        let b = |k: &str, fallback: bool| {
            inst.params.get(k).and_then(|v| v.as_bool()).unwrap_or(fallback)
        };
        mlc_core::chart::render_data::ProfileInk {
            // Declared in percent, which is how it reads in the modal.
            width_ratio: (f("width_pct", 18.0) / 100.0).clamp(0.02, 0.9),
            opacity: f("opacity", d.opacity).clamp(0.02, 1.0),
            left_side: inst
                .params
                .get("side")
                .and_then(|v| v.as_string())
                .map(|s| s.eq_ignore_ascii_case("left"))
                .unwrap_or(d.left_side),
            show_poc: b("show_poc", d.show_poc),
            show_value_area: b("show_value_area", d.show_value_area),
            show_labels: b("show_labels", d.show_labels),
        }
    }

    /// Row count and value-area share for a profile's PREPASS — the two
    /// params that change the computed profile rather than its ink.
    pub fn profile_shape(&self, bid: mlc_core::ids::BubbleId, type_id: &str) -> (usize, f64) {
        let default_rows = if type_id == Self::TPO_PROFILE_TYPE_ID { 60.0 } else { 24.0 };
        let indicators = self.core.indicators();
        let Some(inst) = indicators
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == type_id)
        else {
            return (default_rows as usize, 0.70);
        };
        let rows = inst
            .params
            .get("rows")
            .and_then(|v| v.as_float())
            .unwrap_or(default_rows)
            .clamp(4.0, 400.0) as usize;
        let va = inst
            .params
            .get("value_area")
            .and_then(|v| v.as_float())
            .unwrap_or(70.0)
            .clamp(10.0, 99.0)
            / 100.0;
        (rows, va)
    }

    /// The window a Volume Profile aggregates, from its `mode` param.
    pub fn vp_mode(&self, bid: mlc_core::ids::BubbleId) -> mlc_core::chart::volume_profile::VpMode {
        use mlc_core::chart::volume_profile::VpMode;
        self.core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::VOLUME_PROFILE_TYPE_ID)
            .and_then(|inst| inst.params.get("mode").and_then(|v| v.as_string().map(String::from)))
            .map(|s| match s.as_str() {
                "Session" => VpMode::Session,
                "Composite" => VpMode::Composite,
                _ => VpMode::VisibleRange,
            })
            .unwrap_or(VpMode::VisibleRange)
    }

    /// The venues that can serve `stream`, in a stable display order.
    ///
    /// This is HOST knowledge, not indicator knowledge: an overlay declares
    /// the stream it eats and nothing else, and which exchanges publish that
    /// stream is a property of the connectors — it changes as they gain and
    /// lose endpoints. Keeping the mapping here means a layer that changes
    /// its declared stream automatically gets the right venue list, and no
    /// venue list is written down twice.
    pub fn venues_for_stream(
        &self,
        stream: mlc_backend_indicator_engine::StreamKind,
    ) -> Vec<digdigdig3::ExchangeId> {
        use mlc_backend_indicator_engine::StreamKind;
        use mlc_app_panels::trading::order_flow::multi_dom;
        match stream {
            // The WS forced-close channel, which is a shorter list than the
            // REST liquidation-history capability: we consume the stream.
            StreamKind::Liquidation => multi_dom::LIQ_EXCHANGES
                .iter()
                .filter_map(|slug| digdigdig3::ExchangeId::from_str(slug))
                .collect(),
            // Already capability-filtered by the bridge.
            StreamKind::OpenInterest => self.core.bridge_handle().oi_history_venues(),
            // Books, bars and trades come from the connector registry, which
            // is where every venue DECLARES the feeds it serves. They were a
            // hand-written six here until 2026-08-03 — the Multi-DOM ladder's
            // cycle-add list, borrowed because it was nearby. Candles are the
            // feed essentially every exchange publishes (17 of them), so a
            // profile offered six was quietly hiding two thirds of the
            // market. The sidebar's per-connector switch is applied at ARM
            // time, not here: a venue the owner turned off stays visible as a
            // venue rather than vanishing without explanation.
            StreamKind::Bar => digdigdig3::connector_manager::kline_venues(),
            StreamKind::OrderBook => digdigdig3::connector_manager::orderbook_venues(),
            StreamKind::Tick => digdigdig3::connector_manager::trade_venues(),
            _ => Vec::new(),
        }
    }

    /// The venue set an instance folds: its `venues` param, or every venue
    /// serving its declared stream when that set is empty.
    ///
    /// Empty means ALL — narrowing is a deliberate act, and a fresh overlay
    /// should aggregate what it can rather than nothing.
    pub fn instance_venues(&self, instance_id: u64) -> Vec<digdigdig3::ExchangeId> {
        let indicators = self.core.indicators();
        let Some(type_id) = indicators.type_id_of(instance_id) else { return Vec::new() };
        let offered: Vec<digdigdig3::ExchangeId> = indicators
            .definition_streams(&type_id)
            .first()
            .map(|s| self.venues_for_stream(*s))
            .unwrap_or_default();
        let chosen: Vec<String> = indicators
            .param_value(instance_id, "venues")
            .and_then(|v| v.as_string_list().map(|s| s.to_vec()))
            .unwrap_or_default();
        if chosen.is_empty() {
            return offered;
        }
        offered
            .into_iter()
            .filter(|eid| chosen.iter().any(|c| c.eq_ignore_ascii_case(eid.as_str())))
            .collect()
    }

    /// Same, for the single visible instance of `type_id` on `bid` — the
    /// shape every overlay pump wants, since a pump is keyed by bubble.
    pub fn overlay_venues(
        &self,
        bid: mlc_core::ids::BubbleId,
        type_id: &str,
    ) -> Vec<digdigdig3::ExchangeId> {
        let id = self
            .core
            .indicators()
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == type_id && inst.visible)
            .map(|inst| inst.id);
        id.map(|id| self.instance_venues(id)).unwrap_or_default()
    }

    /// Fill the settings modal's venue picker with what the host can serve
    /// for the open instance. Called each frame the modal is open: the
    /// offered list is live state, not a definition.
    pub(crate) fn refresh_indicator_venue_options(&mut self) {
        let Some(id) = self.panel_app.indicator_settings_state.indicator_id else {
            self.panel_app.indicator_settings_state.venue_options.clear();
            self.panel_app.indicator_settings_state.venue_all = true;
            return;
        };
        let (offered, chosen): (Vec<digdigdig3::ExchangeId>, Vec<String>) = {
            let indicators = self.core.indicators();
            let Some(type_id) = indicators.type_id_of(id) else {
                self.panel_app.indicator_settings_state.venue_options.clear();
                return;
            };
            let offered = indicators
                .definition_streams(&type_id)
                .first()
                .map(|s| self.venues_for_stream(*s))
                .unwrap_or_default();
            let chosen = indicators
                .param_value(id, "venues")
                .and_then(|v| v.as_string_list().map(|s| s.to_vec()))
                .unwrap_or_default();
            (offered, chosen)
        };
        // An EMPTY set means every offered venue, so every row is ticked.
        // Showing them all unticked in that state was backwards — it read as
        // "nothing selected" for the layer's default, which is "all of them".
        let all = chosen.is_empty();
        self.panel_app.indicator_settings_state.venue_all = all;
        self.panel_app.indicator_settings_state.venue_options = offered
            .into_iter()
            .map(|eid| {
                let name = eid.as_str().to_uppercase();
                let on = all || chosen.iter().any(|c| c.eq_ignore_ascii_case(&name));
                (name, on)
            })
            .collect();
    }

    pub const DOM_HEATMAP_TYPE_ID: &'static str = "overlay_dom_heatmap";

    /// Whether the DOM Heatmap overlay is on for `bid`'s bubble. Derived,
    /// same doctrine as [`Self::liq_overlay_active`]: a visible
    /// `overlay_dom_heatmap` instance IS the on/off state.
    pub fn dom_overlay_active(&self, bid: mlc_core::ids::BubbleId) -> bool {
        self.core.indicators().instances_for_bubble(bid)
            .into_iter()
            .any(|inst| inst.type_id == Self::DOM_HEATMAP_TYPE_ID && inst.visible)
    }

    /// [`Self::dom_overlay_active`] keyed by chart instead of bubble.
    pub fn dom_overlay_active_for_chart(&self, chart_id: mlc_core::state::ChartId) -> bool {
        self.bubble_for_chart(chart_id)
            .map(|bid| self.dom_overlay_active(bid))
            .unwrap_or(false)
    }

    /// How the DOM Heatmap overlay on `bid`'s bubble is INKED, from its
    /// `IndicatorInstance` — catalog defaults when no instance exists.
    #[inline]
    pub fn dom_heatmap_ink(&self, bid: mlc_core::ids::BubbleId) -> mlc_core::chart::render_data::DomHeatmapInk {
        use mlc_core::chart::render_data::DomHeatmapInk;
        let d = DomHeatmapInk::default();
        let indicators = self.core.indicators();
        let Some(inst) = indicators
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::DOM_HEATMAP_TYPE_ID)
        else { return d };
        let f = |k: &str, fallback: f64| inst.params.get(k).and_then(|v| v.as_float()).unwrap_or(fallback);
        DomHeatmapInk {
            sensitivity: f("sensitivity", d.sensitivity),
            norm_pct: f("norm_pct", d.norm_pct),
            contrast: f("contrast", d.contrast),
            opacity: f("opacity", d.opacity),
            wall_x: f("wall_x", d.wall_x),
            show_values: inst.params.get("show_values").and_then(|v| v.as_bool()).unwrap_or(d.show_values),
            value_size: f("value_size", d.value_size),
            value_aspect: f("value_aspect", d.value_aspect),
        }
    }

    /// Grid GEOMETRY of the DOM Heatmap on `bid`'s bubble —
    /// `(row_ticks, band_fraction)`. Separate from the ink because these
    /// decide what the buffer STORES: changing either invalidates the grid,
    /// and the ingest arm rebuilds rather than mixing two grains.
    #[inline]
    pub fn dom_heatmap_grid(&self, bid: mlc_core::ids::BubbleId) -> (i64, f64) {
        const DEFAULT_ROW_TICKS: i64 = 4;
        const DEFAULT_BAND_PCT: f64 = 0.6;
        let indicators = self.core.indicators();
        let Some(inst) = indicators
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::DOM_HEATMAP_TYPE_ID)
        else { return (DEFAULT_ROW_TICKS, DEFAULT_BAND_PCT / 100.0) };
        let f = |k: &str, fallback: f64| inst.params.get(k).and_then(|v| v.as_float()).unwrap_or(fallback);
        (
            f("row_ticks", DEFAULT_ROW_TICKS as f64).round().max(1.0) as i64,
            f("band_pct", DEFAULT_BAND_PCT).max(0.01) / 100.0,
        )
    }

    /// Whether the DOM Heatmap on `bid`'s bubble folds every venue the
    /// workspace already streams, or reads only the bubble's own.
    ///
    /// Aggregation is a READ of the shared book registry, never a
    /// subscription: the venues it can see are exactly the ones some DOM
    /// ladder opened. With no other panel open it sees one book and looks
    /// identical to single-venue mode, which is the honest answer rather than
    /// a promise of breadth the workspace is not paying for.
    #[inline]
    pub fn dom_heatmap_aggregate(&self, bid: mlc_core::ids::BubbleId) -> bool {
        let indicators = self.core.indicators();
        indicators
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| inst.type_id == Self::DOM_HEATMAP_TYPE_ID)
            .and_then(|inst| inst.params.get("aggregate").and_then(|v| v.as_bool()))
            .unwrap_or(false)
    }

    /// Resolve a "real" sync group for this chart — returns `Some(BubbleId)`
    /// only when the group has more than one member or was created explicitly
    /// (not via auto-mono).  Auto-mono groups exist for every freshly seeded
    /// chart so it can later promote to a shared sync group, but their
    /// primitives / indicators / alerts still live in the per-window
    /// drawing_manager / panel_grid container.  Anything that asks
    /// "which group owns the *committed state* for this chart" must use this
    /// helper, not `bubble_for_chart` directly — otherwise undo/redo lookups
    /// route to `group.command_history` while the actual primitives are in
    /// `window.drawing_manager`, and the apply path silently misses.
    /// `intercept_completed_primitive_to_group` and `prepare_frame` already
    /// use the same auto-mono guard.
    #[inline]
    pub fn shared_group_for_chart(
        &self,
        chart_id: mlc_core::state::ChartId,
    ) -> Option<mlc_core::ids::BubbleId> {
        let gid = self.bubble_for_chart(chart_id)?;
        let tags = self.core.tags();
        let is_auto_mono = tags.get(gid)
            .map(|t| t.auto_created && t.members.len() <= 1)
            .unwrap_or(false);
        if is_auto_mono { None } else { Some(gid) }
    }

    /// Symbol of the bubble attached to this chart (cloned).
    #[inline]
    pub fn symbol_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<String> {
        let bid = self.bubble_for_chart(chart_id)?;
        self.core.bubbles().symbol(bid).map(String::from)
    }

    /// Exchange identifier of the bubble attached to this chart (cloned).
    #[inline]
    pub fn exchange_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<String> {
        let bid = self.bubble_for_chart(chart_id)?;
        self.core.bubbles().exchange(bid).map(String::from)
    }

    /// Typed exchange id of the bubble attached to this chart — the resolver
    /// every per-container data-plane operation must use instead of the
    /// app-global `active_exchange` (2026-07-15 live bug: `active_exchange`
    /// only moves on a symbol pick, so on a container whose venue differs
    /// from the global — multi-container layouts, or post-boot before any
    /// pick — chart-type/TF switches subscribed klines and folds at the
    /// WRONG venue and the chart stayed empty forever). Falls back to
    /// `active_exchange` only when the chart has no bubble/exchange yet.
    #[inline]
    pub fn exchange_id_for_chart(&self, chart_id: mlc_core::state::ChartId) -> digdigdig3::ExchangeId {
        self.exchange_for_chart(chart_id)
            .and_then(|exch| digdigdig3::ExchangeId::from_str(&exch))
            .unwrap_or(self.active_exchange)
    }

    /// Account type of the bubble attached to this chart. Callers that need
    /// the persisted/display label go through `Display`/`AccountType::as_label`
    /// at the point of use — this is the canonical typed key, see
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1C.
    #[inline]
    pub fn account_type_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<mlc_core::instrument::AccountType> {
        let bid = self.bubble_for_chart(chart_id)?;
        self.core.bubbles().account_type_typed(bid)
    }

    /// Timeframe of the bubble attached to this chart (cloned).
    #[inline]
    pub fn timeframe_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<mlc_core::state::Timeframe> {
        let bid = self.bubble_for_chart(chart_id)?;
        self.core.bubbles().timeframe(bid).cloned()
    }

    /// `chart_type` string of the bubble attached to this chart (e.g.
    /// "renko", "candles"). `None` when the chart isn't attached to a bubble
    /// yet — callers fall back to the `Bars` resolution.
    #[inline]
    pub fn chart_type_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<&'static str> {
        let bid = self.bubble_for_chart(chart_id)?;
        self.core.bubbles().chart_type(bid)
    }

    /// `chart_type_param` of the bubble attached to this chart.
    #[inline]
    pub fn chart_type_param_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<f64> {
        let bid = self.bubble_for_chart(chart_id)?;
        self.core.bubbles().chart_type_param(bid)
    }

    /// `chart_type_param_mode` of the bubble attached to this chart (renko/
    /// kagi ATR(14) box-size mode — granularity arc, кусок 2).
    #[inline]
    pub fn chart_type_param_mode_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<mlc_core::chart_type::ParamMode> {
        let bid = self.bubble_for_chart(chart_id)?;
        self.core.bubbles().chart_type_param_mode(bid)
    }

    /// `chart_type_interval` of the bubble attached to this chart.
    #[inline]
    pub fn chart_type_interval_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<&'static str> {
        let bid = self.bubble_for_chart(chart_id)?;
        self.core.bubbles().chart_type_interval(bid)
    }

    /// Resolution label for this chart's pane overlay tab / legend —
    /// mirrors the TF toolbar button's own dispatch
    /// (`mlc-app-layout::panel_app`'s `tf_button_label` build, owner
    /// doctrine 2026-07-08): `Time`/`Interval` show the TF name unchanged;
    /// `Param` (trade-agg bars, price-action transforms) shows the
    /// formatted `chart_type_param` + suffix ("20R", "0.1V") instead of the
    /// stale wall-clock TF name (legend-honesty fix, 2026-07-08 — a
    /// range_bar/volume_bar/etc. window's `bubble.timeframe` is never
    /// re-stamped on chart-type switch, see `chart_out_events.rs:690-694`).
    ///
    /// Falls back to the plain TF name when the chart_type is unknown or
    /// not yet attached to a bubble.
    pub fn resolution_label_for_chart(&self, chart_id: mlc_core::state::ChartId) -> String {
        let tf_name = self.timeframe_for_chart(chart_id)
            .map(|tf| tf.name.clone())
            .unwrap_or_default();
        let Some(def) = self.chart_type_for_chart(chart_id).and_then(mlc_core::chart_type::by_id) else {
            return tf_name;
        };
        match def.resolution {
            mlc_core::chart_type::ResolutionKind::Param { suffix } => {
                let value = self.chart_type_param_for_chart(chart_id).unwrap_or(0.0);
                if value > 0.0 {
                    mlc_app_layout::panel_app::format_resolution_param(value, suffix)
                } else {
                    // Sentinel (`value <= 0.0`, pre-first-subscribe) — same
                    // canonical resolver the toolbar/modal/subscribe use
                    // (canon-fix wave 2, item 1:
                    // `ChartTypeDef::default_param_label`), NOT
                    // `options.first()` alone — closes the "$1K" vs "$100K"
                    // legend/toolbar divergence for types whose
                    // `inline_default` differs from the ladder's first entry.
                    def.default_param_label()
                        .map(|label| mlc_app_layout::panel_app::format_resolution_param_label(label, suffix))
                        .unwrap_or_else(|| mlc_app_layout::panel_app::format_resolution_param(value, suffix))
                }
            }
            mlc_core::chart_type::ResolutionKind::Time
            | mlc_core::chart_type::ResolutionKind::Interval => tf_name,
        }
    }

    /// Bar-close countdown interval (ms) for this chart's price-scale label —
    /// `None` on `ResolutionKind::Param` chart types (trade-agg bars,
    /// price-action transforms) whose `bubble.timeframe` is a stale leftover
    /// from before the chart-type switch (never re-stamped, see
    /// `chart_out_events.rs:690-694`). Feeding that stale interval into the
    /// countdown renders a nonsense wall-clock timer on volume/range/dollar
    /// bars; hiding it at the source keeps the countdown correct for
    /// `Time`/`Interval` types and absent everywhere else.
    #[inline]
    pub fn countdown_interval_ms_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<i64> {
        let interval_ms = self.timeframe_for_chart(chart_id)?.interval_ms;
        match self.chart_type_for_chart(chart_id).and_then(mlc_core::chart_type::by_id) {
            Some(def) if matches!(def.resolution, mlc_core::chart_type::ResolutionKind::Param { .. }) => None,
            _ => Some(interval_ms),
        }
    }

    /// Resolve the group/bubble id for any sync member id.
    #[inline]
    pub fn group_for_member(&self, member: mlc_core::ids::SyncMemberId) -> Option<mlc_core::ids::BubbleId> {
        self.core.tags().tag_for_member(member)
    }

    /// Run a closure with read access to a specific bubble, returning whatever
    /// the closure produces (owned). The lock is held only for the duration
    /// of `f`. Returns `None` if the bubble id is invalid.
    ///
    /// Use this when a chained read pattern would escape the RwLock guard.
    /// Example:
    /// ```ignore
    /// // Bad — guard drops before .map can deref &DataBubble:
    /// let v: u32 = self.core.bubbles().bubble(bid).map(|b| b.field).unwrap_or(0);
    /// // Good:
    /// let v: u32 = self.with_bubble(bid, |b| b.field).unwrap_or(0);
    /// ```
    #[inline]
    pub fn with_bubble<R>(
        &self,
        bid: mlc_core::ids::BubbleId,
        f: impl FnOnce(&mlc_backend::bubble::DataBubble) -> R,
    ) -> Option<R> {
        self.core.bubbles().with_bubble(bid, f)
    }

    /// Same as `with_bubble` but resolves the bubble by chart id.
    #[inline]
    pub fn with_chart_bubble<R>(
        &self,
        cid: mlc_core::state::ChartId,
        f: impl FnOnce(&mlc_backend::bubble::DataBubble) -> R,
    ) -> Option<R> {
        let bid = self.core.tags().tag_for_chart(cid)?;
        self.core.bubbles().with_bubble(bid, f)
    }

    /// Phase X0 / C5c helper: resolve the bubble's bars for a chart window.
    /// Returns an owned Vec (cloned) to avoid borrow conflicts with
    /// `panel_app.panel_grid.containers_mut()`.
    pub fn bars_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Vec<mlc_core::Bar> {
        self.core.tags().tag_for_chart(chart_id)
            .map(|bid| self.core.bubbles().bars(bid).to_vec())
            .unwrap_or_default()
    }

    /// Phase X0 / C5c helper: resolve the bubble's bars for the active chart.
    pub fn active_chart_bars(&self) -> Vec<mlc_core::Bar> {
        self.panel_app.panel_grid.active_container_id()
            .map(|cid| self.bars_for_chart(cid))
            .unwrap_or_default()
    }

    /// Bars for a bubble the caller has ALREADY resolved.
    ///
    /// [`Self::bars_for_chart`] re-resolves the bubble through the tag
    /// registry, and that registry is populated later than a container's own
    /// `current_tag_id` — so early in a session it answers `None` and the
    /// `unwrap_or_default()` hands back an EMPTY series. Every drawing
    /// hit-test already holds the right `BubbleId` (it resolves the drawing
    /// session from `current_tag_id`), so re-deriving the bubble a second way
    /// buys nothing and costs correctness: with an empty series
    /// `timestamp_ms_to_bar_f64` maps both anchors of every primitive to a
    /// degenerate x, and the hit-test misses objects that are plainly drawn
    /// on screen until the tag registry catches up. Render never takes that
    /// path, which is why the primitive is visible and dead at the same time.
    ///
    /// Any code path holding a `BubbleId` must use this.
    pub fn bars_for_bubble(&self, bid: mlc_core::BubbleId) -> Vec<mlc_core::Bar> {
        self.core.bubbles().bars(bid).to_vec()
    }

    /// Phase X0 / C5c helper: resolve bars for the chart attached to `leaf_id`.
    pub fn bars_for_leaf(&self, leaf_id: uzor::panels::LeafId) -> Vec<mlc_core::Bar> {
        self.panel_app.panel_grid.container_id_for_leaf(leaf_id)
            .map(|cid| self.bars_for_chart(cid))
            .unwrap_or_default()
    }

    /// Audit D8 (ActiveSeries seam): resolve the array `chart_id`'s
    /// chart_type actually renders (owned snapshot, so the bubble lock is
    /// released before the caller mutates windows in a loop). Falls back to
    /// an empty `Bars` series when the container/bubble/panel state isn't
    /// resolvable yet.
    pub fn active_series_for_chart(&self, chart_id: mlc_core::state::ChartId) -> crate::active_series::OwnedActiveSeries {
        let Some(bid) = self.bubble_for_chart(chart_id) else {
            return crate::active_series::OwnedActiveSeries::default();
        };
        let chart_type = self.chart_type_for_chart(chart_id).unwrap_or("candles");
        self.with_bubble(bid, |b| crate::active_series::resolve_active_series_owned(chart_type, b))
            .unwrap_or_default()
    }

    /// Audit D8: same as [`Self::active_series_for_chart`] for the active window.
    pub fn active_chart_active_series(&self) -> crate::active_series::OwnedActiveSeries {
        self.panel_app.panel_grid.active_container_id()
            .map(|cid| self.active_series_for_chart(cid))
            .unwrap_or_default()
    }

    /// Chart-type dropdown data-mode marks for the ACTIVE window's
    /// (exchange, account_type) — hybrid-data-modes-plan-2026-07-08 Step 5.
    /// Computed fresh every frame (RAM-only, no persisted field, same
    /// discipline as `allowed_timeframes`) at the toolbar-render call site.
    /// `None` when there is no active window — dropdown renders with no
    /// markers, same as an empty map would.
    pub fn chart_type_data_modes(
        &self,
    ) -> Option<std::collections::HashMap<&'static str, mlc_core::chart_type::DataMode>> {
        let cid = self.panel_app.panel_grid.active_container_id()?;
        let account_type = self.account_type_for_chart(cid).unwrap_or_default();
        let exchange_id = self.exchange_id_for_chart(cid);
        Some(
            mlc_core::chart_type::all()
                .iter()
                .map(|def| (def.id, self.core.data_mode_for(def.id, exchange_id, account_type)))
                .collect(),
        )
    }

    /// `timeframe_selector` per-item history-capability marks for the
    /// ACTIVE window's (exchange, account_type) — resolution-availability-
    /// 2026-07-08 Phase 3, widened by the granularity arc defect fix
    /// 2026-07-16 (aggregatability, not raw nativeness). Maps every `tf_*`
    /// dropdown item id to whether THIS venue can serve REST-backed history
    /// for that interval, via [`Self::timeframe_has_history`] — `true` when
    /// EITHER the interval is natively served OR it aggregates cleanly
    /// (exact integer multiple) from some interval the venue natively
    /// serves. `false` does not mean the interval is unusable — the station
    /// still builds it live from trade aggregation (`TradeToBarDerived`) —
    /// it means there is no REST-backed history for it, which the dropdown
    /// marks as "live" (client-only doctrine: never imply history that
    /// doesn't exist).
    ///
    /// Computed fresh every frame (RAM-only, same discipline as
    /// `chart_type_data_modes`/`allowed_timeframes`). `None` when there is
    /// no active window, or the venue's kline-interval capability isn't
    /// known yet (exchange not connected) — dropdown renders with no marks
    /// either way.
    ///
    /// Also covers user-favorited custom TF ids (`tf_{name}`,
    /// user-granularity-templates-inner arc, кусок 1) — this is why the map
    /// is keyed by owned `String` rather than `&'static str`: a favorite
    /// name (e.g. `"7m"`) has no `'static` literal to key off. Lookups by
    /// callers holding a `&str` id are unaffected (`HashMap<String,
    /// _>::get` accepts `&str` via `Borrow`).
    pub fn timeframe_native_map(
        &self,
    ) -> Option<std::collections::HashMap<String, bool>> {
        let cid = self.panel_app.panel_grid.active_container_id()?;
        let account_type = self.account_type_for_chart(cid).unwrap_or_default();
        let exchange_id = self.exchange_id_for_chart(cid);
        let native_intervals = self.core.kline_native_intervals(exchange_id, account_type)?;
        let mut map = std::collections::HashMap::new();
        for &tf_id in crate::active_series::ALL_TF_IDS {
            let Some(interval) = crate::active_series::tf_id_to_dig3_interval(tf_id) else {
                continue;
            };
            map.insert(tf_id.to_string(), Self::timeframe_has_history(&interval, &native_intervals));
        }
        for name in &self.core.user().profile().domain.granularity_favorites.time_tfs {
            let Some(interval) = crate::active_series::tf_id_to_dig3_interval(&format!("tf_{}", name)) else {
                continue;
            };
            map.insert(format!("tf_{}", name), Self::timeframe_has_history(&interval, &native_intervals));
        }
        Some(map)
    }

    /// Whether `interval` (canonical dig3 form) carries REST-backed history
    /// on a venue whose natively-served interval set is `native_intervals` —
    /// the aggregatability predicate behind [`Self::timeframe_native_map`]
    /// (granularity arc defect fix 2026-07-16, owner spec: detect
    /// aggregatability, not raw nativeness).
    ///
    /// `true` when EITHER `interval` is itself in `native_intervals`, OR
    /// its duration is an EXACT integer multiple of some interval in
    /// `native_intervals` (client-side re-cut from real bars/trades — same
    /// "kline-sufficient re-cut" doctrine as renko/pnf/kagi: a clean
    /// integer multiple of a natively-history-backed interval has FULL
    /// history, not partial). Divisors come from the venue's OWN native set
    /// — never a hardcoded list — so a venue whose finest native interval
    /// is `"1m"` correctly leaves every sub-minute interval (`"30s"`,
    /// `"15s"`, ...) marked "live" (no minute-granularity interval divides
    /// evenly into a sub-minute duration), while `"7m"`/`"12m"` mark
    /// history-capable wherever `"1m"` is native (7 and 12 are both exact
    /// multiples of 1).
    fn timeframe_has_history(interval: &str, native_intervals: &[String]) -> bool {
        if native_intervals.iter().any(|n| n == interval) {
            return true;
        }
        let Some(target_ms) = crate::active_series::dig3_interval_to_ms(interval) else {
            return false;
        };
        native_intervals.iter().any(|native| {
            crate::active_series::dig3_interval_to_ms(native)
                .map(|native_ms| native_ms > 0 && target_ms % native_ms == 0)
                .unwrap_or(false)
        })
    }

    /// Render-side snapshot of the `timeframe_selector` dropdown's visible
    /// inline granularity editor (user-granularity-templates-inner arc,
    /// кусок 2 — defect fix 2026-07-16: real text-field widget, in-place row
    /// hosting) — `None` when no "+ Add"/edit editor is active
    /// (`granularity_edit.is_none()`).
    ///
    /// Reads the same `TF_FAVORITE_INPUT`/`PARAM_FAVORITE_INPUT` field the
    /// commit handlers in `text_input::dispatch` consume (text/cursor/
    /// selection), keyed by which editor kind is active. `host` carries
    /// which row the dropdown builder (`mlc-app-layout::panel_app`) must
    /// render the editor INTO: `None` for `AddTf`/`AddParam` (the "+ Add"
    /// row), `Some(target)` for `EditTf`/`EditParam` (the existing custom's
    /// own row, identified by its pre-edit value — owner spec (a): editing
    /// renders in place, not relocated to the Add row).
    ///
    /// `cursor_visible` uses a coarse wall-clock read (matches the
    /// `clock_time` computation a few lines above this method's call sites)
    /// rather than a threaded `current_time_ms` — `render_toolbar_only`
    /// (one of the two render paths that reach this) has no such parameter
    /// and widening its signature would ripple into the vello shell for a
    /// blink-cadence detail that tolerates a few ms of jitter.
    pub fn granularity_edit_render(&self) -> Option<mlc_core::chart_type::GranularityEditRender> {
        let edit = self.granularity_edit.as_ref()?;
        let (field, host) = match &edit.kind {
            GranularityEditKind::AddTf => (crate::text_input::TF_FAVORITE_INPUT, None),
            GranularityEditKind::AddParam => (crate::text_input::PARAM_FAVORITE_INPUT, None),
            GranularityEditKind::EditTf { original } => (
                crate::text_input::TF_FAVORITE_INPUT,
                Some(mlc_core::chart_type::GranularityTarget::Tf { name: original.clone() }),
            ),
            GranularityEditKind::EditParam { original } => {
                let chart_type = self.panel_app.panel_grid.active_container_id()
                    .and_then(|cid| self.chart_type_for_chart(cid))
                    .unwrap_or("candles")
                    .to_string();
                (
                    crate::text_input::PARAM_FAVORITE_INPUT,
                    Some(mlc_core::chart_type::GranularityTarget::Param { chart_type, value: *original }),
                )
            }
            GranularityEditKind::AddInterval => (crate::text_input::TF_FAVORITE_INPUT, None),
            GranularityEditKind::EditInterval { original } => (
                crate::text_input::TF_FAVORITE_INPUT,
                Some(mlc_core::chart_type::GranularityTarget::Interval { name: original.clone() }),
            ),
        };
        let field_id = uzor::WidgetId::from(field);
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let tf = self.input_coordinator.borrow();
        let store = tf.text_fields();
        Some(mlc_core::chart_type::GranularityEditRender {
            host,
            text: store.text(&field_id).to_string(),
            cursor: store.cursor(&field_id),
            selection: store.selection_range(&field_id),
            cursor_visible: store.cursor_visible(now_ms),
        })
    }

    /// Seed text for `symbol_quick_edit`/`SYMBOL_QUICK_SEARCH` when opening
    /// the `symbol_selector` fast-search (2026-07-17 rework: the
    /// button-hosted editor is the ONLY quick-search mode). The editor
    /// starts with the FULL COMPOSITE KEY of the active instrument —
    /// `"SYMBOL:exch:AT"` (e.g. `"BTCUSDT:bybit:S"`) — which is valid
    /// composite-query syntax for [`Self::build_symbol_search_results`], so
    /// the dropdown opens on the exact instrument and the user backspaces
    /// the suffix to widen the search. The button rect expands one-off to
    /// fit it (see the editing branch in `chart_toolbar_render_toolbars`).
    /// The caller places the cursor at the end.
    pub(crate) fn symbol_quick_seed_text(&self) -> String {
        let Some(cid) = self.panel_app.panel_grid.active_container_id() else {
            return String::new();
        };
        let Some(sym) = self.symbol_for_chart(cid) else {
            return String::new();
        };
        let exch = self.exchange_for_chart(cid).unwrap_or_default();
        let at = self.account_type_for_chart(cid).unwrap_or_default();
        if exch.is_empty() {
            return sym.to_string();
        }
        // Exchange UPPERCASE in the visible seed too (owner rule
        // 2026-07-17) — safe for search (the query parser lowercases) and
        // for apply (the commit key is built from the result row's
        // canonical `exchange_id`, never from this text).
        format!("{}:{}:{}", sym, exch.to_uppercase(), at.as_label())
    }

    /// Render-side snapshot of the `symbol_selector` toolbar dropdown's
    /// inline fast-search editor (symbol-search-compare-inner arc, Wave 2,
    /// кусок 4) — `None` whenever `self.symbol_quick_edit` is `None` (the
    /// dropdown isn't showing the fast-search input this frame). Mirrors
    /// [`Self::granularity_edit_render`]'s role: reads the live
    /// `SYMBOL_QUICK_SEARCH` text field + recomputes the top-10 result rows
    /// from `build_symbol_search_results` (same per-frame recompute
    /// contract `ModalState::symbol_search_results` uses for the full-screen
    /// modal — mlc-app-layout cannot call back into mlc-app, so the results
    /// are computed HERE and passed down as plain `SearchResult` data,
    /// mirroring how `granularity_favorites` is passed by reference).
    pub fn symbol_quick_search_render(&self) -> Option<mlc_core::state::modal_state::SymbolQuickSearchRender> {
        let edit = self.symbol_quick_edit.as_ref()?;
        let field_id = uzor::WidgetId::from(crate::text_input::SYMBOL_QUICK_SEARCH);
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0);
        let (text, cursor, selection, cursor_visible) = {
            let tf = self.input_coordinator.borrow();
            let store = tf.text_fields();
            (
                store.text(&field_id).to_string(),
                store.cursor(&field_id),
                store.selection_range(&field_id),
                store.cursor_visible(now_ms),
            )
        };
        let mut results = Self::build_symbol_search_results(
            &edit.text,
            self.core.watchlists().find(self.watchlist_ui.active_list_id),
            self.core.exchange_symbols().as_map(),
        );
        // A liquidation panel's picker offers ONLY venues that publish a
        // liquidation channel (owner order 2026-08-02). Every other venue
        // would add a column that can never fill, and a column that never
        // fills is read as a quiet market rather than an absent feed —
        // this session lost time to exactly that reading.
        if let crate::SymbolQuickEditTarget::MultiTapeRoster(pid)
        | crate::SymbolQuickEditTarget::SectionPanel(pid) = edit.target
        {
            if self.panels_store.liq_tape.contains_key(&pid) {
                results.retain(|r| {
                    // Futures only, on top of the publisher filter. A
                    // liquidation is a derivatives event — a SPOT row in
                    // this list can never deliver one, and the owner read
                    // the spot rows as venues that were simply switched off.
                    r.account_type.eq_ignore_ascii_case("F")
                        && mlc_app_panels::trading::order_flow::multi_dom::venue_has_liquidations(
                            &r.exchange_id,
                        )
                });
            }
        }
        // Multi-DOM roster target (owner spec 2026-07-30): the already-added
        // columns are PINNED as the first block (always present regardless of
        // the query; the dropdown builder draws a separator after them),
        // followed by the per-exchange facet of the query with the pinned
        // rows deduped out. Every row carries a DOM-icon toggle whose ACTIVE
        // state means "this (exchange, AT) column is in the Multi-DOM";
        // `in_watchlist` is reused as the active flag.
        let mut roster_mode = false;
        let mut pinned_count = 0usize;
        // Section pickers carry the SECTION'S OWN slot icon on every row
        // (owner order 2026-07-31: «для каждой нужна своя иконка») and the
        // icon click adds the venue to THIS panel — never the watchlist.
        let section_icon: Option<String> = match edit.target {
            crate::SymbolQuickEditTarget::MultiDomRoster(_) => Some("slot_dom".to_string()),
            crate::SymbolQuickEditTarget::MultiTapeRoster(pid)
            | crate::SymbolQuickEditTarget::SectionPanel(pid) => {
                let ps = &self.panels_store;
                if ps.dom.contains_key(&pid) {
                    Some("slot_dom".to_string())
                } else if ps.trade_tape.contains_key(&pid) {
                    Some("slot_tape".to_string())
                } else if ps.l2_tape.contains_key(&pid) {
                    Some("slot_l2".to_string())
                } else if ps.big_trades.contains_key(&pid) {
                    Some("slot_big_trades".to_string())
                } else if ps.liq_tape.contains_key(&pid) {
                    Some("slot_liq".to_string())
                } else {
                    None
                }
            }
            _ => None,
        };
        // Multi-tape roster target shares the same pinned-block contract
        // (M/TT · M/L2 · M/BT, owner 2026-07-31).
        let roster_source: Option<(String, Vec<String>)> = match edit.target {
            crate::SymbolQuickEditTarget::MultiDomRoster(pid) => self
                .panels_store
                .dom
                .get(&pid)
                .map(|s| (s.multi.symbol.clone(), s.multi.config.exchanges.clone())),
            crate::SymbolQuickEditTarget::MultiTapeRoster(pid)
            | crate::SymbolQuickEditTarget::SectionPanel(pid) => {
                let ps = &self.panels_store;
                ps.trade_tape.get(&pid).map(|s| (s.multi.symbol.clone(), s.multi.entries.clone()))
                    .or_else(|| ps.l2_tape.get(&pid).map(|s| (s.multi.symbol.clone(), s.multi.entries.clone())))
                    .or_else(|| ps.big_trades.get(&pid).map(|s| (s.multi.symbol.clone(), s.multi.entries.clone())))
                    // LIQ was missing from this chain, so its picker had no
                    // pinned block at all: the connected venues were not
                    // hoisted above the separator and the list read as a
                    // plain search with a few rows lit at random (owner
                    // report 2026-08-02). Its roster is the same
                    // `multi.entries` every other tape uses.
                    .or_else(|| ps.liq_tape.get(&pid).map(|s| (s.multi.symbol.clone(), s.multi.entries.clone())))
            }
            _ => None,
        };
        {
            if let Some((base, roster)) = roster_source {
                use mlc_app_panels::trading::order_flow::multi_dom;
                roster_mode = true;
                let pinned: Vec<mlc_core::state::modal_state::SearchResult> = roster
                    .iter()
                    .map(|e| mlc_core::state::modal_state::SearchResult {
                        symbol: base.clone(),
                        name: String::new(),
                        exchange: multi_dom::col_exchange(e).to_uppercase(),
                        exchange_id: multi_dom::col_exchange(e).to_string(),
                        asset_type: "Crypto".to_string(),
                        category_icon: String::new(),
                        in_watchlist: true,
                        account_type: multi_dom::col_at(e).to_uppercase(),
                    })
                    .collect();
                pinned_count = pinned.len();
                results.retain(|r| {
                    !(r.symbol.eq_ignore_ascii_case(&base)
                        && roster.iter().any(|e| {
                            multi_dom::col_matches(e, &r.exchange_id, &r.account_type)
                        }))
                });
                results.truncate(10);
                // Result rows are NOT in the roster by construction — the
                // watchlist flag must not light their section icon (owner
                // defect 2026-07-31: watchlist rows looked already-added).
                for r in results.iter_mut() {
                    r.in_watchlist = false;
                }
                let mut merged = pinned;
                merged.extend(std::mem::take(&mut results));
                results = merged;
            }
        }
        if !roster_mode {
            results.truncate(10);
            // Single-mode section picker: the active icon marks the panel's
            // CURRENT venue (the one already streaming) — same "in this
            // panel" semantics the roster mode uses, never the watchlist.
            if section_icon.is_some() {
                if let crate::SymbolQuickEditTarget::SectionPanel(pid) = edit.target {
                    let cur: Option<(String, String, String)> = {
                        let ps = &self.panels_store;
                        ps.dom.get(&pid).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                            .or_else(|| ps.trade_tape.get(&pid).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())))
                            .or_else(|| ps.l2_tape.get(&pid).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())))
                            .or_else(|| ps.big_trades.get(&pid).map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone())))
                    };
                    if let Some((sym, ex, at)) = cur {
                        let at = if at.is_empty() { "S".to_string() } else { at };
                        for r in results.iter_mut() {
                            r.in_watchlist = r.symbol.eq_ignore_ascii_case(&sym)
                                && r.exchange_id.eq_ignore_ascii_case(&ex)
                                && r.account_type.eq_ignore_ascii_case(&at);
                        }
                    }
                }
            }
        }
        Some(mlc_core::state::modal_state::SymbolQuickSearchRender {
            text,
            cursor,
            selection,
            cursor_visible,
            results,
            roster_mode,
            pinned_count,
            section_icon,
        })
    }

    /// Render-side snapshot of the `timeframe_selector` dropdown's active
    /// FAVORITES/CUSTOM row drag-to-reorder (owner spec 2026-07-16 items
    /// 2+3) — `None` whenever `self.granularity_drag` is `None` (no row is
    /// being dragged this frame). Mirrors [`Self::granularity_edit_render`]'s
    /// role: converts `mlc-app`-only state into the `mlc_core::chart_type`
    /// render type `mlc-app-layout`'s dropdown builder can consume without
    /// depending on this crate.
    pub fn granularity_drag_render(&self) -> Option<mlc_core::chart_type::GranularityDragRender> {
        let drag = self.granularity_drag.as_ref()?;
        Some(mlc_core::chart_type::GranularityDragRender {
            family: drag.family.clone(),
            section: drag.section,
            from_index: drag.from_index,
            drop_index: drag.drop_index,
        })
    }

    /// Insertion index for a favorites-bar tool dropped at cursor x: the
    /// count of tool slots (grip / empty / add skipped) whose midpoint
    /// lies left of x. Range 0..=len. Used by in-bar reorder.
    pub(crate) fn fav_drop_index(&self, x: f64) -> usize {
        let Some(fav) = self.last_toolbar_result.as_ref()
            .and_then(|r| r.favorites_bar.as_ref())
        else {
            return 0;
        };
        mlc_core::toolbar::favorites_bar_drop_index(
            fav.item_rects.iter().map(|(id, r)| (id.as_str(), r.x, r.width)),
            x,
        )
    }

    pub fn set_shift_held(&mut self, held: bool) {
        self.shift_held = held;
        if let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) {
            self.core.drawings_mut().set_shift_constrain(bid, held);
        }
    }

    /// Shift-constrain a drawing cursor against the first placed pivot.
    pub(crate) fn snap_draw_point(&self, tool_id: &str, ts0: i64, p0: f64, ts1: i64, p1: f64) -> (i64, f64) {
        let Some(w) = self.panel_app.panel_grid.active_container() else {
            return (ts1, p1);
        };
        let bars = self.active_chart_bars();
        let vp = w.viewport();
        let ps = w.price_scale();
        mlc_core::drawing::snap_drawing_data(
            tool_id, &bars, vp, ps.price_min, ps.price_max, ts0, p0, ts1, p1,
        )
    }

    /// Whether the footprint bucket dropdown (`ResolutionKind::Interval`)
    /// should mark every item "live only" for the ACTIVE window's (exchange,
    /// account_type) — true exactly when `mlc_backend::chart_type::
    /// is_footprint_hybrid` resolves hybrid (no deep REST tier AND no trade
    /// archive — the same capability check that drives footprint's
    /// kline-backdrop hybrid subscribe path; an archive-backed `RecentOnly`
    /// venue, e.g. Bybit spot, resolves `false` here). `false` when there is
    /// no active window or the capability isn't known yet (exchange not
    /// connected) — safer default, matches `is_footprint_hybrid`'s own
    /// `None`-is-not-hybrid contract.
    pub fn footprint_live_only(&self) -> bool {
        let Some(cid) = self.panel_app.panel_grid.active_container_id() else {
            return false;
        };
        let account_type = self.account_type_for_chart(cid).unwrap_or_default();
        let exchange_id = self.exchange_for_chart(cid)
            .and_then(|exch| digdigdig3::ExchangeId::from_str(&exch))
            .unwrap_or(self.active_exchange);
        let at = mlc_backend_data_bridge::account_type_to_dig3(account_type);
        mlc_backend::chart_type::is_footprint_hybrid(self.core.bridge_handle(), exchange_id, at)
    }

    /// Audit D8 (chart-types-xaxis-audit §3 items c/e): the `Vec<Bar>`
    /// timestamp shadow `ChartRenderState.bars` should carry for THIS
    /// chart's chart_type, for time-scale labels + drawings ts→x. `None`
    /// means "keep the real `bubble.bars`" (candle-family, CVD, TPO,
    /// heatmaps, footprint, range/volume/tick/dollar-family — see
    /// `ts_shadow_for_chart_type` for the per-type rationale).
    pub fn ts_shadow_for_chart(&self, chart_id: mlc_core::state::ChartId) -> Option<Vec<mlc_core::Bar>> {
        let bid = self.bubble_for_chart(chart_id)?;
        let chart_type = self.chart_type_for_chart(chart_id).unwrap_or("candles");
        self.with_bubble(bid, |b| crate::active_series::ts_shadow_for_chart_type(chart_type, b))
            .flatten()
    }

    /// Test-only: clone of the `DataBridge` handle. Headless harness uses it
    /// to inject synthetic LiveUpdates via `debug_broadcast_tx`.
    #[doc(hidden)]
    pub fn bridge_arc(&self) -> std::sync::Arc<DataBridge> {
        self.core.bridge_arc()
    }

    /// Create a new chart application for the given symbol.
    ///
    /// Connects to Binance as the default exchange. Bars are requested
    /// asynchronously and fed into the chart window once they arrive.
    ///
    /// Native only — builds a multi-thread tokio runtime and blocks on
    /// `DataBridge::new()`. On wasm32 the caller owns the runtime; use the
    /// async `new_with_bridge` path instead.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(symbol: &str) -> Self {
        // Build a dedicated multi-thread runtime that owns all bridge background
        // tasks. Kept alive for the lifetime of the returned ChartApp (via
        // BackendCore::new_with_runtime) so subscriptions continue running.
        //
        // DataBridge::new() is async; we block_on it here so ChartApp::new
        // remains a sync constructor, compatible with winit / immediate-mode callers.
        let rt = std::sync::Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(4)
                .thread_name("mlc-chart")
                .enable_all()
                .build()
                .expect("failed to build tokio runtime for ChartApp"),
        );
        let rt_handle = rt.handle().clone();
        let (bridge, _first_rx, _connector_ready_rx) = rt.block_on(DataBridge::new());
        let bridge = std::sync::Arc::new(bridge);
        // _first_rx is the broadcast::Receiver from DataBridge::new(). BackendCore::new
        // calls bridge.add_listener() internally, so _first_rx is dropped here.
        let trading = mlc_backend::trading::TradingManager::new(
            bridge.clone(),
            std::path::PathBuf::from("."),
            rt_handle,
        ).ok();
        #[cfg(not(target_arch = "wasm32"))]
        let core = mlc_backend::BackendCore::new_with_runtime(bridge.clone(), trading, rt);
        #[cfg(target_arch = "wasm32")]
        let core = mlc_backend::BackendCore::new(bridge.clone(), trading);

        let mut app = Self {
            bubble_feeds: std::collections::HashMap::new(),
            liq_cursors: std::collections::HashMap::new(),
            liq_armed: std::collections::HashMap::new(),
            oi_hist_armed: std::collections::HashMap::new(),
            dom_books_armed: std::collections::HashMap::new(),
            profile_bars: std::collections::HashMap::new(),
            profile_klines_armed: std::collections::HashMap::new(),
            panel_app: ChartPanelApp::new(symbol),
            input_coordinator: RefCell::new(InputCoordinator::new()),
            input_handler: DefaultChartInputHandler::new(),
            width: 1280,
            height: 800,
            frame_result: None,
            scale_corner_zones_by_leaf: std::collections::HashMap::new(),
            last_loading_spinners: Vec::new(),
            search_modal_result: None,
            context_menu_result: None,
            hovered_context_menu_item_id: None,
            gesture_bindings: mlc_core::state::GestureBindings::default(),
            last_mouse_pos: (0.0, 0.0),
            content_rect: LayoutRect::new(0.0, 0.0, 1280.0, 800.0),
            last_inline_bar_rect: None,
            last_favorites_bar_rect: None,
            fav_item_drag_pending: None,
            fav_item_drag: None,
            fav_group_drag_pending: None,
            fav_group_drag: None,
            modal_state: ModalState::new(),
            pending_screenshot: false,
            capturing_screenshot: false,
            pending_share_live: false,
            pending_reset_cache: false,
            pending_reset_storage: false,
            drag_start_points: None,
            color_picker_drag: None,
            compare_style_before_drag: None,
            undo_seq: 0,
            primitive_json_before_drag: None,
            edit_recording_depth: 0,
            ui_drag_active: false,
            drag_dismissed_popup: false,
            split_separator_drag: None,
            leaf_tab_hit_zones: std::collections::HashMap::new(),
            leaf_tab_hover: mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            system_events: mlc_core::SystemEventLog::new(),
            active_toasts: Vec::new(),
            toast_drag: None,
            sidebar_state: mlc_app_sidebar_content::state::SidebarState::new(),
            // Corrected on first frame by `workspace_focus()`'s live
            // `active_container_id()` fallback — see field doc.
            workspace_focus: focus::WorkspaceFocus::Chart(mlc_core::ChartId(0)),
            watchlist_ui: watchlist_ui::WatchlistUiState::default(),
            counters_symbol: None,
            last_sidebar_result: None,
            sidebar_separator_drag_active: false,
            split_without_group: false,
            right_toolbar_left_x: 0.0,
            selected_indicator_id: None,
            standalone_panels: standalone_panel::StandalonePanelRegistry::new(),
            last_watchlist_modal_result: None,
            last_wl_group_name_result: None,
            needs_initial_viewport_fit: false,
            core,
            tag_ui: tag_ui::TagUiRegistry::new(),
            active_exchange: digdigdig3::ExchangeId::Binance,
            window_id: format!("win_{}", web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()),
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            profile_dirty: false,
            shift_held: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            alerts_sync_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
            pending_autosave: None,
            readonly_preset_ids: std::collections::HashSet::new(),
            shared_chart_meta: std::collections::HashMap::new(),
            pending_open_share: None,
            pending_clone_share: None,
            pending_reopen_shares: Vec::new(),
            pending_share_tokens: std::collections::HashMap::new(),
            deferred_share_reopens: Vec::new(),
            snapshot_actions: Vec::new(),
            template_actions: Vec::new(),
            perf_actions: Vec::new(),
            theme_changed: None,
            recalc_mode_changed: None,
            language_changed: None,
            server_enabled_changed: None,
            local_agent_key_changed: None,
            clipboard_text: None,
            anim_frame: 0,
            key_create_request: None,
            key_delete_request: None,
            key_list_refresh: false,
            pending_open_url: None,
            pending_updater_cmd: None,
            pending_cure_after_preset_load: Vec::new(),
            pending_cold_apply: None,
            pending_background_warm: std::collections::VecDeque::new(),
            background_warm_seeded: false,
            lag_event_count: 0,
            recalc_count: 0,
            recalc_log_timer: web_time::Instant::now(),
            idle_release_sweep_timer: web_time::Instant::now(),
            trade_count: 0,
            diagnostics_enabled: false,
            sidebar_data_dirty: true,
            sidebar_ticker_mark: None,
            last_active_leaf: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            indicator_heal_attempted: std::collections::HashSet::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            pending_sub_pane_hidden: std::collections::HashMap::new(),
            maximized_sub_panes: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            load_state: crate::persistence::LoadState::default(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            diag_queue_len: 0,
            diag_mini_ticker: 0,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_composite_us: 0,
            last_orderbook_panel_us: 0,
            last_dom_panel_us: 0,
            last_l2_panel_us: 0,
            last_heatmap_panel_us: 0,
            last_trade_panel_us: 0,
            last_bar_apply_us: 0,
            last_ob_event_count: 0,
            last_trade_event_count: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            sub_pane_sep_drag: None,

            active_drag_panel: None,
            slot_tradetape_drag: None,
            active_frame_layout: None,
            frame_layouts: std::collections::HashMap::new(),
            pending_compare: Vec::new(),
            compare_add_attempts: 0, compare_add_success: 0, compare_add_queued: 0,
            granularity_edit: None,
            granularity_drag_pending: None,
            granularity_drag: None,
            symbol_quick_edit: None,
            trading_header_tb: mlc_core::state::ChartToolbarState::default(),
            last_trading_header_result: None,
            primitive_clipboard: None,
            inline_prim_text_edit: None,
            trading_center_charts: std::collections::HashMap::new(),
            trading_center_rect: None,
            trading_center_input: DefaultChartInputHandler::new(),
            trading_center_drag_active: false,
            trading_center_corner_zones: None,
            trading_center_sep_drag: None,
            trading_center_ind_overlay: mlc_app_layout::ui::modal_settings::IndicatorOverlayState::default(),
            tc_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            tc_modal_result: None,
            rc_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            rc_modal_result: None,
            dom_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            panel_settings_open: None,
            panel_settings_result: None,
            watchlist_warmup_queue: std::collections::VecDeque::new(),
            watchlist_warmup_budget: std::collections::HashMap::new(),
            watchlist_warmed: std::collections::HashSet::new(),
            agent_key_store: None,
            alert_secrets_store: None,
        };

        // Trading snapshots are per-panel now (`(exchange, account_type)`
        // keyed) — panels bind their own snapshot at spawn time in
        // `panel_click.rs`'s `spawn_panel_in_slot`, not here.

        // Load persisted user state (templates, presets, profile) from disk.
        // Must happen before toolbar_config override so that the active_preset_id
        // from the profile is respected during the first render.
        {
            let (loaded_presets, loaded_tm, loaded_profile, loaded_snapshots, loaded_vault_key) =
                app.panel_app.load_user_state();
            app.core.presets_mut().replace_all(loaded_presets);
            app.core.templates_mut().replace_all(loaded_tm);
            app.core.user_mut().replace_all(loaded_profile, loaded_snapshots, loaded_vault_key);
        }

        // Restore user profile (sidebar state) and watchlist from disk.
        app.load_user_profile();

        let exchange_id = digdigdig3::ExchangeId::Binance;
        let exchange_name = exchange_id.as_str().to_string();
        let data_provider: SharedDataProvider = std::sync::Arc::new(
            LiveDataProvider::new(exchange_id, exchange_name.clone(), digdigdig3::AccountType::Spot, bridge.clone()),
        );

        // Check if a saved preset exists for the active_preset_id.
        let has_saved_preset = app.core.presets().contains_key(&app.panel_app.active_preset_id);

        if has_saved_preset {
            // ── Restore from saved preset ───────────────────────────────
            // Attach data_provider to the initial window so LoadPreset can clone it.
            if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                window.data_provider = data_provider.clone();
                window.toolbar_config = ToolbarConfig::standalone();
            }
            app.panel_app.toolbar_config = ToolbarConfig::standalone();

            // Trigger the full preset restore pipeline (layout, windows, indicators, etc.).
            let preset_id = app.panel_app.active_preset_id.clone();
            app.process_chart_out_event(
                mlc_core::events::ChartOutEvent::LoadPreset { id: preset_id },
            );

            // Ensure all windows have a data_provider and toolbar config.
            // Phase 4 B: read exchange/account_type from branch_meta (no longer on struct).
            // §3.4 (handoff 2026-07-06): tuple widened with chart_type so the
            // request_bars loop below can skip derived windows — those are
            // ignited via `ignite_derived_windows` right after.
            let window_data: Vec<(String, String, mlc_core::state::Timeframe, mlc_core::instrument::AccountType, &'static str)> = {
                let containers = app.panel_app.panel_grid.containers();
                containers.keys()
                    .filter_map(|cid| {
                        let sym = app.symbol_for_chart(*cid).unwrap_or_default();
                        // Container without a bubble yields sym="" — subscribing
                        // it mints a zombie kline series the station resubs on
                        // every silence_timeout forever.
                        if sym.is_empty() {
                            return None;
                        }
                        let exch = app.exchange_for_chart(*cid).unwrap_or_default();
                        let tf = app.timeframe_for_chart(*cid).unwrap_or_else(mlc_core::state::Timeframe::h1);
                        let at = app.account_type_for_chart(*cid).unwrap_or_default();
                        let ct = app.chart_type_for_chart(*cid).unwrap_or("candles");
                        Some((sym, exch, tf, at, ct))
                    })
                    .collect()
            };

            // Pre-collect (chart_id → exchange, account_type) for data_provider seeding.
            let provider_seeds: Vec<(mlc_core::state::ChartId, String, mlc_core::instrument::AccountType)> = {
                app.panel_app.panel_grid.containers().keys()
                    .map(|cid| {
                        let exch = app.exchange_for_chart(*cid).unwrap_or_default();
                        let at = app.account_type_for_chart(*cid).unwrap_or_default();
                        (*cid, exch, at)
                    })
                    .collect()
            };
            for (cid, exch, at_typed) in provider_seeds {
                if let Some(window) = app.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                    let win_exchange_id = digdigdig3::ExchangeId::from_str(&exch)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                    let win_at = mlc_backend_data_bridge::account_type_to_dig3(at_typed);
                    let win_provider: SharedDataProvider = std::sync::Arc::new(
                        LiveDataProvider::new(win_exchange_id, &exch, win_at, bridge.clone()),
                    );
                    window.data_provider = win_provider;
                    window.toolbar_config = ToolbarConfig::standalone();
                }
            }

            // Set active_exchange from the active window's exchange.
            // S2.7.1: read exchange via branch_meta instead of struct field.
            if let Some(cid) = app.panel_app.panel_grid.active_container_id() {
                let exch = app.exchange_for_chart(cid)
                    .unwrap_or_default();
                app.active_exchange = digdigdig3::ExchangeId::from_str(&exch)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
            }

            // Ensure connectors are ready and request bars for each window's own exchange.
            // Skip derived chart types — `ignite_derived_windows` below fires their
            // matching bridge sub instead. Doing the kline `request_bars` for them
            // would only trigger a REST fetch discarded by the ownership gate.
            for (sym, exch, tf, at_typed, ct) in &window_data {
                let eid = digdigdig3::ExchangeId::from_str(&exch)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                if !app.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                    continue;
                }
                if matches!(*ct,
                    "footprint" | "range_bar" | "volume_bar" | "tick_bar"
                    | "dollar_bar" | "tick_imbalance_bar" | "volume_imbalance_bar"
                    | "run_bar" | "renko" | "pnf" | "kagi" | "three_line_break"
                    | "tpo"
                ) {
                    continue;
                }
                let at = mlc_backend_data_bridge::account_type_to_dig3(*at_typed);
                bridge.ensure_connector(eid);
                bridge.request_bars(eid, sym, tf, at, None, Some(app.core.user().profile().domain.bar_count as usize), false);
            }
            // Derived windows: ignite the matching pipeline + stamp bar-series
            // ownership. Runs synchronously right after, so the flag lands before
            // any REST response can. §3.4 (handoff 2026-07-06) widened from
            // footprint-only to the whole derived family.
            app.ignite_derived_windows();
            // DOM/Liquidation heatmap overlays whose `dom_heatmap_show`/
            // `liq_heatmap_show` came back true (chart-type → overlay
            // migration hydration mapping) — sibling of the derived-windows
            // ignition above.
            app.ignite_legacy_heatmap_overlays();
            // Default companions (klines → VOLUME, footprint → CVD) for
            // restored windows — ChangeChartType never ran for them.
            app.sync_default_indicators_for_all_windows();

            // Defer viewport positioning until the first resize() when chart_width is known.
            app.needs_initial_viewport_fit = true;
            // Preset restored successfully.
        } else {
            // ── Fresh state — Binance default ──────────────────────────────
            let bar_count = app.core.user().profile().domain.bar_count as usize;
            app.seed_default_chart(
                digdigdig3::ExchangeId::Binance,
                "BTCUSDT",
                mlc_core::state::Timeframe::h1(),
                digdigdig3::AccountType::Spot,
                bar_count,
            );
        }

        // The full set of enabled connectors is NOT connected here (that fired 21
        // simultaneous dials in the constructor). They warm up once from tick()
        // via warmup_connectors() → bridge.warmup_all_connectors (bridge-guarded,
        // off the UI thread) during the skeleton/loading screen. The bridge is
        // shared and outlives the skeleton→live promote, so by the time the chart
        // appears the connectors are up and the live window does no connector work.
        // The exchanges actually NEEDED right away (active window + watchlist) are
        // connected eagerly above/below this point.

        // Request the full symbol list from Binance (already started above).
        // Moex symbols will be requested via ConnectorReady once it initialises.
        bridge.request_symbols(exchange_id);

        // Subscribe to mini ticker updates for each symbol in the active watchlist,
        // using each symbol's own exchange.
        {
            let wl_guard = app.core.watchlists();
            let wl_syms: Vec<(String, String, mlc_core::instrument::AccountType)> = wl_guard.find(app.watchlist_ui.active_list_id)
                .map(|wl| wl.all_symbols().into_iter().map(|ws| (ws.exchange.clone(), ws.symbol.clone(), ws.account_type)).collect())
                .unwrap_or_default();
            drop(wl_guard);
            for (ws_exchange_str, ws_symbol, ws_account_type) in wl_syms {
                let ws_exchange = digdigdig3::ExchangeId::from_str(&ws_exchange_str)
                    .unwrap_or(exchange_id);
                if !app.sidebar_state.connector_enabled.get(ws_exchange.as_str()).copied().unwrap_or(true) {
                    continue;
                }
                let ws_at = mlc_backend_data_bridge::account_type_to_dig3(ws_account_type);
                bridge.ensure_connector(ws_exchange);
                bridge.subscribe_mini_ticker(ws_exchange, &ws_symbol, ws_at);
            }
        }

        // Every text field this app owns — see `register_all_text_fields`.
        register_all_text_fields(&mut app.input_coordinator.borrow_mut());

        // Populate sub_panes from the real indicator_manager.
        app.sync_sub_panes_from_manager();

        app
    }

    /// Create a blank chart window without loading the user profile from disk.
    ///
    /// Use this when spawning additional windows in a multi-window setup.
    /// The window starts with Binance, "BTCUSDT", "1H" and default indicators.
    ///
    /// Unlike [`new`], this does **not** spin up a second tokio runtime or
    /// connector pool. `core` is the single process-wide `Arc<BackendCore>`
    /// (constructed once by the caller, shared with every other window built
    /// from the same core) — this window reads live updates through
    /// `core.tick_writers`, no independent broadcast receiver of its own.
    pub fn new_empty(core: std::sync::Arc<mlc_backend::BackendCore>) -> Self {
        let mut app = Self {
            bubble_feeds: std::collections::HashMap::new(),
            liq_cursors: std::collections::HashMap::new(),
            liq_armed: std::collections::HashMap::new(),
            oi_hist_armed: std::collections::HashMap::new(),
            dom_books_armed: std::collections::HashMap::new(),
            profile_bars: std::collections::HashMap::new(),
            profile_klines_armed: std::collections::HashMap::new(),
            panel_app: ChartPanelApp::new(""),
            input_coordinator: RefCell::new(InputCoordinator::new()),
            input_handler: DefaultChartInputHandler::new(),
            width: 1280,
            height: 800,
            frame_result: None,
            scale_corner_zones_by_leaf: std::collections::HashMap::new(),
            last_loading_spinners: Vec::new(),
            search_modal_result: None,
            context_menu_result: None,
            hovered_context_menu_item_id: None,
            gesture_bindings: mlc_core::state::GestureBindings::default(),
            last_mouse_pos: (0.0, 0.0),
            content_rect: LayoutRect::new(0.0, 0.0, 1280.0, 800.0),
            last_inline_bar_rect: None,
            last_favorites_bar_rect: None,
            fav_item_drag_pending: None,
            fav_item_drag: None,
            fav_group_drag_pending: None,
            fav_group_drag: None,
            modal_state: ModalState::new(),
            pending_screenshot: false,
            capturing_screenshot: false,
            pending_share_live: false,
            pending_reset_cache: false,
            pending_reset_storage: false,
            drag_start_points: None,
            color_picker_drag: None,
            compare_style_before_drag: None,
            undo_seq: 0,
            primitive_json_before_drag: None,
            edit_recording_depth: 0,
            ui_drag_active: false,
            drag_dismissed_popup: false,
            split_separator_drag: None,
            leaf_tab_hit_zones: std::collections::HashMap::new(),
            leaf_tab_hover: mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            system_events: mlc_core::SystemEventLog::new(),
            active_toasts: Vec::new(),
            toast_drag: None,
            sidebar_state: mlc_app_sidebar_content::state::SidebarState::new(),
            // Corrected on first frame by `workspace_focus()`'s live
            // `active_container_id()` fallback — see field doc.
            workspace_focus: focus::WorkspaceFocus::Chart(mlc_core::ChartId(0)),
            watchlist_ui: watchlist_ui::WatchlistUiState::default(),
            counters_symbol: None,
            last_sidebar_result: None,
            sidebar_separator_drag_active: false,
            split_without_group: false,
            right_toolbar_left_x: 0.0,
            selected_indicator_id: None,
            standalone_panels: standalone_panel::StandalonePanelRegistry::new(),
            last_watchlist_modal_result: None,
            last_wl_group_name_result: None,
            needs_initial_viewport_fit: false,
            core,
            tag_ui: tag_ui::TagUiRegistry::new(),
            active_exchange: digdigdig3::ExchangeId::Binance,
            window_id: format!(
                "win_{}",
                web_time::SystemTime::now()
                    .duration_since(web_time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis()
            ),
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            profile_dirty: false,
            shift_held: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            alerts_sync_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
            pending_autosave: None,
            readonly_preset_ids: std::collections::HashSet::new(),
            shared_chart_meta: std::collections::HashMap::new(),
            pending_open_share: None,
            pending_clone_share: None,
            pending_reopen_shares: Vec::new(),
            pending_share_tokens: std::collections::HashMap::new(),
            deferred_share_reopens: Vec::new(),
            snapshot_actions: Vec::new(),
            template_actions: Vec::new(),
            perf_actions: Vec::new(),
            theme_changed: None,
            recalc_mode_changed: None,
            language_changed: None,
            server_enabled_changed: None,
            local_agent_key_changed: None,
            clipboard_text: None,
            anim_frame: 0,
            key_create_request: None,
            key_delete_request: None,
            key_list_refresh: false,
            pending_open_url: None,
            pending_updater_cmd: None,
            pending_cure_after_preset_load: Vec::new(),
            pending_cold_apply: None,
            pending_background_warm: std::collections::VecDeque::new(),
            background_warm_seeded: false,
            lag_event_count: 0,
            recalc_count: 0,
            recalc_log_timer: web_time::Instant::now(),
            idle_release_sweep_timer: web_time::Instant::now(),
            trade_count: 0,
            diagnostics_enabled: false,
            sidebar_data_dirty: true,
            sidebar_ticker_mark: None,
            last_active_leaf: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            indicator_heal_attempted: std::collections::HashSet::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            pending_sub_pane_hidden: std::collections::HashMap::new(),
            maximized_sub_panes: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            load_state: crate::persistence::LoadState::default(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            diag_queue_len: 0,
            diag_mini_ticker: 0,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_composite_us: 0,
            last_orderbook_panel_us: 0,
            last_dom_panel_us: 0,
            last_l2_panel_us: 0,
            last_heatmap_panel_us: 0,
            last_trade_panel_us: 0,
            last_bar_apply_us: 0,
            last_ob_event_count: 0,
            last_trade_event_count: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            sub_pane_sep_drag: None,

            active_drag_panel: None,
            slot_tradetape_drag: None,
            active_frame_layout: None,
            frame_layouts: std::collections::HashMap::new(),
            pending_compare: Vec::new(),
            compare_add_attempts: 0, compare_add_success: 0, compare_add_queued: 0,
            granularity_edit: None,
            granularity_drag_pending: None,
            granularity_drag: None,
            symbol_quick_edit: None,
            trading_header_tb: mlc_core::state::ChartToolbarState::default(),
            last_trading_header_result: None,
            primitive_clipboard: None,
            inline_prim_text_edit: None,
            trading_center_charts: std::collections::HashMap::new(),
            trading_center_rect: None,
            trading_center_input: DefaultChartInputHandler::new(),
            trading_center_drag_active: false,
            trading_center_corner_zones: None,
            trading_center_sep_drag: None,
            trading_center_ind_overlay: mlc_app_layout::ui::modal_settings::IndicatorOverlayState::default(),
            tc_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            tc_modal_result: None,
            rc_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            rc_modal_result: None,
            dom_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            panel_settings_open: None,
            panel_settings_result: None,
            watchlist_warmup_queue: std::collections::VecDeque::new(),
            watchlist_warmup_budget: std::collections::HashMap::new(),
            watchlist_warmed: std::collections::HashSet::new(),
            agent_key_store: None,
            alert_secrets_store: None,
        };

        app.core.watchlists_mut().replace_all(mlc_backend::watchlist::WatchlistManager::default_seed());

        // Create a single "Untitled" preset tab.
        let untitled_preset = mlc_core::preset::preset::ChartPreset::new("Untitled".to_string());
        let untitled_id = untitled_preset.id.clone();
        app.core.presets_mut().insert(untitled_id.clone(), untitled_preset);
        app.panel_app.open_tabs = vec![untitled_id.clone()];
        app.panel_app.active_preset_id = untitled_id;

        // Seed active window with Binance/BTCUSDT/1H defaults.
        let bar_count = app.core.user().profile().domain.bar_count as usize;
        app.seed_default_chart(
            digdigdig3::ExchangeId::Binance,
            "BTCUSDT",
            mlc_core::state::Timeframe::h1(),
            digdigdig3::AccountType::Spot,
            bar_count,
        );

        // Every text field this app owns — see `register_all_text_fields`.
        register_all_text_fields(&mut app.input_coordinator.borrow_mut());

        app.needs_initial_viewport_fit = true;
        app.sync_sub_panes_from_manager();

        app
    }

    /// Wire the native host's agent-key `SecretStore` so every vault
    /// seal/unlock call-out in `input/credentials.rs` also keeps the Agent
    /// API's key store in lockstep — both are unlocked/sealed off the same
    /// `VaultKey`, never independently keyed (agent-api-auth plan §2,
    /// "Storage"). Call once, right after the host constructs both stores.
    /// A shell that never wires an Agent API (today: `mlc-app-shell-web`'s
    /// wasm build) simply never calls this, leaving the handle at its
    /// default `None` — every seal/unlock call-out is a no-op then.
    pub fn set_agent_key_store(&mut self, store: std::sync::Arc<mlc_vault::SecretStore>) {
        self.agent_key_store = Some(store);
    }

    /// Wire the native host's alert-secrets `SecretStore` sidecar
    /// (`alert_secrets.enc`) — same contract as [`Self::set_agent_key_store`],
    /// just for the Telegram/webhook delivery payload instead of agent-key
    /// records. Call once, right after the host constructs the store.
    pub fn set_alert_secrets_store(&mut self, store: std::sync::Arc<mlc_vault::SecretStore>) {
        self.alert_secrets_store = Some(store);
    }

    /// Snapshot of [`Self::alert_secrets_store`]'s live payload — `Some` only
    /// while wired AND unlocked; `None` covers both "no sidecar on this
    /// shell" and "sealed", which every caller here already treats the same
    /// way (fall back to an empty [`mlc_alert_delivery::NotificationSecrets`]).
    /// Shared by [`Self::refresh_alert_secrets_mirror`] and
    /// `input/credentials.rs`'s post-unlock migration check so the decode
    /// step lives in one place.
    pub(crate) fn alert_secrets_snapshot(&self) -> Option<mlc_alert_delivery::NotificationSecrets> {
        let bytes = self.alert_secrets_store.as_ref()?.get().ok()?;
        serde_json::from_slice(&bytes).ok()
    }

    /// Re-seed `AlertSettingsState`'s delivery-secrets mirror from
    /// [`Self::alert_secrets_store`] — the host's counterpart of
    /// `input/credentials.rs`'s own post-unlock refresh, for the ONE moment
    /// that refresh cannot cover itself: right after [`Self::new_window`]
    /// (whose internal `apply_profile_state` call seeds this mirror from
    /// `profile.domain.notification_secrets`, the pre-sidecar source, before
    /// the caller has had a chance to call [`Self::set_alert_secrets_store`]
    /// on the freshly-built window) wires a store that is already unlocked
    /// this session. A shell with no sidecar (`alert_secrets_store: None`,
    /// e.g. `mlc-app-shell-web`'s wasm build) never calls this — its windows
    /// keep the `apply_profile_state` seed as their only source, exactly as
    /// before this sidecar existed.
    pub fn refresh_alert_secrets_mirror(&mut self) {
        match self.alert_secrets_snapshot() {
            Some(secrets) => {
                let tg_wid = uzor::WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
                self.input_coordinator.borrow_mut().text_fields_mut().set_text(&tg_wid, &secrets.telegram.bot_token);
                self.panel_app.alert_settings_state.notification_secrets = secrets;
                self.panel_app.alert_settings_state.secrets_available = true;
            }
            None => {
                self.panel_app.alert_settings_state.notification_secrets = mlc_alert_delivery::NotificationSecrets::default();
                self.panel_app.alert_settings_state.secrets_available = false;
            }
        }
    }

    // -------------------------------------------------------------------------
    // seed_default_chart — populate the active window with a default instrument
    // -------------------------------------------------------------------------

    /// Seed the active chart window with a default instrument and trigger the
    /// initial bar fetch + connector setup.
    ///
    /// Available on all platforms (no `cfg(not(wasm32))` gate).  The web shell
    /// calls this after [`new_empty`] to replicate the setup that the native
    /// [`new`] performs inline.
    pub fn seed_default_chart(
        &mut self,
        exchange_id: digdigdig3::ExchangeId,
        symbol: &str,
        timeframe: mlc_core::state::Timeframe,
        account_type: digdigdig3::AccountType,
        bar_count: usize,
    ) {
        let exchange_name = exchange_id.as_str().to_string();
        let at_label = account_type_to_label(account_type).to_string();

        let bridge = self.core.bridge_arc();

        // Build a LiveDataProvider for the active window.
        let data_provider: SharedDataProvider = std::sync::Arc::new(
            LiveDataProvider::new(exchange_id, exchange_name.clone(), account_type, bridge.clone()),
        );

        // Approach B: guarantee an active leaf before seeding the chart.
        // On wasm the DockingManager may have been restored from a snapshot
        // that had no persisted active-leaf selection, leaving active_leaf=None.
        // Without this guard every subsequent call that queries active_container
        // silently returns None and the chart window is never seeded.
        if self.panel_app.panel_grid.docking().active_leaf().is_none() {
            if let Some(fallback) = self.panel_app.panel_grid.first_leaf_id() {
                log::debug!(
                    "[seed_default_chart] docking().active_leaf() = None before seed; \
                     activating first leaf {:?}",
                    fallback
                );
                self.panel_app.panel_grid.set_active_leaf(fallback);
            } else {
                log::debug!(
                    "[seed_default_chart] no leaves in panel_grid — \
                     cannot seed chart for {}/{}", exchange_name, symbol
                );
            }
        }

        let active_chart_id = self.panel_app.panel_grid.active_container_id();

        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.data_provider = data_provider;
            window.toolbar_config = ToolbarConfig::standalone();
            // Arm the snap so that when BarsLoaded lands in `react.rs` the
            // fresh-load branch positions the viewport at the right edge
            // with the standard padding. Without this, fresh boot leaves
            // the viewport sitting on the oldest bars (view_start = 0) and
            // the live tick falls off the right side of the screen.
            window.needs_snap_after_backfill = true;
            window.pending_symbol_load = true;
        }

        if let Some(chart_id) = active_chart_id {
            let auto_bid = self.create_auto_bubble(
                [0.0, 0.0, 0.0, 0.0],
                symbol.to_string(),
                timeframe.clone(),
            );
            self.attach_chart_to_tag(chart_id, auto_bid);
            if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                window.auto_tag_id = auto_bid;
                window.current_tag_id = auto_bid;
            }
            self.core.bubbles_mut().set_bid_instrument_full(
                auto_bid,
                symbol.to_string(),
                exchange_name.clone(),
                at_label.clone(),
                timeframe.clone(),
            );
        }

        self.panel_app.toolbar_config = ToolbarConfig::standalone();

        self.core.ensure_connector(exchange_id);
        self.core.request_kline(exchange_id, symbol, &timeframe, account_type, None, Some(bar_count), false);

        // Subscribe mini-tickers for every symbol in the active watchlist.
        // Watchlist subscriptions are independent of whether the sidebar is
        // visible: the connections stay open so prices are fresh the moment
        // the user opens the panel. Native shell does this in `new_window`
        // after loading a preset; wasm shell calls `seed_default_chart`
        // instead and previously skipped this step → mini-ticker DashMap
        // stayed empty and the watchlist froze at 0.0.
        let wl_syms: Vec<(String, String, mlc_core::instrument::AccountType)> = self.core.watchlists().find(self.watchlist_ui.active_list_id)
            .map(|wl| wl.all_symbols().into_iter().map(|ws| (ws.exchange.clone(), ws.symbol.clone(), ws.account_type)).collect())
            .unwrap_or_default();
        for (ws_exchange_str, ws_symbol, ws_account_type) in wl_syms {
            let ws_exchange = digdigdig3::ExchangeId::from_str(&ws_exchange_str)
                .unwrap_or(exchange_id);
            if !self.sidebar_state.connector_enabled.get(ws_exchange.as_str()).copied().unwrap_or(true) {
                continue;
            }
            let ws_at = mlc_backend_data_bridge::account_type_to_dig3(ws_account_type);
            self.core.ensure_connector(ws_exchange);
            bridge.subscribe_mini_ticker(ws_exchange, &ws_symbol, ws_at);
        }

        // Default companions (klines → VOLUME) for the freshly seeded
        // window. This is the FOURTH load path — the wasm shell boots
        // through seed_default_chart directly (shell-web lib.rs), never
        // touching the preset-restore / new-window / LoadPreset paths that
        // already reconcile companions — which is exactly why the default
        // page came up bare (comp_sync_runs stayed 0 in the live dump,
        // owner report 2026-07-05).
        self.sync_default_indicators_for_all_windows();

        self.needs_initial_viewport_fit = true;
    }

    // -------------------------------------------------------------------------
    // drain_watchlist_actions — apply queued WatchlistActions directly to
    // BackendCore's watchlist + bridge.
    // -------------------------------------------------------------------------
    //
    // Native shell (mlc-app-shell-vello/main.rs:2191-2360) drains the same vec into
    // its per-app `AppState::watchlist_manager` cache. Wasm shell has no
    // AppState (one window, no multi-window sync), so it drives the single
    // source of truth — `core.watchlist_manager` — directly.
    //
    // Returns true if any actions were applied (caller can flag sidebar_dirty
    // or autosave_snapshot from this).
    pub fn drain_watchlist_actions(&mut self) -> bool {
        use crate::WatchlistAction;

        let actions: Vec<WatchlistAction> = self.watchlist_actions.drain(..).collect();
        if actions.is_empty() {
            return false;
        }

        let active_list_id = self.watchlist_ui.active_list_id;

        for action in actions {
            match action {
                WatchlistAction::Toggle { symbol, exchange, account_type } => {
                    let address = mlc_core::InstrumentAddress::new(exchange.clone(), symbol.clone(), account_type.parse().unwrap_or_default());
                    let now_in = self.core.watchlists_mut()
                        .toggle_symbol(active_list_id, &address);
                    if now_in {
                        if let Some(eid) = ExchangeId::from_str(&exchange) {
                            let enabled = self.sidebar_state.connector_enabled
                                .get(eid.as_str()).copied().unwrap_or(true);
                            if enabled {
                                let at = account_type_from_label(&account_type);
                                self.core.ensure_connector(eid);
                                self.core.bridge_handle().subscribe_mini_ticker(eid, &symbol, at);
                            }
                        }
                        self.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                    } else {
                        if let Some(eid) = ExchangeId::from_str(&exchange) {
                            let at = account_type_from_label(&account_type);
                            self.core.unsubscribe_mini_ticker(eid, &symbol, at);
                        }
                        if let Some(ref mut snap) = self.watchlist_ui.list_mut(active_list_id).order_snapshot {
                            snap.retain(|s| s.symbol != symbol);
                        }
                        // Toggle-off removes the row — re-arm its warm-up
                        // memory (all TFs) so a later re-add gets a fresh
                        // background fetch instead of being treated as
                        // already-warmed forever.
                        self.watchlist_warmed.retain(|(addr, _tf)| *addr != address);
                    }
                }
                WatchlistAction::Remove { symbol, exchange, account_type } => {
                    let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                    self.core.watchlists_mut().remove_symbol(active_list_id, &address);
                    // Same re-arm as the Toggle-off path above.
                    self.watchlist_warmed.retain(|(addr, _tf)| *addr != address);
                }
                WatchlistAction::ClearOrderSnapshot => {
                    self.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                }
                WatchlistAction::Reorder { from_idx, to_idx } => {
                    self.core.watchlists_mut().reorder_symbol(active_list_id, from_idx, to_idx);
                }
                WatchlistAction::CreateList { name } => {
                    let new_id = self.core.watchlists_mut().create_list(name);
                    self.watchlist_ui.active_list_id = new_id;
                }
                WatchlistAction::RenameList { id, new_name } => {
                    if let Some(list) = self.core.watchlists_mut().find_mut(id) {
                        list.name = new_name;
                    }
                }
                WatchlistAction::DeleteList { id } => {
                    if self.core.watchlists_mut().delete_list(id) {
                        self.watchlist_ui.remove_list(id);
                        if self.watchlist_ui.active_list_id == id {
                            self.watchlist_ui.active_list_id = self.core.watchlists().lists().first().map(|l| l.id).unwrap_or(0);
                        }
                    }
                }
                WatchlistAction::SetActiveList { id } => {
                    self.watchlist_ui.active_list_id = id;
                }
                WatchlistAction::SetColorFlag { symbol, exchange, account_type, color } => {
                    let color_str = color.as_deref().unwrap_or("");
                    let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                    self.core.watchlists_mut().set_color_flag(active_list_id, &address, color_str);
                }
                WatchlistAction::MoveToGroup { symbol, exchange, account_type, group_name } => {
                    let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                    let group_id = self.core.watchlists().find(active_list_id)
                        .and_then(|list| list.groups.iter().find(|g| g.name == group_name).map(|g| g.id));
                    if let Some(group_id) = group_id {
                        self.core.watchlists_mut().move_to_group(active_list_id, &address, group_id);
                    }
                }
                WatchlistAction::RemoveFromGroup { symbol, exchange, account_type } => {
                    let address = mlc_core::InstrumentAddress::new(exchange, symbol, account_type.parse().unwrap_or_default());
                    if let Some(list) = self.core.watchlists_mut().find_mut(active_list_id) {
                        let mut found: Option<mlc_core::instrument::InstrumentAddress> = None;
                        for g in &mut list.groups {
                            if let Some(pos) = g.symbols.iter().position(|s| *s == address) {
                                found = Some(g.symbols.remove(pos));
                                break;
                            }
                        }
                        if let Some(ws) = found {
                            list.ungrouped.push(ws);
                        }
                    }
                }
                WatchlistAction::SetSeparatorOffsets { offsets } => {
                    self.watchlist_ui.list_mut(active_list_id).column_config.separator_offsets = Some(offsets);
                }
                WatchlistAction::ResetSeparatorOffsets => {
                    self.watchlist_ui.list_mut(active_list_id).column_config.separator_offsets = None;
                }
                WatchlistAction::SetSeparatorOffset { index, value } => {
                    let list_ui = self.watchlist_ui.list_mut(active_list_id);
                    let n_cols = {
                        let c = &list_ui.column_config;
                        let mut n: usize = 1;
                        if c.show_exchange     { n += 1; }
                        if c.show_account_type { n += 1; }
                        if c.show_last_price   { n += 1; }
                        if c.show_change_pct   { n += 1; }
                        if c.show_spark        { n += 1; }
                        if c.show_change_abs   { n += 1; }
                        if c.show_high_low     { n += 2; }
                        if c.show_volume       { n += 1; }
                        n
                    };
                    let n_seps = n_cols.saturating_sub(1);
                    let needs_init = list_ui.column_config.separator_offsets
                        .as_ref()
                        .map(|o| o.len() != n_seps)
                        .unwrap_or(true);
                    if needs_init {
                        list_ui.column_config.separator_offsets = Some(vec![0.0; n_seps]);
                    }
                    if let Some(ref mut offsets) = list_ui.column_config.separator_offsets {
                        if index < offsets.len() {
                            offsets[index] = value;
                        }
                    }
                }
                WatchlistAction::ToggleColumnVisibility { column } => {
                    let list_ui = self.watchlist_ui.list_mut(active_list_id);
                    match column.as_str() {
                        "exchange"     => list_ui.column_config.show_exchange     = !list_ui.column_config.show_exchange,
                        "last_price"   => list_ui.column_config.show_last_price   = !list_ui.column_config.show_last_price,
                        "change_pct"   => list_ui.column_config.show_change_pct   = !list_ui.column_config.show_change_pct,
                        "change_abs"   => list_ui.column_config.show_change_abs   = !list_ui.column_config.show_change_abs,
                        "spark"        => list_ui.column_config.show_spark        = !list_ui.column_config.show_spark,
                        "ticker"       => list_ui.column_config.show_ticker_section = !list_ui.column_config.show_ticker_section,
                        "counters"     => list_ui.column_config.show_market_counters = !list_ui.column_config.show_market_counters,
                        "volume"       => list_ui.column_config.show_volume       = !list_ui.column_config.show_volume,
                        "high_low"     => list_ui.column_config.show_high_low     = !list_ui.column_config.show_high_low,
                        "account_type" => list_ui.column_config.show_account_type = !list_ui.column_config.show_account_type,
                        "align_columns" => {
                            list_ui.column_config.align_columns = !list_ui.column_config.align_columns;
                            if list_ui.column_config.align_columns {
                                list_ui.column_config.separator_offsets = None;
                            }
                        }
                        _ => {}
                    }
                    if column != "align_columns" {
                        list_ui.column_config.separator_offsets = None;
                    }
                }
                WatchlistAction::SortCycle => {
                    self.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                }
                WatchlistAction::ResetSort => {
                    self.watchlist_ui.list_mut(active_list_id).order_snapshot = None;
                }
            }
        }

        self.sidebar_data_dirty = true;
        true
    }

    // -------------------------------------------------------------------------
    // new_window — unified per-window constructor (no primary/secondary distinction)
    // -------------------------------------------------------------------------

    /// Create a per-window chart instance.
    ///
    /// All windows are equal — no primary/secondary distinction.
    /// The bridge, profile, and user state are owned by the App (main.rs).
    ///
    /// `user_manager` is loaded once at application startup and passed in here
    /// so that presets, templates, and profile data are not re-read from disk
    /// for each window.
    pub fn new_window(
        core: std::sync::Arc<mlc_backend::BackendCore>,
        bridge: std::sync::Arc<DataBridge>,
        window_id: String,
        restore: Option<&mlc_core::WindowState>,
        profile: &mlc_core::UserProfile,
        user_manager: &crate::user_manager::ProfileManager,
        skeleton: bool,
    ) -> Self {
        let mut app = Self {
            bubble_feeds: std::collections::HashMap::new(),
            liq_cursors: std::collections::HashMap::new(),
            liq_armed: std::collections::HashMap::new(),
            oi_hist_armed: std::collections::HashMap::new(),
            dom_books_armed: std::collections::HashMap::new(),
            profile_bars: std::collections::HashMap::new(),
            profile_klines_armed: std::collections::HashMap::new(),
            panel_app: ChartPanelApp::new(""),
            input_coordinator: RefCell::new(InputCoordinator::new()),
            input_handler: DefaultChartInputHandler::new(),
            width: 1280,
            height: 800,
            frame_result: None,
            scale_corner_zones_by_leaf: std::collections::HashMap::new(),
            last_loading_spinners: Vec::new(),
            search_modal_result: None,
            context_menu_result: None,
            hovered_context_menu_item_id: None,
            gesture_bindings: mlc_core::state::GestureBindings::default(),
            last_mouse_pos: (0.0, 0.0),
            content_rect: LayoutRect::new(0.0, 0.0, 1280.0, 800.0),
            last_inline_bar_rect: None,
            last_favorites_bar_rect: None,
            fav_item_drag_pending: None,
            fav_item_drag: None,
            fav_group_drag_pending: None,
            fav_group_drag: None,
            modal_state: ModalState::new(),
            pending_screenshot: false,
            capturing_screenshot: false,
            pending_share_live: false,
            pending_reset_cache: false,
            pending_reset_storage: false,
            drag_start_points: None,
            color_picker_drag: None,
            compare_style_before_drag: None,
            undo_seq: 0,
            primitive_json_before_drag: None,
            edit_recording_depth: 0,
            ui_drag_active: false,
            drag_dismissed_popup: false,
            split_separator_drag: None,
            leaf_tab_hit_zones: std::collections::HashMap::new(),
            leaf_tab_hover: mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::None,
            leaf_tab_hovered_leaf: None,
            last_toolbar_result: None,
            pending_delivery_events: Vec::new(),
            pending_alert_screenshot: false,
            system_events: mlc_core::SystemEventLog::new(),
            active_toasts: Vec::new(),
            toast_drag: None,
            sidebar_state: mlc_app_sidebar_content::state::SidebarState::new(),
            // Corrected on first frame by `workspace_focus()`'s live
            // `active_container_id()` fallback — see field doc.
            workspace_focus: focus::WorkspaceFocus::Chart(mlc_core::ChartId(0)),
            watchlist_ui: watchlist_ui::WatchlistUiState::default(),
            counters_symbol: None,
            last_sidebar_result: None,
            sidebar_separator_drag_active: false,
            split_without_group: false,
            right_toolbar_left_x: 0.0,
            selected_indicator_id: None,
            standalone_panels: standalone_panel::StandalonePanelRegistry::new(),
            last_watchlist_modal_result: None,
            last_wl_group_name_result: None,
            needs_initial_viewport_fit: false,
            core,
            tag_ui: tag_ui::TagUiRegistry::new(),
            active_exchange: digdigdig3::ExchangeId::Binance,
            window_id,
            window_x: None,
            window_y: None,
            window_width: None,
            window_height: None,
            profile_dirty: false,
            shift_held: false,
            profile_geometry_dirty: false,
            watchlists_dirty: false,
            alerts_sync_dirty: false,
            watchlist_actions: Vec::new(),
            connector_actions: Vec::new(),
            preset_actions: Vec::new(),
            pending_autosave: None,
            readonly_preset_ids: std::collections::HashSet::new(),
            shared_chart_meta: std::collections::HashMap::new(),
            pending_open_share: None,
            pending_clone_share: None,
            pending_reopen_shares: Vec::new(),
            pending_share_tokens: std::collections::HashMap::new(),
            deferred_share_reopens: Vec::new(),
            snapshot_actions: Vec::new(),
            template_actions: Vec::new(),
            perf_actions: Vec::new(),
            theme_changed: None,
            recalc_mode_changed: None,
            language_changed: None,
            server_enabled_changed: None,
            local_agent_key_changed: None,
            clipboard_text: None,
            anim_frame: 0,
            key_create_request: None,
            key_delete_request: None,
            key_list_refresh: false,
            pending_open_url: None,
            pending_updater_cmd: None,
            pending_cure_after_preset_load: Vec::new(),
            pending_cold_apply: None,
            pending_background_warm: std::collections::VecDeque::new(),
            background_warm_seeded: false,
            lag_event_count: 0,
            recalc_count: 0,
            recalc_log_timer: web_time::Instant::now(),
            idle_release_sweep_timer: web_time::Instant::now(),
            trade_count: 0,
            diagnostics_enabled: false,
            sidebar_data_dirty: true,
            sidebar_ticker_mark: None,
            last_active_leaf: None,
            render_timing_us: (0, 0, 0, 0),
            pending_sub_pane_ratios: std::collections::HashMap::new(),
            indicator_heal_attempted: std::collections::HashSet::new(),
            pending_sub_pane_above_main: std::collections::HashMap::new(),
            pending_sub_pane_order: std::collections::HashMap::new(),
            pending_sub_pane_hidden: std::collections::HashMap::new(),
            maximized_sub_panes: std::collections::HashMap::new(),
            live_preset_cache: std::collections::HashMap::new(),
            load_state: crate::persistence::LoadState::default(),
            agent: agent::AgentSessionManager::default(),
            agent_pty_hover_focused: false,
            agent_pty_drag_active: false,
            agent_chat_drag_active: false,
            agent_autostarted: false,
            diag_queue_len: 0,
            diag_mini_ticker: 0,
            last_tick_us: 0,
            last_indicator_recalc_us: 0,
            last_event_process_us: 0,
            last_auto_scale_us: 0,
            last_composite_us: 0,
            last_orderbook_panel_us: 0,
            last_dom_panel_us: 0,
            last_l2_panel_us: 0,
            last_heatmap_panel_us: 0,
            last_trade_panel_us: 0,
            last_bar_apply_us: 0,
            last_ob_event_count: 0,
            last_trade_event_count: 0,
            panels_store: panels_store::TradingPanelsStore::new(),
            agent_sep_drag: None,
            slot_sep_drag: None,
            sub_pane_sep_drag: None,

            active_drag_panel: None,
            slot_tradetape_drag: None,
            active_frame_layout: None,
            frame_layouts: std::collections::HashMap::new(),
            pending_compare: Vec::new(),
            compare_add_attempts: 0, compare_add_success: 0, compare_add_queued: 0,
            granularity_edit: None,
            granularity_drag_pending: None,
            granularity_drag: None,
            symbol_quick_edit: None,
            trading_header_tb: mlc_core::state::ChartToolbarState::default(),
            last_trading_header_result: None,
            primitive_clipboard: None,
            inline_prim_text_edit: None,
            trading_center_charts: std::collections::HashMap::new(),
            trading_center_rect: None,
            trading_center_input: DefaultChartInputHandler::new(),
            trading_center_drag_active: false,
            trading_center_corner_zones: None,
            trading_center_sep_drag: None,
            trading_center_ind_overlay: mlc_app_layout::ui::modal_settings::IndicatorOverlayState::default(),
            tc_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            tc_modal_result: None,
            rc_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            rc_modal_result: None,
            dom_modal: mlc_app_layout::layout::modals::TradingContainerModalState::default(),
            panel_settings_open: None,
            panel_settings_result: None,
            watchlist_warmup_queue: std::collections::VecDeque::new(),
            watchlist_warmup_budget: std::collections::HashMap::new(),
            watchlist_warmed: std::collections::HashSet::new(),
            agent_key_store: None,
            alert_secrets_store: None,
        };

        // Initialize watchlist with a minimal default — overwritten by load_watchlists below.
        app.core.watchlists_mut().replace_all(mlc_backend::watchlist::WatchlistManager::default_seed());

        // Phase 4: use app-level ProfileManager (loaded once in main.rs) instead
        // of calling load_user_state() per-window.  Eliminates redundant disk
        // reads when multiple windows are opened.
        app.core.templates_mut().replace_all(user_manager.template_manager.clone());
        app.core.presets_mut().replace_all(user_manager.presets.clone());

        // Always restore active_preset_id from profile (not just on restore).
        // This prevents skeleton launches from creating spurious new Untitled presets.
        if !user_manager.profile.ui.active_preset_id.is_empty()
            && app.core.presets().contains_key(&user_manager.profile.ui.active_preset_id)
        {
            app.panel_app.active_preset_id = user_manager.profile.ui.active_preset_id.clone();
        }

        // Restore open_tabs from profile — both for saved windows and promote_skeleton().
        // Without this, promote_skeleton (restore=None) would leave open_tabs empty,
        // causing the phantom "__default__" Chart tab to appear.
        {
            app.panel_app.open_tabs = user_manager.profile.ui.open_tabs.clone();
            {
                let presets_guard = app.core.presets();
                app.panel_app.open_tabs.retain(|id| presets_guard.contains_key(id));
                drop(presets_guard);

                // Re-anchor shared (read-only) tabs at their saved slot
                // instead of letting the `retain` above silently drop them
                // (owner report, item 1: "tabs change places after a
                // reload" — see `install_shared_tab_placeholders`'s doc).
                // Skipped when a per-window `restore` is coming (the block
                // below runs its OWN reconcile against `ws.open_shared_tabs`
                // and overwrites `open_tabs` wholesale) so a shared tab
                // never gets two placeholders + two queued re-fetches from
                // the legacy top-level list AND the per-window one.
                if restore.is_none() {
                    let shared = user_manager.profile.ui.open_shared_tabs.clone();
                    app.install_shared_tab_placeholders(&shared);
                }

                let presets_guard = app.core.presets();
                // Migration: old data has no open_tabs — open only the active preset.
                if app.panel_app.open_tabs.is_empty() && !presets_guard.is_empty() {
                    if !app.panel_app.active_preset_id.is_empty()
                        && presets_guard.contains_key(&app.panel_app.active_preset_id)
                    {
                        app.panel_app.open_tabs = vec![app.panel_app.active_preset_id.clone()];
                    } else {
                        // No active preset known — open the newest one.
                        let mut all: Vec<_> = presets_guard.values().collect();
                        all.sort_by_key(|p| std::cmp::Reverse(p.created_at));
                        if let Some(newest) = all.first() {
                            app.panel_app.open_tabs = vec![newest.id.clone()];
                        }
                    }
                }
            }
        }

        // Keep profile + snapshots in BackendCore's UserProfileManager.
        app.core.user_mut().replace_all(
            user_manager.profile.clone(),
            user_manager.snapshots.clone(),
            user_manager.vault_key,
        );

        {
            let presets_guard = app.core.presets();
            let tm_guard = app.core.templates();
            dlog!(
                "[ChartApp] new_window: using {} presets, {} prim-templates, {} ind-templates",
                presets_guard.len(),
                tm_guard.primitive_templates().len(),
                tm_guard.indicator_templates().len(),
            );
        }

        // Load watchlists from disk (profile.json is NOT re-read here — it was already
        // loaded once at startup and is passed in via `profile`).
        app.load_watchlists();

        // Apply per-window profile state (sidebar width, inline bar, connector prefs).
        // Pass the saved WindowState so per-window sidebar fields take precedence
        // over the legacy flat profile fields (backwards-compatible fallback).
        app.apply_profile_state(profile, restore);

        // If restoring a saved window, apply its tab/preset state.
        if let Some(ws) = restore {
            let valid_tabs: Vec<String> = {
                let presets_guard = app.core.presets();
                ws.open_tabs.iter()
                    .filter(|id| presets_guard.contains_key(*id))
                    .cloned()
                    .collect()
            };
            // Only touch `open_tabs` when this window's own restore has
            // something to contribute (a surviving real tab or a shared
            // one to re-anchor) — an empty `ws` otherwise falls through to
            // whatever the legacy top-level seed above already produced,
            // same as the pre-existing `!valid_tabs.is_empty()` gate.
            if !valid_tabs.is_empty() || !ws.open_shared_tabs.is_empty() {
                app.panel_app.open_tabs = valid_tabs;
                // Re-anchor this window's own shared tabs — see the
                // matching (and, for this restore path, deliberately
                // skipped) block in the legacy top-level seed above.
                app.install_shared_tab_placeholders(&ws.open_shared_tabs);
            }
            if !ws.active_preset_id.is_empty()
                && app.core.presets().contains_key(&ws.active_preset_id)
            {
                app.panel_app.active_preset_id = ws.active_preset_id.clone();
            }
        }

        // Setup data provider and load preset or fresh state.
        let exchange_id = digdigdig3::ExchangeId::Binance;
        let exchange_name = exchange_id.as_str().to_string();
        let data_provider: SharedDataProvider = std::sync::Arc::new(
            LiveDataProvider::new(exchange_id, exchange_name.clone(), digdigdig3::AccountType::Spot, bridge.clone()),
        );

        if skeleton {
            // Skeleton is a pure loading screen — no presets, no connectors,
            // no data providers.  Everything happens after promote_skeleton().
            if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                window.data_provider = data_provider.clone();
                window.toolbar_config = ToolbarConfig::standalone();
            }
            app.panel_app.toolbar_config = ToolbarConfig::standalone();
        } else {
            // Blank-preset poison guard: a preset that never received a
            // collect (no bubbles, no window snapshots — the shape
            // `ChartPreset::new` leaves on disk when nothing autosaved after
            // creation) restores as a bubble-less container: sym="" zombie
            // series, dead legend, no data. Route it through the fresh-state
            // path instead — that path adopts the existing active_preset_id
            // and collects the real default chart into it.
            let preset_has_content = app.core.presets()
                .get(&app.panel_app.active_preset_id)
                .map(|p| !p.bubbles.is_empty() || !p.windows.is_empty())
                .unwrap_or(false);

            // §5.3.5: a preset is live in at most one window. Claim it here
            // (idempotent for this same window id — the cold branch's own
            // later claim below is then just a no-op re-claim) so the
            // branch choice reflects reality. A second window spawned while
            // its source window still shows this preset used to route into
            // LoadPreset anyway, get silently refused in
            // chart_out_events.rs, and dead-end on the container's
            // construction sentinel (BubbleId(0), never patched because
            // apply_preset_cold never ran): empty symbol, filtered out of
            // window_data below, request_bars never fires. Refusing the
            // claim here instead falls through to the fresh-state branch.
            let has_saved_preset = preset_has_content
                && app.core.try_claim_preset(&app.panel_app.active_preset_id, &app.window_id);

            // Claim refused → seed the fresh-state branch from the preset's
            // own first bubble, so the new window opens on the SAME
            // instrument as the window it was spawned from — in its own
            // auto-bubble, never the source window's (N charts -> N bubbles
            // -> 1 instrument; a shared bubble would tie the two windows'
            // timeframes together). Old presets without exchange/account_type
            // fall back to the branch's own defaults below.
            let new_window_seed: Option<(String, String, String, mlc_core::state::Timeframe)> =
                if preset_has_content && !has_saved_preset {
                    app.core.presets()
                        .get(&app.panel_app.active_preset_id)
                        .and_then(|p| p.bubbles.first())
                        .map(|b| {
                            let exch = if b.exchange.is_empty() { exchange_name.clone() } else { b.exchange.clone() };
                            let at = if b.account_type.is_empty() { "S".to_string() } else { b.account_type.clone() };
                            (b.symbol.clone(), exch, at, b.timeframe.clone())
                        })
                } else {
                    None
                };

            if has_saved_preset {
                // ── new_window COLD PATH timer anchor ─────────────────────
                // This branch runs on the off-thread promote worker during
                // startup and on hot-reload. It triggers a synchronous
                // LoadPreset → apply_preset_cold for the active preset
                // PLUS per-window data_provider + per-window request_bars.
                // The total here is what "first chart shows up after
                // launch" effectively waits on (modulo the parallel
                // skeleton render on the main thread).
                let _t_nw_cold = web_time::Instant::now();
                // Restore from saved preset.
                if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                    window.data_provider = data_provider.clone();
                    window.toolbar_config = ToolbarConfig::standalone();
                }
                app.panel_app.toolbar_config = ToolbarConfig::standalone();

                let _t_nw_load = web_time::Instant::now();
                let preset_id = app.panel_app.active_preset_id.clone();
                // Clear active_preset_id so LoadPreset doesn't skip with
                // "already active" — the constructor set it above, but the
                // preset windows haven't been built yet.
                app.panel_app.active_preset_id = String::new();
                app.process_chart_out_event(
                    mlc_core::events::ChartOutEvent::LoadPreset { id: preset_id },
                );
                let nw_load_us = _t_nw_load.elapsed().as_micros();

                // Ensure all windows have data_provider and toolbar config.
                // Phase 4 B: read exchange/account_type from branch_meta.
                // §3.4 (handoff 2026-07-06): tuple widened with chart_type — see
                // startup path above for rationale.
                let window_data: Vec<(String, String, mlc_core::state::Timeframe, mlc_core::instrument::AccountType, &'static str)> = {
                    let containers = app.panel_app.panel_grid.containers();
                    containers.keys()
                        .filter_map(|cid| {
                            let sym = app.symbol_for_chart(*cid).unwrap_or_default();
                            // Bubble-less container → sym="" → zombie series;
                            // see the startup-path twin of this loop.
                            if sym.is_empty() {
                                return None;
                            }
                            let exch = app.exchange_for_chart(*cid).unwrap_or_default();
                            let tf = app.timeframe_for_chart(*cid).unwrap_or_else(mlc_core::state::Timeframe::h1);
                            let at = app.account_type_for_chart(*cid).unwrap_or_default();
                            let ct = app.chart_type_for_chart(*cid).unwrap_or("candles");
                            Some((sym, exch, tf, at, ct))
                        })
                        .collect()
                };

                let provider_seeds_win: Vec<(mlc_core::state::ChartId, String, mlc_core::instrument::AccountType)> = {
                    app.panel_app.panel_grid.containers().keys()
                        .map(|cid| {
                            let exch = app.exchange_for_chart(*cid).unwrap_or_default();
                            let at = app.account_type_for_chart(*cid).unwrap_or_default();
                            (*cid, exch, at)
                        })
                        .collect()
                };
                for (cid, exch, at_typed) in provider_seeds_win {
                    if let Some(window) = app.panel_app.panel_grid.containers_mut().get_mut(&cid) {
                        let win_exchange_id = digdigdig3::ExchangeId::from_str(&exch)
                            .unwrap_or(digdigdig3::ExchangeId::Binance);
                        let win_at = mlc_backend_data_bridge::account_type_to_dig3(at_typed);
                        let win_provider: SharedDataProvider = std::sync::Arc::new(
                            LiveDataProvider::new(win_exchange_id, &exch, win_at, bridge.clone()),
                        );
                        window.data_provider = win_provider;
                        window.toolbar_config = ToolbarConfig::standalone();
                    }
                }

                // S2.7.1: read exchange via branch_meta instead of struct field.
                if let Some(cid) = app.panel_app.panel_grid.active_container_id() {
                    let exch = app.exchange_for_chart(cid)
                        .unwrap_or_default();
                    app.active_exchange = digdigdig3::ExchangeId::from_str(&exch)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                }

                let _t_nw_reqbars = web_time::Instant::now();
                let mut nw_reqbars_n: usize = 0;
                for (sym, exch, tf, at_typed, ct) in &window_data {
                    let eid = digdigdig3::ExchangeId::from_str(&exch)
                        .unwrap_or(digdigdig3::ExchangeId::Binance);
                    if !app.sidebar_state.connector_enabled.get(eid.as_str()).copied().unwrap_or(true) {
                        continue;
                    }
                    if matches!(*ct,
                        "footprint" | "range_bar" | "volume_bar" | "tick_bar"
                        | "dollar_bar" | "tick_imbalance_bar" | "volume_imbalance_bar"
                        | "run_bar" | "renko" | "pnf" | "kagi" | "three_line_break"
                        | "tpo"
                    ) {
                        continue;
                    }
                    let at = mlc_backend_data_bridge::account_type_to_dig3(*at_typed);
                    bridge.ensure_connector(eid);
                    bridge.request_bars(eid, sym, tf, at, None, Some(app.core.user().profile().domain.bar_count as usize), false);
                    nw_reqbars_n += 1;
                }
                // Derived windows: ignite the matching pipeline + ownership
                // stamp (see the startup post-load loop for rationale).
                app.ignite_derived_windows();
                app.ignite_legacy_heatmap_overlays();
                app.sync_default_indicators_for_all_windows();
                let nw_reqbars_us = _t_nw_reqbars.elapsed().as_micros();

                let nw_cold_us = _t_nw_cold.elapsed().as_micros();
                log::debug!(
                    "[ChartApp] new_window cold path: load={}us reqbars[{}wins]={}us total={}us",
                    nw_load_us, nw_reqbars_n, nw_reqbars_us, nw_cold_us,
                );

                app.needs_initial_viewport_fit = true;
                // NOTE: background-tab warm-up is intentionally NOT done here.
                // When this runs inside the off-thread promote worker, firing
                // ~11 series requests floods the broadcast channel with
                // BarsLoaded before any live window exists to receive them;
                // the first live tick then drains that backlog through the
                // full (no-match) matching path — the bulk of the ~228ms
                // first-frame freeze. The warm-up is driven AFTER the first
                // live frame instead (app-vello: pending_warm_targets, one
                // series per frame), so the cache still heats without blocking
                // the promote-to-chart transition.
            } else {
                // Fresh state — seeded from the preset this window couldn't
                // claim (new_window_seed, computed above) so a spawned
                // window opens on the same instrument as its source window;
                // Binance/BTCUSDT/1h/Spot when there is no such preset
                // (genuinely fresh profile).
                let _t_nw_fresh = web_time::Instant::now();
                let (seed_symbol, seed_exchange_name, seed_account_type_label, seed_timeframe) =
                    new_window_seed.clone().unwrap_or_else(|| {
                        ("BTCUSDT".to_string(), exchange_name.clone(), "S".to_string(), mlc_core::state::Timeframe::h1())
                    });
                let seed_exchange_id = digdigdig3::ExchangeId::from_str(&seed_exchange_name)
                    .unwrap_or(digdigdig3::ExchangeId::Binance);
                let seed_account_type = mlc_backend_data_bridge::account_type_from_short_label(&seed_account_type_label);
                // Own provider bound to the seeded exchange/account — the
                // shared top-of-function `data_provider` is always Binance
                // Spot, and DataProvider::get_bars() re-fetches backfill
                // through the provider's OWN exchange/account (provider.rs),
                // so reusing it here would silently backfill history from
                // the wrong market once the seed differs from Binance Spot.
                let seed_data_provider: SharedDataProvider = std::sync::Arc::new(
                    LiveDataProvider::new(seed_exchange_id, seed_exchange_name.clone(), seed_account_type, bridge.clone()),
                );
                // Phase 4 B: capture chart_id BEFORE the mutating block.
                let active_chart_id = app.panel_app.panel_grid.active_container_id();
                if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                    window.data_provider = seed_data_provider;
                    window.toolbar_config = ToolbarConfig::standalone();
                    // Arm the same pair `apply_preset_cold`/`seed_default_chart`
                    // arm on every other hydration path (viewport invariant,
                    // CLAUDE.md "every hydration path lands on ...
                    // needs_snap_after_backfill = true"). Without this the
                    // window's first-ever `BarsLoaded` is misclassified as a
                    // BACKFILL (`is_backfill = !pending_symbol_load` reads
                    // `!false`) instead of a fresh load in
                    // `react_to_bubble_event` — `capture_view_anchor` on the
                    // still-empty prior bars returns `None`, so
                    // `reconcile_view_keep_anchor` leaves `view_start` at its
                    // construction default (0) instead of snapping to the
                    // right edge. A later backfill page (older bars
                    // prepended) then anchors relative to that stale
                    // `view_start`, landing the viewport mid-history instead
                    // of at the latest bar (owner repro 2026-09-24:
                    // `view_start` 725/2001 or 1700/2001 on a fresh profile,
                    // the "лендинг в середину истории" bug). The legacy
                    // `needs_initial_viewport_fit` one-shot below is a weaker
                    // safety net that only ever fires once and cannot
                    // recover from a later prepend — it does not substitute
                    // for these two flags.
                    window.pending_symbol_load = true;
                    window.needs_snap_after_backfill = true;
                }
                // Create auto-bubble for the initial chart and assign instrument.
                if let Some(chart_id) = active_chart_id {
                    let auto_bid = app.create_auto_bubble(
                        [0.0, 0.0, 0.0, 0.0],
                        seed_symbol.clone(),
                        seed_timeframe.clone(),
                    );
                    app.attach_chart_to_tag(chart_id, auto_bid);
                    if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
                        window.auto_tag_id = auto_bid;
                        window.current_tag_id = auto_bid;
                    }
                    app.core.bubbles_mut().set_bid_instrument_full(
                        auto_bid,
                        seed_symbol.clone(),
                        seed_exchange_name.clone(),
                        seed_account_type_label.clone(),
                        seed_timeframe.clone(),
                    );
                }
                app.panel_app.toolbar_config = ToolbarConfig::standalone();

                // Create "Untitled 1" preset for genuinely fresh windows
                // (no presets at all — brand new profile).  Uses same numbering
                // logic as OpenPresetNewChart (input.rs) for consistency.
                {
                    let max_n_opt: Option<u32> = {
                        let pg = app.core.presets();
                        if pg.is_empty() {
                            Some(pg.values()
                                .filter_map(|p| p.name.strip_prefix("Untitled "))
                                .filter_map(|s| s.parse::<u32>().ok())
                                .max()
                                .unwrap_or(0))
                        } else {
                            None
                        }
                    };
                    if let Some(max_n) = max_n_opt {
                        let untitled_preset = mlc_core::preset::preset::ChartPreset::new(format!("Untitled {}", max_n + 1));
                        let untitled_id = untitled_preset.id.clone();
                        app.core.presets_mut().insert(untitled_id.clone(), untitled_preset.clone());
                        // Emit the same PresetAction the ordinary edit path uses so the
                        // shell's per-frame drain (main.rs about_to_wait) picks this up,
                        // marks it dirty, and flushes it to disk on the next frame —
                        // without this, a crash between window creation and the next
                        // unrelated profile_dirty/save_all sweep silently lost this
                        // preset (it existed only in the shared in-memory core).
                        app.preset_actions.push(crate::PresetAction::Upsert(untitled_preset));
                        app.panel_app.open_tabs = vec![untitled_id.clone()];
                        app.panel_app.active_preset_id = untitled_id;
                    }
                }

                // Add the real mli-catalog Volume histogram overlay to the fresh
                // window. This is the default-on state for brand-new charts that
                // have no saved preset. "builtin_volume" retired 2026-07-05 —
                // "VOLUME" (BarIndicatorId::Volume) computes/renders identically
                // through the normal indicator pipeline.
                if let Some(chart_id) = active_chart_id {
                    let bid = app.panel_app.panel_grid.containers()
                        .get(&chart_id)
                        .map(|w| w.current_tag_id);
                    // definitions map is keyed by the LOWERCASED catalog id —
                    // create_instance's lookup is case-sensitive.
                    app.core.indicators_mut().add(bid, "volume");
                }

                // Persist the REAL initial state (auto-bubble + volume
                // overlay) into the active preset. `ChartPreset::new` above
                // is a blank shell — a blank on disk restores as a
                // bubble-less container (empty-symbol zombie) on the next
                // boot. This also covers the adopted-blank case (existing
                // empty preset routed here by the poison guard above).
                {
                    let pid = app.panel_app.active_preset_id.clone();
                    if !pid.is_empty() {
                        // bind-then-use: presets() returns a guard — holding
                        // it across the &mut self call below is E0502.
                        let pname = app.core.presets().get(&pid).map(|p| p.name.clone());
                        if let Some(pname) = pname {
                            app.collect_state_into_preset(&pid, &pname);
                            let collected = app.core.presets().get(&pid).cloned();
                            if let Some(p) = collected {
                                app.preset_actions.push(crate::PresetAction::Upsert(p));
                            }
                        }
                    }
                }
                // Fresh charts need the same one-shot snap-to-end the cold
                // path arms — without it the viewport stays parked on the
                // oldest loaded bars after the initial backfill lands.
                app.needs_initial_viewport_fit = true;

                bridge.ensure_connector(seed_exchange_id);
                bridge.request_bars(
                    seed_exchange_id,
                    &seed_symbol,
                    &seed_timeframe,
                    seed_account_type,
                    None,
                    Some(app.core.user().profile().domain.bar_count as usize),
                    false,
                );
                log::debug!(
                    "[ChartApp] new_window fresh-state path total={}us",
                    _t_nw_fresh.elapsed().as_micros(),
                );
            }
        }

        // Connectors warm up once from tick() via warmup_connectors() (bridge-
        // guarded, off the UI thread, during the skeleton screen), not here.
        // Exchanges needed immediately (window + watchlist) connect eagerly above.

        if !skeleton {
            bridge.request_symbols(exchange_id);
        }

        // Subscribe mini tickers for active watchlist.
        if !skeleton {
            let wl_syms: Vec<(String, String, mlc_core::instrument::AccountType)> = app.core.watchlists().find(app.watchlist_ui.active_list_id)
                .map(|wl| wl.all_symbols().into_iter().map(|ws| (ws.exchange.clone(), ws.symbol.clone(), ws.account_type)).collect())
                .unwrap_or_default();
            for (ws_exchange_str, ws_symbol, ws_account_type) in wl_syms {
                let ws_exchange = digdigdig3::ExchangeId::from_str(&ws_exchange_str)
                    .unwrap_or(exchange_id);
                if !app.sidebar_state.connector_enabled.get(ws_exchange.as_str()).copied().unwrap_or(true) {
                    continue;
                }
                let ws_at = mlc_backend_data_bridge::account_type_to_dig3(ws_account_type);
                bridge.ensure_connector(ws_exchange);
                bridge.subscribe_mini_ticker(ws_exchange, &ws_symbol, ws_at);
            }
        }

        // Every text field this app owns — see `register_all_text_fields`.
        register_all_text_fields(&mut app.input_coordinator.borrow_mut());

        app.sync_sub_panes_from_manager();
        app
    }

    /// Apply profile-level state to this window instance.
    ///
    /// Sets sidebar width/panel, inline bar position/dock, and connector enabled state.
    /// When `window_state` is provided its per-window sidebar fields take precedence over
    /// the flat fields on `profile` (backwards-compatible: old profiles without per-window
    /// sidebar data fall back to the profile-level flat fields automatically).
    pub fn apply_profile_state(
        &mut self,
        profile: &mlc_core::UserProfile,
        window_state: Option<&mlc_core::WindowState>,
    ) {
        // Sidebar width — prefer per-window, fall back to profile flat field.
        let sidebar_width = window_state
            .and_then(|ws| ws.sidebar_width)
            .or(profile.ui.sidebar_width);
        if let Some(width) = sidebar_width {
            self.sidebar_state.set_right_width(width);
        }

        // Sidebar panel/visibility — prefer per-window, fall back to profile flat fields.
        let sidebar_visible = window_state.map(|ws| ws.sidebar_visible).unwrap_or(profile.ui.sidebar_visible);
        let sidebar_panel = window_state
            .and_then(|ws| ws.sidebar_panel.as_ref())
            .or(profile.ui.sidebar_panel.as_ref());
        if sidebar_visible {
            if let Some(panel_name) = sidebar_panel {
                let panel = Self::str_to_panel(panel_name);
                self.sidebar_state.set_right_panel(panel);
            }
        }

        // Connector enabled/disabled state — always from profile (app-level, not per-window).
        if !profile.domain.connector_enabled.is_empty() {
            self.sidebar_state.connector_enabled = profile.domain.connector_enabled.clone();
        }

        // Archive CORS-proxy base (unified-data-plane Э6-Ф4) — always from
        // profile (process-level config, not per-window). Only meaningful on
        // wasm (native archive fetches ignore it), but cheap enough to set
        // unconditionally rather than special-case native here.
        mlc_backend_data_bridge::set_archive_proxy_base(profile.domain.data_load.archive_proxy_base.clone());

        // Raw-trade retention window (owner rule 2026-07-15) — process-level
        // like the proxy base: the shared TradeStore's eviction bound AND
        // the backfill planner's no-zip fetch floor both read this global.
        mlc_backend_data_bridge::set_trade_raw_window_days(profile.domain.data_load.trade_raw_window_days);

        // Control-point (handle) shape/size (control-point-style-ui arc,
        // 2026-08-08) — process-level like the two settings above:
        // `set_control_point_style` writes the one global `draw_control_points`
        // and the default `control_point_hit_test` read fresh every frame, so
        // every drawing (including ones loaded from disk long before this
        // window existed) adopts the persisted style on the very next frame,
        // no primitive touched. Mirrored into `user_settings_state` here too
        // so the General tab's pickers show the right selection immediately
        // rather than waiting for `prepare()`'s per-frame sync to catch up.
        mlc_core::drawing::primitives_v2::set_control_point_style(
            mlc_core::drawing::primitives_v2::ControlPointStyle {
                shape: profile.ui.control_point_shape,
                size: profile.ui.control_point_size,
            },
        );
        self.panel_app.user_settings_state.control_point_shape = profile.ui.control_point_shape;
        self.panel_app.user_settings_state.control_point_size = profile.ui.control_point_size;

        // Toast anchor/quiet-mode (toast restyle-and-anchor follow-up to
        // §8, owner ask 2026-09-09) — mirrored into `user_settings_state`
        // here too so the General tab's Notifications section shows the
        // right state immediately rather than waiting for `prepare()`'s
        // per-frame sync to catch up (same reasoning as `control_point_shape`
        // above).
        self.panel_app.user_settings_state.toasts_enabled = profile.ui.toasts_enabled;
        self.panel_app.user_settings_state.toast_anchor = profile.ui.toast_anchor;

        // Header account-button display mode (owner ruling 2026-09-10) —
        // same mirror-at-load reasoning as `toast_anchor` above, so the
        // Account tab's "Show in header:" row shows the right selection
        // immediately.
        self.panel_app.user_settings_state.header_account_display = profile.ui.header_account_display;

        if let Some(style) = mlc_core::UIStyle::from_name(&profile.ui.active_ui_style) {
            self.panel_app.theme_manager.current_mut().set_style(style);
        }

        // EWA engine configuration (owner ask 2026-08-14) — process-level
        // like the control-point style above: `set_ewa_engine_config` writes
        // the one global `wave_label_offset`/`elliott::guard`/each Elliott
        // constructor read fresh, so every open chart (and every NEW
        // structure drawn from here on) adopts the persisted config on the
        // very next use, no primitive touched. Mirrored into
        // `ewa_engine_settings_state` here too so the modal shows the right
        // selection immediately rather than waiting for a click.
        mlc_core::state::ewa_engine::set_ewa_engine_config(profile.ui.ewa_engine.clone());
        self.panel_app.ewa_engine_settings_state.config = profile.ui.ewa_engine.clone();

        // Candle-pattern engine configuration (markup-engines wave,
        // 2026-08-25) — same process-level, read-fresh contract as the EWA
        // engine config above. No settings-modal UI exists for this config
        // yet, so there is no mirrored modal state to update here — see
        // `mlc_core::state::candle_pattern_engine`'s own module doc.
        mlc_core::state::candle_pattern_engine::set_candle_pattern_engine_config(
            profile.ui.candle_pattern_engine.clone(),
        );

        // Smart-money engine configuration (markup-engines wave,
        // 2026-08-25) — same process-level, read-fresh contract as the
        // candle-pattern engine config above. No settings-modal UI exists
        // for this config yet — see `mlc_core::state::smc_engine`'s own
        // module doc.
        mlc_core::state::smc_engine::set_smc_engine_config(profile.ui.smc_engine.clone());

        // Order-flow engine configuration (markup-engines wave, 2026-08-25)
        // — same process-level, read-fresh contract as the smart-money
        // engine config above. No settings-modal UI exists for this config
        // yet — see `mlc_core::state::orderflow_engine`'s own module doc.
        mlc_core::state::orderflow_engine::set_orderflow_engine_config(profile.ui.orderflow_engine.clone());

        // Inline bar position/dock — prefer per-window, fall back to profile flat fields.
        let inline_bar_x = window_state
            .and_then(|ws| ws.inline_bar_x)
            .or(profile.ui.inline_bar_x);
        let inline_bar_y = window_state
            .and_then(|ws| ws.inline_bar_y)
            .or(profile.ui.inline_bar_y);
        let inline_bar_dock = window_state
            .and_then(|ws| ws.inline_bar_dock.as_ref())
            .or(profile.ui.inline_bar_dock.as_ref());
        if let Some(x) = inline_bar_x {
            self.panel_app.toolbar_state.floating_inline_bar.x = x;
        }
        if let Some(y) = inline_bar_y {
            self.panel_app.toolbar_state.floating_inline_bar.y = y;
        }
        if let Some(dock) = inline_bar_dock {
            self.panel_app.toolbar_state.floating_inline_bar.dock_edge = match dock.as_str() {
                "Top" => mlc_core::InlineDockEdge::Top,
                "Free" => mlc_core::InlineDockEdge::Free,
                _ => mlc_core::InlineDockEdge::Bottom,
            };
        }

        // Favorites quick-pick bar position/dock/visibility (favorite-
        // primitives arc) — flat profile fields only (unlike the inline bar
        // there is no per-window WindowState mirror for this bar).
        if let Some(x) = profile.ui.favorites_bar_x {
            self.panel_app.toolbar_state.favorites_bar.x = x;
        }
        if let Some(y) = profile.ui.favorites_bar_y {
            self.panel_app.toolbar_state.favorites_bar.y = y;
        }
        if let Some(dock) = profile.ui.favorites_bar_dock.as_ref() {
            self.panel_app.toolbar_state.favorites_bar.dock_edge = match dock.as_str() {
                "Top" => mlc_core::InlineDockEdge::Top,
                "Free" => mlc_core::InlineDockEdge::Free,
                _ => mlc_core::InlineDockEdge::Bottom,
            };
        }
        self.panel_app.toolbar_state.favorites_bar_visible = profile.ui.favorites_bar_visible;
        self.panel_app.toolbar_state.screenshot_destination = profile.ui.screenshot_destination;

        // System event log (system-event-bus arc, 2026-09-05, owner scope
        // §8) — flat profile field, no per-window mirror (same class as the
        // favorites bar above): restore the persisted ring so events survive
        // a restart; new pushes append onto this live copy from here on.
        self.system_events = profile.ui.system_events.clone();

        if profile.ui.persist_toolbar_last_tools {
            for (dropdown_id, tool_id) in &profile.ui.toolbar_last_tools {
                self.panel_app.toolbar_state.set_quick_select(dropdown_id, tool_id, tool_id);
            }
        }
        if profile.ui.persist_toolbar_toggles {
            self.panel_app.toolbar_state.drawings_locked = profile.ui.drawings_locked;
            self.panel_app.toolbar_state.drawings_visible = profile.ui.drawings_visible;
        }

        // Notification settings — load from profile into alert settings
        // state. Preferences (this struct) are plaintext and always
        // available. Secrets (bot token, subscribers, webhook URL) only
        // exist in `profile.domain.notification_secrets` once the vault has
        // actually been decrypted this session — `vault_key()` is that same
        // truth `save_profile`/`load_profile` already gate on, so reading it
        // here needs no new bookkeeping.
        let secrets_available = self.core.user().vault_key().is_some();
        dlog!(
            "[ChartApp] apply_profile_state: tg_enabled={} secrets_available={} token_len={} subscribers={}",
            profile.domain.notification_settings.telegram.enabled,
            secrets_available,
            profile.domain.notification_secrets.telegram.bot_token.len(),
            profile.domain.notification_secrets.telegram.subscribers.len(),
        );
        self.panel_app.alert_settings_state.notification_settings = profile.domain.notification_settings.clone();
        self.panel_app.alert_settings_state.secrets_available = secrets_available;
        if secrets_available {
            self.panel_app.alert_settings_state.notification_secrets = profile.domain.notification_secrets.clone();
            let tg_wid = uzor::WidgetId::from(crate::text_input::ALERT_TG_TOKEN);
            self.input_coordinator.borrow_mut().text_fields_mut()
                .set_text(&tg_wid, &profile.domain.notification_secrets.telegram.bot_token);
        } else {
            // Vault locked — no secrets to show. Leave the render layer to
            // display "unavailable" (see `secrets_available`) rather than an
            // empty, invitingly-editable token field.
            self.panel_app.alert_settings_state.notification_secrets =
                mlc_alert_delivery::NotificationSecrets::default();
        }

        // Agents docking container restore — per-window profile state.
        // Layout topology (tree shape + splits) is rebuilt from LayoutSnapshot.
        // Per-leaf descriptors are inserted into `agent_leaves`; no sessions
        // are spawned — PTY leaves show "Click Start" skeleton, Chat leaves
        // lazily resume via `--resume <chat_session_id>` on first interaction.
        if let Some(ws) = window_state {
            log::info!(
                "[agents-diag] apply_profile_state: layout_json={} persisted_leaves={}",
                if ws.agents_tab_layout.is_some() { "Some" } else { "None" },
                ws.agents_tab_leaves.len(),
            );
            if let Some(layout_json) = &ws.agents_tab_layout {
                match uzor::panels::serialize::LayoutSnapshot::from_json(layout_json) {
                    Ok(snap) => {
                        // Build a lookup of persisted leaves by their numeric id
                        // so restore_tree can reconstruct the right AgentPaneLeaf
                        // payload for each leaf in the snapshot.
                        let by_id: std::collections::HashMap<u64, &mlc_core::PersistedAgentLeaf> =
                            ws.agents_tab_leaves.iter().map(|p| (p.leaf_id, p)).collect();

                        // Dummy descriptor used if the snapshot references a leaf
                        // id that has no matching PersistedAgentLeaf (defensive
                        // fallback — should never happen for well-formed profiles).
                        let fallback_cli = gate4agent::AgentCli::Claude;
                        let fallback_mode = gate4agent::InstanceMode::Pty;

                        // Rebuild the docking tree. `restore_tree` invokes the
                        // closure once per leaf — we return a zero-instance
                        // `AgentPaneLeaf` keyed by cli/mode so rendering works;
                        // the real InstanceId is stored in `agent_leaves`.
                        let restore_result = snap.restore_tree_with_id(|leaf_id, _type_id| {
                            let (cli, mode) = if let Some(persisted) = by_id.get(&leaf_id) {
                                let cli = match persisted.cli {
                                    mlc_core::PersistedAgentCli::Claude => gate4agent::AgentCli::Claude,
                                    mlc_core::PersistedAgentCli::Codex => gate4agent::AgentCli::Codex,
                                    mlc_core::PersistedAgentCli::Gemini => gate4agent::AgentCli::Gemini,
                                    mlc_core::PersistedAgentCli::OpenCode => gate4agent::AgentCli::OpenCode,
                                };
                                let mode = match persisted.mode {
                                    mlc_core::PersistedInstanceMode::Pty => gate4agent::InstanceMode::Pty,
                                    mlc_core::PersistedInstanceMode::Chat => gate4agent::InstanceMode::Chat,
                                };
                                (cli, mode)
                            } else {
                                (fallback_cli, fallback_mode)
                            };
                            Some(mlc_app_sidebar_content::AgentPaneLeaf {
                                instance_id: gate4agent::InstanceId::new(),
                                cli,
                                mode,
                            })
                        });

                        match restore_result {
                            Ok(tree) => {
                                // Replace the docking manager with the restored tree.
                                *self.sidebar_state.agent_docking.inner_mut() =
                                    uzor::layout::DockState::from_tree(tree);

                                // Rebuild the agent_leaves map from persisted descriptors.
                                // Each leaf gets a registered instance via create_instance so
                                // the Start button can find the ID in MultiCliManager's map.
                                self.sidebar_state.agent_leaves.clear();
                                for persisted in &ws.agents_tab_leaves {
                                    let cli = match persisted.cli {
                                        mlc_core::PersistedAgentCli::Claude => gate4agent::AgentCli::Claude,
                                        mlc_core::PersistedAgentCli::Codex => gate4agent::AgentCli::Codex,
                                        mlc_core::PersistedAgentCli::Gemini => gate4agent::AgentCli::Gemini,
                                        mlc_core::PersistedAgentCli::OpenCode => gate4agent::AgentCli::OpenCode,
                                    };
                                    let mode = match persisted.mode {
                                        mlc_core::PersistedInstanceMode::Pty => gate4agent::InstanceMode::Pty,
                                        mlc_core::PersistedInstanceMode::Chat => gate4agent::InstanceMode::Chat,
                                    };
                                    let leaf_id = uzor::panels::LeafId(persisted.leaf_id);
                                    let workdir = persisted.workdir.clone();
                                    let _ = std::fs::create_dir_all(&workdir);
                                    match self.agent.create_instance(cli, mode, workdir.clone()) {
                                        Ok(instance_id) => {
                                            let desc = mlc_app_sidebar_content::agents_dock::AgentLeafDescriptor {
                                                instance_id,
                                                cli,
                                                mode,
                                                workdir,
                                                chat_session_id: persisted.chat_session_id.clone(),
                                            };
                                            self.sidebar_state.agent_leaves.insert(leaf_id, desc);
                                        }
                                        Err(e) => {
                                            log::warn!("[ChartApp] agents restore create_instance failed: {}", e);
                                        }
                                    }
                                }
                                dlog!(
                                    "[ChartApp] agents docking restored: {} leaves",
                                    self.sidebar_state.agent_leaves.len()
                                );
                                log::info!(
                                    "[agents-diag] restore OK: {} leaves in agent_leaves",
                                    self.sidebar_state.agent_leaves.len()
                                );
                            }
                            Err(e) => {
                                log::warn!("[ChartApp] agents restore_tree failed: {}", e);
                                log::warn!("[agents-diag] restore_tree FAILED: {}", e);
                            }
                        }
                    }
                    Err(e) => {
                        log::warn!("[ChartApp] agents layout deserialize failed: {}", e);
                        log::warn!("[agents-diag] layout deserialize FAILED: {}", e);
                    }
                }
            }
        }
    }

    // -------------------------------------------------------------------------
    // Bridge access
    // -------------------------------------------------------------------------

    /// Return a clone of the `Arc<DataBridge>` so it can be shared with other
    /// windows created via [`new_empty`].
    pub fn bridge(&self) -> std::sync::Arc<DataBridge> {
        self.core.bridge_arc()
    }

    // -------------------------------------------------------------------------
    // Screenshot
    // -------------------------------------------------------------------------

    /// Request a screenshot to be taken on the next rendered frame.
    ///
    /// The vello renderer drains this flag via `drain_pending_screenshot()` and
    /// performs GPU readback to capture the frame as a PNG file.
    pub fn request_screenshot(&mut self) {
        self.pending_screenshot = true;
        dlog!("[Screenshot] Capture requested");
    }

    /// True when the current render pass is producing the frame a shell is
    /// about to capture as a screenshot.
    ///
    /// Overlay painters gate on this — the captured PNG is the chart only:
    /// no tooltip, drawing-tool cursor preview, context menu, open dropdown
    /// or modal may appear in it (owner ruling, screenshot-is-chart-only
    /// arc 2026-09-05). See [`ChartApp::render_overlay`] and
    /// [`ChartApp::render_modal_layer`], plus the vello shell's toolbar/
    /// chrome tooltip render calls (`mlc-app-shell-vello/src/render/
    /// scene_builder.rs`), which gate on this same predicate.
    ///
    /// True for `pending_screenshot` (still set, not yet drained by the web
    /// shell's post-render drain) OR `capturing_screenshot` (explicitly
    /// armed by the vello shell around the specific build pass whose output
    /// becomes the captured frame — see `capturing_screenshot`'s doc for why
    /// vello needs the explicit form).
    pub fn is_capturing_screenshot(&self) -> bool {
        self.pending_screenshot || self.capturing_screenshot
    }

    /// Explicitly mark whether the render pass about to run is capturing a
    /// screenshot. Native vello only — see `capturing_screenshot`'s doc.
    /// The web shell needs no equivalent call: its render call already runs
    /// before `pending_screenshot` is drained, so [`is_capturing_screenshot`]
    /// reads correctly from `pending_screenshot` alone.
    ///
    /// [`is_capturing_screenshot`]: Self::is_capturing_screenshot
    pub fn set_capturing_screenshot(&mut self, capturing: bool) {
        self.capturing_screenshot = capturing;
    }

    /// Request a public live-share link for the active preset (chart-sharing
    /// arc, toolbar "share_chart" button).
    ///
    /// The shell drains this flag via `drain_pending_share_live()` and POSTs
    /// the serialized preset to `/api/share/live` — the cloud session and
    /// its token live in the shell, not here.
    pub fn request_share_live(&mut self) {
        self.pending_share_live = true;
        dlog!("[Share] Live-share link requested");
    }

    // -------------------------------------------------------------------------
    // Live exchange support
    // -------------------------------------------------------------------------

    /// Switch the active chart window to use live data from a real exchange.
    ///
    /// Starts the connector asynchronously and immediately requests bars for
    /// the current symbol / timeframe. The `tick()` method polls the bridge
    /// channel each frame and feeds bars into the window once they arrive.
    pub fn switch_to_exchange(&mut self, exchange_id: digdigdig3::ExchangeId) {
        if !self.sidebar_state.connector_enabled.get(exchange_id.as_str()).copied().unwrap_or(true) {
            dlog!("[ChartApp] Exchange {} is disabled, skipping", exchange_id.as_str());
            return;
        }
        // Capture old trade-stream identity BEFORE updating active_exchange or window state.
        let old_trade_exchange = self.active_exchange;
        // S2.7.1: read symbol/account_type via branch_meta instead of struct field.
        let old_chart_id = self.panel_app.panel_grid.active_container_id();
        let old_trade_symbol = old_chart_id
            .and_then(|cid| self.symbol_for_chart(cid))
            .map(String::from)
            .unwrap_or_default();
        let old_trade_at = old_chart_id
            .and_then(|cid| self.account_type_for_chart(cid))
            .map(mlc_backend_data_bridge::account_type_to_dig3)
            .unwrap_or(digdigdig3::AccountType::Spot);

        let bridge = self.core.bridge_arc();
        self.active_exchange = exchange_id;

        // Start the connector (no-op if already running).
        self.core.ensure_connector(exchange_id);

        // Human-readable exchange name used by the window header.
        let exchange_name = exchange_id.as_str().to_string();

        // Use the active window's account_type for the new provider.
        // S2.7.1: read account_type via branch_meta.
        let switch_at = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.account_type_for_chart(cid))
            .map(mlc_backend_data_bridge::account_type_to_dig3)
            .unwrap_or(digdigdig3::AccountType::Spot);

        // Build a LiveDataProvider backed by this bridge.
        let provider: SharedDataProvider =
            std::sync::Arc::new(LiveDataProvider::new(
                exchange_id,
                exchange_name.clone(),
                switch_at,
                bridge.clone(),
            ));

        // Unsubscribe only the old symbol's trade stream on the old exchange,
        // leaving other windows' streams intact.
        if !old_trade_symbol.is_empty() {
            self.core.unsubscribe_trades(old_trade_exchange, &old_trade_symbol, old_trade_at);
        }

        // Phase 4 S2.2: capture chart_id BEFORE the mutating block.
        let active_chart_id = self.panel_app.panel_grid.active_container_id();
        // Attach the provider to the active window and request paginated bars.
        // Phase 4 B: read symbol/timeframe/account_type from branch_meta.
        let active_cid_exc = self.panel_app.panel_grid.active_container_id();
        let cur_sym_exc = active_cid_exc
            .and_then(|cid| self.symbol_for_chart(cid))
            .map(String::from)
            .unwrap_or_default();
        let cur_tf_exc = active_cid_exc
            .and_then(|cid| self.timeframe_for_chart(cid))
            .unwrap_or_else(mlc_core::state::Timeframe::h1);
        let cur_at_label_exc = active_cid_exc
            .and_then(|cid| self.account_type_for_chart(cid))
            .map(|at| at.to_string())
            .unwrap_or_else(|| "S".to_string());
        let mut mirror_args: Option<(String, mlc_core::state::Timeframe, String, String)> = None;
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.data_provider = provider;
            let at = account_type_from_label(&cur_at_label_exc);
            mirror_args = Some((cur_sym_exc.clone(), cur_tf_exc.clone(), exchange_name.clone(), cur_at_label_exc.clone()));
            // force=true — explicit user exchange switch (see the
            // ApplySymbol handler's dedup-L2 comment).
            self.core.request_kline(exchange_id, &cur_sym_exc, &cur_tf_exc, at, None, Some(self.core.user().profile().domain.bar_count as usize), true);
        }
        if let (Some(chart_id), Some((_, _, exch, _))) = (active_chart_id, mirror_args) {
            if let Some(bid) = self.bubble_for_chart(chart_id) {
                self.core.bubbles_mut().set_exchange_for_bid(bid, exch);
            }
        }
    }

    // -------------------------------------------------------------------------
    // Preset / tab management — public wrappers for the chrome tab system
    // -------------------------------------------------------------------------

    /// Load a preset by its id.
    ///
    /// Fires the `LoadPreset` chart-out event, which restores the full layout,
    /// windows, indicators, and settings saved under that preset id.
    pub fn load_preset(&mut self, id: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::LoadPreset {
            id: id.to_string(),
        });
    }

    /// Close a tab without deleting the preset from disk.
    ///
    /// Fires `CloseTab` which removes the ID from `open_tabs` and switches
    /// to an adjacent tab if the closed tab was active.
    pub fn close_tab(&mut self, id: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::CloseTab {
            id: id.to_string(),
        });
    }

    /// Move an open tab to a new position in the strip (chrome drag&drop).
    /// `to_index` is the final index after removal.
    pub fn move_tab(&mut self, id: &str, to_index: usize) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::MoveTab {
            id: id.to_string(),
            to_index,
        });
    }

    /// Open the "new chart" flow (shows the preset-name input modal).
    pub fn new_chart(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::NewChart);
    }

    /// Open an existing (closed) preset as a new tab.
    pub fn open_tab(&mut self, id: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenTab {
            id: id.to_string(),
        });
    }

    /// Open the chart browser modal (list of all saved presets).
    pub fn open_chart_browser(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenChartBrowser);
    }

    /// Open the rename modal for the currently active preset/tab.
    ///
    /// Fires `OpenPresetRename`, which shows the preset-name input modal in
    /// rename mode for the active preset. Callers targeting a non-active tab
    /// must `load_preset` it first.
    pub fn open_preset_rename(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenPresetRename);
    }

    /// Rename a preset by its id (no modal, no activation needed).
    ///
    /// Fires `RenamePreset`, which targets the preset id directly — the
    /// preset does NOT have to be the active one. Used by the chrome tab
    /// strip's inline rename (double-click a tab).
    pub fn rename_preset(&mut self, preset_id: &str, new_name: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::RenamePreset {
            id: preset_id.to_string(),
            new_name: new_name.to_string(),
        });
    }

    /// Open the chart settings modal.
    pub fn open_settings(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenChartSettings);
    }

    /// Open (or toggle) the user settings modal.
    ///
    /// Called when the chrome gear button is clicked.
    pub fn open_user_settings(&mut self) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::OpenUserSettings);
    }

    /// Programmatic symbol change for a specific chart, bypassing the
    /// symbol-search modal. Same effect as user-driven modal confirmation.
    pub fn apply_symbol(&mut self, chart_id: mlc_core::state::ChartId, symbol: &str, exchange: &str, account_type: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::ApplySymbol {
            chart_id: chart_id.0,
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            account_type: account_type.to_string(),
        });
    }

    /// Programmatic compare-overlay add on the active chart container,
    /// bypassing the compare search modal. Same effect as a user-driven
    /// compare modal item click. Routed through the canonical
    /// `ChartOutEvent::AddCompareSymbol` path.
    pub fn add_compare_symbol(&mut self, symbol: &str, exchange: &str, account_type: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::AddCompareSymbol {
            symbol: symbol.to_string(),
            exchange: exchange.to_string(),
            account_type: account_type.to_string(),
        });
    }

    /// Switch chart type on the active chart (e.g. `"candles"`, `"renko"`,
    /// `"footprint"`, ...). Routed through the same `ChartOutEvent::ChangeChartType`
    /// path the toolbar dropdown uses.
    pub fn change_chart_type(&mut self, chart_type: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::ChangeChartType {
            chart_type: chart_type.to_string(),
        });
    }

    /// Change timeframe on the active chart (e.g. `"1m"`, `"1h"`, `"1D"`).
    /// Routed through `ChartOutEvent::ChangeTimeframe`.
    pub fn change_timeframe(&mut self, timeframe_id: &str) {
        self.process_chart_out_event(mlc_core::events::ChartOutEvent::ChangeTimeframe {
            timeframe_id: timeframe_id.to_string(),
        });
    }

    /// Apply an internal layout split / restore to the active window.
    ///
    /// `kind` accepts the same keys the layout dropdown emits:
    /// `"single"`, `"h"`, `"v"`, `"2x2"`, `"2l1r"`, `"1l2r"`,
    /// `"2t1b"`, `"1t2b"`, `"3cols"`, `"3rows"`, `"1big3small"`.
    pub fn set_internal_layout(&mut self, kind: &str) -> bool {
        use mlc_core::events::ChartOutEvent as E;
        let event = match kind {
            "single"    => E::InternalSetLayoutSingle,
            "h"         => E::InternalSplitHorizontal,
            "v"         => E::InternalSplitVertical,
            "2x2"       => E::InternalSplitGrid2x2,
            "2l1r"      => E::InternalSplit2Left1Right,
            "1l2r"      => E::InternalSplit1Left2Right,
            "2t1b"      => E::InternalSplit2Top1Bottom,
            "1t2b"      => E::InternalSplit1Top2Bottom,
            "3cols"     => E::InternalSplit3Columns,
            "3rows"     => E::InternalSplit3Rows,
            "1big3small"=> E::InternalSplit1Big3Small,
            _ => return false,
        };
        self.process_chart_out_event(event);
        true
    }

    /// Diagnostic: snapshot of the pending-compare queue formatted as
    /// short strings.  Used by `mlc-app-shell-web` to expose state through
    /// `window.MLC_DEBUG.pending_compare` for `dev-attach --eval`.
    pub fn pending_compare_diag(&self) -> Vec<String> {
        self.pending_compare.iter()
            .map(|p| format!("{}@{} ({}) frames={}",
                p.symbol, p.exchange.as_str(), p.account_type_label, p.deadline_frames))
            .collect()
    }

    /// Diagnostic: current modal open state as a short string.
    pub fn modal_state_diag(&self) -> String {
        format!("{:?}", self.modal_state.current)
    }

    /// Diagnostic: total number of times the compare-add path was entered.
    /// Used by `mlc-app-shell-web` to detect whether the compare modal click is
    /// even reaching the `OpenModal::CompareSearch` branch.
    pub fn compare_add_attempts(&self) -> u32 { self.compare_add_attempts }
    pub fn compare_add_success(&self) -> u32 { self.compare_add_success }
    pub fn compare_add_queued(&self) -> u32 { self.compare_add_queued }

    /// Drain the pending screenshot request.
    ///
    /// Returns `true` if a screenshot was requested since the last call.
    /// Called by the vello renderer each frame in `about_to_wait`.
    pub fn drain_pending_screenshot(&mut self) -> bool {
        let pending = self.pending_screenshot;
        self.pending_screenshot = false;
        pending
    }

    /// Drain the pending live-share request.
    ///
    /// Returns `true` if a share was requested since the last call.
    /// Mirror of `drain_pending_screenshot` — the shell performs the
    /// actual upload and clipboard write.
    pub fn drain_pending_share_live(&mut self) -> bool {
        let pending = self.pending_share_live;
        self.pending_share_live = false;
        pending
    }

    /// Return the chart content rect (toolbar-excluded area) from the last render.
    pub fn content_rect(&self) -> &LayoutRect {
        &self.content_rect
    }

    /// Return pixel crop coordinates `(x, y, width, height)` for screenshot
    /// cropping.  Derived from the content_rect set during the last render().
    pub fn screenshot_rect(&self) -> (u32, u32, u32, u32) {
        let r = &self.content_rect;
        (r.x as u32, r.y as u32, r.width as u32, r.height as u32)
    }

    // -------------------------------------------------------------------------
    // Resize
    // -------------------------------------------------------------------------

    /// Update screen dimensions and immediately sync viewport to the new
    /// candle area so that `tick()` (called before `render()`) uses correct
    /// dimensions for Focus-mode follow calculations.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        // Refresh the cached layout so sync_viewport_from_layout reads fresh
        // geometry — resize runs outside prepare_frame where the layout phase
        // has not yet run, so active_frame_layout could be stale or None.
        self.active_frame_layout = Some(self.build_extended_layout());
        self.sync_viewport_from_layout();

        // On the very first resize after a preset restore, chart_width is now
        // known. Reposition every window so the last bar has 5 empty bars of
        // right margin (set_bars() couldn't do this because chart_width was 0).
        self.try_initial_viewport_fit();
    }

    /// One-shot snap-to-end after a hydration/fresh boot, armed via
    /// `needs_initial_viewport_fit`. Called from `resize()` AND once per
    /// frame from the tick: on wasm the settled-dimensions resize fires
    /// BEFORE the initial request_bars round-trip returns and never again
    /// (nobody resizes the browser window), so a resize-only consumer left
    /// the flag armed forever and the viewport parked on the oldest bars.
    pub(crate) fn try_initial_viewport_fit(&mut self) {
        if self.needs_initial_viewport_fit {
            // Snapshot each chart's ACTIVE series up front (audit D8 — a
            // renko/pnf/kagi/... window fits against its own array, not
            // always the kline array), immutable borrow released before the
            // mutable containers_mut() loop below.
            let series_by_chart: std::collections::HashMap<mlc_core::state::ChartId, crate::active_series::OwnedActiveSeries> =
                self.panel_app.panel_grid.containers().keys()
                    .map(|cid| (*cid, self.active_series_for_chart(*cid)))
                    .collect();
            // Only consume the flag if at least one chart actually has data
            // ready to be snapped. Otherwise we'd burn the one-shot on the
            // very first resize that fires before the initial request_bars
            // round-trip returns, and the viewport would freeze on the
            // oldest bars forever. Keeping the flag armed lets the next
            // call (tick after BarsLoaded, or a later resize) do the snap.
            let any_bars = series_by_chart.values().any(|s| !s.as_active_series().is_empty());
            for (cid, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
                let series = series_by_chart.get(cid)
                    .map(|s| s.as_active_series())
                    .unwrap_or(crate::active_series::ActiveSeries::Bars(&[]));
                if window.viewport().chart_width > 0.0 && window.viewport().bar_spacing > 0.0 && !series.is_empty() {
                    window.snap_to_end(crate::chart_container::DEFAULT_SNAP_MARGIN);
                }
                window.calc_auto_scale(series);
            }
            if any_bars {
                self.needs_initial_viewport_fit = false;
            }
        }
    }

    /// Recompute viewport chart_width/chart_height from the current window
    /// size, toolbar config, and scale settings.  Called from `resize()` and
    /// at the start of `render()`.
    ///
    /// Uses `build_extended_layout()` (which accounts for sub-panes) so that
    /// `viewport.chart_height` matches `main_chart.chart.height` used by the
    /// renderer.  Previously this called `FrameLayout::from_chart_panel_with_config`
    /// which does NOT subtract sub-pane heights, causing coordinate mismatches
    /// when RSI/MACD sub-panes are visible.
    fn sync_viewport_from_layout(&mut self) {
        // In split mode, per-leaf dimensions are handled by the split-pane layout
        // block in prepare_frame() which calls build_extended_layout_for_leaf().
        // For non-split mode we compute one layout and apply it to ALL windows so
        // that every window (not just the active one) gets a valid chart_width.
        // Without this, non-active windows keep chart_width = 0.0 which blocks the
        // needs_auto_scale_after_bars deferred snap from ever firing.
        if self.panel_app.panel_grid.is_split() {
            return;
        }
        // Prefer the frame-cached layout; fall back to building on demand so
        // that resize() (called outside prepare_frame, where active_frame_layout
        // may be stale/None) always uses fresh geometry.  Clone to end the
        // immutable borrow before the mutable containers_mut() call below.
        let extended = self.active_frame_layout.clone()
            .unwrap_or_else(|| self.build_extended_layout());
        let new_width = extended.main_chart.chart.width;
        let new_height = extended.main_chart.chart.height;
        // Aux trading-center windows live in the same containers map but are
        // laid out by the trading container's own dock, NOT the main grid —
        // stamping them with main-chart dims here fought the mini chart's
        // own same-frame resize every single frame (447↔151 thrash, ±196
        // bars of view_start per flip: the minichart scale-mode bug AND the
        // main-thread churn behind the Lagged bar drops, owner defect
        // 2026-07-30). They get exactly one writer:
        // `resize_trading_center_geometry` at render time.
        let aux: std::collections::HashSet<ChartId> =
            self.trading_center_charts.values().copied().collect();
        for (cid, window) in self.panel_app.panel_grid.containers_mut().iter_mut() {
            if aux.contains(cid) {
                continue;
            }
            // Pin right edge: shift view_start so the last visible bars stay
            // anchored when the window is resized, matching terminal behavior.
            // Skip bar_shift for windows still waiting for their initial snap
            // (needs_auto_scale_after_bars = true and old_width = 0) — the
            // deferred snap in prepare_frame() will compute view_start correctly
            // once chart_width is set here.
            window.resize_geometry(new_width, new_height);
        }
    }

    // -------------------------------------------------------------------------
    // Backfill after broadcast lag
    // -------------------------------------------------------------------------

    /// Re-fetches only the missing bars for all active chart windows after a
    /// broadcast `Lagged` event.
    ///
    /// Because `DataBridge::request_bars` uses incremental mode when a bar
    /// cache already exists for the symbol (fetching only bars newer than
    /// `last_cached_ts`), this call is cheap — it downloads the gap, not the
    /// full history.
    ///
    // -------------------------------------------------------------------------
    // Indicator recalculation helpers
    // -------------------------------------------------------------------------

    /// Recalculate indicators for every window that displays `symbol`.
    ///
    /// Iterates ALL windows with a matching symbol and calls
    /// `calculate_for_window()` for each, so that indicator instances scoped to
    /// a window are calculated against that window's own bars.  This is
    /// correct when multiple windows show the same symbol on different
    /// timeframes (each window has its own bar series keyed by its timeframe).
    fn recalc_indicators_for_symbol(&mut self, symbol: &str) {
        // Collect only the ChartId values (cheap u64 copies) for every window
        // showing this symbol, avoiding a full bars clone at this stage.
        // S2.7.1: filter by branch_meta symbol instead of w.symbol.
        let matching_ids: Vec<u64> = self
            .panel_app
            .panel_grid
            .containers()
            .iter()
            .filter(|(id, _w)| {
                self.symbol_for_chart(**id)
                    .map_or(false, |s| s == symbol)
            })
            .map(|(id, _w)| id.0)
            .collect();

        // Split-borrow: `panel_app.panel_grid` and `indicator_manager` are
        // distinct fields, so Rust allows simultaneous borrows of both.
        for window_id in matching_ids {
            let chart_id = ChartId(window_id);
            let bars = self.bars_for_chart(chart_id);
            if self.panel_app.panel_grid.containers().get(&chart_id).is_some() {
                self.core.indicators_mut().recalc_bubble(mlc_core::ids::BubbleId(window_id), &bars[..]);
            }
        }
        let _ = symbol; // legacy — recalc filters by window_id, not symbol
        self.recalc_count += 1;
        self.sync_sub_panes_from_manager();
    }


    // -------------------------------------------------------------------------
    // Render
    // -------------------------------------------------------------------------

    // prepare_frame lives in frame_lifecycle/prepare.rs
    /// Convenience wrapper: calls `prepare_frame`, `render_to_scene`, then
    /// `apply_render_output` in sequence.
    ///
    /// Existing call sites can continue using this method unchanged.
    pub fn render(&mut self, ctx: &mut dyn RenderContext, current_time_ms: u64, skip_toolbar_draw: bool) {
        self.prepare_frame(self.width as f64, self.height as f64);
        // User-text label registry: one frame's rects, rebuilt by the
        // primitive passes below (cleared here, never per window, so
        // multi-window frames keep every window's labels).
        mlc_core::engine::render::clear_label_text_rects();
        let output = self.render_to_scene(ctx, current_time_ms, skip_toolbar_draw);
        self.apply_render_output(output);
    }

    /// Render the cursor-dependent overlay that must stay ABOVE toolbar/modal
    /// chrome, for every visible chart leaf: in-progress drawing previews and
    /// the near-cursor tooltip.
    ///
    /// Crosshair lines + scale cursor labels are NOT drawn here (2026-07-25
    /// split) — they are the lowest overlay layer and are drawn earlier,
    /// inside [`render_to_scene`], immediately after each leaf's static chart
    /// content and before the toolbar/dropdown pass. Drawing them here (after
    /// `render()` returns, i.e. after toolbars/dropdowns already painted)
    /// used to make the crosshair bleed on top of open dropdowns.
    ///
    /// Call AFTER [`render`] (or [`render_to_scene`] + [`apply_render_output`])
    /// so preview/tooltip appear on top of all chart content and toolbar chrome.
    ///
    /// The cached [`ExtendedFrameLayout`] built during [`prepare_frame`] is used
    /// directly — no geometry is recomputed here.
    ///
    /// Skips painting entirely while [`is_capturing_screenshot`] is true —
    /// the drawing-tool cursor preview and the near-cursor OHLC tooltip are
    /// not chart content (screenshot-is-chart-only arc, 2026-09-05).
    ///
    /// [`is_capturing_screenshot`]: Self::is_capturing_screenshot
    pub fn render_overlay(&self, ctx: &mut dyn RenderContext) {
        if self.is_capturing_screenshot() {
            return;
        }
        use mlc_app_layout::layout::render_chart_overlay;
        use mlc_core::chart::ChartRect;

        let chart_theme = self.panel_app.chart_theme_for_render();
        let is_split = self.panel_app.panel_grid.is_split();

        if is_split {
            // Iterate all split leaves using the cached per-leaf layouts.
            let leaf_ids: Vec<_> = self.frame_layouts.keys().copied().collect();
            for leaf_id in leaf_ids {
                let layout = match self.frame_layouts.get(&leaf_id) {
                    Some(l) => l,
                    None => continue,
                };
                let window = match self.panel_app.panel_grid.container_for_leaf(leaf_id) {
                    Some(w) => w,
                    None => continue,
                };
                // S2.7.1: read instrument fields via branch_meta.
                let chart_id = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
                let overlay_tf = chart_id
                    .and_then(|cid| self.timeframe_for_chart(cid));
                let overlay_tf_name: String = overlay_tf.as_ref().map(|tf| tf.name.clone()).unwrap_or_default();
                // Countdown gate: None on Param-resolution chart types (see
                // `countdown_interval_ms_for_chart` doc) — not the raw TF
                // interval, which stays stale after a chart-type switch.
                let overlay_tf_interval_ms: Option<i64> = chart_id
                    .and_then(|cid| self.countdown_interval_ms_for_chart(cid));
                let overlay_symbol: String = chart_id
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();

                // Build a chart rect from the main chart area in the layout.
                let chart_rect = ChartRect::new(
                    layout.main_chart.chart.x,
                    layout.main_chart.chart.y,
                    layout.main_chart.chart.width,
                    layout.main_chart.chart.height,
                );
                // Phase 4 C: get panel state (grid/legend/chart_type/tooltip from panels_store).
                let overlay_panel_id = window.chart_panel_id;
                let overlay_default_panel = crate::chart_panel_state::ChartPanelState::new(overlay_panel_id);
                let overlay_panel = self.panels_store.chart_panel
                    .get(&overlay_panel_id)
                    .unwrap_or(&overlay_default_panel);
                // Audit D8: resolve BEFORE taking the bubbles() read guard below
                // (with_bubble takes its own read lock internally).
                let overlay_ts_shadow = chart_id.and_then(|cid| self.ts_shadow_for_chart(cid));
                let overlay_bubble_mgr = self.core.bubbles();
                let overlay_chart_type = overlay_bubble_mgr.chart_type(window.current_tag_id).unwrap_or("candles");
                let overlay_trade_archive_available = self.trade_archive_available_for(&overlay_bubble_mgr, window.current_tag_id);
                let overlay_liq_ink = self.liq_ink(window.current_tag_id);
                let overlay_dom = self.dom_heatmap_ink(window.current_tag_id);
                let overlay_dom_active = self.dom_overlay_active(window.current_tag_id);
                let overlay_liq_active = self.liq_overlay_active(window.current_tag_id);
                let overlay_window_inputs = mlc_backend::bubble::ChartWindowRenderInputs {
                    viewport: window.viewport(),
                    price_scale: window.price_scale(),
                    time_scale: &window.time_scale,
                    crosshair: &window.crosshair,
                    scale_settings: &window.scale_settings,
                    ha_cache: window.ha_cache.as_deref(),
                    vp_overlay: if self.overlay_instance_active(window.current_tag_id, Self::VOLUME_PROFILE_TYPE_ID) { window.vp_overlay.as_ref() } else { None },
                    tpo_profile_overlay: if self.overlay_instance_active(window.current_tag_id, Self::TPO_PROFILE_TYPE_ID) { window.tpo_profile_overlay.as_ref() } else { None },
                    vp_ink: self.profile_ink(window.current_tag_id, Self::VOLUME_PROFILE_TYPE_ID),
                    tpo_ink: self.profile_ink(window.current_tag_id, Self::TPO_PROFILE_TYPE_ID),
                    oi_show: self.overlay_instance_active(window.current_tag_id, Self::OI_DELTA_TYPE_ID),
                    funding_show: self.overlay_instance_active(window.current_tag_id, Self::FUNDING_RATE_TYPE_ID),
                    dom_heatmap_show: overlay_dom_active,
                    dom_params: overlay_dom,
                    liq_heatmap_show: overlay_liq_active,
                    liq_ink: overlay_liq_ink,
                    liq_projection_show: self.liq_projection_active(window.current_tag_id),
                    liq_projection_ink: self.liq_projection_ink(window.current_tag_id),
                    footprint_display: overlay_panel.footprint_display,
                    tick_intensity: overlay_panel.tick_intensity,
                    tpo_display: overlay_panel.tpo_display,
                    line_family_display: overlay_panel.line_family_display,
                    wicks_effective: crate::active_series::effective_wick_flag(
                        overlay_chart_type,
                        self.panel_app.chart_settings_state.instrument_wick_enabled,
                        overlay_panel.bar_family_wicks,
                    ),
                    trade_archive_available: overlay_trade_archive_available,
                };
                if let Some(mut render_state) = overlay_bubble_mgr.render_state_for(
                    window.current_tag_id,
                    overlay_window_inputs,
                    chart_rect,
                    &chart_theme,
                    Some(overlay_tf_name.as_str()),
                    Some(&window.scale_settings.time_format),
                    overlay_tf_interval_ms,
                    &overlay_panel.grid_options,
                    &overlay_panel.legend,
                ) {
                    // Audit D8: derived-chart-type panes get their own array's
                    // timestamps for time-scale labels + drawings ts→x — see
                    // `ts_shadow_for_chart_type` for the per-type rationale.
                    if let Some(shadow) = overlay_ts_shadow.as_deref() {
                        render_state.bars = shadow;
                        render_state.non_temporal_axis = true;
                    }
                    render_state.hide_time_grid = crate::active_series::chart_type_hides_time_grid(overlay_chart_type);
                    render_state.cvd_paint_deltas = self.cvd_paint_deltas_for(window.current_tag_id, overlay_chart_type);

                    // Crosshair + axis labels are drawn earlier, inside
                    // `render_to_scene`, ahead of the toolbar/dropdown pass
                    // (see `render_chart_crosshair_layer`) — this call now
                    // only paints the drawing preview + tooltip, which stay
                    // above toolbar chrome exactly as before.
                    let overlay_drawings_guard = self.core.drawings();
                    let overlay_view = overlay_drawings_guard.with_bubble(window.current_tag_id);
                    render_chart_overlay(
                        ctx,
                        &render_state,
                        layout,
                        Some(&overlay_view as &dyn mlc_core::drawing::DrawingRenderView),
                        Some(&overlay_panel.tooltip),
                        Some(overlay_symbol.as_str()),
                        &overlay_tf_name,
                    );
                    drop(overlay_drawings_guard);
                    self.render_inline_prim_text_editor(ctx, layout, window, &render_state);
                }
            }
        } else {
            // Single-window path: use active_frame_layout.
            let layout = match &self.active_frame_layout {
                Some(l) => l,
                None => return,
            };
            // Get the active leaf window.
            let active_leaf = self.panel_app.panel_grid.docking().active_leaf();
            let window = active_leaf
                .and_then(|lid| self.panel_app.panel_grid.container_for_leaf(lid))
                .or_else(|| {
                    // Fallback: first window in the grid
                    self.panel_app.panel_grid.containers().values().next()
                });
            let window = match window {
                Some(w) => w,
                None => return,
            };
            // S2.7.1: read instrument fields via branch_meta for overlay.
            let single_overlay_cid = self.panel_app.panel_grid.active_container_id();
            let single_overlay_tf = single_overlay_cid
                .and_then(|cid| self.timeframe_for_chart(cid));
            let single_overlay_tf_name: String = single_overlay_tf.as_ref().map(|tf| tf.name.clone()).unwrap_or_default();
            // Countdown gate: see `countdown_interval_ms_for_chart` doc.
            let single_overlay_tf_interval_ms: Option<i64> = single_overlay_cid
                .and_then(|cid| self.countdown_interval_ms_for_chart(cid));
            let single_overlay_symbol: String = single_overlay_cid
                .and_then(|cid| self.symbol_for_chart(cid))
                .map(String::from)
                .unwrap_or_default();

            let chart_rect = ChartRect::new(
                layout.main_chart.chart.x,
                layout.main_chart.chart.y,
                layout.main_chart.chart.width,
                layout.main_chart.chart.height,
            );
            // Phase 4 C: panel state for single-window overlay.
            let single_pid = window.chart_panel_id;
            let single_default_panel = crate::chart_panel_state::ChartPanelState::new(single_pid);
            let single_panel = self.panels_store.chart_panel
                .get(&single_pid)
                .unwrap_or(&single_default_panel);
            // Audit D8: resolve BEFORE taking the bubbles() read guard below.
            let single_overlay_ts_shadow = single_overlay_cid.and_then(|cid| self.ts_shadow_for_chart(cid));
            let single_overlay_bubble_mgr = self.core.bubbles();
            let single_chart_type = single_overlay_bubble_mgr.chart_type(window.current_tag_id).unwrap_or("candles");
            let single_overlay_trade_archive_available = self.trade_archive_available_for(&single_overlay_bubble_mgr, window.current_tag_id);
            let single_overlay_liq_ink = self.liq_ink(window.current_tag_id);
            let single_dom = self.dom_heatmap_ink(window.current_tag_id);
            let single_dom_active = self.dom_overlay_active(window.current_tag_id);
                let single_overlay_liq_active = self.liq_overlay_active(window.current_tag_id);
            let single_overlay_window_inputs = mlc_backend::bubble::ChartWindowRenderInputs {
                viewport: window.viewport(),
                price_scale: window.price_scale(),
                time_scale: &window.time_scale,
                crosshair: &window.crosshair,
                scale_settings: &window.scale_settings,
                ha_cache: window.ha_cache.as_deref(),
                vp_overlay: if self.overlay_instance_active(window.current_tag_id, Self::VOLUME_PROFILE_TYPE_ID) { window.vp_overlay.as_ref() } else { None },
                tpo_profile_overlay: if self.overlay_instance_active(window.current_tag_id, Self::TPO_PROFILE_TYPE_ID) { window.tpo_profile_overlay.as_ref() } else { None },
                vp_ink: self.profile_ink(window.current_tag_id, Self::VOLUME_PROFILE_TYPE_ID),
                tpo_ink: self.profile_ink(window.current_tag_id, Self::TPO_PROFILE_TYPE_ID),
                oi_show: self.overlay_instance_active(window.current_tag_id, Self::OI_DELTA_TYPE_ID),
                funding_show: self.overlay_instance_active(window.current_tag_id, Self::FUNDING_RATE_TYPE_ID),
                dom_heatmap_show: single_dom_active,
                dom_params: single_dom,
                liq_heatmap_show: single_overlay_liq_active,
                liq_ink: single_overlay_liq_ink,
                liq_projection_show: self.liq_projection_active(window.current_tag_id),
                liq_projection_ink: self.liq_projection_ink(window.current_tag_id),
                footprint_display: single_panel.footprint_display,
                tick_intensity: single_panel.tick_intensity,
                tpo_display: single_panel.tpo_display,
                line_family_display: single_panel.line_family_display,
                wicks_effective: crate::active_series::effective_wick_flag(
                    single_chart_type,
                    self.panel_app.chart_settings_state.instrument_wick_enabled,
                    single_panel.bar_family_wicks,
                ),
                trade_archive_available: single_overlay_trade_archive_available,
            };
            if let Some(mut render_state) = single_overlay_bubble_mgr.render_state_for(
                window.current_tag_id,
                single_overlay_window_inputs,
                chart_rect,
                &chart_theme,
                Some(single_overlay_tf_name.as_str()),
                Some(&window.scale_settings.time_format),
                single_overlay_tf_interval_ms,
                &single_panel.grid_options,
                &single_panel.legend,
            ) {
                if let Some(shadow) = single_overlay_ts_shadow.as_deref() {
                    render_state.bars = shadow;
                    render_state.non_temporal_axis = true;
                }
                render_state.hide_time_grid = crate::active_series::chart_type_hides_time_grid(single_chart_type);
                render_state.cvd_paint_deltas = self.cvd_paint_deltas_for(window.current_tag_id, single_chart_type);

                // Crosshair + axis labels are drawn earlier, inside
                // `render_to_scene`, ahead of the toolbar/dropdown pass (see
                // `render_chart_crosshair_layer`) — this call now only paints
                // the drawing preview + tooltip, which stay above toolbar
                // chrome exactly as before.
                let single_drawings_guard = self.core.drawings();
                let single_view = single_drawings_guard.with_bubble(window.current_tag_id);
                render_chart_overlay(
                    ctx,
                    &render_state,
                    layout,
                    Some(&single_view as &dyn mlc_core::drawing::DrawingRenderView),
                    Some(&single_panel.tooltip),
                    Some(single_overlay_symbol.as_str()),
                    &single_overlay_tf_name,
                );
                drop(single_drawings_guard);
                self.render_inline_prim_text_editor(ctx, layout, window, &render_state);
            }
        }
    }

    /// Draw the inline primitive text editor (inline-prim-text arc,
    /// 2026-08-26) for the leaf whose overlay is being painted, when the
    /// active edit belongs to that leaf's bubble: an opaque plate over the
    /// primitive's anchor showing the `TextFieldStore` field's LIVE text
    /// with selection highlight + caret, pushing fresh `(rect,
    /// char_x_positions)` geometry via `update_field` every frame so the
    /// store's generic `on_drag_start`/`on_drag_move` can place the caret —
    /// the `draw_oe_field_row` division of labor (render pushes geometry,
    /// the store consumes it).
    ///
    /// The primitive's stored text keeps rendering underneath until commit;
    /// the plate is sized to cover BOTH the stored and the live text, so
    /// what the user sees is always the field's content. Anchor comes from
    /// the primitive's own first `control_points()` handle (chart-local —
    /// the same space `render_main_chart_primitives` draws selection
    /// handles in — so the pane origin is added), which for every text
    /// primitive is the point its text hangs off.
    fn render_inline_prim_text_editor(
        &self,
        ctx: &mut dyn RenderContext,
        layout: &mlc_core::ExtendedFrameLayout,
        window: &crate::chart_container::ChartContainer,
        render_state: &mlc_core::chart::ChartRenderState,
    ) {
        use uzor::render::{TextAlign, TextBaseline};

        let edit = match self.inline_prim_text_edit.as_ref() {
            Some(e) if e.bid == window.current_tag_id => e,
            _ => return,
        };
        let wid = uzor::WidgetId::from(crate::text_input::PRIM_INLINE_TEXT);

        // Anchor in ABSOLUTE screen coords + the primitive's current text
        // config, read out under a short-lived guard (a read guard held
        // across the coordinator borrow below would be fine, but there is
        // no reason to hold it past the reads).
        let (anchor, label_rect, stored, font_size, text_color) = {
            let guard = self.core.drawings();
            let Some(prim) = guard.primitives(edit.bid).iter().find(|p| p.data().id == edit.prim_id) else {
                // Primitive went away mid-edit (undo, delete hotkey) — the
                // edit state self-clears on the next commit/cancel; nothing
                // to draw.
                return;
            };
            let anchor: Option<(f64, f64)> = match edit.pane {
                None => {
                    let chart = &render_state.chart_rect;
                    let mut vp = render_state.viewport.clone();
                    vp.chart_width = chart.width;
                    vp.chart_height = chart.height;
                    prim.control_points(render_state.bars, &vp, render_state.price_scale)
                        .first()
                        .map(|cp| (chart.x + cp.x, chart.y + cp.y))
                }
                Some(iid) => {
                    match layout.sub_panes.iter().find(|pl| pl.instance_id == iid) {
                        Some(pl) => {
                            let content = pl.content;
                            // Same sub-pane scale derivation
                            // `try_wave_gesture`'s stage 2 uses.
                            let (price_min, price_max) = window.sub_pane_y_state.get(&iid)
                                .map(|s| (s.price_min, s.price_max))
                                .unwrap_or((0.0, 100.0));
                            let ps = mlc_core::PriceScale::new(price_min, price_max);
                            let mut vp = render_state.viewport.clone();
                            vp.chart_width = content.width;
                            vp.chart_height = content.height;
                            prim.control_points(render_state.bars, &vp, &ps)
                                .first()
                                .map(|cp| (content.x + cp.x, content.y + cp.y))
                        }
                        None => None,
                    }
                }
            };
            let data = prim.data();
            let t = data.text.as_ref();
            let stored = if let Some(level) = edit.level {
                prim.level_value_label(level)
            } else {
                t.map(|t| t.content.clone()).unwrap_or_default()
            };
            let font_size = t.map(|t| t.font_size).unwrap_or(12.0);
            let text_color = t.and_then(|t| t.color.clone())
                .unwrap_or_else(|| data.color.stroke.clone());
            // Prefer the label's own rendered rect (this frame's registry)
            // over the control-point anchor: for a labeled line/channel the
            // text hangs OFF the first handle, so the plate must sit where
            // the label is drawn, not where the line starts. Level-value
            // edits use the Fib/Gann plate registry. The registry rect is
            // pane-local — shift by the pane origin, same space the
            // anchor math above produces.
            let raw_rect = if let Some(level) = edit.level {
                mlc_core::engine::render::level_label_rect_for(
                    edit.bid, edit.pane, edit.prim_id, level)
            } else {
                mlc_core::engine::render::label_text_rect_for(
                    edit.bid, edit.pane, edit.prim_id)
            };
            let label_rect = raw_rect.map(|(x0, y0, x1, y1)| {
                    let origin = match edit.pane {
                        None => (render_state.chart_rect.x, render_state.chart_rect.y),
                        Some(iid) => layout.sub_panes.iter()
                            .find(|pl| pl.instance_id == iid)
                            .map(|pl| (pl.content.x, pl.content.y))
                            .unwrap_or((0.0, 0.0)),
                    };
                    (x0 + origin.0, y0 + origin.1, x1 + origin.0, y1 + origin.1)
                });
            match anchor {
                Some(a) => (a, label_rect, stored, font_size, text_color),
                None => return,
            }
        };
        let (anchor_x, anchor_y) = anchor;

        let (live_text, cursor, selection) = {
            let coord = self.input_coordinator.borrow();
            let store = coord.text_fields();
            (
                store.text(&wid).to_string(),
                store.cursor(&wid),
                store.selection_range(&wid),
            )
        };

        ctx.save();
        let font = format!("{}px sans-serif", font_size as i32);
        ctx.set_font(&font);
        let line_height = font_size * 1.2;
        let stored_lines = stored.lines().count().max(1);
        let stored_w = stored.lines().map(|l| ctx.measure_text(l)).fold(0.0, f64::max);
        let live_w = ctx.measure_text(&live_text);
        let pad = 4.0;
        let box_w = stored_w.max(live_w) + pad * 2.0;
        let box_h = line_height * stored_lines as f64 + pad * 2.0;
        // The stored text renders centered on the anchor with its top AT the
        // anchor (`PrimitiveText` defaults h_align Center / v_align Start —
        // see `render_primitive_text`), so the plate covers exactly that
        // extent. When the label registry has this primitive's own rect the
        // plate centers on THAT instead — labeled lines/channels hang their
        // text off the line, not off the first control handle.
        let (box_x, box_y) = match label_rect {
            Some((rx0, ry0, rx1, ry1)) => (
                (rx0 + rx1) / 2.0 - box_w / 2.0,
                (ry0 + ry1) / 2.0 - box_h / 2.0,
            ),
            None => (anchor_x - box_w / 2.0, anchor_y - pad),
        };

        ctx.set_fill_color(&render_state.theme.background);
        ctx.fill_rect(box_x, box_y, box_w, box_h);
        ctx.set_stroke_color(&render_state.theme.text);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rect(box_x, box_y, box_w, box_h);

        let text_left = box_x + pad;
        let text_mid_y = box_y + pad + line_height / 2.0;
        let char_count = live_text.chars().count();
        let mut char_x_positions: Vec<f64> = Vec::with_capacity(char_count + 1);
        for i in 0..=char_count {
            let slice: String = live_text.chars().take(i).collect();
            char_x_positions.push(text_left + ctx.measure_text(&slice));
        }

        if let Some((lo, hi)) = selection {
            let sel_x0 = char_x_positions.get(lo).copied().unwrap_or(text_left);
            let sel_x1 = char_x_positions.get(hi).copied().unwrap_or(text_left);
            ctx.set_fill_color("rgba(41,98,255,0.35)");
            ctx.fill_rect(sel_x0, box_y + 1.0, (sel_x1 - sel_x0).max(0.0), box_h - 2.0);
        }

        ctx.set_fill_color(&text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&live_text, text_left, text_mid_y);

        let caret_x = char_x_positions.get(cursor).copied().unwrap_or(text_left + live_w);
        ctx.set_fill_color(&text_color);
        ctx.fill_rect(caret_x, box_y + pad, 1.5, line_height);

        self.input_coordinator.borrow_mut().text_fields_mut().update_field(
            &wid,
            (box_x, box_y, box_w, box_h),
            char_x_positions,
        );
        ctx.restore();
    }

    /// Pure-rendering pass.
    ///
    /// Emits all vector graphics into `ctx`, registers widget hit-zones with the
    /// `input_coordinator` (via interior mutability), and returns a [`RenderOutput`]
    /// containing the cached results of this frame.
    ///
    /// Call [`prepare_frame`] first to sync mutable state (viewport, sidebar data,
    /// etc.), then call [`apply_render_output`] afterward to persist the results.
    ///
    /// When `skip_toolbar_draw` is `true`, the toolbar vector graphics are NOT
    /// re-emitted into `ctx`.  Instead, hit zones are re-registered from the
    /// cached `self.last_toolbar_result` so input routing remains correct.
    pub fn render_to_scene(&mut self, ctx: &mut dyn RenderContext, current_time_ms: u64, skip_toolbar_draw: bool) -> RenderOutput {
        let _rt0 = web_time::Instant::now();
        // One step per rendered frame — this is the cadence the clock's
        // hover animation runs at.
        self.anim_frame = self.anim_frame.wrapping_add(1);
        let w = self.width as f64;
        let h = self.height as f64;

        // Sidebar width — when open, the sidebar appears between the chart
        // content (price scale) and the right toolbar.  The right toolbar stays
        // fixed at the window edge.  Only the chart content area shrinks.
        //
        // The profile manager and welcome wizard are ordinary modals floating
        // over a live chart now (owner's ruling 2026-08-31) — they no longer
        // replace the content area, so the sidebar keeps its normal width
        // under them, same as under any other modal.
        let sidebar_w = self.sidebar_state.right_width();

        // 1. Begin frame — propagate current pointer position
        let input_state = InputState::default()
            .with_pointer_pos(self.last_mouse_pos.0, self.last_mouse_pos.1);
        self.input_coordinator.borrow_mut().begin_frame(input_state);

        // 2. Full window rect for toolbar layout (right toolbar at window edge).
        let window_rect = LayoutRect::new(0.0, 0.0, w, h);

        // 3. Compute toolbar layout from full window (right toolbar at edge).
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        // Content rect for chart rendering — shrink by sidebar_w.
        // Sidebar appears between chart's right edge and right toolbar.
        // Right toolbar stays at window edge, unaffected.
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };
        // content_rect and right_toolbar_left_x are returned in RenderOutput and
        // applied by apply_render_output().
        let out_right_toolbar_left_x = panel_layout.right_toolbar_rect.x;

        // 4. Render chart content.
        //    When split, iterate over panel_grid leaves and call
        //    render_full_chart_panel() for each with ToolbarConfig::minimal()
        //    (0 toolbars — full chart with scales, crosshair, sub-panes, etc.).
        //    When not split, use the original single-window path.

        // Local leaf tab hit zones for this frame — returned via RenderOutput.
        let mut out_leaf_tab_hit_zones: std::collections::HashMap<mlc_core::LeafId, mlc_app_layout::layout::panel_overlay::LeafTabHitZones> = std::collections::HashMap::new();
        // Submenu state update from toolbar dropdown rendering.
        let mut out_open_submenu_update: Option<Option<String>> = None;
        // Sub-pane range writebacks: render computes symmetrized+padded ranges,
        // applied by apply_render_output() via container_for_leaf_mut.
        let mut out_sub_pane_range_writebacks: Vec<(mlc_core::LeafId, usize, f64, f64)> = Vec::new();
        // Sub-pane overlay result writebacks: cached per-window for hit testing.
        let mut out_sub_pane_overlay_writebacks: Vec<(mlc_core::LeafId, Vec<mlc_core::SubPaneOverlayResult>)> = Vec::new();
        // Loading-spinner geometry per leaf for the host renderer to composite as overlay.
        let mut out_loading_spinners: Vec<(mlc_core::LeafId, (f64, f64, f64))> = Vec::new();

        let _rt1 = web_time::Instant::now(); // checkpoint: before chart render
        let frame_theme = self.panel_app.frame_theme_for_render();
        let leaf_tab_toolbar_theme = self.panel_app.toolbar_theme_for_render();
        // main_chart_y is set in the single-window branch below; used by the
        // indicator overlay chevron rendered after the chart block.
        let mut main_chart_y_single = content_rect.y;
        // False while a maximized sub-pane covers the single-mode container —
        // chart-anchored overlays (leaf tab, chevron) are skipped then.
        let mut single_leaf_tab_visible = true;
        let corner_zones = if self.panel_app.panel_grid.is_split() {
            // Compute sub-chart rectangles.
            // Use origin (0, 0) so that panel_rects() returns rects relative
            // to the content area.  We add content_rect origin later when
            // converting to absolute screen coords.
            // Snapshot leaf rects (layout was already computed in prepare_frame).
            let leaf_rects: Vec<_> = self.panel_app.panel_grid.panel_rects()
                .iter()
                .map(|(&leaf_id, &sub_rect)| (leaf_id, sub_rect))
                .collect();

            // Register chart panes on InputCoordinator as BlackboxPanel widgets.
            // BlackboxPanel makes is_over_ui() return false while the cursor is over
            // the chart canvas, so ui_drag_active no longer blocks scrollbar drags.
            // Chart-internal input (scales, sub-pane separators, drawing tools) is
            // dispatched by the chart's own hit-tester via panel_grid.resolve_input.
            {
                use uzor::input::{Sense, WidgetKind};
                use mlc_app_layout::ui::z_order::ZLayer;
                let chart_layer = ZLayer::ChartCanvas.push(&mut self.input_coordinator.borrow_mut());
                for &(leaf_id, sub_rect) in &leaf_rects {
                    if sub_rect.width > 0.0 && sub_rect.height > 0.0 {
                        self.input_coordinator.borrow_mut().register_composite(
                            format!("chart:pane:{}", leaf_id.0),
                            WidgetKind::BlackboxPanel,
                            uzor::Rect::new(
                                content_rect.x + sub_rect.x as f64,
                                content_rect.y + sub_rect.y as f64,
                                sub_rect.width as f64,
                                sub_rect.height as f64,
                            ),
                            Sense::CLICK
                                | Sense::DRAG
                                | Sense::SCROLL
                                | Sense::RIGHT_CLICK
                                | Sense::DOUBLE_CLICK
                                | Sense::HOVER,
                            &chart_layer,
                        );
                    }
                }
            }

            let chart_theme = self.panel_app.chart_theme_for_render();
            let no_toolbar = mlc_core::ToolbarConfig::minimal();

            // Compute alert indicator values ONCE per render — they are
            // window-independent and can be reused across all split-pane leaves.
            let all_alerts_for_render: Vec<mlc_core::alert::AlertItem> = self.core.alerts().items_all();
            let alert_indicator_values_cache = Self::build_indicator_values_for_alerts(
                &all_alerts_for_render,
                &self.core.indicators(),
            );

            // Pre-build drawing points per chart window. In split-pane mode the
            // same window may appear as multiple leaves; computing points once
            // and reusing avoids redundant Vec allocations per leaf.
            let drawing_points_cache: std::collections::HashMap<
                mlc_core::ChartId,
                Vec<(u64, Vec<(f64, f64)>, mlc_core::alert::DrawingExtendMode)>,
            > = {
                let drawings_guard = self.core.drawings();
                self.panel_app.panel_grid.containers()
                    .iter()
                    .map(|(&chart_id, w)| {
                        let bars_vec: Vec<mlc_core::Bar> = self.core.bubbles().bars(w.current_tag_id).to_vec();
                        let bars = &bars_vec[..];
                        let prims_iter = drawings_guard.primitives(w.current_tag_id);
                        let pts: Vec<(u64, Vec<(f64, f64)>, mlc_core::alert::DrawingExtendMode)> = prims_iter
                            .iter()
                            .map(|p| {
                                let pts_bar: Vec<(f64, f64)> = p.points()
                                    .into_iter()
                                    .map(|(ts_ms, price)| (mlc_core::timestamp_ms_to_bar_f64(bars, ts_ms), price))
                                    .collect();
                                (p.data().id, pts_bar, mlc_core::alert::DrawingExtendMode::from_u8(p.extend_mode_raw()))
                            })
                            .collect();
                        (chart_id, pts)
                    })
                    .collect()
            };

            let mut sub_pane_writebacks: Vec<(mlc_core::LeafId, Vec<(usize, f64, f64)>)> = Vec::new();
            let mut overlay_writebacks: Vec<(mlc_core::LeafId, Vec<mlc_core::SubPaneOverlayResult>)> = Vec::new();
            // Collect rendered scale-corner zones per leaf so handle_canvas_click
            // can hit-test them in split mode (avoids the empty-default bug).
            let mut split_corner_zones: Vec<(mlc_core::LeafId, ScaleCornerHitZones)> = Vec::new();

            // Hold the indicator manager read guard for the duration of the
            // split-render loop so `&self.indicator_manager` borrows stay alive.
            let indicator_guard_split = self.core.indicators();

            for (leaf_id, sub_rect) in leaf_rects {
                let window = match self.panel_app.panel_grid.container_for_leaf(leaf_id) {
                    Some(w) => w,
                    None => continue,
                };
                // S2.7.1: read instrument fields via branch_meta.
                // branch_meta and panel_app.panel_grid are disjoint fields — split borrow OK.
                let leaf_chart_id = self.panel_app.panel_grid.container_id_for_leaf(leaf_id);
                let split_symbol: String = leaf_chart_id
                    .and_then(|cid| self.symbol_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let split_exchange: String = leaf_chart_id
                    .and_then(|cid| self.exchange_for_chart(cid))
                    .map(String::from)
                    .unwrap_or_default();
                let split_account_type: String = leaf_chart_id
                    .and_then(|cid| self.account_type_for_chart(cid))
                    .map(|at| at.to_string())
                    .unwrap_or_default();
                let split_tf = leaf_chart_id
                    .and_then(|cid| self.timeframe_for_chart(cid));
                let split_tf_name: String = split_tf.as_ref().map(|tf| tf.name.clone()).unwrap_or_default();
                // Countdown gate: see `countdown_interval_ms_for_chart` doc.
                let split_tf_interval_ms: Option<i64> = leaf_chart_id
                    .and_then(|cid| self.countdown_interval_ms_for_chart(cid));
                // Overlay tab legend uses the resolution-aware label (TF name for
                // Time/Interval, formatted param for Param types) — kept distinct
                // from `split_tf_name` above, which stays the raw TF name feeding
                // `render_state_for`'s time-scale formatting.
                let split_tab_label: String = leaf_chart_id
                    .map(|cid| self.resolution_label_for_chart(cid))
                    .unwrap_or_default();

                // Convert sub_rect to absolute LayoutRect.
                let leaf_rect = LayoutRect {
                    x: content_rect.x + sub_rect.x as f64,
                    y: content_rect.y + sub_rect.y as f64,
                    width: sub_rect.width as f64,
                    height: sub_rect.height as f64,
                };

                use mlc_core::chart::ChartRect;
                let chart_rect = ChartRect::new(
                    leaf_rect.x, leaf_rect.y,
                    leaf_rect.width, leaf_rect.height,
                );
                // Phase 4 C: get panel state from panels_store.
                let split_pid = window.chart_panel_id;
                let split_default_panel = crate::chart_panel_state::ChartPanelState::new(split_pid);
                let split_panel = self.panels_store.chart_panel
                    .get(&split_pid)
                    .unwrap_or(&split_default_panel);
                let scale_corner_state = window.to_corner_state();
                // Audit D8: resolve BEFORE taking the bubbles() read guard below.
                let split_ts_shadow = leaf_chart_id.and_then(|cid| self.ts_shadow_for_chart(cid));
                let split_bubble_mgr = self.core.bubbles();
                let split_chart_type = split_bubble_mgr.chart_type(window.current_tag_id).unwrap_or("candles");
                let split_trade_archive_available = self.trade_archive_available_for(&split_bubble_mgr, window.current_tag_id);
                let split_liq_ink = self.liq_ink(window.current_tag_id);
                let split_dom = self.dom_heatmap_ink(window.current_tag_id);
                let split_dom_active = self.dom_overlay_active(window.current_tag_id);
                let split_liq_active = self.liq_overlay_active(window.current_tag_id);
                let split_window_inputs = mlc_backend::bubble::ChartWindowRenderInputs {
                    viewport: window.viewport(),
                    price_scale: window.price_scale(),
                    time_scale: &window.time_scale,
                    crosshair: &window.crosshair,
                    scale_settings: &window.scale_settings,
                    ha_cache: window.ha_cache.as_deref(),
                    vp_overlay: if self.overlay_instance_active(window.current_tag_id, Self::VOLUME_PROFILE_TYPE_ID) { window.vp_overlay.as_ref() } else { None },
                    tpo_profile_overlay: if self.overlay_instance_active(window.current_tag_id, Self::TPO_PROFILE_TYPE_ID) { window.tpo_profile_overlay.as_ref() } else { None },
                    vp_ink: self.profile_ink(window.current_tag_id, Self::VOLUME_PROFILE_TYPE_ID),
                    tpo_ink: self.profile_ink(window.current_tag_id, Self::TPO_PROFILE_TYPE_ID),
                    oi_show: self.overlay_instance_active(window.current_tag_id, Self::OI_DELTA_TYPE_ID),
                    funding_show: self.overlay_instance_active(window.current_tag_id, Self::FUNDING_RATE_TYPE_ID),
                    dom_heatmap_show: split_dom_active,
                    dom_params: split_dom,
                    liq_heatmap_show: split_liq_active,
                    liq_ink: split_liq_ink,
                    liq_projection_show: self.liq_projection_active(window.current_tag_id),
                    liq_projection_ink: self.liq_projection_ink(window.current_tag_id),
                    footprint_display: split_panel.footprint_display,
                    tick_intensity: split_panel.tick_intensity,
                    tpo_display: split_panel.tpo_display,
                    line_family_display: split_panel.line_family_display,
                    wicks_effective: crate::active_series::effective_wick_flag(
                        split_chart_type,
                        self.panel_app.chart_settings_state.instrument_wick_enabled,
                        split_panel.bar_family_wicks,
                    ),
                    trade_archive_available: split_trade_archive_available,
                };
                let mut render_state = match split_bubble_mgr.render_state_for(
                    window.current_tag_id,
                    split_window_inputs,
                    chart_rect,
                    &chart_theme,
                    Some(split_tf_name.as_str()),
                    Some(&window.scale_settings.time_format),
                    split_tf_interval_ms,
                    &split_panel.grid_options,
                    &split_panel.legend,
                ) {
                    Some(rs) => rs,
                    None => continue,  // window not yet attached to a bubble
                };
                if let Some(shadow) = split_ts_shadow.as_deref() {
                    render_state.bars = shadow;
                    render_state.non_temporal_axis = true;
                }
                render_state.hide_time_grid = crate::active_series::chart_type_hides_time_grid(split_chart_type);
                render_state.cvd_paint_deltas = self.cvd_paint_deltas_for(window.current_tag_id, split_chart_type);
                let crosshair_config = if self.pending_alert_screenshot {
                    mlc_core::chart::CrosshairConfig { vert_visible: false, horz_visible: false, ..Default::default() }
                } else {
                    mlc_core::chart::CrosshairConfig::default()
                };
                let render_config = ChartRenderConfig {
                    scale_theme: self.panel_app.scale_theme_for_render(),
                    chart_type: split_chart_type,
                    crosshair_config,
                    ..ChartRenderConfig::default()
                };

                let alert_current_bar = 0usize.saturating_sub(1) as f64;
                // Look up pre-built drawing points from cache (computed once per
                // chart window before this loop to avoid per-leaf reallocation).
                let empty_pts: Vec<(u64, Vec<(f64, f64)>, mlc_core::alert::DrawingExtendMode)> = Vec::new();
                let chart_id_for_cache = leaf_chart_id;
                let alert_drawing_points: &Vec<(u64, Vec<(f64, f64)>, mlc_core::alert::DrawingExtendMode)> =
                    chart_id_for_cache
                        .and_then(|cid| drawing_points_cache.get(&cid))
                        .unwrap_or(&empty_pts);
                // Reuse the indicator values computed once before this loop.
                let alert_indicator_values = &alert_indicator_values_cache;
                let alert_render_data: Vec<AlertRenderData> = all_alerts_for_render.iter()
                    .filter(|a| a.status == mlc_core::alert::AlertStatus::Active)
                    .filter(|a| matches!(a.source, mlc_core::alert::AlertSource::Price { .. }))
                    .filter(|a| a.matches_window(&split_symbol, &split_exchange, &split_account_type))
                    .filter_map(|alert| {
                        let price = mlc_backend::alert::AlertManager::resolve_price_static(
                            alert,
                            alert_current_bar,
                            alert_drawing_points,
                            alert_indicator_values,
                        )?;
                        Some(AlertRenderData {
                            price,
                            status: AlertRenderStatus::Active,
                        })
                    })
                    .collect();

                let inner_proportions_vec = crate::chart_container::collect_inner_proportions(&window.inner_docking);
                let inner_sub_ids_vec = crate::chart_container::collect_inner_instance_ids(&window.inner_docking);
                let split_drawings_guard = self.core.drawings();
                let split_view = split_drawings_guard.with_bubble(window.current_tag_id);
                let split_indicator_view = indicator_guard_split.with_bubble(window.current_tag_id);
                let panel_data = ChartPanelRenderData {
                    state: &render_state,
                    config: &render_config,
                    corner_state: &scale_corner_state,
                    drawing_manager: Some(&split_view as &dyn mlc_core::drawing::DrawingRenderView),
                    indicator_source: Some(&split_indicator_view as &dyn IndicatorSource),
                    symbol: Some(split_symbol.as_str()),
                    sub_pane_y_state: Some(&window.sub_pane_y_state),
                    compare_overlay: Some(&window.compare_overlay),
                    watermark: split_panel.watermark.as_ref(),
                    tooltip: Some(&split_panel.tooltip),
                    alert_render_data: &alert_render_data,
                    scale_settings: &window.scale_settings,
                    selected_indicator_id: self.selected_indicator_id,
                    frame_theme: &frame_theme,
                    sub_pane_overlay_states: &window.sub_pane_overlay_states,
                    toolbar_config: &no_toolbar,
                    is_split: true,
                    inner_proportions: &inner_proportions_vec,
                    inner_sub_instance_ids: &inner_sub_ids_vec,
                    data_error: window.data_error.as_deref(),
                    maximized_sub_pane_id: self.maximized_sub_panes
                        .get(&window.id.0)
                        .map(|(iid, _)| *iid),
                    above_main_ids: Some(&window.sub_pane_above_main),
                };

                let render_result = render_full_chart_panel(ctx, &leaf_rect, &panel_data);
                // Capture exact rendered button rects for this leaf's scale corner.
                split_corner_zones.push((leaf_id, render_result.corner_zones.clone()));
                if !render_result.sub_pane_ranges.is_empty() {
                    sub_pane_writebacks.push((leaf_id, render_result.sub_pane_ranges));
                }
                // Store overlay results for next frame's hit testing.
                overlay_writebacks.push((leaf_id, render_result.sub_pane_overlays));
                if let Some(geom) = render_result.loading_spinner {
                    out_loading_spinners.push((leaf_id, geom));
                }

                // Crosshair + its price/time axis cursor labels — the LOWEST
                // cursor-dependent overlay layer. Painted right here, above
                // this leaf's static content and BELOW the toolbar/dropdown
                // draw further down, so an open dropdown (e.g. the chart-type
                // selector) always paints above it (owner-reported bug, fixed
                // 2026-07-25 — it used to bleed through because the crosshair
                // was only drawn later, in the separate cursor-overlay pass
                // that ran after the toolbar). See `render_chart_crosshair_layer`.
                if let Some(crosshair_layout) = self.frame_layouts.get(&leaf_id) {
                    let crosshair_render_config = ChartRenderConfig {
                        scale_theme: self.panel_app.scale_theme_for_render(),
                        chart_type: split_chart_type,
                        crosshair_config: mlc_core::chart::CrosshairConfig::default(),
                        ..ChartRenderConfig::default()
                    };
                    render_chart_crosshair_layer(
                        ctx,
                        &render_state,
                        crosshair_layout,
                        &crosshair_render_config,
                        None, // sub_pane_ranges — unset, matching the pre-split behavior
                    );
                }

                // Post-render: draw bell icons for alerts bound to drawing
                // primitives and overlay indicators in this leaf.
                let main_chart_y;
                let leaf_tab_visible;
                {
                    // Sub-pane order + proportions both come from the
                    // container's own InnerLayout -- single source of truth.
                    // indicator_manager visibility is a filter applied on top.
                    // Indicator ownership is by bubble — `window.current_tag_id`
                    // is this leaf's bubble regardless of whether leaf_chart_id
                    // resolved (S2.7.1).
                    let visible_set: std::collections::HashSet<u64> = self.core.indicators()
                        .instances_for_bubble(window.current_tag_id)
                        .into_iter()
                        .filter(|i| i.visible && i.pane > 0)
                        .map(|i| i.id)
                        .collect();
                    let all_ids = crate::chart_container::collect_inner_instance_ids(&window.inner_docking);
                    let all_props = crate::chart_container::collect_inner_proportions(&window.inner_docking);
                    let (sub_pane_ids, sub_proportions): (Vec<u64>, Vec<f64>) = all_ids
                        .iter()
                        .enumerate()
                        .filter(|(_, id)| visible_set.contains(id))
                        .map(|(i, &id)| (id, all_props.get(i + 1).copied().unwrap_or(0.0)))
                        .unzip();
                    // Mirror render_full_chart_panel's maximize override: when a
                    // sub-pane is maximized it covers the whole container, the
                    // main chart is zero-height, and chart-anchored overlays
                    // (leaf tab, alert bells) must not poke through.
                    let maximized_iid = leaf_chart_id
                        .and_then(|cid| self.maximized_sub_panes.get(&cid.0))
                        .map(|(iid, _)| *iid)
                        .filter(|iid| sub_pane_ids.contains(iid));
                    let extended = if let Some(miid) = maximized_iid {
                        mlc_core::ExtendedFrameLayout::maximized(&leaf_rect, miid, &window.scale_settings)
                    } else {
                        mlc_core::ExtendedFrameLayout::from_proportions(
                            &leaf_rect,
                            &sub_pane_ids,
                            &sub_proportions,
                            all_props.first().copied().filter(|_| all_props.len() > 1),
                            &window.scale_settings,
                            1.0,
                            &window.sub_pane_above_main,
                        )
                    };
                    let main = &extended.main_chart;
                    let chart_area = LayoutRect {
                        x: main.chart.x,
                        y: main.chart.y,
                        width: main.chart.width,
                        height: main.chart.height,
                    };
                    let mut corrected_vp = window.viewport().clone();
                    corrected_vp.chart_width = main.chart.width;
                    corrected_vp.chart_height = main.chart.height;

                    // alert-bell filter is by bubble (indicator ownership)
                    let window_id = leaf_chart_id.and_then(|cid|
                        self.panel_app.panel_grid.containers()
                            .get(&cid).map(|w| w.current_tag_id.0));

                    // S2.7.1: pass split_symbol/exchange/account_type from branch_meta.
                    // Skip chart-anchored bells when the main chart is collapsed
                    // (maximized sub-pane covers the container).
                    if chart_area.height >= 1.0 {
                        let split_bars_vec: Vec<mlc_core::Bar> = self.core.bubbles().bars(window.current_tag_id).to_vec();
                        let split_bell_drawings_guard = self.core.drawings();
                        let split_bell_prims = split_bell_drawings_guard.primitives(window.current_tag_id);
                        let bells = Self::draw_alert_bell_icons(
                            ctx,
                            chart_area,
                            &corrected_vp,
                            window.price_scale().price_min,
                            window.price_scale().price_max,
                            &split_bars_vec[..],
                            split_bell_prims,
                            &self.core.indicators(),
                            &all_alerts_for_render,
                            window_id,
                            &split_symbol,
                            &split_exchange,
                            &split_account_type,
                        );
                        for (widget_id, bx, by, bsize) in bells {
                            use uzor::input::Sense;
                            let hw = bsize / 2.0 + 2.0;
                            self.input_coordinator.borrow_mut().register(
                                widget_id,
                                uzor::Rect::new(bx - hw, by - hw, hw * 2.0, hw * 2.0),
                                Sense::CLICK,
                            );
                        }
                    }
                    main_chart_y = extended.main_chart.chart.y;
                    leaf_tab_visible = extended.main_chart.chart.height >= 1.0;

                    // chart:price_scale, chart:time_scale, chart:sub_sep, chart:sub_price_scale
                    // registrations removed — chart is now a BlackboxPanel and its
                    // internal hit-tester (ExtendedLayoutHitTester / panel_grid.resolve_input)
                    // handles these interactions without coordinator involvement.
                }

                // Render overlay tab header at the top-left of this leaf.
                let is_active_leaf = self.panel_app.panel_grid.docking()
                    .active_leaf() == Some(leaf_id);
                let hover_zone = if self.leaf_tab_hovered_leaf == Some(leaf_id) {
                    self.leaf_tab_hover
                } else {
                    mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::None
                };
                let color_tag = self.panel_app.leaf_color_tags.get(&leaf_id).copied();
                // S2.7.1: use split_symbol/tf_name/exchange/account_type from branch_meta.
                // Skipped entirely while a maximized sub-pane covers the container
                // (leaf_tab_visible == false): the tab belongs to the chart layer,
                // which sits BELOW the maximized pane in z-order.
                if leaf_tab_visible {
                let hit_zones = mlc_app_layout::layout::panel_overlay::render_leaf_tab(
                    ctx,
                    leaf_rect.x + 2.0,
                    main_chart_y + 2.0,
                    leaf_rect.width - 4.0,
                    &split_symbol,
                    &split_tab_label,
                    &split_exchange,
                    &split_account_type,
                    is_active_leaf,
                    hover_zone,
                    color_tag,
                    &leaf_tab_toolbar_theme,
                    None, // static text — inline editing is Trading-tab only
                );
                let tab_rect = hit_zones.tab_rect;
                let dots_rect = hit_zones.dots_rect;
                let color_tag_rect = hit_zones.color_tag_rect;
                out_leaf_tab_hit_zones.insert(leaf_id, hit_zones);

                // Register overlay tab sub-zones so crosshair hides, cursor is default,
                // and hover state can be derived from InputCoordinator hovered_widget().
                {
                    use uzor::input::Sense;
                    let mut coord = self.input_coordinator.borrow_mut();
                    // Body registered first — sub-zones registered after win overlaps.
                    let [rx, ry, rw, rh] = tab_rect;
                    if rw > 0.0 && rh > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:body", leaf_id.0),
                            uzor::Rect::new(rx, ry, rw, rh),
                            Sense::CLICK,
                        );
                    }
                    let [dx, dy, dw, dh] = dots_rect;
                    if dw > 0.0 && dh > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:gear", leaf_id.0),
                            uzor::Rect::new(dx, dy, dw, dh),
                            Sense::CLICK,
                        );
                    }
                    let [cx, cy, cw, ch] = color_tag_rect;
                    if cw > 0.0 && ch > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:color_tag", leaf_id.0),
                            uzor::Rect::new(cx, cy, cw, ch),
                            Sense::CLICK,
                        );
                    }
                }
                }

                // Render per-leaf indicator overlay (chevron) for this leaf.
                {
                    // Collect compare series info before any mutable borrow of panel_app.
                    let compare_series_info: Vec<(String, bool, String)> = window
                        .compare_overlay
                        .series
                        .iter()
                        .map(|s| (s.symbol.clone(), s.visible, s.color.clone()))
                        .collect();
                    let instances_empty = self.core.indicators()
                        .instances_for_bubble(window.current_tag_id)
                        .is_empty();
                    let _has_indicators = !instances_empty;
                    let _has_compare = !compare_series_info.is_empty();

                    // overlay visibility was set in prepare_frame; read the state here.
                    let overlay_state = self.panel_app
                        .indicator_overlay_states
                        .get(&leaf_id)
                        .cloned()
                        .unwrap_or_default();

                    // leaf_tab_visible: hidden while a maximized sub-pane covers
                    // the container (chevron belongs to the chart layer).
                    if leaf_tab_visible && (overlay_state.visible || overlay_state.is_open) {
                        let mut indicators: Vec<IndicatorOverlayInfo> = {
                            let im = self.core.indicators();
                            let instances = im.instances_for_bubble(window.current_tag_id);
                            // The chart legend lists CHART indicators only —
                            // the ones painting on the price plot. A sub-pane
                            // indicator is captioned inside its own pane and
                            // reached from the object tree; listing it here
                            // too was a duplicate that also let the chevron
                            // act on a row that does not live on the chart.
                            instances
                                .iter()
                                .filter(|inst| inst.pane == 0)
                                .map(|inst| {
                                    let display_name = Self::format_indicator_display_name(
                                        &im, inst,
                                    );
                                    IndicatorOverlayInfo {
                                        id: inst.id,
                                        display_name,
                                        visible: inst.visible,
                                        is_compare: false,
                                        symbol: None,
                                        color: None,
                                    }
                                })
                                .collect()
                        };

                        for (sym, vis, col) in &compare_series_info {
                            indicators.push(IndicatorOverlayInfo {
                                id: 0,
                                display_name: sym.clone(),
                                visible: *vis,
                                is_compare: true,
                                symbol: Some(sym.clone()),
                                color: Some(col.clone()),
                            });
                        }

                        if !indicators.is_empty() {
                            let toolbar_theme_for_overlay = self.panel_app.toolbar_theme_for_render();
                            let overlay_rect = uzor::types::Rect::new(
                                leaf_rect.x,
                                main_chart_y,
                                leaf_rect.width,
                                leaf_rect.height,
                            );
                            let overlay_result = render_indicator_overlay(
                                ctx,
                                &overlay_rect,
                                &indicators,
                                &overlay_state,
                                &frame_theme,
                                &toolbar_theme_for_overlay,
                            );

                            // Register indicator overlay hit zones for this leaf.
                            {
                                use uzor::input::Sense;
                                use mlc_app_layout::ui::z_order::ZLayer;

                                let prefix = format!("ind_overlay:leaf{}:", leaf_id.0);
                                let cmp_prefix = format!("cmp_overlay:leaf{}:", leaf_id.0);
                                let ov_layer = ZLayer::Toolbar.push_named(
                                    &mut self.input_coordinator.borrow_mut(),
                                    &format!("ind_overlay_leaf{}", leaf_id.0),
                                );

                                let br = &overlay_result.button_rect;
                                if br.width > 0.0 {
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}toggle", prefix),
                                        uzor::Rect::new(br.x, br.y, br.width, br.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                }

                                if let Some(ref close_rect) = overlay_result.close_button_rect {
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}close", prefix),
                                        uzor::Rect::new(close_rect.x, close_rect.y, close_rect.width, close_rect.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                }

                                for row in &overlay_result.indicator_rows {
                                    let id = row.instance_id;
                                    // Compare entries use cmp_overlay:leaf{N}: prefix;
                                    // regular indicators use ind_overlay:leaf{N}: prefix.
                                    let row_prefix: &str = if row.is_compare { &cmp_prefix } else { &prefix };
                                    let rr = &row.row_rect;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}row:{}", row_prefix, id),
                                        uzor::Rect::new(rr.x, rr.y, rr.width, rr.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                    let ar = &row.alert_btn;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}alert:{}", row_prefix, id),
                                        uzor::Rect::new(ar.x, ar.y, ar.width, ar.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                    let vr = &row.visibility_btn;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}vis:{}", row_prefix, id),
                                        uzor::Rect::new(vr.x, vr.y, vr.width, vr.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                    let sr = &row.settings_btn;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}settings:{}", row_prefix, id),
                                        uzor::Rect::new(sr.x, sr.y, sr.width, sr.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                    let dr = &row.delete_btn;
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}delete:{}", row_prefix, id),
                                        uzor::Rect::new(dr.x, dr.y, dr.width, dr.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                }

                                self.input_coordinator.borrow_mut().pop_layer(&ov_layer);
                            }
                        }
                    }
                }
            }

            // Flatten multi-panel sub-pane writebacks into the output list.
            for (leaf_id, ranges) in sub_pane_writebacks {
                for (pane_idx, min, max) in ranges {
                    out_sub_pane_range_writebacks.push((leaf_id, pane_idx, min, max));
                }
            }

            // Collect multi-panel overlay writebacks.
            out_sub_pane_overlay_writebacks.extend(overlay_writebacks);

            // Render separators between split leaves.
            // thickness_for_state() returns 2.0 for idle, 4.0 for hover/dragging.
            let separators: Vec<_> = self.panel_app.panel_grid.docking()
                .separators()
                .iter()
                .map(|sep| {
                    let thickness = sep.thickness_for_state();
                    (sep.orientation, sep.position, sep.start, sep.length, thickness)
                })
                .collect();

            use mlc_core::SeparatorOrientation;
            for (_idx, (orientation, position, start, length, thickness)) in separators.iter().copied().enumerate() {
                let color: &str = &frame_theme.toolbar_border;
                // Convert content-relative separator coords to absolute screen coords.
                let (rect_x, rect_y, rect_w, rect_h) = match orientation {
                    SeparatorOrientation::Vertical => {
                        // position = x center, start = y begin, length = y extent
                        let abs_x = content_rect.x + (position - thickness / 2.0) as f64;
                        let abs_y = content_rect.y + start as f64;
                        (abs_x, abs_y, thickness as f64, length as f64)
                    }
                    SeparatorOrientation::Horizontal => {
                        // position = y center, start = x begin, length = x extent
                        let abs_x = content_rect.x + start as f64;
                        let abs_y = content_rect.y + (position - thickness / 2.0) as f64;
                        (abs_x, abs_y, length as f64, thickness as f64)
                    }
                };
                ctx.set_fill_color(color);
                ctx.fill_rect(rect_x, rect_y, rect_w, rect_h);
            }

            split_corner_zones.into_iter().collect::<std::collections::HashMap<_, _>>()
        } else {
            let scale_corner_state = self.panel_app.panel_grid.active_container()
                .map(|w| w.to_corner_state())
                .unwrap_or_default();
            let active_chart_type = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.chart_type_for_chart(cid))
                .unwrap_or("candles");
            let crosshair_config = if self.pending_alert_screenshot {
                mlc_core::chart::CrosshairConfig { vert_visible: false, horz_visible: false, ..Default::default() }
            } else {
                mlc_core::chart::CrosshairConfig::default()
            };
            let render_config = ChartRenderConfig {
                scale_theme: self.panel_app.scale_theme_for_render(),
                chart_type: active_chart_type,
                crosshair_config,
                ..ChartRenderConfig::default()
            };

            let chart_theme = self.panel_app.chart_theme_for_render();

            // S2.7.1: snapshot symbol/timeframe/exchange/account_type from branch_meta.
            let single_window_info: Option<(String, String, String, String)> = {
                let cid = self.panel_app.panel_grid.active_container_id();
                cid.map(|chart_id| {
                    let sym = self.symbol_for_chart(chart_id)
                        .map(String::from).unwrap_or_default();
                    let tf_name = self.timeframe_for_chart(chart_id)
                        .map(|tf| tf.name.clone())
                        .unwrap_or_default();
                    let exch = self.exchange_for_chart(chart_id)
                        .map(String::from).unwrap_or_default();
                    let at = self.account_type_for_chart(chart_id)
                        .map(|at| at.to_string()).unwrap_or_default();
                    (sym, tf_name, exch, at)
                })
            };
            // Countdown gate: see `countdown_interval_ms_for_chart` doc.
            let single_tf_interval_ms: Option<i64> = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.countdown_interval_ms_for_chart(cid));

            // Hold the indicator manager read guard for the duration of the
            // single-window render path so `&self.indicator_manager` borrows stay alive.
            let indicator_guard_single = self.core.indicators();

            // Group primitive sync was done in prepare_frame; use the already-synced drawing_manager here.

            let (single_alert_current_bar, single_alert_drawing_points) = if let Some(window) = self.panel_app.panel_grid.active_container() {
                let single_bars_vec: Vec<mlc_core::Bar> = self.core.bubbles().bars(window.current_tag_id).to_vec();
                let cb = single_bars_vec.len().saturating_sub(1) as f64;
                let bars = &single_bars_vec[..];
                let single_drawings_alert_guard = self.core.drawings();
                let prims2_iter = single_drawings_alert_guard.primitives(window.current_tag_id);
                let pts: Vec<(u64, Vec<(f64, f64)>, mlc_core::alert::DrawingExtendMode)> = prims2_iter
                    .iter()
                    .map(|p| {
                        let pts_bar: Vec<(f64, f64)> = p.points()
                            .into_iter()
                            .map(|(ts_ms, price)| (mlc_core::timestamp_ms_to_bar_f64(bars, ts_ms), price))
                            .collect();
                        (p.data().id, pts_bar, mlc_core::alert::DrawingExtendMode::from_u8(p.extend_mode_raw()))
                    })
                    .collect();
                (cb, pts)
            } else {
                (0.0, Vec::new())
            };
            let single_all_alerts: Vec<mlc_core::alert::AlertItem> = self.core.alerts().items_all();
            let single_alert_indicator_values = Self::build_indicator_values_for_alerts(
                &single_all_alerts,
                &self.core.indicators(),
            );
            let single_alert_render_data: Vec<AlertRenderData> = {
                let (single_sym, _, single_exch, single_at) = single_window_info
                    .as_ref()
                    .map(|(s, tf, e, at)| (s.as_str(), tf.as_str(), e.as_str(), at.as_str()))
                    .unwrap_or(("", "", "", ""));
                single_all_alerts.iter()
                    .filter(|a| a.status == mlc_core::alert::AlertStatus::Active)
                    .filter(|a| matches!(a.source, mlc_core::alert::AlertSource::Price { .. }))
                    .filter(|a| a.matches_window(single_sym, single_exch, single_at))
                    .filter_map(|alert| {
                        let price = mlc_backend::alert::AlertManager::resolve_price_static(
                            alert,
                            single_alert_current_bar,
                            &single_alert_drawing_points,
                            &single_alert_indicator_values,
                        )?;
                        Some(AlertRenderData {
                            price,
                            status: AlertRenderStatus::Active,
                        })
                    })
                    .collect()
            };

            let mut single_sub_pane_ranges: Vec<(usize, f64, f64)> = Vec::new();
            let mut single_sub_pane_overlays: Vec<mlc_core::SubPaneOverlayResult> = Vec::new();
            let mut single_loading_spinner: Option<(f64, f64, f64)> = None;

            // S2.7.1: extract single-window instrument values from branch_meta tuple.
            let (single_sym, single_tf_name, single_exch, single_at) = single_window_info
                .as_ref()
                .map(|(s, tf, e, at)| (s.as_str(), tf.as_str(), e.as_str(), at.as_str()))
                .unwrap_or(("", "", "", ""));

            // Phase 4 C: get single-window panel state before borrowing window.
            let single_window_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
            let single_fallback_panel = crate::chart_panel_state::ChartPanelState::new(
                single_window_pid.unwrap_or(mlc_core::state::ChartPanelId(0))
            );
            let single_window_panel: &crate::chart_panel_state::ChartPanelState = single_window_pid
                .and_then(|pid| self.panels_store.chart_panel.get(&pid))
                .unwrap_or(&single_fallback_panel);
            let window_opt = self.panel_app.panel_grid.active_container();
            // Audit D8: resolve BEFORE taking the bubbles() read guard below.
            let single_window_ts_shadow = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.ts_shadow_for_chart(cid));
            let single_window_bubble_mgr = self.core.bubbles();
            let single_window_render_state = window_opt.and_then(|window| {
                use mlc_core::chart::ChartRect;
                let chart_rect = ChartRect::new(
                    content_rect.x,
                    content_rect.y,
                    content_rect.width,
                    content_rect.height,
                );
                let single_window_chart_type_for_wicks = single_window_bubble_mgr.chart_type(window.current_tag_id).unwrap_or("candles");
                let single_window_trade_archive_available = self.trade_archive_available_for(&single_window_bubble_mgr, window.current_tag_id);
                let single_window_liq_ink = self.liq_ink(window.current_tag_id);
                let single_window_dom = self.dom_heatmap_ink(window.current_tag_id);
                let single_window_dom_active = self.dom_overlay_active(window.current_tag_id);
                let single_window_liq_active = self.liq_overlay_active(window.current_tag_id);
                let single_window_inputs = mlc_backend::bubble::ChartWindowRenderInputs {
                    viewport: window.viewport(),
                    price_scale: window.price_scale(),
                    time_scale: &window.time_scale,
                    crosshair: &window.crosshair,
                    scale_settings: &window.scale_settings,
                    ha_cache: window.ha_cache.as_deref(),
                    vp_overlay: if self.overlay_instance_active(window.current_tag_id, Self::VOLUME_PROFILE_TYPE_ID) { window.vp_overlay.as_ref() } else { None },
                    tpo_profile_overlay: if self.overlay_instance_active(window.current_tag_id, Self::TPO_PROFILE_TYPE_ID) { window.tpo_profile_overlay.as_ref() } else { None },
                    vp_ink: self.profile_ink(window.current_tag_id, Self::VOLUME_PROFILE_TYPE_ID),
                    tpo_ink: self.profile_ink(window.current_tag_id, Self::TPO_PROFILE_TYPE_ID),
                    oi_show: self.overlay_instance_active(window.current_tag_id, Self::OI_DELTA_TYPE_ID),
                    funding_show: self.overlay_instance_active(window.current_tag_id, Self::FUNDING_RATE_TYPE_ID),
                    dom_heatmap_show: single_window_dom_active,
                    dom_params: single_window_dom,
                    liq_heatmap_show: single_window_liq_active,
                    liq_ink: single_window_liq_ink,
                    liq_projection_show: self.liq_projection_active(window.current_tag_id),
                    liq_projection_ink: self.liq_projection_ink(window.current_tag_id),
                    footprint_display: single_window_panel.footprint_display,
                    tick_intensity: single_window_panel.tick_intensity,
                    tpo_display: single_window_panel.tpo_display,
                    line_family_display: single_window_panel.line_family_display,
                    wicks_effective: crate::active_series::effective_wick_flag(
                        single_window_chart_type_for_wicks,
                        self.panel_app.chart_settings_state.instrument_wick_enabled,
                        single_window_panel.bar_family_wicks,
                    ),
                    trade_archive_available: single_window_trade_archive_available,
                };
                single_window_bubble_mgr.render_state_for(
                    window.current_tag_id,
                    single_window_inputs,
                    chart_rect,
                    &chart_theme,
                    Some(single_tf_name),
                    Some(&window.scale_settings.time_format),
                    single_tf_interval_ms,
                    &single_window_panel.grid_options,
                    &single_window_panel.legend,
                )
            });
            let corner_zones_single = if let (Some(window), Some(mut render_state)) = (window_opt, single_window_render_state) {
                if let Some(shadow) = single_window_ts_shadow.as_deref() {
                    render_state.bars = shadow;
                    render_state.non_temporal_axis = true;
                }
                let single_window_chart_type = single_window_bubble_mgr.chart_type(window.current_tag_id).unwrap_or("candles");
                render_state.hide_time_grid = crate::active_series::chart_type_hides_time_grid(single_window_chart_type);
                render_state.cvd_paint_deltas = self.cvd_paint_deltas_for(window.current_tag_id, single_window_chart_type);
                let inner_proportions_vec = crate::chart_container::collect_inner_proportions(&window.inner_docking);
                let inner_sub_ids_vec = crate::chart_container::collect_inner_instance_ids(&window.inner_docking);
                let single_window_drawings_guard = self.core.drawings();
                let single_window_view = single_window_drawings_guard.with_bubble(window.current_tag_id);
                let single_indicator_view = indicator_guard_single.with_bubble(window.current_tag_id);
                let panel_data = ChartPanelRenderData {
                    state: &render_state,
                    config: &render_config,
                    corner_state: &scale_corner_state,
                    drawing_manager: Some(&single_window_view as &dyn mlc_core::drawing::DrawingRenderView),
                    indicator_source: Some(&single_indicator_view as &dyn IndicatorSource),
                    symbol: Some(single_sym),
                    sub_pane_y_state: Some(&window.sub_pane_y_state),
                    compare_overlay: Some(&window.compare_overlay),
                    watermark: single_window_panel.watermark.as_ref(),
                    tooltip: Some(&single_window_panel.tooltip),
                    alert_render_data: &single_alert_render_data,
                    scale_settings: &window.scale_settings,
                    selected_indicator_id: self.selected_indicator_id,
                    frame_theme: &frame_theme,
                    sub_pane_overlay_states: &window.sub_pane_overlay_states,
                    toolbar_config: &self.panel_app.toolbar_config,
                    is_split: false,
                    inner_proportions: &inner_proportions_vec,
                    inner_sub_instance_ids: &inner_sub_ids_vec,
                    data_error: window.data_error.as_deref(),
                    maximized_sub_pane_id: self.maximized_sub_panes
                        .get(&window.id.0)
                        .map(|(iid, _)| *iid),
                    above_main_ids: Some(&window.sub_pane_above_main),
                };

                // Render chart into content_rect (shrunk by sidebar).
                // Use content_rect as the full rect with minimal toolbar config
                // so render_full_chart_panel treats the entire rect as chart area
                // (toolbars are rendered separately via render_toolbars_with_theme).
                let chart_render_rect = content_rect;
                let mut chart_panel_data = panel_data;
                let no_toolbar = mlc_core::ToolbarConfig::minimal();
                chart_panel_data.toolbar_config = &no_toolbar;
                let render_result = render_full_chart_panel(ctx, &chart_render_rect, &chart_panel_data);
                // Stash ranges and overlay results for writeback after the immutable borrow on `window` ends.
                single_sub_pane_ranges = render_result.sub_pane_ranges;
                single_sub_pane_overlays = render_result.sub_pane_overlays;
                single_loading_spinner = render_result.loading_spinner;
                let corner_zones_ret = render_result.corner_zones;

                // Crosshair + its price/time axis cursor labels — the LOWEST
                // cursor-dependent overlay layer. Painted right here, above
                // the static content and BELOW the toolbar/dropdown draw
                // further down, so an open dropdown (e.g. the chart-type
                // selector) always paints above it (owner-reported bug, fixed
                // 2026-07-25). See `render_chart_crosshair_layer`.
                if let Some(crosshair_layout) = self.active_frame_layout.as_ref() {
                    let crosshair_render_config = ChartRenderConfig {
                        scale_theme: self.panel_app.scale_theme_for_render(),
                        chart_type: single_window_chart_type,
                        crosshair_config: mlc_core::chart::CrosshairConfig::default(),
                        ..ChartRenderConfig::default()
                    };
                    render_chart_crosshair_layer(
                        ctx,
                        &render_state,
                        crosshair_layout,
                        &crosshair_render_config,
                        None, // sub_pane_ranges — unset, matching the pre-split behavior
                    );
                }

                // Post-render: draw bell icons for alerts bound to this window's
                // drawing primitives and overlay indicators.
                // main_chart_y is updated once ExtendedFrameLayout is computed.
                {
                    // Compute the corrected chart area rect (same logic as
                    // render_full_chart_panel's internal ExtendedFrameLayout).
                    // Indicator ownership is by bubble — `window.current_tag_id`
                    // is this window's bubble.
                    let visible_set: std::collections::HashSet<u64> = self.core.indicators()
                        .instances_for_bubble(window.current_tag_id)
                        .into_iter()
                        .filter(|i| i.visible && i.pane > 0)
                        .map(|i| i.id)
                        .collect();
                    let all_ids2 = crate::chart_container::collect_inner_instance_ids(&window.inner_docking);
                    let all_props2 = crate::chart_container::collect_inner_proportions(&window.inner_docking);
                    let (sub_pane_ids, sub_proportions): (Vec<u64>, Vec<f64>) = all_ids2
                        .iter()
                        .enumerate()
                        .filter(|(_, id)| visible_set.contains(id))
                        .map(|(i, &id)| (id, all_props2.get(i + 1).copied().unwrap_or(0.0)))
                        .unzip();
                    // Mirror render_full_chart_panel's maximize override (see the
                    // split-mode twin above): chart-anchored overlays must not
                    // poke through a maximized sub-pane.
                    let maximized_iid = self.panel_app.panel_grid.active_container_id()
                        .and_then(|cid| self.maximized_sub_panes.get(&cid.0))
                        .map(|(iid, _)| *iid)
                        .filter(|iid| sub_pane_ids.contains(iid));
                    let extended = if let Some(miid) = maximized_iid {
                        mlc_core::ExtendedFrameLayout::maximized(&chart_render_rect, miid, &window.scale_settings)
                    } else {
                        mlc_core::ExtendedFrameLayout::from_proportions(
                            &chart_render_rect,
                            &sub_pane_ids,
                            &sub_proportions,
                            all_props2.first().copied().filter(|_| all_props2.len() > 1),
                            &window.scale_settings,
                            1.0,
                            &window.sub_pane_above_main,
                        )
                    };
                    let main = &extended.main_chart;
                    let chart_area = LayoutRect {
                        x: main.chart.x,
                        y: main.chart.y,
                        width: main.chart.width,
                        height: main.chart.height,
                    };
                    // Corrected viewport matching main chart area dimensions.
                    let mut corrected_vp = window.viewport().clone();
                    corrected_vp.chart_width = main.chart.width;
                    corrected_vp.chart_height = main.chart.height;

                    // alert-bell filter is by bubble
                    let window_id = Some(window.current_tag_id.0);

                    // S2.7.1: use single_sym/exch/at from branch_meta.
                    // Skip chart-anchored bells when the main chart is collapsed
                    // (maximized sub-pane covers the container).
                    if chart_area.height >= 1.0 {
                        let single_bars_vec2: Vec<mlc_core::Bar> = self.core.bubbles().bars(window.current_tag_id).to_vec();
                        let single_bell_drawings_guard = self.core.drawings();
                        let single_bell_prims = single_bell_drawings_guard.primitives(window.current_tag_id);
                        let bells = Self::draw_alert_bell_icons(
                            ctx,
                            chart_area,
                            &corrected_vp,
                            window.price_scale().price_min,
                            window.price_scale().price_max,
                            &single_bars_vec2[..],
                            single_bell_prims,
                            &self.core.indicators(),
                            &single_all_alerts,
                            window_id,
                            single_sym,
                            single_exch,
                            single_at,
                        );
                        // Register bell click zones.
                        for (widget_id, bx, by, bsize) in bells {
                            use uzor::input::Sense;
                            let hw = bsize / 2.0 + 2.0; // a little larger than the icon
                            self.input_coordinator.borrow_mut().register(
                                widget_id,
                                uzor::Rect::new(bx - hw, by - hw, hw * 2.0, hw * 2.0),
                                Sense::CLICK,
                            );
                        }
                    }
                    main_chart_y_single = extended.main_chart.chart.y;
                    single_leaf_tab_visible = extended.main_chart.chart.height >= 1.0;
                }

                corner_zones_ret
            } else {
                main_chart_y_single = content_rect.y;
                ScaleCornerHitZones::default()
            };

            // Register single chart pane as BlackboxPanel — see split-mode comment above.
            if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                use uzor::input::{Sense, WidgetKind};
                use mlc_app_layout::ui::z_order::ZLayer;
                let chart_layer = ZLayer::ChartCanvas.push(&mut self.input_coordinator.borrow_mut());
                self.input_coordinator.borrow_mut().register_composite(
                    format!("chart:pane:{}", active_leaf.0),
                    WidgetKind::BlackboxPanel,
                    uzor::Rect::new(
                        content_rect.x,
                        content_rect.y,
                        content_rect.width,
                        content_rect.height,
                    ),
                    Sense::CLICK
                        | Sense::DRAG
                        | Sense::SCROLL
                        | Sense::RIGHT_CLICK
                        | Sense::DOUBLE_CLICK
                        | Sense::HOVER,
                    &chart_layer,
                );
            }

            // Collect single-window sub-pane ranges for writeback via RenderOutput.
            if !single_sub_pane_ranges.is_empty() {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(mlc_core::LeafId(0));
                for (pane_idx, min, max) in single_sub_pane_ranges {
                    out_sub_pane_range_writebacks.push((single_leaf_id, pane_idx, min, max));
                }
            }

            // Collect single-window overlay results for writeback via RenderOutput.
            {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(mlc_core::LeafId(0));
                out_sub_pane_overlay_writebacks.push((single_leaf_id, single_sub_pane_overlays));
                if let Some(geom) = single_loading_spinner {
                    out_loading_spinners.push((single_leaf_id, geom));
                }
            }

            // Render overlay tab header (single mode) — skipped while a
            // maximized sub-pane covers the container.
            if single_leaf_tab_visible {
            if let Some((symbol, _timeframe, exchange, account_type_label)) = single_window_info {
                let single_leaf_id = self.panel_app.panel_grid.docking()
                    .active_leaf()
                    .unwrap_or(mlc_core::LeafId(0));
                let hover_zone = if self.leaf_tab_hovered_leaf == Some(single_leaf_id) {
                    self.leaf_tab_hover
                } else {
                    mlc_app_layout::layout::panel_overlay::LeafTabHoverZone::None
                };
                let single_color_tag = self.panel_app.leaf_color_tags.get(&single_leaf_id).copied();
                // Overlay tab legend uses the resolution-aware label (see
                // `split_tab_label` above) — distinct from the raw TF name
                // this tuple otherwise carries. Falls back to empty (mirrors
                // `split_tab_label`'s `unwrap_or_default()` above), never to
                // the raw stale `_timeframe` — printing that on a Param-type
                // chart (e.g. renko) showed a nonsense stale TF instead of
                // the resolution label.
                let single_tab_label: String = self.panel_app.panel_grid.active_container_id()
                    .map(|cid| self.resolution_label_for_chart(cid))
                    .unwrap_or_default();
                let hit_zones = mlc_app_layout::layout::panel_overlay::render_leaf_tab(
                    ctx,
                    content_rect.x + 2.0,
                    main_chart_y_single + 2.0,
                    content_rect.width - 4.0,
                    &symbol,
                    &single_tab_label,
                    &exchange,
                    &account_type_label,
                    true, // always active in single mode
                    hover_zone,
                    single_color_tag,
                    &leaf_tab_toolbar_theme,
                    None, // static text — inline editing is Trading-tab only
                );
                let tab_rect = hit_zones.tab_rect;
                let dots_rect = hit_zones.dots_rect;
                let color_tag_rect = hit_zones.color_tag_rect;
                out_leaf_tab_hit_zones.insert(single_leaf_id, hit_zones);

                // Register overlay tab sub-zones so crosshair hides, cursor is default,
                // and hover state can be derived from InputCoordinator hovered_widget().
                {
                    use uzor::input::Sense;
                    let mut coord = self.input_coordinator.borrow_mut();
                    // Body registered first — sub-zones registered after win overlaps.
                    let [rx, ry, rw, rh] = tab_rect;
                    if rw > 0.0 && rh > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:body", single_leaf_id.0),
                            uzor::Rect::new(rx, ry, rw, rh),
                            Sense::CLICK,
                        );
                    }
                    let [dx, dy, dw, dh] = dots_rect;
                    if dw > 0.0 && dh > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:gear", single_leaf_id.0),
                            uzor::Rect::new(dx, dy, dw, dh),
                            Sense::CLICK,
                        );
                    }
                    let [cx, cy, cw, ch] = color_tag_rect;
                    if cw > 0.0 && ch > 0.0 {
                        coord.register(
                            format!("leaf_tab:{}:color_tag", single_leaf_id.0),
                            uzor::Rect::new(cx, cy, cw, ch),
                            Sense::CLICK,
                        );
                    }
                }
            }
            }

            // Wrap single-window corner zones in a per-leaf map so the
            // type matches the split path and handle_canvas_click can use
            // a unified walk regardless of split/single mode.
            let single_leaf_id = self.panel_app.panel_grid.docking()
                .active_leaf()
                .unwrap_or(mlc_core::LeafId(0));
            let mut single_map: std::collections::HashMap<mlc_core::LeafId, ScaleCornerHitZones> =
                std::collections::HashMap::new();
            single_map.insert(single_leaf_id, corner_zones_single);
            single_map
        };
        let out_scale_corner_zones = corner_zones;
        let _rt2 = web_time::Instant::now(); // checkpoint: after chart render
        // Build selected primitive config for the inline config toolbar.
        //
        // DrawingManager::get_selected_config() reads the primitive registry and
        // the selected primitive's data in one call, returning None when nothing
        // is selected.  The result is a cheap owned value so no borrow escapes.
        let selected_config: Option<mlc_core::state::selected_config::SelectedPrimitiveConfig> = {
            let bid = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            let drawings_guard = self.core.drawings();
            bid.and_then(|bid| drawings_guard.selected_config(bid))
        };

        // Generate clock time from active window's timezone settings
        let clock_time = {
            let utc_secs = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);

            if let Some(window) = self.panel_app.panel_grid.active_container() {
                let time_fmt = &window.scale_settings.time_format;
                let time = time_fmt.format_clock_time(utc_secs);
                if !time_fmt.show_utc_prefix {
                    time
                } else if time_fmt.timezone_offset_hours >= 0 {
                    format!("[UTC+{}] {}", time_fmt.timezone_offset_hours, time)
                } else {
                    format!("[UTC{}] {}", time_fmt.timezone_offset_hours, time)
                }
            } else {
                let hours = (utc_secs / 3600) % 24;
                let minutes = (utc_secs / 60) % 60;
                let seconds = utc_secs % 60;
                format!("[UTC+0] {:02}:{:02}:{:02}", hours, minutes, seconds)
            }
        };

        // 4b. Render indicator overlay (top-left of chart content area).
        //     Drawn BEFORE toolbars so that toolbar dropdowns render on top.
        //     In split mode each leaf renders its own chevron inside the split loop above.
        if !self.panel_app.panel_grid.is_split() && single_leaf_tab_visible {
            // overlay visibility was set in prepare_frame; read the state here.

            let toolbar_theme_for_overlay = self.panel_app.toolbar_theme_for_render();
            let overlay_state = &self.panel_app.indicator_overlay_state;
            if overlay_state.visible || overlay_state.is_open {
                // compare_series still needs window borrow.
                let compare_series: Vec<(String, bool, String)> =
                    self.panel_app.panel_grid.active_container()
                        .map(|w| w.compare_overlay.series.iter()
                            .map(|s| (s.symbol.clone(), s.visible, s.color.clone()))
                            .collect())
                        .unwrap_or_default();

                let ind_overlay_bid = self.panel_app.panel_grid.active_container()
                    .map(|w| w.current_tag_id);
                let mut indicators: Vec<IndicatorOverlayInfo> = {
                    let im = self.core.indicators();
                    let instances = match ind_overlay_bid {
                        Some(bid) => im.instances_for_bubble(bid),
                        None => Vec::new(),
                    };
                    instances
                        .iter()
                        // Chart legend = price-plot indicators only; sub-panes
                        // caption themselves.
                        .filter(|inst| inst.pane == 0)
                        .map(|inst| {
                            let display_name = Self::format_indicator_display_name(
                                &im, inst,
                            );
                            IndicatorOverlayInfo {
                                id: inst.id,
                                display_name,
                                visible: inst.visible,
                                is_compare: false,
                                symbol: None,
                                color: None,
                            }
                        })
                        .collect()
                };

                for (sym, vis, col) in compare_series {
                    indicators.push(IndicatorOverlayInfo {
                        id: 0,
                        display_name: sym.clone(),
                        visible: vis,
                        is_compare: true,
                        symbol: Some(sym),
                        color: Some(col),
                    });
                }

                if !indicators.is_empty() {
                    let chart_rect = uzor::types::Rect::new(
                        content_rect.x,
                        main_chart_y_single,
                        content_rect.width,
                        content_rect.height,
                    );
                    let overlay_result = render_indicator_overlay(
                        ctx,
                        &chart_rect,
                        &indicators,
                        overlay_state,
                        &frame_theme,
                        &toolbar_theme_for_overlay,
                    );

                    // Register indicator overlay hit zones with InputCoordinator.
                    {
                        use uzor::input::Sense;
                        use mlc_app_layout::ui::z_order::ZLayer;

                        let ov_layer = ZLayer::Toolbar.push_named(&mut self.input_coordinator.borrow_mut(), "ind_overlay");

                        // Main toggle button (when dropdown is closed)
                        let br = &overlay_result.button_rect;
                        if br.width > 0.0 {
                            self.input_coordinator.borrow_mut().register_on_layer(
                                "ind_overlay:toggle",
                                uzor::Rect::new(br.x, br.y, br.width, br.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                        }

                        // Close button (when dropdown is open)
                        if let Some(ref close_rect) = overlay_result.close_button_rect {
                            self.input_coordinator.borrow_mut().register_on_layer(
                                "ind_overlay:close",
                                uzor::Rect::new(close_rect.x, close_rect.y, close_rect.width, close_rect.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                        }

                        // Per-indicator / per-compare-series row action buttons.
                        // Compare entries use "cmp_overlay:" prefix; indicators use "ind_overlay:".
                        for row in &overlay_result.indicator_rows {
                            let id = row.instance_id;
                            let row_prefix = if row.is_compare { "cmp_overlay" } else { "ind_overlay" };
                            // Full row rect — registered first (lower z-priority) so individual
                            // icon widgets take precedence, but the text/gap area still triggers
                            // default cursor via its prefix in input.rs.
                            let rr = &row.row_rect;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:row:{}", row_prefix, id),
                                uzor::Rect::new(rr.x, rr.y, rr.width, rr.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                            // Alert button (only meaningful for indicators; compare entries show it too
                            // for layout consistency but the handler is a no-op for compare)
                            let ar = &row.alert_btn;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:alert:{}", row_prefix, id),
                                uzor::Rect::new(ar.x, ar.y, ar.width, ar.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                            // Visibility toggle
                            let vr = &row.visibility_btn;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:vis:{}", row_prefix, id),
                                uzor::Rect::new(vr.x, vr.y, vr.width, vr.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                            // Settings button
                            let sr = &row.settings_btn;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:settings:{}", row_prefix, id),
                                uzor::Rect::new(sr.x, sr.y, sr.width, sr.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                            // Delete button
                            let dr = &row.delete_btn;
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("{}:delete:{}", row_prefix, id),
                                uzor::Rect::new(dr.x, dr.y, dr.width, dr.height),
                                Sense::CLICK,
                                &ov_layer,
                            );
                        }

                        self.input_coordinator.borrow_mut().pop_layer(&ov_layer);
                    }
                }
            }
        }

        // 4c. Render toolbars and their dropdowns on top of the indicator overlay.
        //
        // When `skip_toolbar_draw` is true, skip the expensive vector draw and
        // re-register hit zones from the cached `last_toolbar_result` instead.
        // The caller composites the previously-built toolbar scene on top.
        let out_last_toolbar_result: Option<mlc_core::ChartToolbarRenderResult> = if !skip_toolbar_draw {
            // S2.7.1: read active symbol/timeframe from branch_meta.
            let (mut active_sym_str, active_tf_str, active_gid, mut active_exch_str, mut active_at_str) = {
                let cid = self.panel_app.panel_grid.active_container_id();
                let sym = cid.and_then(|c| self.symbol_for_chart(c))
                    .map(String::from).unwrap_or_default();
                let tf = cid.and_then(|c| self.timeframe_for_chart(c))
                    .map(|tf| tf.name.clone())
                    .unwrap_or_default();
                let gid = cid.and_then(|c| self.bubble_for_chart(c));
                // Adaptive symbol-button suffix (owner spec 2026-07-17):
                // venue + account-type of the active instrument, threaded
                // to the 3-tier label fit in chart_toolbar_render_toolbars.
                let exch = cid.and_then(|c| self.exchange_for_chart(c)).unwrap_or_default();
                let at = cid.and_then(|c| self.account_type_for_chart(c))
                    .map(|a| a.as_label().to_string()).unwrap_or_default();
                (sym, tf, gid, exch, at)
            };
            // Workspace-panel arc — same derivation swap + suppress-list
            // as `render_toolbar_only`'s sibling block; see its comments.
            let trading_focus_pid = self.workspace_focus().trading_panel();
            if let Some(pid) = trading_focus_pid {
                if let Some(tc) = self.panels_store.trading_container.get(&pid) {
                    active_sym_str = tc.symbol.clone();
                    active_exch_str = tc.exchange.clone();
                    active_at_str = tc.account_type.clone();
                }
            }
            let suppress_chart_controls = trading_focus_pid.is_some()
                && self.core.user().profile().ui.trading_panel_topbar_takeover;
            self.stamp_active_right_toolbar_id();
            let bubble_guard_tb = self.core.bubbles();
            let tags_guard_tb = self.core.tags();
            let presets_guard_tb = self.core.presets();
            let (active_sync_flags_owned, active_is_mono_group) = active_gid
                .and_then(|gid| bubble_guard_tb.sync_flags(gid).map(|sf| (Some(sf), tags_guard_tb.get(gid).map(|t| t.members.len() <= 1).unwrap_or(true))))
                .unwrap_or((None, true));
            // TF picker gating (audit D8 follow-up, 2026-07-04): computed
            // fresh every frame from the active window's chart_type + series
            // depth — RAM-only, no persisted field (SWC).
            let active_ct_str: Option<&'static str> = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.chart_type_for_chart(cid));
            // TF button resolution-picker label (owner doctrine 2026-07-08):
            // the active window's live `chart_type_param`, read when the
            // active chart_type is `ResolutionKind::Param`.
            let active_ct_param: Option<f64> = self.panel_app.panel_grid.active_container_id()
                .and_then(|cid| self.chart_type_param_for_chart(cid));
            let allowed_tfs = {
                let cid = self.panel_app.panel_grid.active_container_id();
                cid.map(|cid| {
                    let chart_type = self.chart_type_for_chart(cid).unwrap_or("candles");
                    let active_series = self.active_series_for_chart(cid);
                    let now_ms = web_time::SystemTime::now()
                        .duration_since(web_time::UNIX_EPOCH)
                        .unwrap_or_default()
                        .as_millis() as i64;
                    crate::active_series::allowed_timeframes(active_series.as_active_series(), chart_type, now_ms)
                }).unwrap_or(None)
            };
            let chart_type_data_modes = self.chart_type_data_modes();
            let timeframe_native_map = self.timeframe_native_map();
            let footprint_live_only = self.footprint_live_only();
            let granularity_favorites = self.core.user().profile().domain.granularity_favorites.clone();
            let favorite_primitives = self.core.user().profile().ui.favorite_primitives.clone();
            let favorites_drag_render = self.fav_item_drag.as_ref().map(|(tool_id, cx, cy)|
                mlc_core::state::toolbar_state::FavoritesDragRender { tool_id: tool_id.clone(), cursor_x: *cx, cursor_y: *cy });
            let granularity_edit_render = self.granularity_edit_render();
            let pin_granularity_favorites = self.core.user().profile().ui.pin_granularity_favorites;
            let granularity_drag_render = self.granularity_drag_render();
            let split_symbol_compare = self.core.user().profile().ui.split_symbol_compare;
            // Target-gated (owner defect 2026-07-25): the topbar's
            // `symbol_selector` button only hosts the inline editor when it
            // is the ACTUAL edit target — a live TradingPanel edit must
            // render EXCLUSIVELY in the Trading tab's header (render_right_
            // sidebar's `trading_symbol_edit` param), never leak into both
            // places at once.
            let symbol_quick_search_render = match self.symbol_quick_edit.as_ref().map(|e| e.target) {
                Some(SymbolQuickEditTarget::TopbarChart) => self.symbol_quick_search_render(),
                _ => None,
            };
            let symbol_quick_search = self.core.user().profile().ui.symbol_quick_search;
            let clock_glitch_enabled = self.core.user().profile().ui.clock_glitch_enabled;
            let is_capturing_screenshot = self.is_capturing_screenshot();
            let toolbar_result = self.panel_app.render_toolbars_with_theme(
                ctx,
                &panel_layout,
                selected_config.as_ref(),
                Some(clock_time.as_str()),
                self.split_without_group,
                Some(active_sym_str.as_str()),
                Some(active_tf_str.as_str()),
                sidebar_w,
                active_sync_flags_owned.as_ref(),
                active_is_mono_group,
                presets_guard_tb.values(),
                allowed_tfs.as_ref(),
                active_ct_str,
                active_ct_param,
                chart_type_data_modes.as_ref(),
                timeframe_native_map.as_ref(),
                footprint_live_only,
                &granularity_favorites,
                &favorite_primitives,
                granularity_edit_render.as_ref(),
                pin_granularity_favorites,
                granularity_drag_render.as_ref(),
                favorites_drag_render.as_ref(),
                split_symbol_compare,
                symbol_quick_search_render.as_ref(),
                symbol_quick_search,
                Some(active_exch_str.as_str()),
                Some(active_at_str.as_str()),
                suppress_chart_controls,
                Some(self.last_mouse_pos),
                current_time_ms as f64 / 1000.0,
                self.anim_frame,
                clock_glitch_enabled,
                is_capturing_screenshot,
            );
            drop(bubble_guard_tb);
            drop(tags_guard_tb);
            drop(presets_guard_tb);
            Some(toolbar_result)
        } else {
            // Preserve the cached toolbar result from the previous frame.
            self.last_toolbar_result.clone()
        };

        // Register toolbar hit zones — always done every frame (cheap coordinate
        // registration), whether the toolbar was redrawn or cached.
        let mut out_last_inline_bar_rect: Option<LayoutRect> = None;
        let mut out_last_favorites_bar_rect: Option<LayoutRect> = None;
        if let Some(ref toolbar_result) = out_last_toolbar_result {
            use uzor::input::Sense;
            use mlc_app_layout::ui::z_order::ZLayer;

            // Application chrome (2026-08-08): the main toolbar must stay
            // clickable while a dropdown/popup/modal is open elsewhere on
            // screen (owner report — colour picker inside primitive
            // settings froze the whole screen). `push_chrome` marks this
            // layer as reachable underneath a modal barrier; see
            // `InputCoordinator::push_layer_ex`.
            let tb_layer = ZLayer::Toolbar.push_chrome(&mut self.input_coordinator.borrow_mut());

            // Drawing toolbar (left side)
            for (id, rect) in &toolbar_result.left_toolbar.item_rects {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("dtb:{}", id),
                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
            }

            // Control strip (top)
            for (id, rect) in &toolbar_result.top_toolbar.item_rects {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("csb:{}", id),
                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
            }

            // Right toolbar
            for (id, rect) in &toolbar_result.right_toolbar.item_rects {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("rtb:{}", id),
                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
            }

            // Bottom toolbar
            for (id, rect) in &toolbar_result.bottom_toolbar.item_rects {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("btb:{}", id),
                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
            }

            // Floating inline config toolbar items
            if let Some(ref inline_cfg) = toolbar_result.inline_config {
                // Register bar background first so gap clicks between buttons are absorbed
                // and don't fall through to handle_canvas_click() which would deselect the primitive.
                self.input_coordinator.borrow_mut().register_on_layer(
                    "ilb:__bg__",
                    uzor::Rect::new(inline_cfg.bar_rect.x, inline_cfg.bar_rect.y, inline_cfg.bar_rect.width, inline_cfg.bar_rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
                // Register individual button hit zones
                for (id, rect) in &inline_cfg.item_rects {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("ilb:{}", id),
                        uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
                out_last_inline_bar_rect = Some(inline_cfg.bar_rect);
            } else {
                out_last_inline_bar_rect = None;
            }

            // Floating favorites quick-pick bar items (favorite-primitives
            // arc) — same registration contract as the inline bar above:
            // background first so gap clicks are absorbed, item zones after
            // so they win on overlap.
            if let Some(ref fav_cfg) = toolbar_result.favorites_bar {
                self.input_coordinator.borrow_mut().register_on_layer(
                    "fav:__bg__",
                    uzor::Rect::new(fav_cfg.bar_rect.x, fav_cfg.bar_rect.y, fav_cfg.bar_rect.width, fav_cfg.bar_rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
                for (id, rect) in &fav_cfg.item_rects {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("fav:{}", id),
                        uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
                out_last_favorites_bar_rect = Some(fav_cfg.bar_rect);
            } else {
                out_last_favorites_bar_rect = None;
            }

            // Register chevron hit zones for each toolbar that is overflowing.
            // IDs use the same prefix as the toolbar's items so dispatch_panel_click
            // can route them.  The suffix "__chevron_left" / "__chevron_right"
            // identifies the direction.
            let chevron_pairs: &[(&str, Option<uzor::types::Rect>, Option<uzor::types::Rect>)] = &[
                ("csb",
                 toolbar_result.top_left_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height)),
                 toolbar_result.top_right_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height))),
                ("btb",
                 toolbar_result.bottom_left_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height)),
                 toolbar_result.bottom_right_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height))),
                ("dtb",
                 toolbar_result.left_up_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height)),
                 toolbar_result.left_down_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height))),
                ("rtb",
                 toolbar_result.right_up_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height)),
                 toolbar_result.right_down_chevron.map(|r| uzor::Rect::new(r.x, r.y, r.width, r.height))),
            ];
            for (prefix, left_rect, right_rect) in chevron_pairs {
                if let Some(rect) = left_rect {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("{}:__chevron_left", prefix),
                        *rect,
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
                if let Some(rect) = right_rect {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("{}:__chevron_right", prefix),
                        *rect,
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
            }

            // Inline dropdown hit zones (style/width popup)
            if let Some(ref idd) = toolbar_result.inline_dropdown_result {
                // Background absorber
                self.input_coordinator.borrow_mut().register_on_layer(
                    "ilb:inline_dropdown:__bg__",
                    uzor::Rect::new(idd.menu_rect.x, idd.menu_rect.y, idd.menu_rect.width, idd.menu_rect.height),
                    Sense::CLICK,
                    &tb_layer,
                );
                // Individual items — format: "ilb:{item_id}"
                for (item_id, rect) in &idd.item_rects {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("ilb:{}", item_id),
                        uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                        Sense::CLICK,
                        &tb_layer,
                    );
                }
            }

            self.input_coordinator.borrow_mut().pop_layer(&tb_layer);

            // Register dropdown hit zones (higher z-order)
            if let Some(ref dd) = toolbar_result.dropdown_result {
                use uzor::input::Sense;
                use mlc_app_layout::ui::z_order::ZLayer;

                let dd_layer = ZLayer::Dropdown.push_named(&mut self.input_coordinator.borrow_mut(), "chart_dropdown");

                // Menu background
                self.input_coordinator.borrow_mut().register_on_layer(
                    "dropdown:__bg__",
                    uzor::Rect::new(dd.menu_rect.x, dd.menu_rect.y, dd.menu_rect.width, dd.menu_rect.height),
                    Sense::CLICK,
                    &dd_layer,
                );

                // Dropdown items — format: "dropdown:{dropdown_id}:{item_id}"
                for (item_id, rect) in &dd.item_rects {
                    self.input_coordinator.borrow_mut().register_on_layer(
                        format!("dropdown:{}:{}", dd.dropdown_id, item_id),
                        uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                        Sense::CLICK,
                        &dd_layer,
                    );
                }

                self.input_coordinator.borrow_mut().pop_layer(&dd_layer);

                // Hover-based submenu: if draw_dropdown detected the pointer is over a
                // submenu-trigger item, open that submenu immediately (no click required).
                // If the pointer is over a non-submenu item, clear the open submenu.
                // Only update submenu state when we actually redrew the toolbar; on
                // cached frames the submenu state from the previous draw is preserved.
                if !skip_toolbar_draw {
                    if let Some(ref submenu_id) = dd.open_submenu {
                        out_open_submenu_update = Some(Some(submenu_id.clone()));
                    } else if dd.hovered.is_some() {
                        // Hovering a regular (non-submenu) item — close any open submenu.
                        out_open_submenu_update = Some(None);
                    }
                }
            }
        }

        let _rt3 = web_time::Instant::now(); // checkpoint: after toolbars

        // 5. Modals are NO LONGER drawn here.
        // They are drawn in render_modal_layer(), called by the VelloGpu compositor
        // AFTER the crosshair overlay is composited, so modals appear on top of the
        // crosshair rather than below it.
        // render_modal_layer() stores results directly into self.frame_result,
        // self.search_modal_result, self.context_menu_result, etc.
        // For non-VelloGpu backends, render_modal_layer() is called on the same ctx
        // immediately after render() returns (see scene_builder.rs).
        let out_frame_result: Option<ChartModalRenderResult> = None;
        let out_search_modal_result: Option<mlc_app_layout::layout::ModalSearchResult> = None;
        let out_context_menu_result: Option<ContextMenuResult> = None;

        // 8. Render right sidebar if a panel is open.
        //    Sidebar sits between chart content (price scale) and right toolbar.
        //    Right toolbar stays at window edge.  content_rect was already shrunk.
        //      x = content_rect.x + content_rect.width (right edge of shrunk chart)
        //      y = top_toolbar_h
        //      h = window_h - top_toolbar_h - bottom_toolbar_h
        //
        // The sidebar is always rendered on every frame so that widget
        // hit-zone registration happens inside the open begin_frame/end_frame
        // window.  The caller (chart-app-vello) composites the cached
        // sidebar_scene on top via Scene::append, visually covering these
        // pixels when the scene is unchanged.

        let mut out_trading_header_result: Option<mlc_core::ChartToolbarRenderResult> = None;
        let out_last_sidebar_result: Option<mlc_app_sidebar_content::render::RightSidebarResult> = if self.sidebar_state.is_right_open() {
            let top_h = panel_layout.top_toolbar_rect.height;
            let bottom_h = panel_layout.bottom_toolbar_rect.height;
            let sidebar_x = content_rect.x + content_rect.width;
            let sidebar_y = top_h;
            let sidebar_h = h - top_h - bottom_h;

            let sidebar_rect = LayoutRect::new(sidebar_x, sidebar_y, sidebar_w, sidebar_h);
            let sidebar_toolbar_theme = self.panel_app.toolbar_theme_for_render();
            let panel_theme = panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());

            // Pre-compute per-panel color + sync-flags maps before render_right_sidebar
            // mutably borrows self.sidebar_state. This avoids E0502 between the &mut
            // borrow of sidebar_state and the closures' immutable reads of self.core.bubble.
            let (sidebar_panel_color_map, sidebar_panel_sync_flags_map): (
                std::collections::HashMap<u64, [f32; 4]>,
                std::collections::HashMap<u64, mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot>,
            ) = {
                use mlc_core::ids::SyncMemberId;
                let bubbles = self.core.bubbles();
                let tags = self.core.tags();
                let mut colors = std::collections::HashMap::new();
                let mut flags = std::collections::HashMap::new();
                for (tag_id, tag) in tags.iter() {
                    let bubble_default = bubbles.sync_flags(*tag_id).unwrap_or_default();
                    for &member in &tag.members {
                        if let SyncMemberId::Panel(pid) = member {
                            if !tag.auto_created {
                                colors.insert(pid, tag.color);
                            }
                            flags.insert(pid, mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot {
                                sync_symbol: tag.effective_sync_symbol(member, bubble_default.sync_symbol),
                                sync_crosshair: tag.effective_sync_crosshair(member, bubble_default.sync_crosshair),
                            });
                        }
                    }
                }
                (colors, flags)
            };

            // Pre-compute watchlist column config + a clone of the active
            // list (for color-flag lookups by InstrumentAddress) before
            // render_right_sidebar mutably borrows self.sidebar_state (same
            // E0502 rationale as sidebar_panel_color_map above).
            let watchlist_col_cfg = self.watchlist_ui.column_config(self.watchlist_ui.active_list_id);
            let watchlist_active_list: Option<mlc_core::WatchlistList> = self.core.watchlists()
                .find(self.watchlist_ui.active_list_id)
                .cloned();

            // Trading-header-symbol-edit / trading-header-tf arcs
            // (2026-07-25): read BEFORE `panels_store` borrows `self`
            // immutably below.
            let trading_symbol_edit_render = self.symbol_quick_edit.as_ref().and_then(|e| match e.target {
                crate::SymbolQuickEditTarget::TradingPanel(pid)
                | crate::SymbolQuickEditTarget::SectionPanel(pid)
                | crate::SymbolQuickEditTarget::MultiDomRoster(pid)
                | crate::SymbolQuickEditTarget::MultiTapeRoster(pid) => {
                    self.symbol_quick_search_render().map(|r| (pid.0, r))
                }
                crate::SymbolQuickEditTarget::TopbarChart => None,
            });

            // Draw sidebar and register hit zones every frame.
            // When the cached sidebar_scene is composited on top these pixels
            // are overwritten, but the widget registrations survive until end_frame().
            let panels_store = &self.panels_store;
            let sidebar_result = mlc_app_sidebar_content::render::render_right_sidebar(
                ctx,
                &sidebar_rect,
                &mut self.sidebar_state,
                &sidebar_toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
                &mut |item, rect, ctx, coord, slot_prefix| {
                    panels_render::render_free_item(panels_store, item, rect.0, rect.1, rect.2, rect.3, ctx, &panel_theme, coord, slot_prefix);
                },
                &|item: &mlc_core::state::Panel| -> Option<(bool, f64, f64, bool)> {
                    use mlc_core::state::Panel;
                    match item {
                        Panel::Dom(id) => panels_store.dom.get(id).map(|s| (s.auto_center, s.tick_size, s.min_volume_filter, s.multi_mode)),
                        _ => None,
                    }
                },
                &|item: &mlc_core::state::Panel| -> Option<[f32; 4]> {
                    sidebar_panel_color_map.get(&item.panel_id().0).copied()
                },
                &|item: &mlc_core::state::Panel| -> Option<mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot> {
                    sidebar_panel_sync_flags_map.get(&item.panel_id().0).cloned()
                },
                &|item: &mlc_core::state::Panel| -> Option<(String, String, String, String)> {
                    use mlc_core::state::Panel;
                    match item {
                        Panel::Dom(id) => panels_store.dom.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::Footprint(id) => panels_store.footprint.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::VolumeProfile(id) => panels_store.volume_profile.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::BigTrades(id) => panels_store.big_trades.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::L2Tape(id) => panels_store.l2_tape.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::TradeTape(id) => panels_store.trade_tape.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::LiqTape(id) => panels_store.liq_tape.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::OrderEntry(id) => panels_store.order_entry.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::TradingContainer(id) => panels_store.trading_container.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), s.timeframe.clone())),
                        Panel::IndicatorPanel(id) => panels_store.indicator_panel.get(id)
                            .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                        Panel::PositionManager(_)
                        | Panel::TradeLog(_)
                        | Panel::RiskCalculator(_)
                        // Chart doesn't expose symbol-source / overlay info
                        // to the sidebar header — dedicated render path.
                        | Panel::Chart(_) => None,
                    }
                },
                &|item: &mlc_core::state::Panel| -> Option<Vec<bool>> {
                    use mlc_core::state::Panel;
                    match item {
                        Panel::Dom(id) => panels_store.dom.get(id).map(|s| vec![
                            s.column_config.show_bid_orders,
                            s.column_config.show_sell_trades,
                            s.column_config.show_buy_trades,
                            s.column_config.show_ask_orders,
                        ]),
                        Panel::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| vec![
                            s.column_config.show_time,
                            s.column_config.show_type,
                            s.column_config.show_side,
                            s.column_config.show_price,
                            s.column_config.show_qty,
                        ]),
                        Panel::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| vec![
                            s.column_config.show_time,
                            s.column_config.show_price,
                            s.column_config.show_size,
                        ]),
                        Panel::BigTrades(id) => panels_store.big_trades.get(id).map(|s| vec![
                            s.column_config.show_time,
                            s.column_config.show_side,
                            s.column_config.show_price,
                            s.column_config.show_size,
                            s.column_config.show_notional,
                        ]),
                        _ => None,
                    }
                },
                &watchlist_col_cfg,
                &|address: &mlc_core::InstrumentAddress| -> Option<String> {
                    watchlist_active_list.as_ref()
                        .and_then(|l| l.get_color_flag(address))
                        .map(|s| s.to_string())
                },
            );

            // Register panel overlay zones (color tag + gear) here — NOT only
            // in `render_sidebar_only()`. `render_sidebar_only()` is a
            // vello-only dirty-scene-cache helper (called from
            // `scene_builder.rs` AFTER this function, same open frame); the
            // web/headless shells call `render()` (== this function +
            // `apply_render_output`) and NEVER call `render_sidebar_only()` —
            // registering only there left `panel_overlay:{pid}:color_tag|gear`
            // completely unreachable on wasm (rects computed, widgets never
            // registered, clicks silently swallowed). Safe to also run again
            // inside `render_sidebar_only()` for the vello path — re-registering
            // the same ids/rects on the same frame is idempotent.
            self.register_panel_overlay_zones(&sidebar_result);

            // Embedded center chart (chart-in-container arc, 2026-07-26):
            // when the trading container's center shows Chart, draw the aux
            // `ChartContainer` over the container's center region. Rect =
            // the container body (`slot:N:leaf:M:focus_content` row pushed
            // by `render_slot_panel`) run through the container's own
            // `v1_layout_rects().dom`. Cached into `trading_center_rect`
            // for input hit-testing; viewport geometry is adopted
            // same-frame via `resize_trading_center_geometry` below.
            {
                let tc_center: Option<(mlc_core::state::PanelId, LayoutRect)> =
                    if self.sidebar_state.right_panel
                        == mlc_app_sidebar_content::state::RightSidebarPanel::SlotTrading
                    {
                        self.panels_store.trading_container.iter().next().and_then(|(pid, tc)| {
                            sidebar_result
                                .item_rects
                                .iter()
                                .find(|(id, _)| {
                                    id.starts_with("slot:") && id.ends_with(":focus_content")
                                })
                                .and_then(|(_, r)| {
                                    // Mode-dispatching rect (Fixed: center
                                    // region when Chart; Free: Chart leaf).
                                    tc.embedded_chart_rect(r.x, r.y, r.width, r.height)
                                        .map(|(dx, dy, dw, dh)| (*pid, LayoutRect::new(dx, dy, dw, dh)))
                                })
                        })
                    } else {
                        None
                    };
                self.trading_center_rect = tc_center;
                let tc_out = match tc_center {
                    Some((tc_pid_c, center_rect)) => {
                        // Same-frame geometry: viewport adopts THIS rect
                        // before the draw reads it (one-frame-lag fix).
                        self.resize_trading_center_geometry(tc_pid_c, &center_rect);
                        self.draw_trading_center_chart(ctx, tc_pid_c, &center_rect)
                    }
                    None => None,
                };
                match tc_out {
                    Some(out) => {
                        self.trading_center_corner_zones = Some(out.corner_zones);
                        // Sub-pane overlay writeback — aux counterpart of
                        // the split loop's `overlay_writebacks` (button
                        // rects for hover + click hit-testing next frame).
                        if let Some((tc_pid_c, _)) = tc_center {
                            if let Some(&aux_cid) = self.trading_center_charts.get(&tc_pid_c) {
                                if let Some(w) =
                                    self.panel_app.panel_grid.containers_mut().get_mut(&aux_cid)
                                {
                                    w.sub_pane_overlay_results = out.sub_pane_overlays;
                                }
                            }
                        }
                        // Indicator legend overlay hit zones
                        // (`ind_overlay:tcc:{action}` — routed by
                        // `handle_trading_center_ind_overlay_action`).
                        if let Some(ref ov) = out.ind_overlay {
                            use uzor::input::Sense;
                            use mlc_app_layout::ui::z_order::ZLayer;
                            let ov_layer = ZLayer::Toolbar.push_named(
                                &mut self.input_coordinator.borrow_mut(),
                                "ind_overlay_tcc",
                            );
                            let prefix = "ind_overlay:tcc:";
                            let br = &ov.button_rect;
                            if br.width > 0.0 {
                                self.input_coordinator.borrow_mut().register_on_layer(
                                    format!("{}toggle", prefix),
                                    uzor::Rect::new(br.x, br.y, br.width, br.height),
                                    Sense::CLICK,
                                    &ov_layer,
                                );
                            }
                            if let Some(ref cr) = ov.close_button_rect {
                                self.input_coordinator.borrow_mut().register_on_layer(
                                    format!("{}close", prefix),
                                    uzor::Rect::new(cr.x, cr.y, cr.width, cr.height),
                                    Sense::CLICK,
                                    &ov_layer,
                                );
                            }
                            for row in &ov.indicator_rows {
                                let id = row.instance_id;
                                for (suffix, r) in [
                                    ("row", &row.row_rect),
                                    ("alert", &row.alert_btn),
                                    ("vis", &row.visibility_btn),
                                    ("settings", &row.settings_btn),
                                    ("delete", &row.delete_btn),
                                ] {
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("{}{}:{}", prefix, suffix, id),
                                        uzor::Rect::new(r.x, r.y, r.width, r.height),
                                        Sense::CLICK,
                                        &ov_layer,
                                    );
                                }
                            }
                            self.input_coordinator.borrow_mut().pop_layer(&ov_layer);
                        }
                    }
                    None => {
                        self.trading_center_corner_zones = None;
                    }
                }

                // Free-mode overlay pass (chips + drag feedback) — MUST run
                // after `draw_trading_center_chart` above: the chart paints
                // over the container body and would eat chips drawn during
                // the panel render (the 2026-07-29 "no chips on chart/DOM"
                // defect).
                if self.sidebar_state.right_panel
                    == mlc_app_sidebar_content::state::RightSidebarPanel::SlotTrading
                {
                    if let Some(tc) = self.panels_store.trading_container.values().next() {
                        let overlay_theme =
                            panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());
                        tc.render_free_overlay(
                            ctx,
                            &overlay_theme,
                            &mut self.input_coordinator.borrow_mut(),
                            self.rc_modal.is_open || self.tc_modal.is_open,
                        );
                    }
                }
            }

            // Trading container EMBEDDED header toolbar (owner spec
            // 2026-07-25): the container header IS an alternative top
            // toolbar — same composer, separate ChartToolbarState, item ids
            // shared with the main topbar so the symbol-search / TF systems
            // behave identically; hit zones registered as `tch:{id}`.
            if self.sidebar_state.right_panel == mlc_app_sidebar_content::state::RightSidebarPanel::SlotTrading {
                if let Some((tc_pid, tc)) = self.panels_store.trading_container.iter().next() {
                    let close_w = 36.0;
                    let header_rect = LayoutRect::new(
                        sidebar_rect.x,
                        sidebar_rect.y,
                        (sidebar_rect.width - close_w).max(0.0),
                        40.0,
                    );
                    let th_gran_favs = self.core.user().profile().domain.granularity_favorites.clone();
                    let th_pin_favs = self.core.user().profile().ui.pin_granularity_favorites;
                    let th_gran_edit = self.granularity_edit_render();
                    let th_gran_drag = self.granularity_drag_render();
                    let th_tf_native = self.timeframe_native_map();
                    let th_sq_render = trading_symbol_edit_render.as_ref().map(|(_, r)| r);
                    let th_sqs_on = self.core.user().profile().ui.symbol_quick_search;
                    // Embedded chart's type drives the chart_type_selector icon.
                    let th_active_ct: Option<&'static str> = self.trading_center_charts
                        .get(tc_pid)
                        .and_then(|&cid| self.chart_type_for_chart(cid));
                    let th_result = self.panel_app.render_trading_header_toolbar(
                        &self.trading_header_tb,
                        ctx,
                        &header_rect,
                        &tc.symbol,
                        &tc.timeframe,
                        &tc.exchange,
                        &tc.account_type,
                        &th_gran_favs,
                        th_gran_edit.as_ref(),
                        th_pin_favs,
                        th_gran_drag.as_ref(),
                        th_sq_render,
                        th_sqs_on,
                        th_tf_native.as_ref(),
                        tc.center_kind.label(),
                        {
                            use mlc_app_panels::trading::trading::trading_container::{CenterKind, TcLayoutMode};
                            match tc.layout_mode {
                                TcLayoutMode::Fixed => tc.center_kind == CenterKind::Chart,
                                TcLayoutMode::Free => tc.has_free_chart_leaf(),
                            }
                        },
                        tc.layout_mode == mlc_app_panels::trading::trading::trading_container::TcLayoutMode::Free,
                        th_active_ct,
                    );

                    // Color-sync tag square over the blank `tc_color_tag`
                    // button (composer items are icon/text-driven; the tag
                    // color is per-panel dynamic state).
                    if let Some((_, tag_rect)) = th_result.top_toolbar.item_rects.iter().find(|(id, _)| id == "tc_color_tag") {
                        let tag_color = sidebar_panel_color_map.get(&tc_pid.0).copied();
                        let css = match tag_color {
                            Some(c) => format!(
                                "rgba({},{},{},1.0)",
                                (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8
                            ),
                            None => "rgba(128,128,128,0.55)".to_string(),
                        };
                        let sq = 10.0;
                        ctx.set_fill_color(&css);
                        ctx.fill_rect(
                            tag_rect.x + (tag_rect.width - sq) / 2.0,
                            tag_rect.y + (tag_rect.height - sq) / 2.0,
                            sq,
                            sq,
                        );
                    }

                    {
                        use uzor::input::Sense;
                        use mlc_app_layout::ui::z_order::ZLayer;
                        // Section/panel header chrome (2026-08-08) — stays
                        // reachable underneath an open modal, same as the
                        // main toolbar. See `ZLayer::push_named_chrome`.
                        let tch_layer = ZLayer::Toolbar.push_named_chrome(&mut self.input_coordinator.borrow_mut(), "tch");
                        for (id, rect) in &th_result.top_toolbar.item_rects {
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("tch:{}", id),
                                uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                                Sense::CLICK,
                                &tch_layer,
                            );
                        }
                        for (suffix, chev) in [
                            ("__chevron_left", th_result.top_left_chevron),
                            ("__chevron_right", th_result.top_right_chevron),
                        ] {
                            if let Some(r) = chev {
                                self.input_coordinator.borrow_mut().register_on_layer(
                                    format!("tch:{}", suffix),
                                    uzor::Rect::new(r.x, r.y, r.width, r.height),
                                    Sense::CLICK,
                                    &tch_layer,
                                );
                            }
                        }
                        self.input_coordinator.borrow_mut().pop_layer(&tch_layer);

                        if let Some(ref dd) = th_result.dropdown_result {
                            let dd_layer = ZLayer::Dropdown.push_named(&mut self.input_coordinator.borrow_mut(), "tch_dropdown");
                            self.input_coordinator.borrow_mut().register_on_layer(
                                "dropdown:__bg__",
                                uzor::Rect::new(dd.menu_rect.x, dd.menu_rect.y, dd.menu_rect.width, dd.menu_rect.height),
                                Sense::CLICK,
                                &dd_layer,
                            );
                            for (item_id, rect) in &dd.item_rects {
                                self.input_coordinator.borrow_mut().register_on_layer(
                                    format!("dropdown:{}:{}", dd.dropdown_id, item_id),
                                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                                    Sense::CLICK,
                                    &dd_layer,
                                );
                            }
                            self.input_coordinator.borrow_mut().pop_layer(&dd_layer);
                        }
                    }

                    out_trading_header_result = Some(th_result);
                }
            }

            // Data-section header toolbar (global-docking Stage B, owner
            // spec 2026-07-30): DOM / Tape / L2 / BigTrades sections reuse
            // the tch instance with the section def — symbol quick-search +
            // color tag (+ DOM thematic chips). The slot control-plane is
            // gone; this header is the section's only chrome.
            {
                use mlc_app_sidebar_content::state::RightSidebarPanel as RSP;
                let rp = self.sidebar_state.right_panel;
                if matches!(rp, RSP::SlotDom | RSP::SlotTape | RSP::SlotL2 | RSP::SlotBigTrades | RSP::SlotLiq) {
                    if let Some(item) = self.section_panel_for(rp) {
                        let s_pid = item.panel_id();
                        let (s_sym, s_ex, s_at) = self.section_symbol_triplet(&item);
                        let dom_chips = if let mlc_core::state::Panel::Dom(id) = &item {
                            self.panels_store.dom.get(id).map(|s| if s.multi_mode {
                                // Multi/Aggregated mode: F = multi level
                                // filter, the tick chip = the multi GRID.
                                mlc_core::toolbar::SectionDomChips {
                                    multi_mode: true,
                                    agg_mode: s.agg_mode,
                                    locked: s.multi.locked,
                                    min_volume_filter: s.multi.min_level_qty,
                                    tick_size: s.multi.config.grid_size,
                                    show_imbalance: Some(s.multi.config.show_imbalance),
                                }
                            } else {
                                mlc_core::toolbar::SectionDomChips {
                                    multi_mode: false,
                                    agg_mode: false,
                                    locked: s.auto_center,
                                    min_volume_filter: s.min_volume_filter,
                                    tick_size: s.tick_size,
                                    show_imbalance: None,
                                }
                            })
                        } else {
                            None
                        };
                        let close_w = 36.0;
                        let header_rect = LayoutRect::new(
                            sidebar_rect.x,
                            sidebar_rect.y,
                            (sidebar_rect.width - close_w).max(0.0),
                            40.0,
                        );
                        let th_sq_render = trading_symbol_edit_render.as_ref().map(|(_, r)| r);
                        let th_sqs_on = self.core.user().profile().ui.symbol_quick_search;
                        let th_result = self.panel_app.render_section_header_toolbar(
                            &self.trading_header_tb,
                            ctx,
                            &header_rect,
                            &s_sym,
                            &s_ex,
                            &s_at,
                            th_sq_render,
                            th_sqs_on,
                            mlc_core::toolbar::section_header_toolbar(dom_chips),
                        );

                        // Color-sync tag square (same convention as the TC header).
                        if let Some((_, tag_rect)) = th_result.top_toolbar.item_rects.iter().find(|(id, _)| id == "tc_color_tag") {
                            let tag_color = sidebar_panel_color_map.get(&s_pid.0).copied();
                            let css = match tag_color {
                                Some(c) => format!(
                                    "rgba({},{},{},1.0)",
                                    (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8
                                ),
                                None => "rgba(128,128,128,0.55)".to_string(),
                            };
                            let sq = 10.0;
                            ctx.set_fill_color(&css);
                            ctx.fill_rect(
                                tag_rect.x + (tag_rect.width - sq) / 2.0,
                                tag_rect.y + (tag_rect.height - sq) / 2.0,
                                sq,
                                sq,
                            );
                        }

                        {
                            use uzor::input::Sense;
                            use mlc_app_layout::ui::z_order::ZLayer;
                            // Section/panel header chrome (2026-08-08) — stays
                        // reachable underneath an open modal, same as the
                        // main toolbar. See `ZLayer::push_named_chrome`.
                        let tch_layer = ZLayer::Toolbar.push_named_chrome(&mut self.input_coordinator.borrow_mut(), "tch");
                            for (id, rect) in &th_result.top_toolbar.item_rects {
                                self.input_coordinator.borrow_mut().register_on_layer(
                                    format!("tch:{}", id),
                                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                                    Sense::CLICK,
                                    &tch_layer,
                                );
                            }
                            for (suffix, chev) in [
                                ("__chevron_left", th_result.top_left_chevron),
                                ("__chevron_right", th_result.top_right_chevron),
                            ] {
                                if let Some(r) = chev {
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("tch:{}", suffix),
                                        uzor::Rect::new(r.x, r.y, r.width, r.height),
                                        Sense::CLICK,
                                        &tch_layer,
                                    );
                                }
                            }
                            self.input_coordinator.borrow_mut().pop_layer(&tch_layer);

                            if let Some(ref dd) = th_result.dropdown_result {
                                let dd_layer = ZLayer::Dropdown.push_named(&mut self.input_coordinator.borrow_mut(), "tch_dropdown");
                                self.input_coordinator.borrow_mut().register_on_layer(
                                    "dropdown:__bg__",
                                    uzor::Rect::new(dd.menu_rect.x, dd.menu_rect.y, dd.menu_rect.width, dd.menu_rect.height),
                                    Sense::CLICK,
                                    &dd_layer,
                                );
                                for (item_id, rect) in &dd.item_rects {
                                    self.input_coordinator.borrow_mut().register_on_layer(
                                        format!("dropdown:{}:{}", dd.dropdown_id, item_id),
                                        uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                                        Sense::CLICK,
                                        &dd_layer,
                                    );
                                }
                                self.input_coordinator.borrow_mut().pop_layer(&dd_layer);
                            }
                        }
                        out_trading_header_result = Some(th_result);
                    }
                }

                // Agents header (Stage B agents reform): composer toolbar —
                // mode toggle icon + backend dropdown + gear. Same tch
                // instance and dispatch as the section headers.
                if rp == RSP::Agents {
                    let is_pty = self.sidebar_state.agent_spawn_mode == gate4agent::InstanceMode::Pty;
                    let (bl_full, bl_short) = mlc_app_sidebar_content::render::agent_backend_labels(
                        &self.sidebar_state.agent_selected_backend,
                    );
                    // Big/small name mechanics by free sidebar space.
                    let label = if sidebar_rect.width >= 220.0 { bl_full } else { bl_short };
                    let close_w = 36.0;
                    let header_rect = LayoutRect::new(
                        sidebar_rect.x,
                        sidebar_rect.y,
                        (sidebar_rect.width - close_w).max(0.0),
                        40.0,
                    );
                    let th_result = self.panel_app.render_section_header_toolbar(
                        &self.trading_header_tb,
                        ctx,
                        &header_rect,
                        "",
                        "",
                        "",
                        None,
                        false,
                        mlc_core::toolbar::agents_header_toolbar(is_pty, label),
                    );
                    {
                        use uzor::input::Sense;
                        use mlc_app_layout::ui::z_order::ZLayer;
                        // Section/panel header chrome (2026-08-08) — stays
                        // reachable underneath an open modal, same as the
                        // main toolbar. See `ZLayer::push_named_chrome`.
                        let tch_layer = ZLayer::Toolbar.push_named_chrome(&mut self.input_coordinator.borrow_mut(), "tch");
                        for (id, rect) in &th_result.top_toolbar.item_rects {
                            self.input_coordinator.borrow_mut().register_on_layer(
                                format!("tch:{}", id),
                                uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                                Sense::CLICK,
                                &tch_layer,
                            );
                        }
                        self.input_coordinator.borrow_mut().pop_layer(&tch_layer);
                        if let Some(ref dd) = th_result.dropdown_result {
                            let dd_layer = ZLayer::Dropdown.push_named(&mut self.input_coordinator.borrow_mut(), "tch_dropdown");
                            self.input_coordinator.borrow_mut().register_on_layer(
                                "dropdown:__bg__",
                                uzor::Rect::new(dd.menu_rect.x, dd.menu_rect.y, dd.menu_rect.width, dd.menu_rect.height),
                                Sense::CLICK,
                                &dd_layer,
                            );
                            for (item_id, rect) in &dd.item_rects {
                                self.input_coordinator.borrow_mut().register_on_layer(
                                    format!("dropdown:{}:{}", dd.dropdown_id, item_id),
                                    uzor::Rect::new(rect.x, rect.y, rect.width, rect.height),
                                    Sense::CLICK,
                                    &dd_layer,
                                );
                            }
                            self.input_coordinator.borrow_mut().pop_layer(&dd_layer);
                        }
                    }
                    out_trading_header_result = Some(th_result);
                }
            }

            Some(sidebar_result)
        } else {
            None
        };

        // 8a-8d. Modals (color pickers, watchlist, clock popup) moved to render_modal_layer().

        let out_last_watchlist_modal_result: Option<mlc_app_layout::layout::modals::watchlist_modal::WatchlistModalResult> = None;
        let out_last_wl_group_name_result: Option<WlGroupNameInputResult> = None;

        // 9. End frame — dispatch widget responses (clicks / drag / scroll).
        //
        // The hover SNAPSHOT this bakes is no longer used to resolve the cursor
        // type or hover highlights — those are now hit-tested LIVE on the pointer
        // event via InputCoordinator::hit_test_now (see on_mouse_move/get_cursor),
        // which eliminates the one-frame lag and does not depend on which layers
        // registered before vs after end_frame. end_frame stays here (baseline
        // cadence: once per render pass) purely for response dispatch.
        let _rt4 = web_time::Instant::now(); // checkpoint: after sidebar + modals
        let out_render_timing_us = (
            _rt2.duration_since(_rt1).as_micros() as u64, // chart
            _rt3.duration_since(_rt2).as_micros() as u64, // toolbar
            _rt4.duration_since(_rt3).as_micros() as u64, // sidebar + modals
            _rt1.duration_since(_rt0).as_micros() as u64, // layout + setup
        );
        let _responses = self.input_coordinator.borrow_mut().end_frame();

        RenderOutput {
            scale_corner_zones_by_leaf: out_scale_corner_zones,
            last_toolbar_result: out_last_toolbar_result,
            frame_result: out_frame_result,
            search_modal_result: out_search_modal_result,
            context_menu_result: out_context_menu_result,
            last_sidebar_result: out_last_sidebar_result,
            trading_header_result: out_trading_header_result,
            last_watchlist_modal_result: out_last_watchlist_modal_result,
            last_wl_group_name_result: out_last_wl_group_name_result,
            leaf_tab_hit_zones: out_leaf_tab_hit_zones,
            render_timing_us: out_render_timing_us,
            content_rect,
            right_toolbar_left_x: out_right_toolbar_left_x,
            last_inline_bar_rect: out_last_inline_bar_rect,
            last_favorites_bar_rect: out_last_favorites_bar_rect,
            open_submenu_update: out_open_submenu_update,
            sub_pane_range_writebacks: out_sub_pane_range_writebacks,
            sub_pane_overlay_writebacks: out_sub_pane_overlay_writebacks,
            loading_spinners: out_loading_spinners,
        }
    }

    /// Applies the [`RenderOutput`] returned by [`render_to_scene`] back to
    /// `self`, persisting cached render results for input handlers.
    ///
    /// Call this immediately after `render_to_scene` completes.
    pub fn apply_render_output(&mut self, output: RenderOutput) {
        self.scale_corner_zones_by_leaf = output.scale_corner_zones_by_leaf;
        self.last_toolbar_result = output.last_toolbar_result;
        self.frame_result = output.frame_result;
        self.search_modal_result = output.search_modal_result;
        self.context_menu_result = output.context_menu_result;
        self.last_sidebar_result = output.last_sidebar_result;
        self.last_trading_header_result = output.trading_header_result;

        // Persist agent terminal rect on sidebar_state for hover-focus in CursorMoved.
        self.sidebar_state.agent_terminal_rect = self
            .last_sidebar_result
            .as_ref()
            .and_then(|r| r.agent_terminal_rect)
            .map(|r| (r.x as f32, r.y as f32, r.width as f32, r.height as f32));

        // Resize focused PTY leaf when the terminal content area changes grid dimensions.
        let new_size = self
            .last_sidebar_result
            .as_ref()
            .and_then(|r| r.agent_terminal_size);
        if new_size.is_some() && new_size != self.sidebar_state.agent_terminal_size {
            self.sidebar_state.agent_terminal_size = new_size;
            // Resize ALL visible PTY leaves using their actual rect dimensions.
            let leaf_rects: Vec<(uzor::panels::LeafId, uzor::panels::PanelRect)> = self
                .sidebar_state
                .agent_docking
                .inner()
                .panel_rects()
                .iter()
                .map(|(&id, &r)| (id, r))
                .collect();
            for (leaf_id, rect) in leaf_rects {
                if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                    if desc.mode == gate4agent::InstanceMode::Pty {
                        // Only read on native — no PTY runtime to resize on wasm.
                        let _cols = ((rect.width / 7.0) as u16).max(1);
                        let _rows = ((rect.height / 19.0) as u16).max(1);
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(rt) = self.core.native_rt() {
                            rt.block_on(self.agent.resize_instance(desc.instance_id, _cols, _rows));
                        }
                    }
                }
            }
        }

        self.last_watchlist_modal_result = output.last_watchlist_modal_result;
        self.last_wl_group_name_result = output.last_wl_group_name_result;
        self.leaf_tab_hit_zones = output.leaf_tab_hit_zones;
        self.render_timing_us = output.render_timing_us;
        self.content_rect = output.content_rect;
        self.right_toolbar_left_x = output.right_toolbar_left_x;
        self.last_inline_bar_rect = output.last_inline_bar_rect;
        self.last_favorites_bar_rect = output.last_favorites_bar_rect;
        if let Some(submenu_update) = output.open_submenu_update {
            self.panel_app.toolbar_state.open_submenu_id = submenu_update;
        }
        // Write back render-computed sub-pane ranges (symmetrization + padding already applied).
        for (leaf_id, pane_idx, min, max) in output.sub_pane_range_writebacks {
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                let instance_id = crate::chart_container::collect_inner_instance_ids(
                    &window.inner_docking,
                ).into_iter().nth(pane_idx);
                if let Some(iid) = instance_id {
                    let entry = window.sub_pane_y_state
                        .entry(iid)
                        .or_default();
                    if entry.auto_scale {
                        entry.price_min = min;
                        entry.price_max = max;
                    }
                }
            }
        }
        // Write back sub-pane overlay button rects for next frame's hit testing.
        for (leaf_id, overlays) in output.sub_pane_overlay_writebacks {
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                window.sub_pane_overlay_results = overlays;
            }
        }
        // Loading-spinner geometry for this frame, read by the host renderer.
        self.last_loading_spinners = output.loading_spinners;
        // Auto-snap chat scroll to bottom when new messages arrive.
        // Check ALL chat leaves (not just focused) so switching to a leaf
        // that received messages while unfocused shows the latest content.
        {
            let chat_leaf_ids: Vec<uzor::panels::LeafId> = self.sidebar_state.agent_leaves.iter()
                .filter(|(_, d)| d.mode == gate4agent::InstanceMode::Chat)
                .map(|(id, _)| *id)
                .collect();
            for leaf_id in chat_leaf_ids {
                let new_len = self.sidebar_state.agent_leaf_snapshots.get(&leaf_id)
                    .and_then(|snap| {
                        if let mlc_app_sidebar_content::agent_types::AgentSnapshotMode::Chat(ref msgs) = snap.mode {
                            Some(msgs.len())
                        } else {
                            None
                        }
                    })
                    .unwrap_or(0);
                if new_len == 0 { continue; }
                // For the focused leaf use current-frame dimensions from sidebar_result.
                // For non-focused leaves we don't have exact dimensions — snap
                // offset to a large value; render will clamp to actual max_scroll.
                let is_focused = self.sidebar_state.focused_agent_leaf == Some(leaf_id);
                let scroll = self.sidebar_state.agent_chat_scrolls.entry(leaf_id).or_default();
                if is_focused {
                    if let Some(ref sidebar_result) = self.last_sidebar_result {
                        let content_h = sidebar_result.agent_chat_content_height;
                        let viewport_h = sidebar_result.agent_chat_viewport_h;
                        let max_scroll = (content_h - viewport_h).max(0.0);
                        // Resolve the f64::MAX sentinel to the real clamped value so
                        // that the was_at_bottom check reflects where the renderer
                        // actually placed the viewport.  Without this, offset stays
                        // at f64::MAX permanently and the user can never scroll up.
                        if scroll.offset >= 1e18 {
                            scroll.offset = max_scroll;
                        }
                        let was_at_bottom = scroll.offset >= (max_scroll - 1.0).max(0.0);
                        if was_at_bottom {
                            scroll.offset = max_scroll;
                        }
                    }
                } else {
                    // Non-focused: resolve the sentinel using per-leaf dimensions
                    // when available, so that was_at_bottom detection works the
                    // same way as for the focused leaf.  This prevents the offset
                    // from staying at f64::MAX forever and reverting after new
                    // messages arrive.
                    if let Some((content_h, vp_h)) = self.last_sidebar_result
                        .as_ref()
                        .and_then(|sr| sr.agent_leaf_content_heights.get(&leaf_id).copied())
                    {
                        let max_scroll = (content_h - vp_h).max(0.0);
                        if scroll.offset >= 1e18 {
                            scroll.offset = max_scroll;
                        }
                        let was_at_bottom = scroll.offset >= (max_scroll - 1.0).max(0.0);
                        if was_at_bottom {
                            scroll.offset = max_scroll;
                        }
                    } else {
                        // No dimensions yet (first frame / leaf not rendered).
                        // Keep sentinel so render will show bottom on first display.
                        if scroll.offset == 0.0 {
                            scroll.offset = f64::MAX;
                        }
                    }
                }
            }
        }
    }

    /// Render ONLY the toolbar vector graphics into `ctx`.
    ///
    /// This is used by the dirty-caching path in `chart-app-vello`: when
    /// `toolbar_dirty` is set, the renderer calls this method with a context
    /// wrapping the dedicated `toolbar_scene`, caches it, and composites it
    /// on top of the main scene every frame.
    ///
    /// Updates `self.last_toolbar_result` so that subsequent frames with
    /// `skip_toolbar_draw=true` still have correct hit zones.
    ///
    /// Does NOT call `input_coordinator.begin_frame()` / `end_frame()` — that
    /// is the responsibility of the enclosing `render()` call.
    pub fn render_toolbar_only(&mut self, ctx: &mut dyn RenderContext) {
        let w = self.width as f64;
        let h = self.height as f64;
        let sidebar_w = self.sidebar_state.right_width();

        let window_rect = LayoutRect::new(0.0, 0.0, w, h);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        let selected_config: Option<mlc_core::state::selected_config::SelectedPrimitiveConfig> = {
            let bid2 = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
            let drawings_guard2 = self.core.drawings();
            bid2.and_then(|bid| drawings_guard2.selected_config(bid))
        };

        let clock_time = {
            let utc_secs = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .map(|d| d.as_secs() as i64)
                .unwrap_or(0);
            if let Some(window) = self.panel_app.panel_grid.active_container() {
                let time_fmt = &window.scale_settings.time_format;
                let time = time_fmt.format_clock_time(utc_secs);
                if !time_fmt.show_utc_prefix {
                    time
                } else if time_fmt.timezone_offset_hours >= 0 {
                    format!("[UTC+{}] {}", time_fmt.timezone_offset_hours, time)
                } else {
                    format!("[UTC{}] {}", time_fmt.timezone_offset_hours, time)
                }
            } else {
                let hours = (utc_secs / 3600) % 24;
                let minutes = (utc_secs / 60) % 60;
                let seconds = utc_secs % 60;
                format!("[UTC+0] {:02}:{:02}:{:02}", hours, minutes, seconds)
            }
        };

        // S2.7.1: read active symbol/timeframe from branch_meta.
        let (mut active_sym_str, active_tf_str, active_gid, mut active_exch_str, mut active_at_str) = {
            let cid = self.panel_app.panel_grid.active_container_id();
            let sym = cid.and_then(|c| self.symbol_for_chart(c))
                .map(String::from).unwrap_or_default();
            let tf = cid.and_then(|c| self.timeframe_for_chart(c))
                .map(|tf| tf.name.clone())
                .unwrap_or_default();
            let gid = cid.and_then(|c| self.bubble_for_chart(c));
            // Adaptive symbol-button suffix (owner spec 2026-07-17) — see
            // the `render()` call site for rationale.
            let exch = cid.and_then(|c| self.exchange_for_chart(c)).unwrap_or_default();
            let at = cid.and_then(|c| self.account_type_for_chart(c))
                .map(|a| a.as_label().to_string()).unwrap_or_default();
            (sym, tf, gid, exch, at)
        };
        // Workspace-panel arc: when focus is the Trading tab's container,
        // the topbar's symbol/exchange/account_type button derives from the
        // container instead of the active chart — opening symbol-search
        // from it commits back to the container (reader 1,
        // `commit_symbol_search_to_trading_panel`). Timeframe/chart-type/
        // allowed-timeframes stay chart-derived regardless (Stage 4's
        // suppress-list below hides those buttons entirely instead of
        // re-deriving their content for a container that has neither).
        let trading_focus_pid = self.workspace_focus().trading_panel();
        if let Some(pid) = trading_focus_pid {
            if let Some(tc) = self.panels_store.trading_container.get(&pid) {
                active_sym_str = tc.symbol.clone();
                active_exch_str = tc.exchange.clone();
                active_at_str = tc.account_type.clone();
            }
        }
        // Stage 4 ON mode: topbar-takeover bool AND focus on the Trading
        // tab's container together suppress every chart-semantic button.
        let suppress_chart_controls = trading_focus_pid.is_some()
            && self.core.user().profile().ui.trading_panel_topbar_takeover;

        self.stamp_active_right_toolbar_id();
        let bubble_guard_tb2 = self.core.bubbles();
        let tags_guard_tb2 = self.core.tags();
        let presets_guard_tb2 = self.core.presets();
        let (active_sync_flags_owned, active_is_mono_group) = active_gid
            .and_then(|gid| bubble_guard_tb2.sync_flags(gid).map(|sf| (Some(sf), tags_guard_tb2.get(gid).map(|t| t.members.len() <= 1).unwrap_or(true))))
            .unwrap_or((None, true));
        // TF picker gating (audit D8 follow-up, 2026-07-04) — see the
        // `render()` call site for rationale; kept in parity here per the
        // shell-web/native toolbar-duty binding invariant.
        let active_ct_str: Option<&'static str> = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.chart_type_for_chart(cid));
        let active_ct_param: Option<f64> = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.chart_type_param_for_chart(cid));
        let allowed_tfs = self.panel_app.panel_grid.active_container_id().map(|cid| {
            let chart_type = self.chart_type_for_chart(cid).unwrap_or("candles");
            let active_series = self.active_series_for_chart(cid);
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as i64;
            crate::active_series::allowed_timeframes(active_series.as_active_series(), chart_type, now_ms)
        }).unwrap_or(None);
        let chart_type_data_modes = self.chart_type_data_modes();
        let timeframe_native_map = self.timeframe_native_map();
        let footprint_live_only = self.footprint_live_only();
        let granularity_favorites = self.core.user().profile().domain.granularity_favorites.clone();
        let favorite_primitives = self.core.user().profile().ui.favorite_primitives.clone();
        let favorites_drag_render = self.fav_item_drag.as_ref().map(|(tool_id, cx, cy)|
            mlc_core::state::toolbar_state::FavoritesDragRender { tool_id: tool_id.clone(), cursor_x: *cx, cursor_y: *cy });
        let granularity_edit_render = self.granularity_edit_render();
        let pin_granularity_favorites = self.core.user().profile().ui.pin_granularity_favorites;
        let granularity_drag_render = self.granularity_drag_render();
        let split_symbol_compare = self.core.user().profile().ui.split_symbol_compare;
        // Target-gated (owner defect 2026-07-25) — see the twin block in
        // `render_to_scene` for the rationale.
        let symbol_quick_search_render = match self.symbol_quick_edit.as_ref().map(|e| e.target) {
            Some(SymbolQuickEditTarget::TopbarChart) => self.symbol_quick_search_render(),
            _ => None,
        };
        let symbol_quick_search = self.core.user().profile().ui.symbol_quick_search;
        let clock_glitch_enabled = self.core.user().profile().ui.clock_glitch_enabled;
        // `render_toolbar_only` has no threaded `current_time_ms` (see
        // `granularity_edit_render`'s own doc comment above for why that
        // param was never added here) — a coarse wall-clock read, same
        // idiom this method already uses a few lines above
        // (`allowed_tfs`'s own `now_ms`), is precise enough for the clock
        // hover-glitch shimmer cadence.
        let toolbar_anim_time_s = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .map(|d| d.as_secs_f64())
            .unwrap_or(0.0);
        let is_capturing_screenshot = self.is_capturing_screenshot();
        let toolbar_result = self.panel_app.render_toolbars_with_theme(
            ctx,
            &panel_layout,
            selected_config.as_ref(),
            Some(clock_time.as_str()),
            self.split_without_group,
            Some(active_sym_str.as_str()),
            Some(active_tf_str.as_str()),
            sidebar_w,
            active_sync_flags_owned.as_ref(),
            active_is_mono_group,
            presets_guard_tb2.values(),
            allowed_tfs.as_ref(),
            active_ct_str,
            active_ct_param,
            chart_type_data_modes.as_ref(),
            timeframe_native_map.as_ref(),
            footprint_live_only,
            &granularity_favorites,
            &favorite_primitives,
            granularity_edit_render.as_ref(),
            pin_granularity_favorites,
            granularity_drag_render.as_ref(),
            favorites_drag_render.as_ref(),
            split_symbol_compare,
            symbol_quick_search_render.as_ref(),
            symbol_quick_search,
            Some(active_exch_str.as_str()),
            Some(active_at_str.as_str()),
            suppress_chart_controls,
            Some(self.last_mouse_pos),
            toolbar_anim_time_s,
            // `render_toolbar_only` is the dirty-cache repaint path and has
            // no threaded frame number; bump the same counter here so the
            // animation keeps advancing when only the toolbar is redrawn.
            {
                self.anim_frame = self.anim_frame.wrapping_add(1);
                self.anim_frame
            },
            clock_glitch_enabled,
            is_capturing_screenshot,
        );
        drop(bubble_guard_tb2);
        drop(tags_guard_tb2);
        drop(presets_guard_tb2);

        self.last_toolbar_result = Some(toolbar_result);
    }

    /// Render ONLY the sidebar vector graphics into `ctx`.
    ///
    /// This is used by the dirty-caching path in `chart-app-vello`: when
    /// `sidebar_dirty_scene` is set, the renderer calls this method with a
    /// context wrapping the dedicated `sidebar_scene` to rebuild the cached
    /// scene.  It must be called AFTER `render()` so that the widget
    /// registrations go into the frame already opened by `render()`'s
    /// `input_coordinator.begin_frame()`.
    ///
    /// Does NOT call `input_coordinator.begin_frame()` / `end_frame()` — that
    /// is the responsibility of the enclosing `render()` call (same contract as
    /// `render_toolbar_only`).
    pub fn render_sidebar_only(&mut self, ctx: &mut dyn RenderContext) {
        let w = self.width as f64;
        let h = self.height as f64;
        let sidebar_w = self.sidebar_state.right_width();

        if !self.sidebar_state.is_right_open() {
            return;
        }

        // Sync cursor blink state — render() does this in prepare_frame,
        // but render_sidebar_only() skips prepare_frame so we must do it here.
        let chat_id = WidgetId::from(text_input::AGENT_CHAT);
        if self.input_coordinator.borrow().text_fields().is_focused(&chat_id) {
            let now_ms = web_time::SystemTime::now()
                .duration_since(web_time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            self.sidebar_state.agent_input_cursor_visible =
                self.input_coordinator.borrow().text_fields().cursor_visible(now_ms);
            self.sidebar_state.agent_input_focused_leaf = self.sidebar_state.focused_agent_leaf;
        }

        let window_rect = LayoutRect::new(0.0, 0.0, w, h);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        // Compute content_rect (same logic as in render()) so sidebar_x is correct.
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };

        let top_h = panel_layout.top_toolbar_rect.height;
        let bottom_h = panel_layout.bottom_toolbar_rect.height;
        let sidebar_x = content_rect.x + content_rect.width;
        let sidebar_y = top_h;
        let sidebar_h = h - top_h - bottom_h;

        let sidebar_rect = LayoutRect::new(sidebar_x, sidebar_y, sidebar_w, sidebar_h);
        let sidebar_toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let panel_theme = panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());

        // Provide current agent state to sidebar for the Agents panel.
        // Snapshot each registered leaf instance.
        {
            let leaf_ids: Vec<uzor::panels::LeafId> = self.sidebar_state.agent_leaves.keys().copied().collect();
            for leaf_id in leaf_ids {
                if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                    if let Some(snap) = self.agent.snapshot_instance(desc.instance_id) {
                        self.sidebar_state.agent_leaf_snapshots.insert(leaf_id, snap);
                    }
                }
            }
        }

        // Pre-compute per-panel maps before render_right_sidebar mut-borrows sidebar_state.
        let (panel_color_map_main, panel_sync_flags_map_main): (
            std::collections::HashMap<u64, [f32; 4]>,
            std::collections::HashMap<u64, mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot>,
        ) = {
            use mlc_core::ids::SyncMemberId;
            let bubbles = self.core.bubbles();
            let tags = self.core.tags();
            let mut colors = std::collections::HashMap::new();
            let mut flags = std::collections::HashMap::new();
            for (tag_id, tag) in tags.iter() {
                let bubble_default = bubbles.sync_flags(*tag_id).unwrap_or_default();
                for &member in &tag.members {
                    if let SyncMemberId::Panel(pid) = member {
                        if !tag.auto_created {
                            colors.insert(pid, tag.color);
                        }
                        flags.insert(pid, mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot {
                            sync_symbol: tag.effective_sync_symbol(member, bubble_default.sync_symbol),
                            sync_crosshair: tag.effective_sync_crosshair(member, bubble_default.sync_crosshair),
                        });
                    }
                }
            }
            (colors, flags)
        };

        // Pre-compute watchlist column config + a clone of the active list
        // (for color-flag lookups by InstrumentAddress) before
        // render_right_sidebar mut-borrows self.sidebar_state.
        let watchlist_col_cfg_main = self.watchlist_ui.column_config(self.watchlist_ui.active_list_id);
        let watchlist_active_list_main: Option<mlc_core::WatchlistList> = self.core.watchlists()
            .find(self.watchlist_ui.active_list_id)
            .cloned();

        // Trading-header-symbol-edit / trading-header-tf arcs (2026-07-25):
        // read BEFORE `panels_store` borrows `self` immutably below.
        let trading_symbol_edit_render_main = self.symbol_quick_edit.as_ref().and_then(|e| match e.target {
            crate::SymbolQuickEditTarget::TradingPanel(pid)
            | crate::SymbolQuickEditTarget::SectionPanel(pid)
            | crate::SymbolQuickEditTarget::MultiDomRoster(pid)
            | crate::SymbolQuickEditTarget::MultiTapeRoster(pid) => {
                self.symbol_quick_search_render().map(|r| (pid.0, r))
            }
            crate::SymbolQuickEditTarget::TopbarChart => None,
        });

        let panels_store = &self.panels_store;
        let sidebar_result = mlc_app_sidebar_content::render::render_right_sidebar(
            ctx,
            &sidebar_rect,
            &mut self.sidebar_state,
            &sidebar_toolbar_theme,
            &mut self.input_coordinator.borrow_mut(),
            &mut |item, rect, ctx, coord, slot_prefix| {
                panels_render::render_free_item(panels_store, item, rect.0, rect.1, rect.2, rect.3, ctx, &panel_theme, coord, slot_prefix);
            },
            &|item: &mlc_core::state::Panel| -> Option<(bool, f64, f64, bool)> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id).map(|s| (s.auto_center, s.tick_size, s.min_volume_filter, s.multi_mode)),
                    _ => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<[f32; 4]> {
                panel_color_map_main.get(&item.panel_id().0).copied()
            },
            &|item: &mlc_core::state::Panel| -> Option<mlc_app_sidebar_content::render::PanelSyncFlagsSnapshot> {
                panel_sync_flags_map_main.get(&item.panel_id().0).cloned()
            },
            &|item: &mlc_core::state::Panel| -> Option<(String, String, String, String)> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::Footprint(id) => panels_store.footprint.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::VolumeProfile(id) => panels_store.volume_profile.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::LiquidityHeatmap(id) => panels_store.liquidity_heatmap.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::LiqTape(id) => panels_store.liq_tape.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::OrderEntry(id) => panels_store.order_entry.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::TradingContainer(id) => panels_store.trading_container.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), s.timeframe.clone())),
                    Panel::IndicatorPanel(id) => panels_store.indicator_panel.get(id)
                        .map(|s| (s.symbol.clone(), s.exchange.clone(), s.account_type.clone(), String::new())),
                    Panel::PositionManager(_)
                    | Panel::TradeLog(_)
                    | Panel::RiskCalculator(_)
                    // Chart renders via dedicated path — no overlay info.
                    | Panel::Chart(_) => None,
                }
            },
            &|item: &mlc_core::state::Panel| -> Option<Vec<bool>> {
                use mlc_core::state::Panel;
                match item {
                    Panel::Dom(id) => panels_store.dom.get(id).map(|s| vec![
                        s.column_config.show_bid_orders,
                        s.column_config.show_sell_trades,
                        s.column_config.show_buy_trades,
                        s.column_config.show_ask_orders,
                    ]),
                    Panel::L2Tape(id) => panels_store.l2_tape.get(id).map(|s| vec![
                        s.column_config.show_time,
                        s.column_config.show_type,
                        s.column_config.show_side,
                        s.column_config.show_price,
                        s.column_config.show_qty,
                    ]),
                    Panel::TradeTape(id) => panels_store.trade_tape.get(id).map(|s| vec![
                        s.column_config.show_time,
                        s.column_config.show_price,
                        s.column_config.show_size,
                    ]),
                    Panel::BigTrades(id) => panels_store.big_trades.get(id).map(|s| vec![
                        s.column_config.show_time,
                        s.column_config.show_side,
                        s.column_config.show_price,
                        s.column_config.show_size,
                        s.column_config.show_notional,
                    ]),
                    _ => None,
                }
            },
            &watchlist_col_cfg_main,
            &|address: &mlc_core::InstrumentAddress| -> Option<String> {
                watchlist_active_list_main.as_ref()
                    .and_then(|l| l.get_color_flag(address))
                    .map(|s| s.to_string())
            },
        );

        // Persist agent terminal rect for hover-focus.
        self.sidebar_state.agent_terminal_rect = sidebar_result
            .agent_terminal_rect
            .map(|r| (r.x as f32, r.y as f32, r.width as f32, r.height as f32));

        // Resize focused PTY leaf when the terminal content area changes grid dimensions.
        let new_size = sidebar_result.agent_terminal_size;
        if new_size.is_some() && new_size != self.sidebar_state.agent_terminal_size {
            self.sidebar_state.agent_terminal_size = new_size;
            // Resize ALL visible PTY leaves using their actual rect dimensions.
            let leaf_rects: Vec<(uzor::panels::LeafId, uzor::panels::PanelRect)> = self
                .sidebar_state
                .agent_docking
                .inner()
                .panel_rects()
                .iter()
                .map(|(&id, &r)| (id, r))
                .collect();
            for (leaf_id, rect) in leaf_rects {
                if let Some(desc) = self.sidebar_state.agent_leaves.get(&leaf_id).cloned() {
                    if desc.mode == gate4agent::InstanceMode::Pty {
                        // Only read on native — no PTY runtime to resize on wasm.
                        let _cols = ((rect.width / 7.0) as u16).max(1);
                        let _rows = ((rect.height / 19.0) as u16).max(1);
                        #[cfg(not(target_arch = "wasm32"))]
                        if let Some(rt) = self.core.native_rt() {
                            rt.block_on(self.agent.resize_instance(desc.instance_id, _cols, _rows));
                        }
                    }
                }
            }
        }

        // Update text-field geometry for agent chat input field.
        if let (Some(rect), Some(char_positions)) = (
            sidebar_result.agent_input_rect,
            sidebar_result.agent_input_char_positions.clone(),
        ) {
            let chat_id = WidgetId::from(text_input::AGENT_CHAT);
            self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                &chat_id,
                (rect.x, rect.y, rect.width, rect.height),
                char_positions,
            );
        }

        // Stamp AGENT_PTY as on-screen whenever the focused leaf's terminal
        // is drawn (stale-focus audit 2026-09-23 — this field was never
        // stamped at all, unlike AGENT_CHAT right above it). A raw PTY grid
        // has no char-boundary caret of its own (selection is the leaf's
        // OWN `agent_pty_selections`, not TextFieldStore's), so this uses
        // `update_field` with an empty char-position list rather than
        // inventing fake ones — nothing reads `last_char_positions` for a
        // Raw-capability field, so this cannot change click behavior.
        if let Some(rect) = sidebar_result.agent_terminal_rect {
            let pty_id = WidgetId::from(text_input::AGENT_PTY);
            self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                &pty_id,
                (rect.x, rect.y, rect.width, rect.height),
                Vec::new(),
            );
        }

        // Register panel overlay zones (color tag + gear) on InputCoordinator
        // so on_click routes through process_click → dispatch_panel_click.
        // Also done in `render_to_scene` (the path every shell, incl. wasm,
        // actually calls) — see that call site for why. Re-registering here
        // for the vello dirty-scene-cache path is idempotent.
        self.register_panel_overlay_zones(&sidebar_result);

        // Trading container embedded header toolbar — the vello dirty-scene
        // cache composites this sidebar scene OVER `render_to_scene`'s
        // pixels, so the header toolbar must be drawn into THIS scene too
        // (draw-only; hit zones already registered by `render_to_scene`).
        if self.sidebar_state.right_panel == mlc_app_sidebar_content::state::RightSidebarPanel::SlotTrading {
            // Embedded center chart — draw-only vello copy (the dirty-scene
            // cache composites this sidebar scene OVER `render_to_scene`'s
            // pixels; without this copy the chart vanishes on the native
            // shell whenever only the sidebar scene is rebuilt). Hit zones /
            // rect cache are owned by `render_to_scene`.
            {
                let tc_center: Option<(mlc_core::state::PanelId, LayoutRect)> = self
                    .panels_store
                    .trading_container
                    .iter()
                    .next()
                    .and_then(|(pid, tc)| {
                        sidebar_result
                            .item_rects
                            .iter()
                            .find(|(id, _)| {
                                id.starts_with("slot:") && id.ends_with(":focus_content")
                            })
                            .and_then(|(_, r)| {
                                tc.embedded_chart_rect(r.x, r.y, r.width, r.height)
                                    .map(|(dx, dy, dw, dh)| (*pid, LayoutRect::new(dx, dy, dw, dh)))
                            })
                    });
                if let Some((tc_pid_c, center_rect)) = tc_center {
                    self.resize_trading_center_geometry(tc_pid_c, &center_rect);
                    let _ = self.draw_trading_center_chart(ctx, tc_pid_c, &center_rect);
                }

                // Free-mode overlay pass — draw-only vello copy (same
                // ordering contract as `render_to_scene`: after the
                // embedded chart; widget registration is idempotent).
                if let Some(tc) = self.panels_store.trading_container.values().next() {
                    let overlay_theme =
                        panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());
                    tc.render_free_overlay(
                        ctx,
                        &overlay_theme,
                        &mut self.input_coordinator.borrow_mut(),
                        self.rc_modal.is_open || self.tc_modal.is_open,
                    );
                }
            }

            if let Some(tc) = self.panels_store.trading_container.values().next() {
                let close_w = 36.0;
                let header_rect = LayoutRect::new(
                    sidebar_rect.x,
                    sidebar_rect.y,
                    (sidebar_rect.width - close_w).max(0.0),
                    40.0,
                );
                let th_gran_favs = self.core.user().profile().domain.granularity_favorites.clone();
                let th_pin_favs = self.core.user().profile().ui.pin_granularity_favorites;
                let th_gran_edit = self.granularity_edit_render();
                let th_gran_drag = self.granularity_drag_render();
                let th_tf_native = self.timeframe_native_map();
                let th_sq_render = trading_symbol_edit_render_main.as_ref().map(|(_, r)| r);
                let th_sqs_on = self.core.user().profile().ui.symbol_quick_search;
                let th_active_ct_main: Option<&'static str> = self.panels_store.trading_container
                    .keys().next().copied()
                    .and_then(|pid| self.trading_center_charts.get(&pid).copied())
                    .and_then(|cid| self.chart_type_for_chart(cid));
                let _ = self.panel_app.render_trading_header_toolbar(
                    &self.trading_header_tb,
                    ctx,
                    &header_rect,
                    &tc.symbol,
                    &tc.timeframe,
                    &tc.exchange,
                    &tc.account_type,
                    &th_gran_favs,
                    th_gran_edit.as_ref(),
                    th_pin_favs,
                    th_gran_drag.as_ref(),
                    th_sq_render,
                    th_sqs_on,
                    th_tf_native.as_ref(),
                    tc.center_kind.label(),
                    {
                        use mlc_app_panels::trading::trading::trading_container::{CenterKind, TcLayoutMode};
                        match tc.layout_mode {
                            TcLayoutMode::Fixed => tc.center_kind == CenterKind::Chart,
                            TcLayoutMode::Free => tc.has_free_chart_leaf(),
                        }
                    },
                    tc.layout_mode == mlc_app_panels::trading::trading::trading_container::TcLayoutMode::Free,
                    th_active_ct_main,
                );
            }


        }

        // Data-section header toolbar — draw-only vello copy (hit zones are
        // registered by `render_to_scene`; parity note as for the TC header).
        {
            use mlc_app_sidebar_content::state::RightSidebarPanel as RSP;
            let rp = self.sidebar_state.right_panel;
            if matches!(rp, RSP::SlotDom | RSP::SlotTape | RSP::SlotL2 | RSP::SlotBigTrades | RSP::SlotLiq) {
                if let Some(item) = self.section_panel_for(rp) {
                    let s_pid = item.panel_id();
                    let (s_sym, s_ex, s_at) = self.section_symbol_triplet(&item);
                    let dom_chips = if let mlc_core::state::Panel::Dom(id) = &item {
                        self.panels_store.dom.get(id).map(|s| if s.multi_mode {
                            mlc_core::toolbar::SectionDomChips {
                                multi_mode: true,
                                agg_mode: s.agg_mode,
                                locked: s.multi.locked,
                                min_volume_filter: s.multi.min_level_qty,
                                tick_size: s.multi.config.grid_size,
                                show_imbalance: Some(s.multi.config.show_imbalance),
                            }
                        } else {
                            mlc_core::toolbar::SectionDomChips {
                                multi_mode: false,
                                agg_mode: false,
                                locked: s.auto_center,
                                min_volume_filter: s.min_volume_filter,
                                tick_size: s.tick_size,
                                show_imbalance: None,
                            }
                        })
                    } else {
                        None
                    };
                    let close_w = 36.0;
                    let header_rect = LayoutRect::new(
                        sidebar_rect.x,
                        sidebar_rect.y,
                        (sidebar_rect.width - close_w).max(0.0),
                        40.0,
                    );
                    let th_sq_render = trading_symbol_edit_render_main.as_ref().map(|(_, r)| r);
                    let th_sqs_on = self.core.user().profile().ui.symbol_quick_search;
                    let th_result = self.panel_app.render_section_header_toolbar(
                        &self.trading_header_tb,
                        ctx,
                        &header_rect,
                        &s_sym,
                        &s_ex,
                        &s_at,
                        th_sq_render,
                        th_sqs_on,
                        mlc_core::toolbar::section_header_toolbar(dom_chips),
                    );
                    if let Some((_, tag_rect)) = th_result.top_toolbar.item_rects.iter().find(|(id, _)| id == "tc_color_tag") {
                        let tag_color = panel_color_map_main.get(&s_pid.0).copied();
                        let css = match tag_color {
                            Some(c) => format!(
                                "rgba({},{},{},1.0)",
                                (c[0] * 255.0) as u8, (c[1] * 255.0) as u8, (c[2] * 255.0) as u8
                            ),
                            None => "rgba(128,128,128,0.55)".to_string(),
                        };
                        let sq = 10.0;
                        ctx.set_fill_color(&css);
                        ctx.fill_rect(
                            tag_rect.x + (tag_rect.width - sq) / 2.0,
                            tag_rect.y + (tag_rect.height - sq) / 2.0,
                            sq,
                            sq,
                        );
                    }
                }
            }
        }

        self.last_sidebar_result = Some(sidebar_result);
    }

    /// Register the color-tag square + gear hit zones for every free-slot
    /// leaf's overlay tab (`panel_overlay_zones`) on the InputCoordinator's
    /// `"toolbar"` layer, so `on_click` → `process_click` →
    /// `dispatch_panel_click`'s `panel_overlay:{pid}:color_tag|gear` branch
    /// can reach them. Must run on every render path that produces a
    /// `RightSidebarResult` — `render_to_scene` (used by every shell) and
    /// `render_sidebar_only` (vello dirty-scene-cache path) both call this.
    fn register_panel_overlay_zones(&mut self, sidebar_result: &mlc_app_sidebar_content::render::RightSidebarResult) {
        for (panel_id, _leaf_id, zones) in &sidebar_result.panel_overlay_zones {
            let [cx, cy, cw, ch] = zones.color_tag_rect;
            if cw > 0.0 && ch > 0.0 {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("panel_overlay:{}:color_tag", panel_id),
                    uzor::types::Rect::new(cx, cy, cw, ch),
                    uzor::input::Sense::CLICK,
                    &uzor::input::LayerId::new("toolbar"),
                );
            }
            let [dx, dy, dw, dh] = zones.dots_rect;
            if dw > 0.0 && dh > 0.0 {
                self.input_coordinator.borrow_mut().register_on_layer(
                    format!("panel_overlay:{}:gear", panel_id),
                    uzor::types::Rect::new(dx, dy, dw, dh),
                    uzor::input::Sense::CLICK,
                    &uzor::input::LayerId::new("toolbar"),
                );
            }
        }
    }

    /// Render panel-specific overlay popups that must appear above the sidebar.
    ///
    /// Renders the sync color grid popup when it targets a trading panel.
    /// Skipped in `render_to_scene` and rendered here instead so that it is
    /// composited after the sidebar scene and is never covered by it.
    ///
    /// Does NOT call `input_coordinator.begin_frame()` / `end_frame()` — that
    /// is the responsibility of the enclosing `render()` call (same contract as
    /// `render_toolbar_only` and `render_sidebar_only`).
    /// Cheap predicate: is there any panel-overlay popup to draw this frame?
    /// Lets the scene builder skip the per-frame `Scene::new()` + render + append
    /// when nothing is open (the common case).
    ///
    /// Always `false` while [`is_capturing_screenshot`] is true — these
    /// popups can sit directly over a chart panel, which is exactly the
    /// captured rect (screenshot-is-chart-only arc, 2026-09-05).
    ///
    /// [`is_capturing_screenshot`]: Self::is_capturing_screenshot
    pub fn has_panel_overlay_popups(&self) -> bool {
        if self.is_capturing_screenshot() {
            return false;
        }
        (self.panel_app.sync_color_grid.is_open() && self.panel_app.sync_color_grid.target_is_panel())
            || matches!(
                self.symbol_quick_edit.as_ref().map(|e| e.target),
                Some(SymbolQuickEditTarget::TradingPanel(_))
                    | Some(SymbolQuickEditTarget::SectionPanel(_))
                    | Some(SymbolQuickEditTarget::MultiDomRoster(_))
                    | Some(SymbolQuickEditTarget::MultiTapeRoster(_))
            )
    }

    pub fn render_panel_overlay_popups(&self, ctx: &mut dyn RenderContext) {
        let toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let w = self.width as f64;
        let h = self.height as f64;

        // Panel-targeting sync color grid popup.
        if self.panel_app.sync_color_grid.is_open() && self.panel_app.sync_color_grid.target_is_panel() {
            use mlc_app_layout::layout::ChartModalLayout;
            let modal_layout = ChartModalLayout {
                prim_screen_w: w,
                prim_screen_h: h,
                prim_modal_y: 60.0,
                ind_screen_w: w,
                ind_screen_h: h,
                chart_x: 0.0,
                chart_y: 0.0,
                content_w: w,
                content_h: h,
            };
            let target_panel_group_color = self
                .panel_app
                .sync_color_grid
                .target_panel
                .and_then(|pid| {
                    let member = mlc_core::SyncMemberId::Panel(pid);
                    let tags = self.core.tags();
                    let gid = tags.tag_for_member(member)?;
                    let tag = tags.get(gid)?;
                    (!tag.auto_created).then_some(tag.color)
                });
            self.panel_app.render_color_picker_popups(
                ctx,
                &modal_layout,
                &toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
                target_panel_group_color,
            );
        }

        // (Bespoke trading-header popups removed 2026-07-26 — the embedded
        // header toolbar's composer dropdowns replaced them.)
   }

    /// Warm up all enabled connectors exactly once, via the shared bridge.
    ///
    /// The one-shot guard lives in the BRIDGE (which outlives the skeleton→live
    /// promote), not on ChartApp (which is dropped+recreated by the promote). So
    /// the dials fire once — during the empty skeleton/loading screen — and the
    /// live window's call is a no-op. This replaces the old per-ChartApp batch
    /// cursor, which reset on promote and re-walked all 21 exchanges in the
    /// visible chart (the source of the 30-40s startup churn). The dials run
    /// concurrently on the bridge's own runtime, off the UI thread.
    pub fn warmup_connectors(&self) {
        let enabled: Vec<digdigdig3::ExchangeId> = ALL_EXCHANGE_IDS
            .iter()
            .copied()
            .filter(|eid| {
                self.sidebar_state.connector_enabled
                    .get(eid.as_str())
                    .copied()
                    .unwrap_or(true)
            })
            .collect();
        self.core.bridge_handle().warmup_all_connectors(&enabled);
    }

    // -------------------------------------------------------------------------
    // Modal layer (drawn ABOVE the crosshair overlay)
    // -------------------------------------------------------------------------

    /// Right-sidebar width to reserve when computing the modal/search-overlay
    /// world rect (`ChartModalLayout` / `ChartScreenArea`).
    ///
    /// Every other right-sidebar panel is a passive display panel, so modals
    /// and the search overlay narrow their world by the sidebar's width to
    /// avoid drawing under it. The Trading tab is different (owner directive,
    /// 2026-07 workspace-panel arc): it is an equal interactive workspace
    /// panel, not a passive sidebar tab, so modals/search may position over
    /// it — treat the sidebar as closed for modal-layout purposes ONLY. The
    /// sidebar itself keeps its real width; only this derived layout input
    /// changes. Single source for both `render_modal_layer`'s
    /// `ChartModalLayout` and the search overlay's `ChartScreenArea` — they
    /// both consume `content_rect`, which is derived from
    /// this one value, so there is nothing to fork per-modal.
    fn effective_modal_sidebar_w(&self) -> f64 {
        use mlc_app_sidebar_content::state::RightSidebarPanel;
        if self.sidebar_state.right_panel == RightSidebarPanel::SlotTrading {
            0.0
        } else {
            self.sidebar_state.right_width()
        }
    }

    /// Draw all modal / popup / dialog layers into `ctx` and store their results
    /// directly into `self` (no `RenderOutput` — same contract as
    /// `render_toolbar_only` / `render_sidebar_only`).
    ///
    /// # Drawing order
    /// chart → overlay(crosshair) → **modals** (this method)
    ///
    /// The VelloGpu compositor appends the scene produced here AFTER the overlay
    /// scene, ensuring modals always draw on top of the crosshair.
    ///
    /// # Input-frame contract
    /// Does NOT call `input_coordinator.begin_frame()` / `end_frame()`.
    /// Widget registrations made here accumulate in `self.input_coordinator.widgets`
    /// and are cleared by the NEXT frame's `begin_frame()` — exactly the same
    /// contract as `render_panel_overlay_popups`.  Input events that arrive between
    /// renders will hit-test against these registrations, which is correct because
    /// modal hit-zones must beat chart/toolbar zones (modals are the topmost layer).
    ///
    /// Skips painting entirely while [`is_capturing_screenshot`] is true —
    /// modals, context menus, the watchlist popup, panel tree manager and
    /// every drag-preview this method draws are chrome, not chart content
    /// (screenshot-is-chart-only arc, 2026-09-05).
    ///
    /// [`is_capturing_screenshot`]: Self::is_capturing_screenshot
    pub fn render_modal_layer(&mut self, ctx: &mut dyn RenderContext, current_time_ms: u64) {
        if self.is_capturing_screenshot() {
            return;
        }
        let w = self.width as f64;
        let h = self.height as f64;

        // Recompute layout locals that the modal blocks depend on.
        // This mirrors what render_to_scene computes; the cost is negligible
        // (pure arithmetic, no allocation). The profile manager and welcome
        // wizard are ordinary floating modals now (owner's ruling
        // 2026-08-31) — no more sidebar_w=0 override for them.
        let sidebar_w = self.effective_modal_sidebar_w();
        let window_rect = LayoutRect::new(0.0, 0.0, w, h);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };
        // Draggable modals move freely across the WHOLE window (owner rule
        // 2026-07-31) and are SIZED from it too. The old guard — the
        // content edge minus the data sidebar — is a relic: with a wide
        // sidebar open it pushed the clamp limit NEGATIVE and shoved
        // modals off the left screen edge. Modals may overlap it.
        //
        // Three still drank from it after the rule landed, because the
        // comment here carved out an exception for "the centered search
        // overlay" that stopped being true once that overlay became
        // draggable: search, the watchlist modal and the watchlist
        // group-name prompt. It cost them more than position — the
        // watchlist sizes itself as a FRACTION of what it is handed, so
        // with the data sidebar open it collapsed to its 380px floor and
        // its eight columns overlapped each other.
        let modal_layout = ChartModalLayout {
            prim_screen_w: w,
            prim_screen_h: h,
            prim_modal_y: 60.0,
            ind_screen_w: w,
            ind_screen_h: h,
            chart_x: content_rect.x,
            chart_y: content_rect.y,
            content_w: content_rect.width,
            content_h: content_rect.height,
        };
        let frame_theme = self.panel_app.frame_theme_for_render();
        let toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let toolbar_state = ToolbarState::default();

        // Shared-chart viewer row (native-share-viewer arc, 2026-09-06) —
        // a slim "Shared by <owner>" banner atop the chart canvas, drawn
        // here (not in `mlc-app-layout`) because `ChartApp` is the only
        // layer holding both `is_readonly_tab` and `shared_chart_meta`
        // (see `share_viewer.rs`'s own module doc). `render_modal_layer`
        // is called by every shell every frame (same contract as
        // `render_panel_overlay_popups`), so this needs no separate
        // shell wiring the way a brand-new host method would.
        // `draw_share_viewer_row` itself draws nothing and registers no
        // widget when the link carries no "clone" right — an anonymous
        // viewer only ever sees the label.
        {
            let active_preset_id = self.panel_app.active_preset_id.clone();
            if self.is_readonly_tab(&active_preset_id) {
                if let Some(meta) = self.shared_chart_meta(&active_preset_id) {
                    let owner_label = meta.owner_label.clone();
                    let can_clone = meta.has_right("clone");
                    // Sit BELOW every leaf's own top-left symbol header
                    // (`render_leaf_tab`, `mlc-app-layout::layout::
                    // panel_overlay`) instead of on top of it. That header
                    // renders at `<leaf/main_chart>_y + 2.0` for
                    // `LEAF_TAB_HEIGHT` (24px) at the grid's own top-left
                    // corner in EVERY leaf (single-pane or the first pane of
                    // a split) — `content_rect.y` (this row's old top) lands
                    // in that exact same corner, mashing the two texts
                    // together (owner report, "Untitled 78 — zeng3ld"
                    // screenshot, 2-pane split, left pane only). One row
                    // spanning the full `content_rect` width, moved below the
                    // header band, keeps a single banner/button for the
                    // whole tab — no duplicate "Clone" hit zones across
                    // panes — while never overlapping a leaf's own header in
                    // either layout.
                    let row_y = content_rect.y
                        + mlc_app_layout::layout::panel_overlay::LEAF_TAB_HEIGHT
                        + 4.0;
                    let row_rect = uzor::types::Rect::new(
                        content_rect.x, row_y, content_rect.width, 28.0,
                    );
                    let mut coord = self.input_coordinator.borrow_mut();
                    mlc_app_layout::ui::share_viewer_row::draw_share_viewer_row(
                        ctx,
                        &mut coord,
                        &uzor::input::LayerId::new("toolbar"),
                        row_rect,
                        &active_preset_id,
                        &owner_label,
                        can_clone,
                        &toolbar_theme,
                    );
                }
            }
        }

        // 5. Render chart-level modals (primitive settings, indicator settings,
        //    chart settings, compare settings, etc.)
        //
        // render_modals reads panel_app modal state and needs the active
        // bubble's drawing render view for the primitive settings modal.
        let active_dm_bid = self.panel_app.panel_grid
            .active_container()
            .map(|w| w.current_tag_id);
        let drawings_guard_modals = self.core.drawings();
        let dm_view_modals = active_dm_bid.map(|bid| drawings_guard_modals.with_bubble(bid));
        let dm_ref: Option<&dyn mlc_core::drawing::DrawingRenderView> = dm_view_modals
            .as_ref()
            .map(|v| v as &dyn mlc_core::drawing::DrawingRenderView);
        let chart_settings_data = self.build_chart_settings_data();

        // Wave 2b: populate active_render from TextFieldStore before render_modals so
        // that layout renderers can read text/cursor/selection without touching the store.
        {
            use mlc_app_layout::ui::modal_settings::ActiveFieldRender;
            let tf = self.input_coordinator.borrow();
            let tf = tf.text_fields();
            let cursor_blink = tf.cursor_visible(current_time_ms);

            // Primitive settings
            if let Some(ref fid) = self.panel_app.primitive_settings_state.active_field_id.clone() {
                let wid = uzor::WidgetId::from(text_input::PRIMITIVE_SETTINGS_FIELD);
                let text = tf.text(&wid).to_string();
                let cursor = tf.cursor(&wid);
                let selection_start = tf.selection_range(&wid).map(|(s, _)| s);
                self.panel_app.primitive_settings_state.active_render = ActiveFieldRender {
                    text,
                    cursor,
                    selection_start,
                    cursor_visible: cursor_blink && self.panel_app.primitive_settings_state.active_field_id.as_deref() == Some(fid.as_str()),
                };
            } else {
                self.panel_app.primitive_settings_state.active_render = ActiveFieldRender::default();
            }

            // Indicator settings
            if let Some(ref fid) = self.panel_app.indicator_settings_state.active_field_id.clone() {
                let wid = uzor::WidgetId::from(text_input::INDICATOR_SETTINGS_FIELD);
                let text = tf.text(&wid).to_string();
                let cursor = tf.cursor(&wid);
                let selection_start = tf.selection_range(&wid).map(|(s, _)| s);
                self.panel_app.indicator_settings_state.active_render = ActiveFieldRender {
                    text,
                    cursor,
                    selection_start,
                    cursor_visible: cursor_blink && self.panel_app.indicator_settings_state.active_field_id.as_deref() == Some(fid.as_str()),
                };
            } else {
                self.panel_app.indicator_settings_state.active_render = ActiveFieldRender::default();
            }

            // Chart settings
            if let Some(ref fid) = self.panel_app.chart_settings_state.active_field_id.clone() {
                let wid = uzor::WidgetId::from(text_input::CHART_SETTINGS_FIELD);
                let text = tf.text(&wid).to_string();
                let cursor = tf.cursor(&wid);
                let selection_start = tf.selection_range(&wid).map(|(s, _)| s);
                self.panel_app.chart_settings_state.active_render = ActiveFieldRender {
                    text,
                    cursor,
                    selection_start,
                    cursor_visible: cursor_blink && self.panel_app.chart_settings_state.active_field_id.as_deref() == Some(fid.as_str()),
                };
            } else {
                self.panel_app.chart_settings_state.active_render = ActiveFieldRender::default();
            }

            // Compare settings
            if let Some(ref fid) = self.panel_app.compare_settings_state.active_field_id.clone() {
                let wid = uzor::WidgetId::from(text_input::COMPARE_SETTINGS_FIELD);
                let text = tf.text(&wid).to_string();
                let cursor = tf.cursor(&wid);
                let selection_start = tf.selection_range(&wid).map(|(s, _)| s);
                self.panel_app.compare_settings_state.active_render = ActiveFieldRender {
                    text,
                    cursor,
                    selection_start,
                    cursor_visible: cursor_blink && self.panel_app.compare_settings_state.active_field_id.as_deref() == Some(fid.as_str()),
                };
            } else {
                self.panel_app.compare_settings_state.active_render = ActiveFieldRender::default();
            }

            // Wave 3 — profile management fields
            {
                let populate_proxy = |id: &str, tf: &uzor::input::TextFieldStore, focused: bool| -> ActiveFieldRender {
                    let wid = uzor::WidgetId::from(id);
                    ActiveFieldRender {
                        text: tf.text(&wid).to_string(),
                        cursor: tf.cursor(&wid),
                        selection_start: tf.selection_range(&wid).map(|(s, _)| s),
                        cursor_visible: cursor_blink && focused,
                    }
                };
                let (pf_new_profile_name, pf_profile_rename, pf_new_passphrase, pf_confirm_passphrase, pf_recovery_key, pf_recovery_key_reveal, pf_account_nick) = {
                    let uss = &self.panel_app.user_settings_state;
                    (
                        populate_proxy(text_input::NEW_PROFILE_NAME, tf, uss.new_profile_name_focused),
                        populate_proxy(text_input::PROFILE_RENAME, tf, uss.profile_rename_focused),
                        populate_proxy(text_input::NEW_PASSPHRASE, tf, uss.new_passphrase_focused),
                        populate_proxy(text_input::CONFIRM_PASSPHRASE, tf, uss.confirm_passphrase_focused),
                        populate_proxy(text_input::RECOVERY_KEY, tf, uss.recovery_key_display_focused),
                        // Separate id, same shared focus flag: `ShowRecoveryKey`
                        // and `UseRecoveryKey`/`ConfirmRecoveryKey` never render
                        // at the same time, so mirroring both proxies off one
                        // bool every frame is safe — see `RECOVERY_KEY_DISPLAY`'s
                        // own doc for why the two fields are not the same id.
                        populate_proxy(text_input::RECOVERY_KEY_DISPLAY, tf, uss.recovery_key_display_focused),
                        populate_proxy(text_input::ACCOUNT_NICK, tf, uss.nick_edit_focused),
                    )
                };
                let uss_mut = &mut self.panel_app.user_settings_state;
                uss_mut.new_profile_name_render = pf_new_profile_name;
                uss_mut.profile_rename_render = pf_profile_rename;
                uss_mut.new_passphrase_render = pf_new_passphrase;
                uss_mut.confirm_passphrase_render = pf_confirm_passphrase;
                uss_mut.recovery_key_display_render = pf_recovery_key;
                uss_mut.recovery_key_reveal_render = pf_recovery_key_reveal;
                uss_mut.nick_edit_render = pf_account_nick;
            }

            // Wave 3 — alert token field
            {
                let tg_wid = uzor::WidgetId::from(text_input::ALERT_TG_TOKEN);
                let tg_text = tf.text(&tg_wid).to_string();
                let tg_cursor = tf.cursor(&tg_wid);
                let tg_sel = tf.selection_range(&tg_wid).map(|(s, _)| s);
                let tg_focused = tf.is_focused(&tg_wid);
                self.panel_app.alert_settings_state.tg_token_render = ActiveFieldRender {
                    text: tg_text,
                    cursor: tg_cursor,
                    selection_start: tg_sel,
                    cursor_visible: cursor_blink && tg_focused,
                };
            }

            // Wave 4 — credentials modal fields (webhook URL edit + the
            // "add key" form). Same `populate_proxy` shape as Wave 3 above,
            // local to this block since Wave 3's own closure is already out
            // of scope by here.
            {
                let populate = |id: &str, focused: bool| -> ActiveFieldRender {
                    let wid = uzor::WidgetId::from(id);
                    ActiveFieldRender {
                        text: tf.text(&wid).to_string(),
                        cursor: tf.cursor(&wid),
                        selection_start: tf.selection_range(&wid).map(|(s, _)| s),
                        cursor_visible: cursor_blink && focused,
                    }
                };
                let (webhook_url, add_exchange, add_label, add_api_key, add_api_secret, add_passphrase) = {
                    let cs = &self.panel_app.credentials_state;
                    (
                        populate(text_input::CRED_WEBHOOK_URL, cs.webhook_url_focused),
                        populate(text_input::CRED_ADD_EXCHANGE, cs.add_exchange_focused),
                        populate(text_input::CRED_ADD_LABEL, cs.add_label_focused),
                        populate(text_input::CRED_ADD_API_KEY, cs.add_api_key_focused),
                        populate(text_input::CRED_ADD_API_SECRET, cs.add_api_secret_focused),
                        populate(text_input::CRED_ADD_PASSPHRASE, cs.add_passphrase_focused),
                    )
                };
                let cs_mut = &mut self.panel_app.credentials_state;
                cs_mut.webhook_url_render = webhook_url;
                cs_mut.add_exchange_render = add_exchange;
                cs_mut.add_label_render = add_label;
                cs_mut.add_api_key_render = add_api_key;
                cs_mut.add_api_secret_render = add_api_secret;
                cs_mut.add_passphrase_render = add_passphrase;
            }

            // Wave 4b — Server tab's agent-API key manager "label" field
            // (agent-api-auth-and-secret-store plan, piece 4). Same
            // self-contained shape as the Wave 3 alert-token field above —
            // Wave 4's own `populate` closure is already out of scope here.
            {
                let wid = uzor::WidgetId::from(text_input::KEY_LABEL_INPUT);
                let focused = self.panel_app.user_settings_state.key_label_focused;
                self.panel_app.user_settings_state.key_label_render = ActiveFieldRender {
                    text: tf.text(&wid).to_string(),
                    cursor: tf.cursor(&wid),
                    selection_start: tf.selection_range(&wid).map(|(s, _)| s),
                    cursor_visible: cursor_blink && focused,
                };
            }
        }

        let theme_manager_ptr: *const mlc_core::theme::ThemeManager =
            &self.panel_app.theme_manager as *const _;
        let indicator_guard_modals = self.core.indicators();
        // Modals only ever call `get_settings_data(id)` (id-keyed, no bubble
        // scoping needed) on this view — the bound bid is a placeholder.
        let indicator_view_modals = indicator_guard_modals.with_bubble(mlc_core::ids::BubbleId(0));

        let bubble_guard_modals = self.core.bubbles();
        let tags_guard_modals = self.core.tags();
        let tm_guard_modals = self.core.templates();
        let presets_guard_modals = self.core.presets();
        // Build TagView slice for the render layer (it no longer reads
        // BubbleStore directly).  Selected_group is the one Details tab
        // currently focuses on (state.selected_group_id).
        let mut groups_for_modals: Vec<mlc_core::TagView> = Vec::new();
        bubble_guard_modals.each_bubble(|bv| {
            if let Some(tag) = tags_guard_modals.get(bv.id()) {
                groups_for_modals.push(crate::tag_ui::build_tag_view(bv, tag));
            }
        });
        let selected_group_for_modals: Option<&mlc_core::TagView> = self
            .panel_app
            .tags_tabs_state
            .selected_group_id
            .and_then(|gid| groups_for_modals.iter().find(|gv| gv.id == gid));
        let modal_result = self.panel_app.render_modals(
            ctx,
            &modal_layout,
            dm_ref,
            Some(&indicator_view_modals as &dyn IndicatorSource),
            Some(&chart_settings_data),
            // SAFETY: theme_manager lives inside panel_app which render_modals
            // does not mutate (it only reads modal state fields).
            Some(unsafe { &*theme_manager_ptr }),
            &frame_theme,
            &toolbar_theme,
            &toolbar_state,
            current_time_ms,
            &mut self.input_coordinator.borrow_mut(),
            &groups_for_modals,
            selected_group_for_modals,
            &tm_guard_modals,
            presets_guard_modals.values(),
        );
        drop(bubble_guard_modals);
        drop(tags_guard_modals);
        drop(tm_guard_modals);
        drop(presets_guard_modals);
        let mut frame_result: Option<ChartModalRenderResult> = Some(modal_result);

        // 5b. Search overlay (indicator / symbol / compare)
        let search_modal_result: Option<mlc_app_layout::layout::ModalSearchResult> =
            if self.modal_state.current.is_search_overlay() {
                let screen = ChartScreenArea { x: 0.0, y: 0.0, width: w, height: h };
                let indicator_catalog = self.build_indicator_catalog();
                let hovered = self.modal_state.hovered_item_id.as_deref();
                let toolbar_theme_search = self.panel_app.toolbar_theme_for_render();
                let tm_guard_search = self.core.templates();
                let indicator_sets = tm_guard_search.indicator_sets();
                let search_field_id = match self.modal_state.current {
                    mlc_core::state::modal_state::OpenModal::SymbolSearch => crate::text_input::SYMBOL_SEARCH,
                    mlc_core::state::modal_state::OpenModal::IndicatorSearch => crate::text_input::INDICATOR_SEARCH,
                    mlc_core::state::modal_state::OpenModal::CompareSearch => crate::text_input::COMPARE_SEARCH,
                    _ => crate::text_input::SYMBOL_SEARCH,
                };
                let (split_symbol_compare_search, symbol_quick_search_search, favorite_indicators_search) = {
                    let user_guard = self.core.user();
                    let ui = &user_guard.profile().ui;
                    (ui.split_symbol_compare, ui.symbol_quick_search, ui.favorite_indicators.clone())
                };
                Some(render_search_overlay(
                    ctx,
                    screen,
                    &self.modal_state,
                    &indicator_catalog,
                    &favorite_indicators_search,
                    indicator_sets,
                    hovered,
                    &frame_theme,
                    &toolbar_theme_search,
                    current_time_ms,
                    &mut self.input_coordinator.borrow_mut(),
                    search_field_id,
                    split_symbol_compare_search,
                    symbol_quick_search_search,
                ))
            } else {
                None
            };

        // 5c. Preset name input (CreateIndicatorSet mode)
        if self.panel_app.preset_name_input.is_open
            && self.panel_app.preset_name_input.mode
                == mlc_app_layout::ui::modal_settings::PresetNameInputMode::CreateIndicatorSet
        {
            use mlc_app_layout::layout::modals::preset_name_input::render_preset_name_input;
            let pni_result = render_preset_name_input(
                ctx,
                w, h,
                &self.panel_app.preset_name_input,
                &frame_theme,
                &toolbar_theme,
                current_time_ms,
                &mut self.input_coordinator.borrow_mut(),
            );
            if let Some(ref mut fr) = frame_result {
                fr.preset_name_input = Some(pni_result);
            }
        }

        // 6. Context menu
        let context_menu_result: Option<ContextMenuResult> =
            if self.panel_app.context_menu_state.is_open() {
                let dropdown_theme = self.panel_app.dropdown_theme_for_render();
                let hovered_id = self.hovered_context_menu_item_id.as_deref();
                Some(render_context_menu(
                    ctx,
                    &self.panel_app.context_menu_state,
                    &dropdown_theme,
                    hovered_id,
                    &mut self.input_coordinator.borrow_mut(),
                ))
            } else {
                None
            };

        // 8a. Color picker popups (non-panel-targeting ones).
        // Panel-targeting sync_color_grid is still handled by render_panel_overlay_popups().
        if !self.panel_app.sync_color_grid.target_is_panel() {
            // target_is_panel() is false → sync_color_grid is for a leaf, not
            // a panel.  Pass `None` for the panel color (only consumed in the
            // panel-target branch inside render_color_picker_popups).
            let cp_result = self.panel_app.render_color_picker_popups(
                ctx,
                &modal_layout,
                &toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
                None,
            );
            if let Some(ref mut fr) = frame_result {
                if cp_result.color_picker.is_some() {
                    fr.color_picker = cp_result.color_picker;
                }
                if cp_result.sync_color_grid.is_some() {
                    fr.sync_color_grid = cp_result.sync_color_grid;
                }
            }
        }

        // 8b. Watchlist modal
        let last_watchlist_modal_result: Option<mlc_app_layout::layout::modals::watchlist_modal::WatchlistModalResult> =
            if self.standalone_panels.watchlist_modal.is_open() {
                let active_list_id = self.watchlist_ui.active_list_id;
                let wl_guard = self.core.watchlists();
                let color_flags: Vec<String> = self.sidebar_state.watchlist_items.iter()
                    .map(|item| {
                        wl_guard.find(active_list_id)
                            .and_then(|l| l.get_color_flag(&item.address))
                            .unwrap_or_default()
                            .to_string()
                    })
                    .collect();
                let entries: Vec<WatchlistEntry> = self.sidebar_state.watchlist_items.iter()
                    .zip(color_flags.iter())
                    .map(|(item, flag)| WatchlistEntry {
                        symbol: item.symbol.clone(),
                        exchange: item.exchange.clone(),
                        price: item.last_price,
                        change_pct: item.change_percent,
                        change_abs: item.last_price - (item.last_price / (1.0 + item.change_percent / 100.0)),
                        high_24h: item.high_24h,
                        low_24h: item.low_24h,
                        volume_24h: item.volume_24h,
                        color_flag: flag.clone(),
                        account_type: item.account_type.clone(),
                    })
                    .collect();
                let groups_info: Vec<WatchlistGroupInfo> = wl_guard.lists().iter()
                    .map(|list| WatchlistGroupInfo {
                        id: list.id,
                        name: list.name.clone(),
                        color: if list.groups.is_empty() {
                            String::new()
                        } else {
                            self.watchlist_ui.list(list.id)
                                .and_then(|l| l.groups.get(&list.groups[0].id))
                                .map(|g| g.color.clone())
                                .unwrap_or_default()
                        },
                        symbol_count: list.all_symbols().len(),
                        is_active: list.id == active_list_id,
                    })
                    .collect();
                drop(wl_guard);
                Some(render_watchlist_modal(
                    ctx,
                    w,
                    h,
                    &self.standalone_panels.watchlist_modal,
                    &entries,
                    &groups_info,
                    &frame_theme,
                    &toolbar_theme,
                    current_time_ms,
                    &mut self.input_coordinator.borrow_mut(),
                ))
            } else {
                None
            };

        // 8c. Watchlist group name input modal
        let last_wl_group_name_result: Option<WlGroupNameInputResult> =
            if self.standalone_panels.wl_group_name_input.is_open() {
                Some(render_wl_group_name_input(
                    ctx,
                    w,
                    h,
                    &self.standalone_panels.wl_group_name_input,
                    &frame_theme,
                    &toolbar_theme,
                    current_time_ms,
                    &mut self.input_coordinator.borrow_mut(),
                ))
            } else {
                None
            };

        // 8d. Clock popup (timezone selector + 24h toggle)
        if self.panel_app.clock_popup_state.is_open {
            let bottom_toolbar_y = h - panel_layout.bottom_toolbar_rect.height;
            let (active_offset, use_24h, show_utc) = self.panel_app.panel_grid.active_container()
                .map(|w| (
                    w.scale_settings.time_format.timezone_offset_hours,
                    w.scale_settings.time_format.use_24h,
                    w.scale_settings.time_format.show_utc_prefix,
                ))
                .unwrap_or((0, false, true));
            let hovered = self.panel_app.clock_popup_state.hovered_item.as_deref();
            let clock_glitch = self.core.user().profile().ui.clock_glitch_enabled;
            render_clock_popup(
                ctx,
                w,
                h,
                bottom_toolbar_y,
                active_offset,
                use_24h,
                show_utc,
                clock_glitch,
                hovered,
                &frame_theme,
                &toolbar_theme,
                &mut self.input_coordinator.borrow_mut(),
            );
        }

        // 8e. Trading Container modal — gear on the container header (owner
        // spec 2026-07-28: card MAP of the hosted panels + setup section).
        // Rendered last so it sits above every other popup in this layer.
        let tc_modal_result = if self.tc_modal.is_open {
            self.panels_store.trading_container.iter().next().map(|(_pid, tc)| {
                let (map_panels, csize) = tc_modal_map_panels(tc);
                let mc = &tc.metrics.config;
                let is_free = tc.layout_mode
                    == mlc_app_panels::trading::trading::trading_container::TcLayoutMode::Free;
                // SETUP configures the LIVE blocks (owner spec 2026-07-29):
                // in Free mode a disconnected panel's knobs are hidden.
                let connected = |kind: mlc_app_panels::trading::trading::trading_container::TcLeaf| {
                    !is_free || tc.free_leaf_hosting(&kind).is_some()
                };
                use mlc_app_panels::trading::trading::trading_container::TcLeaf;
                let mut hist_rows = Vec::new();
                if connected(TcLeaf::Positions) {
                    hist_rows.push(("Positions".to_string(), "positions".to_string(), tc.position_manager.history_depth.option_index()));
                }
                if connected(TcLeaf::Orders) {
                    hist_rows.push(("Orders".to_string(), "orders".to_string(), tc.orders_log.history_depth.option_index()));
                }
                if connected(TcLeaf::Trades) {
                    hist_rows.push(("Trades".to_string(), "trades".to_string(), tc.trade_log.history_depth.option_index()));
                }
                let setup = mlc_app_layout::layout::modals::TcSetupData {
                    layout_free: is_free,
                    center_label: tc.center_kind.label().to_string(),
                    hist_rows,
                    tape_full: tc.ticker_tape.format
                        == mlc_app_panels::trading::trading::ticker_tape::TapeFormat::Full,
                    metrics: vec![
                        ("uPnL".to_string(), "upnl".to_string(), mc.upnl),
                        ("rPnL".to_string(), "rpnl".to_string(), mc.rpnl),
                        ("Balance".to_string(), "balance".to_string(), mc.balance),
                        ("Positions".to_string(), "positions".to_string(), mc.positions),
                        ("Orders".to_string(), "orders".to_string(), mc.orders),
                        ("Trades".to_string(), "trades".to_string(), mc.trades),
                    ],
                    show_tape: connected(TcLeaf::Tape),
                    show_metrics: connected(TcLeaf::Metrics),
                    rc_overlay: tc.rc_overlay,
                    focus_bar: tc.focus_style_bar,
                };
                mlc_app_layout::layout::modals::render_trading_container_modal(
                    ctx,
                    &self.tc_modal,
                    &frame_theme,
                    &toolbar_theme,
                    &map_panels,
                    csize,
                    &setup,
                    w,
                    h,
                    &mut self.input_coordinator.borrow_mut(),
                )
            })
        } else {
            None
        };
        self.tc_modal_result = tc_modal_result;

        // 8e2. Risk Calculator modal — overlay hosting of the calculator
        // (gauge chip on the OE header, owner spec 2026-07-29). Standard
        // modal machinery: own modal layer, draggable header, ✕ +
        // outside-click close. The calculator panel renders 1:1 into the
        // modal body with its plain registrations joined to the modal
        // layer (`set_default_layer`); its internal title strip is
        // clipped away — the modal header IS the title.
        let rc_modal_result = if self.rc_modal.is_open {
            self.panels_store.trading_container.iter().next().and_then(|(_pid, tc)| {
                if !tc.rc_overlay {
                    return None;
                }
                let prefix = tc.last_slot_prefix.borrow().clone();
                if prefix.is_empty() {
                    return None;
                }
                use uzor::render::{TextAlign, TextBaseline};
                const RC_W: f64 = 260.0;
                const RC_H: f64 = 380.0;
                const RC_HEADER_H: f64 = 28.0;
                const RC_INNER_TITLE_H: f64 = 20.0; // RiskCalculatorState's own strip
                let (mx, my) = self
                    .rc_modal
                    .position
                    .unwrap_or(((w - RC_W) / 2.0, (h - RC_H) / 2.0));
                let mx = mx.max(0.0).min((w - RC_W).max(0.0));
                let my = my.max(0.0).min((h - RC_H).max(0.0));
                let mut result = mlc_app_layout::layout::modals::TcModalResult::default();
                result.modal_rect = uzor::Rect::new(mx, my, RC_W, RC_H);
                result.header_rect = uzor::Rect::new(mx, my, RC_W, RC_HEADER_H);

                let ptheme =
                    panels_render::panel_theme_from_runtime(self.panel_app.theme_manager.current());
                let coord = &mut *self.input_coordinator.borrow_mut();
                use mlc_app_layout::ui::z_order::ZLayer;
                let layer_id = ZLayer::Modal.push_named(coord, "rc_modal");
                coord.register_on_layer(
                    "rc_modal:header",
                    result.header_rect,
                    uzor::input::Sense::DRAG,
                    &layer_id,
                );
                coord.register_on_layer(
                    "rc_modal:modal_bg",
                    result.modal_rect,
                    uzor::input::Sense::CLICK,
                    &layer_id,
                );

                // Frame + header.
                ctx.set_fill_color(&frame_theme.modal_bg);
                ctx.fill_rounded_rect(mx, my, RC_W, RC_H, 6.0);
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_font("12px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Risk Calculator", mx + 12.0, my + RC_HEADER_H / 2.0);
                ctx.set_stroke_color(&frame_theme.toolbar_border);
                ctx.set_stroke_width(1.0);
                ctx.begin_path();
                ctx.move_to(mx, my + RC_HEADER_H);
                ctx.line_to(mx + RC_W, my + RC_HEADER_H);
                ctx.stroke();

                // Close ✕.
                const CLOSE: f64 = 16.0;
                let cx = mx + RC_W - CLOSE - 8.0;
                let cy = my + (RC_HEADER_H - CLOSE) / 2.0;
                let close_hov =
                    coord.is_hovered(&uzor::WidgetId::from("rc_modal:close"));
                if close_hov {
                    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                    ctx.fill_rounded_rect(cx, cy, CLOSE, CLOSE, 2.0);
                }
                ctx.set_stroke_color(&toolbar_theme.item_text);
                ctx.set_stroke_width(1.5);
                ctx.begin_path();
                ctx.move_to(cx + 4.5, cy + 4.5);
                ctx.line_to(cx + CLOSE - 4.5, cy + CLOSE - 4.5);
                ctx.stroke();
                ctx.begin_path();
                ctx.move_to(cx + CLOSE - 4.5, cy + 4.5);
                ctx.line_to(cx + 4.5, cy + CLOSE - 4.5);
                ctx.stroke();
                coord.register_on_layer(
                    "rc_modal:close",
                    uzor::Rect::new(cx, cy, CLOSE, CLOSE),
                    uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
                    &layer_id,
                );

                // Body — the calculator panel, its own title strip clipped
                // away (shifted up under the modal header).
                coord.set_default_layer(Some(layer_id.clone()));
                ctx.save();
                ctx.clip_rect(mx, my + RC_HEADER_H, RC_W, RC_H - RC_HEADER_H);
                use mlc_app_panels::panel_trait::TradingPanel as _;
                tc.risk_calculator.render(
                    ctx,
                    mx as f32,
                    (my + RC_HEADER_H - RC_INNER_TITLE_H) as f32,
                    RC_W as f32,
                    (RC_H - RC_HEADER_H + RC_INNER_TITLE_H) as f32,
                    &ptheme,
                    coord,
                    &prefix,
                );
                ctx.restore();
                coord.set_default_layer(None);

                // Border last — above the body fill.
                ctx.set_stroke_color(&frame_theme.toolbar_border);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rect(mx + 0.5, my + 0.5, RC_W - 1.0, RC_H - 1.0);

                Some(result)
            })
        } else {
            None
        };
        self.rc_modal_result = rc_modal_result;

        // 8e3. DOM settings modal (Stage B) — one modal for both DOM modes,
        // opened by the DOM section header gear (`tch:sec_gear`).
        if self.dom_modal.is_open {
            use mlc_app_sidebar_content::state::RightSidebarPanel as RSP;
            let rp = self.sidebar_state.right_panel;
            // Agents settings modal (Stage B agents reform).
            if rp == RSP::Agents {
                let is_pty = self.sidebar_state.agent_spawn_mode == gate4agent::InstanceMode::Pty;
                let selected = self.sidebar_state.agent_selected_backend.clone();
                let coord = &mut *self.input_coordinator.borrow_mut();
                crate::dom_settings_modal::render_agents_settings_modal(
                    ctx, coord, &frame_theme, &toolbar_theme,
                    w, h, self.dom_modal.position, is_pty, &selected,
                );
            } else {
            let section = if matches!(rp, RSP::SlotDom | RSP::SlotTape | RSP::SlotL2 | RSP::SlotBigTrades | RSP::SlotLiq) {
                self.section_panel_for(rp)
            } else {
                None
            };
            let coord = &mut *self.input_coordinator.borrow_mut();
            match section {
                Some(mlc_core::state::Panel::Dom(pid)) => {
                    if let Some(dom) = self.panels_store.dom.get(&pid) {
                        crate::dom_settings_modal::render_dom_settings_modal(
                            ctx, coord, &frame_theme, &toolbar_theme,
                            w, h, self.dom_modal.position, pid, dom,
                            self.panel_app.theme_manager.current().panel_style,
                        );
                    }
                }
                Some(mlc_core::state::Panel::TradeTape(pid)) => {
                    if let Some(s) = self.panels_store.trade_tape.get(&pid) {
                        let cc = &s.column_config;
                        let cols = [
                            ("Time", cc.show_time),
                            ("Price", cc.show_price),
                            ("Size", cc.show_size),
                            ("Notional", cc.show_notional),
                        ];
                        let skin_label = s
                            .skin
                            .map(|k| k.label().to_string())
                            .unwrap_or_else(|| "Auto (theme)".to_string());
                        crate::dom_settings_modal::render_section_settings_modal(
                            ctx, coord, &frame_theme, &toolbar_theme,
                            w, h, self.dom_modal.position,
                            "Trade Tape Settings", &cols, None,
                            Some(&skin_label),
                            Some((if s.multi_mode { 1 } else { 0 }, false)),
                            Some((s.show_imbalance, s.imb_window_min)),
                            Some(s.flag_notional),
                            Some(s.filter_min_qty),
                            None,
                        );
                    }
                }
                Some(mlc_core::state::Panel::L2Tape(pid)) => {
                    if let Some(s) = self.panels_store.l2_tape.get(&pid) {
                        let cc = &s.column_config;
                        let cols = [
                            ("Time", cc.show_time),
                            ("Type", cc.show_type),
                            ("Side", cc.show_side),
                            ("Price", cc.show_price),
                            ("Qty", cc.show_qty),
                        ];
                        crate::dom_settings_modal::render_section_settings_modal(
                            ctx, coord, &frame_theme, &toolbar_theme,
                            w, h, self.dom_modal.position,
                            "L2 Tape Settings", &cols, None,
                            None,
                            Some((if s.multi_mode { 1 } else { 0 }, false)),
                            Some((s.show_imbalance, s.imb_window_min)),
                            None,
                            None,
                            Some((s.show_adds, s.show_mods, s.show_cxls, s.show_execs)),
                        );
                    }
                }
                Some(mlc_core::state::Panel::BigTrades(pid)) => {
                    if let Some(s) = self.panels_store.big_trades.get(&pid) {
                        let cc = &s.column_config;
                        let cols = [
                            ("Time", cc.show_time),
                            ("Side", cc.show_side),
                            ("Price", cc.show_price),
                            ("Size", cc.show_size),
                            ("Notional", cc.show_notional),
                        ];
                        let skin_label = s
                            .skin
                            .map(|k| k.label().to_string())
                            .unwrap_or_else(|| "Auto (theme)".to_string());
                        crate::dom_settings_modal::render_section_settings_modal(
                            ctx, coord, &frame_theme, &toolbar_theme,
                            w, h, self.dom_modal.position,
                            "Big Trades Settings", &cols,
                            Some((s.size_threshold, s.notional_threshold, Some(s.show_big_markers))),
                            Some(&skin_label),
                            Some((
                                if s.agg_mode { 2 } else if s.multi_mode { 1 } else { 0 },
                                true,
                            )),
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                }
                Some(mlc_core::state::Panel::LiqTape(pid)) => {
                    if let Some(s) = self.panels_store.liq_tape.get(&pid) {
                        let cc = &s.column_config;
                        let cols = [
                            ("Time", cc.show_time),
                            ("Side", cc.show_side),
                            ("Price", cc.show_price),
                            ("Size", cc.show_size),
                            ("Notional", cc.show_notional),
                        ];
                        let skin_label = s
                            .skin
                            .map(|k| k.label().to_string())
                            .unwrap_or_else(|| "Auto (theme)".to_string());
                        crate::dom_settings_modal::render_section_settings_modal(
                            ctx, coord, &frame_theme, &toolbar_theme,
                            w, h, self.dom_modal.position,
                            "Liquidations Settings", &cols,
                            Some((s.size_threshold, s.notional_threshold, None)),
                            Some(&skin_label),
                            Some((s.mode_index(), true)),
                            None,
                            None,
                            None,
                            None,
                        );
                    }
                }
                _ => {
                    self.dom_modal.is_open = false;
                }
            }
            } // !Agents
        }

        // 8f. Standalone panel-settings surface (overlay gear, owner spec
        // 2026-07-28) — rendered last, topmost.
        let pset_result = self.build_panel_settings_view().map(|view| {
            mlc_app_layout::layout::modals::render_panel_settings_popup(
                ctx,
                &view,
                &frame_theme,
                &toolbar_theme,
                w,
                h,
                &mut self.input_coordinator.borrow_mut(),
            )
        });
        self.panel_settings_result = pset_result;

        // Store results directly into self — input handlers read these fields
        // between renders.  apply_render_output() may set them to None when the
        // chart scene is rebuilt (chart_dirty), but render_modal_layer() always
        // runs on the same frame and overwrites them here before any input event
        // could read stale values.
        self.frame_result = frame_result;
        self.search_modal_result = search_modal_result;
        self.context_menu_result = context_menu_result;
        self.last_watchlist_modal_result = last_watchlist_modal_result;
        self.last_wl_group_name_result = last_wl_group_name_result;

        // Drop remaining read guards on core (drawings + indicators) so the
        // mutable borrow inside update_text_field_geometry below is allowed.
        drop(drawings_guard_modals);
        drop(indicator_guard_modals);

        // Feed fresh modal geometry into TextFieldStore so click-to-cursor,
        // drag-select, and Ctrl+shortcuts hit the right rect.  MUST run AFTER
        // the self.* assignments above (those write the freshly-rendered
        // results from this frame).  apply_render_output runs BEFORE this
        // path on the same frame and would see stale Nones.
        self.update_text_field_geometry();
    }

    /// Draw active toasts — restyled to the modal frame (owner ask
    /// 2026-09-09, toast restyle-and-anchor follow-up to §8: "reuse the
    /// modal frame drawing helper or its constants"). A separate call from
    /// [`Self::render_modal_layer`] rather than folded into its tail: every
    /// shell already has its own dedicated toast-overlay slot in its
    /// composite sequence (after the chrome context menu / panel popups,
    /// before the tooltip layer — the same slot the pre-restyle ad-hoc
    /// `Vec<mlc_alert_delivery::ToastNotification>` painter used), and
    /// folding this into `render_modal_layer`'s own composite step would
    /// have moved a toast BELOW those chrome-owned overlays instead.
    ///
    /// `refresh_active_toasts` runs first — same "call once per frame per
    /// window that is actually on screen" contract its own doc gives,
    /// satisfied here because every shell calls this once per visible
    /// window per frame, same as `render_modal_layer` itself.
    pub fn render_toasts(&mut self, ctx: &mut dyn RenderContext, current_time_ms: u64) {
        use mlc_app_layout::ui::widgets::ModalTheme;

        self.refresh_active_toasts(current_time_ms as f64);
        if self.active_toasts.is_empty() {
            return;
        }
        let now_ms = current_time_ms as f64;
        let facts: Vec<mlc_app_layout::ui::toast::ToastFact> = self
            .active_toasts
            .iter()
            .map(|t| mlc_app_layout::ui::toast::ToastFact {
                id: t.id,
                title: t.title.clone(),
                body: t.body.clone(),
                remaining: t.remaining_fraction(now_ms),
            })
            .collect();
        let frame_theme = self.panel_app.frame_theme_for_render();
        let toolbar_theme = self.panel_app.toolbar_theme_for_render();
        let toast_theme = ModalTheme::from_toolbar(&frame_theme.modal_bg, &toolbar_theme.separator, &toolbar_theme);
        let anchor = self.core.user().profile().ui.toast_anchor;
        let (w, h) = (self.width as f64, self.height as f64);
        let mut coord = self.input_coordinator.borrow_mut();
        mlc_app_layout::ui::toast::render_toasts(ctx, &mut coord, &facts, &toast_theme, anchor, (w, h));
    }

    /// Build the standalone panel-settings surface view (overlay gear,
    /// owner spec 2026-07-28) from the open panel's live state. `None`
    /// while no surface is open. The universal "Open as" row is prepended
    /// here; panel-specific rows follow per kind — panels with no real
    /// settings yet surface just the universal row.
    fn build_panel_settings_view(
        &self,
    ) -> Option<mlc_app_layout::layout::modals::PanelSettingsView> {
        use mlc_app_layout::layout::modals::{PanelSettingRow, PanelSettingsView};
        use mlc_app_panels::trading::trading::history::HistoryDepth;
        use mlc_core::state::Panel;
        use uzor::panels::DockPanel as _;

        let (panel, anchor) = self.panel_settings_open.clone()?;
        let as_modal = self.core.user().profile().ui.panel_gear_opens_modal;
        let mut rows = vec![PanelSettingRow::Choice {
            id: "open_as".to_string(),
            label: "Open as".to_string(),
            options: vec!["Dropdown".to_string(), "Modal".to_string()],
            active: as_modal as usize,
        }];

        let hist_row = |d: HistoryDepth| PanelSettingRow::Choice {
            id: "hist".to_string(),
            label: "History".to_string(),
            options: HistoryDepth::OPTIONS.iter().map(|o| o.label()).collect(),
            active: d.option_index(),
        };
        match &panel {
            Panel::PositionManager(pid) => {
                if let Some(s) = self.panels_store.position_manager.get(pid) {
                    rows.push(hist_row(s.history_depth));
                }
            }
            Panel::TradeLog(pid) => {
                if let Some(s) = self.panels_store.trade_log.get(pid) {
                    rows.push(hist_row(s.history_depth));
                }
            }
            _ => {}
        }

        Some(PanelSettingsView {
            title: panel.title().to_string(),
            rows,
            as_modal,
            anchor,
        })
    }

    /// Feed fresh modal/sidebar geometry into uzor TextFieldStore so
    /// `on_drag_start(x, y)` can hit-test, click-to-cursor lands on the
    /// right glyph, drag-select highlights correctly, and Ctrl+shortcuts
    /// reach the right field.
    ///
    /// MUST be called every frame AFTER `render_modal_layer` finished
    /// writing into `self.frame_result` / `self.search_modal_result` /
    /// `self.last_watchlist_modal_result` / `self.last_wl_group_name_result`.
    /// `render_modal_layer` calls this at the end automatically; external
    /// callers that bypass that path must invoke it themselves.
    pub fn update_text_field_geometry(&mut self) {
        // Symbol fast-search open/close reconcile (symbol-search-compare
        // arc, Wave 2 hotfix): the symbol_selector dropdown can open through
        // TWO paths — the `OpenSymbolSearch` handler (which seeds
        // `symbol_quick_edit` + focuses `SYMBOL_QUICK_SEARCH`) and the
        // toolbar's own Dropdown-toggle arm (which knows nothing about
        // either — live repro 2026-07-16: dropdown open, caret rendered,
        // keystrokes dropped because `symbol_quick_edit` was `None` and the
        // field never focused). Reconciling here — a guaranteed per-frame
        // call in both shells — makes the edit-state lifecycle path-agnostic:
        // dropdown open without edit state → seed + focus; dropdown gone
        // (click-outside, Escape, other dropdown) with edit state → clear +
        // blur (only when our field still holds focus, never someone else's).
        {
            // Scoped to the TOPBAR `symbol_selector` editor only (trading-
            // header-symbol-edit arc, 2026-07-25): a live TradingContainer
            // header-tab edit has its own open/close lifecycle
            // (`open_trading_symbol_quick_edit` /
            // `close_symbol_quick_dropdown` / Escape / click-away) and must
            // never be touched here just because `open_dropdown_id` (a
            // toolbar-only concept) isn't `"symbol_selector"` — without this
            // guard the `!quick_open && is_some()` arm below would clear a
            // freshly-opened trading edit on the very next frame.
            let is_trading_edit = matches!(
                self.symbol_quick_edit.as_ref().map(|e| e.target),
                Some(crate::SymbolQuickEditTarget::TradingPanel(_))
                    | Some(crate::SymbolQuickEditTarget::SectionPanel(_))
                    | Some(crate::SymbolQuickEditTarget::MultiDomRoster(_))
                    | Some(crate::SymbolQuickEditTarget::MultiTapeRoster(_))
            );
            let quick_open = !is_trading_edit
                && self.panel_app.toolbar_state.open_dropdown_id.as_deref() == Some("symbol_selector");
            let quick_enabled = self.core.user().profile().ui.symbol_quick_search;
            if is_trading_edit {
                // Owned elsewhere — see comment above.
            } else if quick_open && quick_enabled && self.symbol_quick_edit.is_none() {
                // Mode 3 "inline" seeds the active symbol (see
                // `symbol_quick_seed_text`'s doc) — same seeding contract
                // `open_symbol_quick_dropdown` uses; this reconcile is the
                // fallback path when the dropdown opened via the raw
                // toolbar Dropdown-toggle arm instead of going through that
                // helper.
                let seed = self.symbol_quick_seed_text();
                let cursor = seed.chars().count();
                self.symbol_quick_edit = Some(crate::SymbolQuickEdit {
                    target: crate::SymbolQuickEditTarget::TopbarChart,
                    text: seed.clone(),
                    cursor,
                    selection: None,
                });
                let q_id = WidgetId::from(text_input::SYMBOL_QUICK_SEARCH);
                let mut ic = self.input_coordinator.borrow_mut();
                let tf = ic.text_fields_mut();
                tf.set_text(&q_id, &seed);
                tf.begin_edit(&q_id);
                tf.focus(text_input::SYMBOL_QUICK_SEARCH);
            } else if quick_open && self.symbol_quick_edit.is_some() {
                // Focus is sticky while the dropdown is open: a generic
                // click-outside-any-field blur pass (row clicks, toggle
                // clicks) must not leave the keyboard dead — the input is
                // the only focusable thing in this dropdown.
                let q_id = WidgetId::from(text_input::SYMBOL_QUICK_SEARCH);
                let mut ic = self.input_coordinator.borrow_mut();
                if !ic.text_fields().is_focused(&q_id) {
                    ic.text_fields_mut().focus(text_input::SYMBOL_QUICK_SEARCH);
                }
            } else if !quick_open && self.symbol_quick_edit.is_some() {
                self.symbol_quick_edit = None;
                let q_id = WidgetId::from(text_input::SYMBOL_QUICK_SEARCH);
                let mut ic = self.input_coordinator.borrow_mut();
                if ic.text_fields().is_focused(&q_id) {
                    ic.text_fields_mut().blur();
                }
            }
        }
        // Update text-field geometry for the HexColor field whenever the
        // L2 color picker is visible.  This gives on_drag_start accurate rect +
        // char positions so cursor-from-click works correctly.
        if let Some(ref fr) = self.frame_result {
            if let Some(ref cp) = fr.color_picker {
                if let Some(ref l2) = cp.l2_result {
                    let r = &l2.hex_input_rect;
                    let hex_id = WidgetId::from(text_input::HEX_COLOR);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &hex_id,
                        (r.x, r.y, r.width, r.height),
                        l2.hex_char_positions.clone(),
                    );
                }
            }
        }
        // Update text-field geometry for the agent chat input field so
        // on_drag_start can compute cursor-from-click correctly.
        if let Some(ref sidebar_result) = self.last_sidebar_result {
            if let (Some(rect), Some(char_positions)) = (
                sidebar_result.agent_input_rect,
                sidebar_result.agent_input_char_positions.clone(),
            ) {
                let chat_id = WidgetId::from(text_input::AGENT_CHAT);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &chat_id,
                    (rect.x, rect.y, rect.width, rect.height),
                    char_positions,
                );
            }
            // AGENT_PTY — see the sibling stamp in `prepare_frame` (above,
            // where `agent_terminal_rect` is first threaded through) for why
            // this is an empty char-position `update_field` rather than
            // `mark_seen`.
            if let Some(rect) = sidebar_result.agent_terminal_rect {
                let pty_id = WidgetId::from(text_input::AGENT_PTY);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &pty_id,
                    (rect.x, rect.y, rect.width, rect.height),
                    Vec::new(),
                );
            }
        }

        // Update text-field geometry for Wave 1 drag-select fields.
        // Enables on_drag_start / on_drag_move rect-based hit-testing.
        if let Some(ref wl) = self.last_watchlist_modal_result {
            let r = &wl.search_input_rect;
            let wl_id = WidgetId::from(text_input::WATCHLIST_SEARCH);
            self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                &wl_id,
                (r.x, r.y, r.width, r.height),
                wl.search_char_positions.clone(),
            );
        }
        if let Some(ref gni) = self.last_wl_group_name_result {
            let r = &gni.input_text_rect;
            let gn_id = WidgetId::from(text_input::WATCHLIST_GROUP_NAME);
            self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                &gn_id,
                (r.x, r.y, r.width, r.height),
                gni.char_x_positions.clone(),
            );
        }
        // Chart browser and preset name — stored in frame_result.
        if let Some(ref fr) = self.frame_result {
            if let Some(ref cbr) = fr.chart_browser {
                let r = &cbr.search_input_rect;
                let cb_id = WidgetId::from(text_input::CHART_BROWSER_SEARCH);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &cb_id,
                    (r.x, r.y, r.width, r.height),
                    cbr.search_char_positions.clone(),
                );
            }
            // Community modal's Forum title/body (compose) / reply (thread)
            // fields, Forum search box, and DM draft/peer boxes — same
            // "no drag-selection, no click-to-position without this"
            // reasoning as the gift-code box above. Geometry comes from
            // `CommunityResult::content` (community-modal arc W1) — moved
            // out of `fr.cloud_overlay` when Forum/Messages moved out of the
            // News overlay into their own modal.
            #[cfg(feature = "cloud")]
            if let Some(ov) = fr.community.as_ref().map(|c| &c.content) {
                if let Some(r) = ov.forum_title_rect {
                    let id = WidgetId::from(text_input::FORUM_TITLE);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &id,
                        (r.x, r.y, r.width, r.height),
                        ov.forum_title_char_positions.clone(),
                    );
                }
                if let Some(r) = ov.forum_body_rect {
                    let id = WidgetId::from(text_input::FORUM_BODY);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &id,
                        (r.x, r.y, r.width, r.height),
                        ov.forum_body_char_positions.clone(),
                    );
                }
                if let Some(r) = ov.forum_reply_rect {
                    let id = WidgetId::from(text_input::FORUM_REPLY);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &id,
                        (r.x, r.y, r.width, r.height),
                        ov.forum_reply_char_positions.clone(),
                    );
                }
                // Forum v2's search box and DM's draft/peer boxes — same
                // "no drag-selection, no click-to-position without this"
                // reasoning as the three fields above. `forum_search_rect`/
                // `dm_draft_rect` have carried geometry since W2 but were
                // never wired here; `dm_peer_rect` is new this pass.
                if let Some(r) = ov.forum_search_rect {
                    let id = WidgetId::from(text_input::FORUM_SEARCH);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &id,
                        (r.x, r.y, r.width, r.height),
                        ov.forum_search_char_positions.clone(),
                    );
                }
                if let Some(r) = ov.dm_draft_rect {
                    let id = WidgetId::from(text_input::DM_DRAFT);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &id,
                        (r.x, r.y, r.width, r.height),
                        ov.dm_draft_char_positions.clone(),
                    );
                }
                if let Some(r) = ov.dm_peer_rect {
                    let id = WidgetId::from(text_input::DM_PEER);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &id,
                        (r.x, r.y, r.width, r.height),
                        ov.dm_peer_char_positions.clone(),
                    );
                }
            }
            if let Some(ref pni) = fr.preset_name_input {
                let r = &pni.input_text_rect;
                let pni_id = WidgetId::from(text_input::PRESET_NAME_INPUT);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &pni_id,
                    (r.x, r.y, r.width, r.height),
                    pni.char_x_positions.clone(),
                );
            }
            // Vault tab (User Settings) fields — webhook URL + the five
            // add-key form fields. `render_vault_tab` (mlc-app-layout) keys
            // both `content_items` and `input_char_positions` by the SAME
            // click-dispatch id (`cred:webhook_url`, `cred:add:exchange`,
            // ...), which is what lets this stay one small table instead of
            // six near-identical `update_field` calls. Without this, none
            // of the six fields' `last_rect` is ever populated and
            // `on_drag_start`'s hit-test never finds them — exactly the
            // "no drag-selection, no click-to-position" the vault fields
            // shipped with when they only ever went through TextFieldStore
            // via a plain `.focus()` call.
            if let Some(ref us) = fr.user_settings {
                const VAULT_FIELDS: &[(&str, &str)] = &[
                    (text_input::CRED_WEBHOOK_URL, "cred:webhook_url"),
                    (text_input::CRED_ADD_EXCHANGE, "cred:add:exchange"),
                    (text_input::CRED_ADD_LABEL, "cred:add:label"),
                    (text_input::CRED_ADD_API_KEY, "cred:add:api_key"),
                    (text_input::CRED_ADD_API_SECRET, "cred:add:api_secret"),
                    (text_input::CRED_ADD_PASSPHRASE, "cred:add:passphrase"),
                    // Server tab's agent-API key manager "label" field
                    // (agent-api-auth-and-secret-store plan, piece 4) —
                    // unprefixed content-item id, see `input/mod.rs`'s
                    // `VAULT_TEXT_FIELDS` comment for why.
                    (text_input::KEY_LABEL_INPUT, "key_label"),
                ];
                for &(store_id, ui_id) in VAULT_FIELDS {
                    if let Some((_, rect)) = us.content_items.iter().find(|(k, _)| k.as_str() == ui_id) {
                        let char_positions = us.input_char_positions.iter()
                            .find(|(k, _)| k.as_str() == ui_id)
                            .map(|(_, v)| v.clone())
                            .unwrap_or_default();
                        let wid = WidgetId::from(store_id);
                        self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                            &wid,
                            (rect.x, rect.y, rect.width, rect.height),
                            char_positions,
                        );
                    }
                }
            }
            // Profile Manager cluster (new profile name, inline rename,
            // new/confirm passphrase, recovery key entry + reveal, account
            // nickname) — the SAME gap `VAULT_FIELDS` closed for the
            // credentials fields above, still open here (stale-focus audit
            // 2026-09-23): `profile_manager.rs`/`user_settings.rs` already
            // push every one of these into `content_items`/
            // `input_char_positions` (see e.g. `user_settings.rs`'s own
            // "Surface outer rect + char positions to the embedder..."
            // comment on `profile_rename_input`), but nothing ever consumed
            // them into `update_field` — so `last_rect` stayed `None`
            // forever and `focused_is_stale()` could never tell a leaked
            // focus on one of these apart from a live one. Several ids are
            // drawn from more than one page/host (mutually exclusive per
            // frame — only one page is ever open at a time), so each entry
            // lists every content-item key that can supply it; the first
            // one present this frame wins.
            if let Some(ref us) = fr.user_settings {
                const PROFILE_MGR_FIELDS: &[(&str, &[&str])] = &[
                    (text_input::NEW_PROFILE_NAME, &["profile_mgr:name_input", "user_settings:new_profile_name_input"]),
                    (text_input::PROFILE_RENAME, &["user_settings:profile_rename_input"]),
                    // "e2e_passphrase_input": shared `render_passphrase_input`
                    // helper (UnlockPassphrase + CreatePassphrase pages).
                    // "profile_mgr:new_passphrase_input": SetNewPassphrase page.
                    (text_input::NEW_PASSPHRASE, &["e2e_passphrase_input", "profile_mgr:new_passphrase_input"]),
                    // "profile_mgr:create_confirm_passphrase_input": CreatePassphrase
                    // page. "profile_mgr:confirm_passphrase_input": SetNewPassphrase page.
                    (text_input::CONFIRM_PASSPHRASE, &["profile_mgr:create_confirm_passphrase_input", "profile_mgr:confirm_passphrase_input"]),
                    // "profile_mgr:recovery_key_input": UseRecoveryKey page.
                    // "profile_mgr:confirm_recovery_input": ConfirmRecoveryKey page.
                    (text_input::RECOVERY_KEY, &["profile_mgr:recovery_key_input", "profile_mgr:confirm_recovery_input"]),
                    (text_input::RECOVERY_KEY_DISPLAY, &["profile_mgr:recovery_key_display"]),
                    (text_input::ACCOUNT_NICK, &["user_settings:account_nick_input"]),
                    // Account tab's gift/trial code box (owner ruling
                    // 2026-09-23: moved here from the old cloud-login
                    // overlay, which stamped `REDEEM_CODE` via its own
                    // `fr.cloud_login.redeem_input_rect` — now gone with the
                    // overlay's own copy of this field).
                    (text_input::REDEEM_CODE, &["user_settings:account_redeem_input"]),
                ];
                for &(store_id, ui_ids) in PROFILE_MGR_FIELDS {
                    for &ui_id in ui_ids {
                        if let Some((_, rect)) = us.content_items.iter().find(|(k, _)| k.as_str() == ui_id) {
                            let char_positions = us.input_char_positions.iter()
                                .find(|(k, _)| k.as_str() == ui_id)
                                .map(|(_, v)| v.clone())
                                .unwrap_or_default();
                            let wid = WidgetId::from(store_id);
                            self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                                &wid,
                                (rect.x, rect.y, rect.width, rect.height),
                                char_positions,
                            );
                            break;
                        }
                    }
                }
            }
            // Alert settings notifications tab's Telegram bot token field —
            // same gap as the Profile Manager cluster above: `alert_settings.rs`
            // already pushes "alert_set:notif:tg_token" into
            // `fr.alert_settings.content_items`/`input_char_positions`, but
            // `fr.alert_settings` was never read anywhere in this geometry
            // sync (stale-focus audit 2026-09-23).
            if let Some(ref al) = fr.alert_settings {
                const TG_TOKEN_UI_ID: &str = "alert_set:notif:tg_token";
                if let Some((_, rect)) = al.content_items.iter().find(|(k, _)| k.as_str() == TG_TOKEN_UI_ID) {
                    let char_positions = al.input_char_positions.iter()
                        .find(|(k, _)| k.as_str() == TG_TOKEN_UI_ID)
                        .map(|(_, v)| v.clone())
                        .unwrap_or_default();
                    let wid = WidgetId::from(text_input::ALERT_TG_TOKEN);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &wid,
                        (rect.x, rect.y, rect.width, rect.height),
                        char_positions,
                    );
                }
            }
            // "Open shared chart…" overlay's link/token field — same gap:
            // `share_open.rs` returns `input_rect`/`char_x_positions` in its
            // own `ShareOpenResult`, but nothing fed them to `update_field`,
            // so a click via the generic `on_drag_start(x, y)` path
            // (`panel_click.rs`'s `"share_open:input"` arm) could never find
            // this field's rect either (stale-focus audit 2026-09-23).
            if let Some(ref so) = fr.share_open {
                let wid = WidgetId::from(text_input::SHARE_OPEN_INPUT);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &wid,
                    (so.input_rect.x, so.input_rect.y, so.input_rect.width, so.input_rect.height),
                    so.char_x_positions.clone(),
                );
            }
            // Template name overlay modal (save-as dialog) — one instance per
            // settings modal it can be opened from. Without this, the field's
            // `last_rect` is never populated and `on_drag_start`'s hit-test
            // never finds it, so a click inside the field takes no caret
            // (2026-08-09 fix, mirrors chart_browser/preset_name_input above).
            if let Some(ref ptn) = fr.prim_template_name {
                let r = &ptn.input_rect;
                let id = WidgetId::from(text_input::PRIMITIVE_SETTINGS_TEMPLATE_NAME);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &id,
                    (r.x, r.y, r.width, r.height),
                    ptn.char_x_positions.clone(),
                );
            }
            if let Some(ref itn) = fr.ind_template_name {
                let r = &itn.input_rect;
                let id = WidgetId::from(text_input::INDICATOR_SETTINGS_TEMPLATE_NAME);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &id,
                    (r.x, r.y, r.width, r.height),
                    itn.char_x_positions.clone(),
                );
            }
            if let Some(ref ctn) = fr.cmp_template_name {
                let r = &ctn.input_rect;
                let id = WidgetId::from(text_input::COMPARE_SETTINGS_TEMPLATE_NAME);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &id,
                    (r.x, r.y, r.width, r.height),
                    ctn.char_x_positions.clone(),
                );
            }
            if let Some(ref htn) = fr.chart_template_name {
                let r = &htn.input_rect;
                let id = WidgetId::from(text_input::CHART_SETTINGS_TEMPLATE_NAME);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &id,
                    (r.x, r.y, r.width, r.height),
                    htn.char_x_positions.clone(),
                );
            }
        }
        // Search overlay field — stored in self.search_modal_result.
        if let Some(ref smr) = self.search_modal_result {
            let search_id = match self.modal_state.current {
                mlc_core::state::modal_state::OpenModal::SymbolSearch => WidgetId::from(text_input::SYMBOL_SEARCH),
                mlc_core::state::modal_state::OpenModal::IndicatorSearch => WidgetId::from(text_input::INDICATOR_SEARCH),
                mlc_core::state::modal_state::OpenModal::CompareSearch => WidgetId::from(text_input::COMPARE_SEARCH),
                _ => WidgetId::from(text_input::SYMBOL_SEARCH),
            };
            let r = &smr.input_rect;
            self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                &search_id,
                (r.x, r.y, r.width, r.height),
                smr.search_char_positions.clone(),
            );
        }
        // Wave 2b: settings sub-field editors — update geometry so on_drag_start works.
        if let Some(ref fr) = self.frame_result {
            if let Some(ref ps) = fr.primitive_settings {
                if let (Some(rect), positions) = (ps.active_input_rect, ps.active_input_char_positions.clone()) {
                    if !positions.is_empty() {
                        let prim_id = WidgetId::from(text_input::PRIMITIVE_SETTINGS_FIELD);
                        self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                            &prim_id,
                            (rect.x, rect.y, rect.width, rect.height),
                            positions,
                        );
                    }
                }
            }
            if let Some(ref ind_s) = fr.indicator_settings {
                if let (Some(rect), positions) = (ind_s.active_input_rect, ind_s.active_input_char_positions.clone()) {
                    if !positions.is_empty() {
                        let ind_id = WidgetId::from(text_input::INDICATOR_SETTINGS_FIELD);
                        self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                            &ind_id,
                            (rect.x, rect.y, rect.width, rect.height),
                            positions,
                        );
                    }
                }
            }
            if let Some(ref cs) = fr.chart_settings {
                if let (Some(rect), positions) = (cs.active_input_rect, cs.active_input_char_positions.clone()) {
                    if !positions.is_empty() {
                        let chart_id = WidgetId::from(text_input::CHART_SETTINGS_FIELD);
                        self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                            &chart_id,
                            (rect.x, rect.y, rect.width, rect.height),
                            positions,
                        );
                    }
                }
            }
            if let Some(ref cmp) = fr.compare_settings {
                if let (Some(rect), positions) = (cmp.tf_active_input_rect, cmp.tf_active_input_char_positions.clone()) {
                    if !positions.is_empty() {
                        let cmp_id = WidgetId::from(text_input::COMPARE_SETTINGS_FIELD);
                        self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                            &cmp_id,
                            (rect.x, rect.y, rect.width, rect.height),
                            positions,
                        );
                    }
                }
            }
        }

        // Inline granularity editor's widget-overlay input (mouse-support
        // arc, item 1) — `TF_FAVORITE_INPUT`/`PARAM_FAVORITE_INPUT` are the
        // SAME two fields `granularity_edit_render`'s `field_id` reads
        // (keyed by `GranularityEditKind`, see that method); feeding their
        // geometry here lets `on_drag_start`/`on_drag_move` hit-test
        // click-to-cursor and drag-select against the rect+char positions
        // `chart_toolbar_render_dropdown`'s widget overlay just rendered
        // with, mirroring every other modal text field above.
        if let Some(ref toolbar_result) = self.last_toolbar_result {
            if let Some(ref dd) = toolbar_result.dropdown_result {
                if let Some((rect, ref positions)) = dd.granularity_edit_input {
                    let field = match self.granularity_edit.as_ref().map(|e| &e.kind) {
                        Some(GranularityEditKind::AddParam) | Some(GranularityEditKind::EditParam { .. }) => {
                            text_input::PARAM_FAVORITE_INPUT
                        }
                        _ => text_input::TF_FAVORITE_INPUT,
                    };
                    let field_id = WidgetId::from(field);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &field_id,
                        (rect.x, rect.y, rect.width, rect.height),
                        positions.clone(),
                    );
                }
                // Settings-gear submenu's custom watermark text field — same
                // threading role as `granularity_edit_input` just above (stale-
                // focus audit 2026-09-23: this field's geometry was computed and
                // discarded every frame until `watermark_text_input` carried it
                // out of `chart_toolbar_render_dropdown`).
                if let Some((rect, ref positions)) = dd.watermark_text_input {
                    let wid = WidgetId::from(text_input::WATERMARK_TEXT);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &wid,
                        (rect.x, rect.y, rect.width, rect.height),
                        positions.clone(),
                    );
                }
            }
            // Quick-search editor geometry (2026-07-17 rework: the
            // button-hosted editor is the ONLY quick-search input) — the
            // `SYMBOL_QUICK_SEARCH` field's rect/char-positions come from
            // the toolbar BUTTON's own overlay.
            if let Some((rect, ref positions)) = toolbar_result.symbol_inline_input {
                let field_id = WidgetId::from(text_input::SYMBOL_QUICK_SEARCH);
                self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                    &field_id,
                    (rect.x, rect.y, rect.width, rect.height),
                    positions.clone(),
                );
            }
        }

        // Trading header inline symbol editor geometry (trading-header-
        // symbol-edit arc, 2026-07-25) — the SAME `SYMBOL_QUICK_SEARCH`
        // field's rect/char-positions when it instead comes from the
        // Trading Container header tab's own overlay
        // (`render_leaf_tab`'s `text_edit` output, carried here via
        // `last_sidebar_result.panel_overlay_zones`), mirroring
        // `symbol_inline_input`'s role for the topbar surface above.
        if let Some(edit) = self.symbol_quick_edit.as_ref() {
            if let SymbolQuickEditTarget::TradingPanel(pid) = edit.target {
                if let Some((rect, positions)) = self.last_sidebar_result.as_ref()
                    .and_then(|sr| sr.panel_overlay_zones.iter()
                        .find(|(p, _, _)| *p == pid.0)
                        .and_then(|(_, _, zones)| zones.text_edit.clone()))
                {
                    let field_id = WidgetId::from(text_input::SYMBOL_QUICK_SEARCH);
                    self.input_coordinator.borrow_mut().text_fields_mut().update_field(
                        &field_id,
                        (rect[0], rect[1], rect[2], rect[3]),
                        positions,
                    );
                }
            }
        }
    }


    // -------------------------------------------------------------------------
    // Agent hover-focus
    // -------------------------------------------------------------------------

    /// Update `agent_pty_hover_focused` based on the current mouse position.
    ///
    /// Call from the `CursorMoved` handler in `chart-app-vello` with the
    /// chart-relative coordinates (i.e. after subtracting chrome height from
    /// the window `y`).  Returns `true` if the hover state changed, indicating
    /// that the sidebar scene should be marked dirty.
    ///
    /// The hover focus is *transient* — it is cleared automatically when the
    /// cursor leaves the terminal rect.  A click inside the rect should promote
    /// to a persistent click-focus (handled separately in the click handler).
    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Apply the group's current instrument key (symbol / exchange / account_type)
    /// to all panel members of the given sync group.
    ///
    /// Called when the active chart changes (after `reassign_synced_panels`) and
    /// when a chart changes its symbol (from `propagate_symbol_to_sync_group`).
    pub(crate) fn apply_key_to_panels_in_group(
        &mut self,
        group_id: mlc_core::ids::BubbleId,
    ) {
        let (symbol, exchange, account_type, panel_ids) = {
            let mgr = self.core.bubbles();
            let Some(instrument) = mgr.instrument(group_id) else { return };
            (
                instrument.address.symbol,
                instrument.address.exchange,
                instrument.address.account_type.as_label().to_string(),
                self.core.tags().panel_members(group_id),
            )
        };

        // Track which (exchange, symbol, account_type) combos need depth subscription
        // so panels that receive orderbook data start getting the new symbol's stream.
        let mut needs_depth_sub = false;

        // Collect old depth subscriptions that differ from the new symbol so we
        // can unsubscribe them after updating panel state.  Each entry is
        // (exchange_string, symbol_string, account_type_string).
        let mut old_depth_subs: Vec<(String, String, String)> = Vec::new();

        for pid in &panel_ids {
            let panel_id = mlc_core::state::PanelId(*pid);
            // DOM symbol change: unsubscribe old orderbook + trades, subscribe new.
            // Two-pass to avoid holding a mutable borrow while calling bridge.
            let dom_change = self.panels_store.dom.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = dom_change {
                // Unsubscribe old orderbook and trades.
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.core.unsubscribe_orderbook(eid, &old_sym, old_at);
                        self.core.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                // Subscribe new orderbook and trades.
                let (new_ob_handle, new_trade_handle) = if !symbol.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&exchange) {
                        let new_at = account_type_from_label(&account_type);
                        let ob = self.core.subscribe_orderbook(eid, &symbol, new_at);
                        let tr = self.core.subscribe_trades(eid, &symbol, new_at);
                        (ob, tr)
                    } else {
                        (None, None)
                    }
                } else {
                    (None, None)
                };
                // Update state.
                if let Some(s) = self.panels_store.dom.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_orderbook = new_ob_handle;
                    s.last_seen_update_id = None;
                    s.level_qty_bid.clear();
                    s.level_qty_ask.clear();
                    s.shared_trades = new_trade_handle;
                    s.volume_by_price.clear();
                    s.max_volume = 0.0;
                    s.recent_fills.clear();
                }
            }
            if let Some(s) = self.panels_store.footprint.get_mut(&panel_id) {
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
            }
            if let Some(s) = self.panels_store.volume_profile.get_mut(&panel_id) {
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
            }
            if let Some(s) = self.panels_store.liquidity_heatmap.get_mut(&panel_id) {
                if s.symbol != symbol && !s.symbol.is_empty() {
                    old_depth_subs.push((s.exchange.clone(), s.symbol.clone(), s.account_type.clone()));
                }
                let symbol_changed = s.symbol != symbol;
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
                if symbol_changed {
                    s.snapshots.clear();
                    s.max_depth = 0.0;
                }
                needs_depth_sub = true;
            }
            // BigTrades symbol change: unsubscribe old, subscribe new.
            // We do this in two passes to avoid holding a mutable borrow on
            // panels_store at the same time as calling bridge methods.
            let bt_change = self.panels_store.big_trades.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = bt_change {
                // Unsubscribe old.
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.core.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                // Subscribe new.
                let new_handle = if !symbol.is_empty() {
                    digdigdig3::ExchangeId::from_str(&exchange).and_then(|eid| {
                        let new_at = account_type_from_label(&account_type);
                        self.core.subscribe_trades(eid, &symbol, new_at)
                    })
                } else {
                    None
                };
                // Update state.
                if let Some(s) = self.panels_store.big_trades.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_trades = new_handle;
                    s.last_seen_ts_ms = 0;
                    s.big_trades.clear();
                }
            }
            // LiqTape symbol change: reset the all-venue roster to the new
            // symbol — handles/cursors drop, the feed self-heal refills.
            if let Some(s) = self.panels_store.liq_tape.get_mut(&panel_id) {
                let symbol_changed = s.symbol != symbol;
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
                if symbol_changed && !symbol.is_empty() {
                    s.ensure_roster(&symbol);
                }
            }
            // VolumeProfile symbol change: unsubscribe old, subscribe new.
            let vp_change = self.panels_store.volume_profile.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = vp_change {
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.core.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                let new_handle = if !symbol.is_empty() {
                    digdigdig3::ExchangeId::from_str(&exchange).and_then(|eid| {
                        let new_at = account_type_from_label(&account_type);
                        self.core.subscribe_trades(eid, &symbol, new_at)
                    })
                } else {
                    None
                };
                if let Some(s) = self.panels_store.volume_profile.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_trades = new_handle;
                    s.last_seen_ts_ms = 0;
                    s.volume_by_price.clear();
                    s.buy_sell_by_price.clear();
                    s.total_volume = 0.0;
                    s.max_volume_at_price = 0.0;
                    s.poc = 0.0;
                }
            }
            // Footprint symbol change: unsubscribe old, subscribe new.
            let fp_change = self.panels_store.footprint.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = fp_change {
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.core.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                let new_handle = if !symbol.is_empty() {
                    digdigdig3::ExchangeId::from_str(&exchange).and_then(|eid| {
                        let new_at = account_type_from_label(&account_type);
                        self.core.subscribe_trades(eid, &symbol, new_at)
                    })
                } else {
                    None
                };
                if let Some(s) = self.panels_store.footprint.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_trades = new_handle;
                    s.last_seen_ts_ms = 0;
                    s.footprints.clear();
                    s.poc_by_candle.clear();
                    s.imbalances.clear();
                }
            }
            // TradeTape symbol change: unsubscribe old, subscribe new.
            let tt_change = self.panels_store.trade_tape.get(&panel_id).and_then(|s| {
                if s.symbol != symbol || s.exchange != exchange || s.account_type != account_type {
                    Some((s.symbol.clone(), s.exchange.clone(), s.account_type.clone()))
                } else {
                    None
                }
            });
            if let Some((old_sym, old_exch, old_at_label)) = tt_change {
                if !old_sym.is_empty() {
                    if let Some(eid) = digdigdig3::ExchangeId::from_str(&old_exch) {
                        let old_at = account_type_from_label(&old_at_label);
                        self.core.unsubscribe_trades(eid, &old_sym, old_at);
                    }
                }
                let new_handle = if !symbol.is_empty() {
                    digdigdig3::ExchangeId::from_str(&exchange).and_then(|eid| {
                        let new_at = account_type_from_label(&account_type);
                        self.core.subscribe_trades(eid, &symbol, new_at)
                    })
                } else {
                    None
                };
                if let Some(s) = self.panels_store.trade_tape.get_mut(&panel_id) {
                    s.symbol = symbol.clone();
                    s.exchange = exchange.clone();
                    s.account_type = account_type.clone();
                    s.shared_trades = new_handle;
                    s.last_seen_ts_ms = 0;
                    s.trades.clear();
                }
            }
            if let Some(s) = self.panels_store.l2_tape.get_mut(&panel_id) {
                if s.symbol != symbol && !s.symbol.is_empty() {
                    old_depth_subs.push((s.exchange.clone(), s.symbol.clone(), s.account_type.clone()));
                }
                let symbol_changed = s.symbol != symbol;
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
                if symbol_changed {
                    s.events.clear();
                    s.previous_book.clear();
                    s.spoof_alerts.clear();
                }
                needs_depth_sub = true;
            }
            if let Some(s) = self.panels_store.order_entry.get_mut(&panel_id) {
                s.symbol = symbol.clone();
                s.exchange = exchange.clone();
                s.account_type = account_type.clone();
            }
            // TradingContainer: color-sync groups are the container's ONLY
            // symbol-follow mechanism now (Auto/Pinned source badge removed).
            // A naive field write here is not enough — the container's hosted
            // DOM/OrderEntry/PositionManager/TradeLog need the FULL rebind
            // (unsubscribe old feeds, clear DOM aggregation, resubscribe,
            // refresh trading snapshot). `rebind_trading_container` already
            // no-ops when `resolved` matches the container's current binding.
            if self.panels_store.trading_container.contains_key(&panel_id) {
                self.rebind_trading_container(panel_id, mlc_app_panels::trading::ResolvedSymbol {
                    symbol: symbol.clone(),
                    exchange: exchange.clone(),
                    account_type: account_type.clone(),
                });
            }
            // Merged DOM's embedded multi-exchange book (Stage A): keep its
            // symbol in lockstep — resubscribe the roster to the new symbol
            // and drop stale books. Roster itself is tag-group independent.
            let md_change = self.panels_store.dom.get(&panel_id).and_then(|s| {
                if s.multi.symbol != symbol && !s.multi.config.exchanges.is_empty() {
                    Some((s.multi.config.exchanges.clone(), s.multi_mode))
                } else {
                    None
                }
            });
            if let Some((exchanges, multi_active)) = md_change {
                if multi_active && !symbol.is_empty() {
                    let new_at = digdigdig3::AccountType::Spot;
                    for ex_label in &exchanges {
                        if let Some(eid) = digdigdig3::ExchangeId::from_str(ex_label) {
                            let _ = self.core.subscribe_orderbook(eid, &symbol, new_at);
                        }
                    }
                }
                if let Some(s) = self.panels_store.dom.get_mut(&panel_id) {
                    s.multi.symbol = symbol.clone();
                    // Drop stale books + handles; the per-tick feed-handle
                    // self-heal resubscribes fresh trackers for the new key.
                    for (b, a) in &mut s.multi.books_by_exchange {
                        b.clear();
                        a.clear();
                    }
                    s.multi.handles.clear();
                    s.multi.mid_price = 0.0;
                }
            }
        }

        // Unsubscribe old depth streams that are no longer needed.  We deduplicate
        // so that multiple panels sharing the same old (exchange, symbol) only send
        // one RemoveSymbol command.
        old_depth_subs.sort_unstable();
        old_depth_subs.dedup();
        for (old_exch, old_sym, old_at_label) in &old_depth_subs {
            // Skip if this old key is the same as the new key — nothing to remove.
            if old_sym == &symbol && old_exch == &exchange && old_at_label == &account_type {
                continue;
            }
            let old_eid = self.core.exchange_symbols()
                .find_by_label(old_exch.as_str())
                .unwrap_or(self.active_exchange);
            let old_at = crate::account_type_from_label(old_at_label);
            self.core.unsubscribe_depth(old_eid, old_sym, old_at);
            dlog!(
                "[BubbleStore] unsubscribed depth for panel group {} ← {} @ {} ({})",
                group_id.0, old_sym, old_exch, old_at_label
            );
        }

        // If any panel needs orderbook data for the new symbol, subscribe the depth
        // WebSocket stream so live updates start arriving for it.
        if needs_depth_sub && !symbol.is_empty() {
            let eid = self.core.exchange_symbols()
                .find_by_label(&exchange)
                .unwrap_or(self.active_exchange);
            let at = crate::account_type_from_label(&account_type);
            self.core.subscribe_depth(eid, &symbol, at);
            dlog!(
                "[BubbleStore] subscribed depth for panel group {} → {} @ {} ({})",
                group_id.0, symbol, exchange, account_type
            );
        }
    }

    /// Request bars for every non-active open preset so the bridge cache is
    /// populated before the user clicks a background tab.
    ///
    /// Safe to call repeatedly: `request_bars` is deduped by the bridge's
    /// in-flight guard, so a double-call never creates a duplicate fetch.
    /// Call this once per startup after presets and open_tabs are populated
    /// (i.e. after `LoadPreset` completes in `new_window`).
    pub fn warm_background_tabs(&mut self) {
        let active_id = self.panel_app.active_preset_id.clone();
        let bar_count = self.core.user().profile().domain.bar_count as usize;
        let presets_guard = self.core.presets();
        let warm_targets: Vec<(String, String, mlc_core::state::Timeframe, String)> = self
            .panel_app
            .open_tabs
            .iter()
            .filter(|tab| **tab != active_id)
            .filter_map(|tab| presets_guard.get(tab))
            .flat_map(|preset| {
                preset.windows.iter().map(move |w| {
                    // Prefer the owning group's identity so the warm-up
                    // request matches the key the live restore will use.
                    // A window snapshot may carry stale account_type (e.g.
                    // "S") when the group's is "F" — using the window's
                    // snapshot would produce a cache-miss on restore.
                    if let Some(bid) = w.bubble_id {
                        if let Some(b) = preset.bubbles.iter().find(|b| b.id == bid) {
                            if !b.symbol.is_empty() {
                                return (
                                    b.symbol.clone(),
                                    b.exchange.clone(),
                                    b.timeframe.clone(),
                                    b.account_type.clone(),
                                );
                            }
                        }
                    }
                    (w.symbol.clone(), w.exchange.clone(), w.timeframe.clone(), w.account_type.clone())
                })
            })
            .filter(|(sym, _, _, _)| !sym.is_empty())
            .collect();
        drop(presets_guard);
        let mut warmed = 0usize;
        for (sym, exch, tf, at_label) in warm_targets {
            let eid = digdigdig3::ExchangeId::from_str(&exch)
                .unwrap_or(digdigdig3::ExchangeId::Binance);
            let at = account_type_from_label(&at_label);
            self.core.ensure_connector(eid);
            self.core.request_kline(eid, &sym, &tf, at, None, Some(bar_count), false);
            warmed += 1;
        }
        log::debug!("[ChartApp] startup: warmed {} background tab series", warmed);
    }

    /// Amortized variant of [`warm_background_tabs`] for the post-promote
    /// path: return the target list without executing anything, so the
    /// caller can warm ONE series per frame instead of blocking the main
    /// thread for the whole batch (~120 ms per series measured).
    pub fn warm_background_tab_targets(&self) -> Vec<(String, String, mlc_core::state::Timeframe, String)> {
        let active_id = self.panel_app.active_preset_id.clone();
        let presets_guard = self.core.presets();
        self.panel_app
            .open_tabs
            .iter()
            .filter(|tab| **tab != active_id)
            .filter_map(|tab| presets_guard.get(tab))
            .flat_map(|preset| {
                preset.windows.iter().map(move |w| {
                    if let Some(bid) = w.bubble_id {
                        if let Some(b) = preset.bubbles.iter().find(|b| b.id == bid) {
                            if !b.symbol.is_empty() {
                                return (
                                    b.symbol.clone(),
                                    b.exchange.clone(),
                                    b.timeframe.clone(),
                                    b.account_type.clone(),
                                );
                            }
                        }
                    }
                    (w.symbol.clone(), w.exchange.clone(), w.timeframe.clone(), w.account_type.clone())
                })
            })
            .filter(|(sym, _, _, _)| !sym.is_empty())
            .collect()
    }

    /// Warm a single background-tab series (one step of the amortized
    /// post-promote warm-up).
    pub fn warm_one_background_tab(&mut self, target: &(String, String, mlc_core::state::Timeframe, String)) {
        let (sym, exch, tf, at_label) = target;
        let bar_count = self.core.user().profile().domain.bar_count as usize;
        let eid = digdigdig3::ExchangeId::from_str(&exch)
            .unwrap_or(digdigdig3::ExchangeId::Binance);
        let at = account_type_from_label(at_label);
        self.core.ensure_connector(eid);
        self.core.request_kline(eid, sym, tf, at, None, Some(bar_count), false);
    }

    /// CVD Display=Candles overlay body-painter input (owner spec
    /// 2026-07-11): per-bar delta diffed from the CVD instance's cumulative
    /// per-bar output, index-aligned to `bubble.bars`. `None` unless the
    /// chart type is `ChartTypeDef::delta_paintable` AND a visible CVD
    /// instance on `bid` has Display=Candles — so the default render pays
    /// nothing. Read-only facade access, statement-scoped guard.
    fn cvd_paint_deltas_for(&self, bid: mlc_core::ids::BubbleId, chart_type: &str) -> Option<Vec<f64>> {
        if !mlc_core::chart_type::by_id(chart_type).is_some_and(|d| d.delta_paintable) {
            return None;
        }
        let ind = self.core.indicators();
        let cumulative: Vec<f64> = ind
            .instances_for_bubble(bid)
            .into_iter()
            .find(|inst| {
                inst.visible
                    && inst.type_id.eq_ignore_ascii_case("cvd")
                    && inst.params.get("display").and_then(|v| v.as_string()) == Some("Candles")
            })
            .and_then(|inst| {
                // Primary output = the definition's first output name.
                let def = ind.definition(&inst.type_id)?;
                let name = def.outputs.first().map(|o| o.name.as_str())?;
                inst.values.get(name).cloned()
            })?;
        if cumulative.len() < 2 {
            return None;
        }
        let mut deltas = vec![f64::NAN; cumulative.len()];
        for i in 1..cumulative.len() {
            let (a, b) = (cumulative[i - 1], cumulative[i]);
            if a.is_finite() && b.is_finite() {
                deltas[i] = b - a;
            }
        }
        Some(deltas)
    }

    /// Whether the venue publishes a bulk daily trade-archive for `bid`'s
    /// `(exchange, account_type)` — thin resolve-at-render-time wrapper over
    /// `BackendCore::trade_archive_available`. Takes the caller's already-
    /// held `Bubbles` guard (`bubbles`) instead of re-acquiring
    /// `self.core.bubbles()` — every call site calls this while already
    /// holding one, and `BackendCore`'s bubble lock is a plain
    /// `std::sync::RwLock` (nested read-acquisition on the same thread is
    /// not guaranteed non-deadlocking). Feeds
    /// `ChartWindowRenderInputs::trade_archive_available`, which
    /// `build_chart_render_state` forwards 1:1 onto `ChartRenderState`; see
    /// that field's doc comment for what it gates. `false` when `bid`'s
    /// exchange string doesn't parse (unknown venue) or isn't connected
    /// yet — no promise, never a guess.
    fn trade_archive_available_for(&self, bubbles: &mlc_backend::bubble::Bubbles<'_>, bid: mlc_core::ids::BubbleId) -> bool {
        let Some(exchange) = bubbles.exchange(bid) else { return false };
        let Some(eid) = digdigdig3::ExchangeId::from_str(exchange) else { return false };
        let account_type = bubbles.account_type_typed(bid).unwrap_or_default();
        self.core.trade_archive_available(eid, account_type)
    }

    /// Synchronise `window.inner_docking` with the external `IndicatorManager`.
    ///
    /// Reads desired sub-pane instance ids from `indicator_manager`, walks the
    /// inner docking tree, adds missing leaves via `split_leaf`, hides vanished
    /// leaves via `hide_leaf`, restores re-appeared leaves via `show_leaf`.
    /// Also seeds `sub_pane_y_state` for brand-new panes from the visible range
    /// and drains pending preset restore state into the tree.
    ///
    /// Call after any operation that adds/removes indicators or recalculates
    /// indicator values (init, tick, indicator create/delete).
    pub fn sync_sub_panes_from_manager(&mut self) {
        use crate::chart_container::collect_inner_instance_ids;
        use crate::chart_container_leaf::ChartContainerLeaf;

        // Step 1: collect window-level read-only data before taking mutable borrows.
        // Iterates the containers MAP (not the leaf pairs) so the trading
        // container's leafless embedded center chart reconciles its
        // sub-pane docking exactly like grid charts (chart-in-container
        // arc, 2026-07-26 — a leaf-paired walk silently skipped it and
        // sub-pane indicators added to the aux bubble never materialized).
        let window_data: Vec<(mlc_core::ChartId, u64, usize, usize)> = self
            .panel_app
            .panel_grid
            .containers()
            .iter()
            .map(|(&cid, window)| {
                // Indicators are scoped by bubble — pass current_tag_id.
                let bubble_id_val = window.current_tag_id.0;
                let (vs, ve) = window.viewport().visible_range();
                (cid, bubble_id_val, vs, ve)
            })
            .collect();

        // Step 2: for each window, collect desired instance_ids from indicator_manager.
        for (cid, bubble_id_val, visible_start, visible_end) in &window_data {
            let pane_ids: Vec<u64> = self.core.indicators()
                .instances_for_bubble(mlc_core::ids::BubbleId(*bubble_id_val))
                .into_iter()
                .filter(|i| i.pane > 0)
                // CVD Display=Candles (owner spec 2026-07-11): the instance
                // becomes a main-chart overlay body-painter — its sub-pane
                // is hidden while the mode is active (flipping back to Line
                // re-adds the pane through this same sync).
                .filter(|i| {
                    !(i.type_id.eq_ignore_ascii_case("cvd")
                        && i.params.get("display").and_then(|v| v.as_string()) == Some("Candles"))
                })
                .map(|i| i.id)
                .collect();
            let sub_pane_data: Vec<(u64, Option<(f64, f64)>)> = pane_ids
                .into_iter()
                .map(|iid| {
                    let range = self.core.indicators_mut().calculate_pane_range(
                        iid,
                        *visible_start,
                        *visible_end,
                    );
                    (iid, range)
                })
                .collect();

            let window = match self.panel_app.panel_grid.containers_mut().get_mut(cid) {
                Some(w) => w,
                None => continue,
            };

            let pending_ratios = self.pending_sub_pane_ratios.get(&window.id.0).cloned();
            let pending_order = self.pending_sub_pane_order.get(&window.id.0).cloned();
            let pending_hidden = self.pending_sub_pane_hidden.get(&window.id.0).cloned();
            let pending_above = self.pending_sub_pane_above_main.get(&window.id.0).cloned();

            // Apply above-main set from preset restore (one-shot via pending map).
            if let Some(ref above_set) = pending_above {
                window.sub_pane_above_main = above_set.clone();
            }

            // Determine the desired order (from preset restore or indicator_manager order).
            let desired_ids: Vec<u64> = if let Some(ref order) = pending_order {
                // Preset order first, then append newly-discovered ids.
                let mut ids: Vec<u64> = order.iter()
                    .filter(|&&iid| sub_pane_data.iter().any(|(id, _)| *id == iid))
                    .copied()
                    .collect();
                for (iid, _) in &sub_pane_data {
                    if !ids.contains(iid) {
                        ids.push(*iid);
                    }
                }
                ids
            } else {
                sub_pane_data.iter().map(|(id, _)| *id).collect()
            };

            let range_map: std::collections::HashMap<u64, Option<(f64, f64)>> =
                sub_pane_data.iter().map(|(id, r)| (*id, *r)).collect();

            // Step 3: reconcile inner_docking with desired_ids.
            // Get current leaf set (excluding MainChart leaf at index 0).
            let current_ids = collect_inner_instance_ids(&window.inner_docking);

            // Find the main chart leaf id (always first child).
            let main_leaf_id = window.inner_docking.tree().root().children
                .first()
                .and_then(|n| if let uzor::panels::PanelNode::Leaf(l) = n { Some(l.id) } else { None });

            // Add leaves for desired_ids not yet in the tree (or hidden).
            for &iid in &desired_ids {
                let existing_leaf = window.inner_docking.tree().leaves().iter()
                    .find(|l| l.panels.first().and_then(|p| p.indicator_instance_id()) == Some(iid))
                    .map(|l| (l.id, l.hidden));
                match existing_leaf {
                    None => {
                        // New indicator: append as a flat sibling under the root branch.
                        // split_leaf would nest the new leaf inside a fresh sub-branch,
                        // breaking the [MainChart, SubPane1, SubPane2..] sibling list
                        // that the render path's collect_inner_* helpers depend on.
                        let last_leaf_id = window.inner_docking.tree().root().children
                            .last()
                            .and_then(|n| if let uzor::panels::PanelNode::Leaf(l) = n { Some(l.id) } else { None })
                            .or(main_leaf_id);
                        if let Some(anchor) = last_leaf_id {
                            window.inner_docking.tree_mut().add_leaf_near(
                                ChartContainerLeaf::SubPane { instance_id: iid },
                                anchor,
                            );
                        }
                        // Seed y_state for the new pane.
                        let entry = window.sub_pane_y_state.entry(iid).or_default();
                        if let Some(Some((p_min, p_max))) = range_map.get(&iid) {
                            entry.price_min = *p_min;
                            entry.price_max = *p_max;
                        }
                    }
                    Some((leaf_id_inner, true)) => {
                        // Leaf exists but was hidden — restore it.
                        window.inner_docking.tree_mut().show_leaf(leaf_id_inner);
                    }
                    Some((_, false)) => {
                        // Already present and visible — nothing to do.
                    }
                }
            }

            // Hide leaves for instance_ids that have vanished from desired set.
            let current_ids_after = collect_inner_instance_ids(&window.inner_docking);
            for iid in &current_ids_after {
                if !desired_ids.contains(iid) {
                    if let Some(leaf_id_inner) = window.inner_docking.tree().leaves().iter()
                        .find(|l| l.panels.first().and_then(|p| p.indicator_instance_id()) == Some(*iid))
                        .map(|l| l.id)
                    {
                        window.inner_docking.tree_mut().hide_leaf(leaf_id_inner);
                    }
                }
            }

            // Apply preset-restore hidden set.
            if let Some(ref hidden_set) = pending_hidden {
                for &iid in hidden_set {
                    if let Some(leaf_id_inner) = window.inner_docking.tree().leaves().iter()
                        .find(|l| l.panels.first().and_then(|p| p.indicator_instance_id()) == Some(iid))
                        .map(|l| l.id)
                    {
                        window.inner_docking.tree_mut().hide_leaf(leaf_id_inner);
                    }
                }
            }

            // Set preserve_if_empty on root branch so splits aren't collapsed when empty.
            let root_id = window.inner_docking.tree().root().id;
            window.inner_docking.tree_mut().set_branch_preserve_if_empty(root_id, true);

            // Re-pin the vertical stack layout after the add/hide/show
            // reconcile above: uzor re-infers HORIZONTAL layouts from child
            // count on every structural change, which breaks separator drag
            // math (see pin_vertical_stack_layout docs).
            crate::chart_container::pin_vertical_stack_layout(&mut window.inner_docking);

            // Apply preset-restore proportions if available.
            if let Some(ref ratios) = pending_ratios {
                let current_visible_ids = collect_inner_instance_ids(&window.inner_docking);
                if !current_visible_ids.is_empty() {
                    let sub_total: f64 = current_visible_ids.iter()
                        .map(|iid| ratios.get(iid).copied().unwrap_or(0.15) as f64)
                        .sum::<f64>()
                        .min(0.75);
                    let n = current_visible_ids.len();
                    let default_sub = sub_total / n as f64;
                    let main_share = (1.0 - sub_total).max(0.1);
                    let mut props = vec![main_share];
                    for iid in &current_visible_ids {
                        props.push(ratios.get(iid).copied().unwrap_or(default_sub as f32) as f64);
                    }
                    let total: f64 = props.iter().sum();
                    if total > 0.0 {
                        for v in &mut props { *v /= total; }
                    }
                    let root_branch_id = window.inner_docking.tree().root().id;
                    window.inner_docking.tree_mut().set_branch_proportions(root_branch_id, props);
                }
            } else {
                // Seed proportions when none have been set yet (e.g. after
                // add_leaf_near clears them on every structural change).
                // Without seeding, drag_separator falls back to equal 1/N
                // shares which differs from the rendered layout (main large,
                // sub-panes ~0.2 each) causing a visible jump on first grab.
                //
                // Rule: each sub-pane gets 0.2 of total, main gets the rest,
                // clamped so main is never below 0.4. This mirrors
                // InnerLayout::add_indicator which also carves 0.2 per pane.
                let root_child_count = window.inner_docking.tree().root().children.len();
                let stored_props_len = window.inner_docking.tree().root().proportions.len();
                if stored_props_len != root_child_count && root_child_count >= 2 {
                    let n_panes = root_child_count - 1; // subtract main-chart leaf
                    // 0.2 per sub-pane, main = remainder, clamped to [0.4, 1.0].
                    let sub_share_each = 0.2_f64;
                    let main_share = (1.0 - sub_share_each * n_panes as f64).max(0.4);
                    let mut props = vec![main_share];
                    for _ in 0..n_panes {
                        props.push(sub_share_each);
                    }
                    // Normalize so sum == 1.0.
                    let total: f64 = props.iter().sum();
                    if total > 0.0 {
                        for v in &mut props {
                            *v /= total;
                        }
                    }
                    let root_branch_id = window.inner_docking.tree().root().id;
                    window.inner_docking.tree_mut().set_branch_proportions(root_branch_id, props);
                }
            }

            // Suppress "current_ids" unused warning from the step before reconcile.
            let _ = current_ids;
        }

        // Clear all pending restore state once applied (one-shot).
        self.pending_sub_pane_ratios.clear();
        self.pending_sub_pane_above_main.clear();
        self.pending_sub_pane_order.clear();
        self.pending_sub_pane_hidden.clear();

        // CVD fold resubscribe on hydration (owner arc 2026-07-11): a
        // hydrated CVD instance has no live fold — the subscribe only ever
        // fired on the add click — leaving the Line pane flat and the
        // Candles body-painter silent after every reload. Re-install per
        // window when missing; `cvd_line` presence gates repeats
        // (subscribe_cvd_fold seeds it, so steady state is a cheap
        // None-check). The subscribe helper self-gates footprint / missing
        // instrument / disabled connector.
        let resub: Vec<(mlc_core::state::ChartId, mlc_core::ids::BubbleId)> = self
            .panel_app
            .panel_grid
            .iter_containers()
            .filter_map(|(leaf_id, w)| {
                let cid = self.panel_app.panel_grid.container_id_for_leaf(leaf_id)?;
                Some((cid, w.current_tag_id))
            })
            .collect();
        for (cid, bid) in resub {
            let has_cvd = self
                .core
                .indicators()
                .instances_for_bubble(bid)
                .into_iter()
                .any(|i| i.type_id.eq_ignore_ascii_case("cvd"));
            if !has_cvd {
                continue;
            }
            let fold_missing = self.with_bubble(bid, |b| b.cvd_line.is_none()).unwrap_or(false);
            if fold_missing {
                self.subscribe_cvd_line_for_indicator_add(Some(cid));
            }
        }
    }

    /// Sync sub-pane pixel geometry (height, y_offset, chart_width) from the
    /// computed layout into each window's sub_pane structs.
    ///
    /// Must be called with `&mut self` so we can write to sub_panes.  Call
    /// once per frame inside `prepare_frame`, after the layout has been
    /// computed, so that `PanSubPane` action handlers and other code that
    /// reads these fields see up-to-date values.
    fn sync_sub_pane_geometry(&mut self) {
        // Phase 1: compute (leaf_id, leaf_rect, extended_layout) using &self.
        // `build_extended_layout_for_leaf` borrows self immutably, so we must
        // finish all those borrows before the mutable writes in phase 2.
        let content_rect = self.content_rect;

        let leaf_rects: Vec<(mlc_core::LeafId, LayoutRect)> = self
            .panel_app
            .panel_grid
            .panel_rects()
            .iter()
            .map(|(&leaf_id, &sub_rect)| {
                let leaf_rect = LayoutRect {
                    x: content_rect.x + sub_rect.x as f64,
                    y: content_rect.y + sub_rect.y as f64,
                    width: sub_rect.width as f64,
                    height: sub_rect.height as f64,
                };
                (leaf_id, leaf_rect)
            })
            .collect();

        let extended_layouts: Vec<(mlc_core::LeafId, mlc_core::ExtendedFrameLayout)> =
            leaf_rects
                .iter()
                .filter_map(|&(leaf_id, leaf_rect)| {
                    let extended = self.build_extended_layout_for_leaf(leaf_id, &leaf_rect)?;
                    Some((leaf_id, extended))
                })
                .collect();

        // Phase 2: geometry writeback was previously to SubPane structs.
        // After S4 the inner_docking tree drives layout; no per-pane geometry
        // fields need to be written back here. Leave loop body empty.
        let _ = &extended_layouts;

        // Phase 3: store per-leaf layouts as state so input handlers can read
        // them without rebuilding (producer/consumer split).  Rebuild fresh each
        // call so removed leaves never linger.
        self.frame_layouts = extended_layouts.into_iter().collect();

        // Build and cache the single/active-leaf layout used by non-split input
        // paths and process_output_actions.  This is the correct place because
        // sync_sub_pane_geometry runs in the layout phase (prepare_frame) once
        // per frame, before any input events for that frame are processed.
        self.active_frame_layout = Some(self.build_extended_layout());
    }

    /// Build indicator catalog items from the IndicatorManager definitions.
    fn build_indicator_catalog(&self) -> Vec<IndicatorCatalogItem> {
        let mut items: Vec<IndicatorCatalogItem> = self.core.indicators().definitions().iter().map(|def| {
            IndicatorCatalogItem::new(&def.type_id, &def.name, &def.short_name, def.category)
                .with_description(&def.description)
                .with_overlay(def.overlay)
        }).collect();
        items.sort_by(|a, b| a.short_name.cmp(&b.short_name));
        items
    }

    /// Build `SearchResult` list from exchange symbols, filtered by `query`.
    ///
    /// Iterates all exchanges and collects up to 100 results per exchange
    /// (200 total). Returns empty list if no symbols are loaded yet.
    pub(crate) fn build_symbol_search_results(
        query: &str,
        active_watchlist: Option<&mlc_core::WatchlistList>,
        exchange_symbols: &std::collections::HashMap<digdigdig3::ExchangeId, Vec<mlc_backend::CatalogueSymbol>>,
    ) -> Vec<mlc_core::state::modal_state::SearchResult> {
        use mlc_core::state::modal_state::SearchResult;
        use std::collections::HashMap;

        let q_full = query.to_lowercase();

        // Composite query syntax: "SYMBOL[:EXCHANGE[:AT]]" — TradingView-style.
        // Part 1 filters exchanges (full slug, slug prefix, or a short alias),
        // part 2 filters account type ("s"/"f", also "spot"/"fut"/"futures"/"perp").
        // ANY common separator is accepted — `:`, `/`, or whitespace (owner
        // rule 2026-07-17: "ввести как удобно — принять и зарезолвить");
        // parts stay positional. Symbols never contain these characters in
        // canonical form, so the split is unambiguous.
        let mut q_parts = q_full.splitn(3, |c: char| {
            c == ':' || c == '/' || c == '\\' || c == '.' || c.is_whitespace()
        });
        let q = q_parts.next().unwrap_or_default().to_string();
        let q_exchange = q_parts.next().map(str::trim).filter(|p| !p.is_empty());
        let q_at = q_parts.next().map(str::trim).filter(|p| !p.is_empty());
        // "SYMBOL:AT" shorthand (owner spec 2026-07-30): when the second part
        // is an account-type token rather than an exchange, treat it as the
        // AT filter — "btcusdt:f" == "btcusdt::f".
        let (q_exchange, q_at) = match (q_exchange, q_at) {
            (Some(p), None) if matches!(p, "s" | "spot" | "f" | "fut" | "futures" | "perp") => {
                (None, Some(p))
            }
            other => other,
        };

        /// Short exchange aliases for the composite search syntax.
        fn exchange_alias(alias: &str) -> Option<&'static str> {
            Some(match alias {
                "bn" => "binance",
                "bb" => "bybit",
                "kc" => "kucoin",
                "gt" => "gateio",
                "hl" => "hyperliquid",
                "ltr" => "lighter",
                "dr" => "deribit",
                "up" => "upbit",
                "cc" => "cryptocom",
                "cb" => "coinbase",
                "bg" => "bitget",
                "bf" => "bitfinex",
                "bs" => "bitstamp",
                "bx" => "bingx",
                "gm" => "gemini",
                "kr" => "kraken",
                "dy" => "dydx",
                "mx" => "mexc",
                _ => return None,
            })
        }

        /// Does the exchange slug match the query's exchange part?
        /// Accepts an exact alias ("hl") or any prefix of the full slug ("hyp").
        fn exchange_matches(slug: &str, part: &str) -> bool {
            if let Some(full) = exchange_alias(part) {
                return slug == full;
            }
            slug.starts_with(part)
        }

        /// Does the account-type short label ("S"/"F") match the query's AT part?
        fn at_matches(short_label: &str, part: &str) -> bool {
            match part {
                "s" | "spot" => short_label.eq_ignore_ascii_case("S"),
                "f" | "fut" | "futures" | "perp" => short_label.eq_ignore_ascii_case("F"),
                other => short_label.eq_ignore_ascii_case(other),
            }
        }

        /// Compute the canonical key for a symbol when base+quote are non-empty.
        /// Returns `None` when either asset field is empty (no canonicalization).
        fn canonical_key(s: &mlc_backend::CatalogueSymbol) -> Option<String> {
            if s.base_asset.is_empty() || s.quote_asset.is_empty() {
                return None;
            }
            Some(format!("{}{}", s.base_asset, s.quote_asset).to_uppercase())
        }

        if !exchange_symbols.is_empty() {
            let mut results: Vec<SearchResult> = Vec::new();

            for (exchange_id, symbols) in exchange_symbols {
                // Canonical lowercase slug — used for both storage and display.
                let exchange_slug = exchange_id.as_str();

                // Composite-query exchange filter.
                if let Some(part) = q_exchange {
                    if !exchange_matches(&exchange_slug.to_lowercase(), part) {
                        continue;
                    }
                }

                // Build uniqueness count: (account_type_label, canonical_form) → count.
                // Only symbols with a computable canonical form are counted.
                // Symbols whose canonical form appears more than once within this
                // (exchange, account_type) group are NOT canonicalized — the raw
                // venue-native string keeps them distinct.
                let mut canonical_counts: HashMap<(String, String), usize> = HashMap::new();
                for s in symbols.iter() {
                    if let Some(canon) = canonical_key(s) {
                        *canonical_counts
                            .entry((s.account_type.short_label().to_string(), canon))
                            .or_insert(0) += 1;
                    }
                }

                let filtered: Vec<&mlc_backend::CatalogueSymbol> = symbols
                    .iter()
                    .filter(|s| {
                        // Composite-query account-type filter ("...:S" / "...:F").
                        if let Some(part) = q_at {
                            if !at_matches(s.account_type.short_label(), part) {
                                return false;
                            }
                        }
                        if q.is_empty() {
                            true
                        } else {
                            // Match against raw symbol, base asset, OR the canonical
                            // form so that e.g. "btcusdt" finds OKX "BTC-USDT".
                            let canon_match = canonical_key(s)
                                .map(|c| c.to_lowercase().contains(&q))
                                .unwrap_or(false);
                            s.symbol.to_lowercase().contains(&q)
                                || s.base_asset.to_lowercase().contains(&q)
                                || canon_match
                        }
                    })
                    .collect();

                for s in filtered.iter().take(100) {
                    // Decide the canonical symbol key for this search result.
                    // Use the canonical form only when it is unique within this
                    // (exchange, account_type) group; otherwise fall back to raw.
                    let symbol_key = canonical_key(s)
                        .filter(|c| {
                            canonical_counts
                                .get(&(s.account_type.short_label().to_string(), c.clone()))
                                .copied()
                                .unwrap_or(0)
                                == 1
                        })
                        .unwrap_or_else(|| s.symbol.clone());

                    let search_address = mlc_core::InstrumentAddress::new(
                        exchange_slug.to_string(),
                        symbol_key.clone(),
                        s.account_type.short_label().parse().unwrap_or_default(),
                    );
                    let in_watchlist = active_watchlist
                        .map(|l| l.contains(&search_address))
                        .unwrap_or(false);
                    let asset_type = if exchange_slug == "moex" { "Stock" } else { "Crypto" };
                    let category_icon = if exchange_slug == "moex" { "S" } else { "C" };
                    results.push(SearchResult {
                        symbol: symbol_key,
                        name: format!("{}/{}", s.base_asset, s.quote_asset),
                        exchange: exchange_slug.to_string(),
                        exchange_id: exchange_slug.to_string(),
                        asset_type: asset_type.to_string(),
                        category_icon: category_icon.to_string(),
                        in_watchlist,
                        account_type: s.account_type.short_label().to_string(),
                    });
                }
            }

            // Sort: query matches first by relevance, then by exchange.
            if !q.is_empty() {
                results.sort_by(|a, b| {
                    // Exact symbol match first.
                    let a_exact = a.symbol.to_lowercase() == q;
                    let b_exact = b.symbol.to_lowercase() == q;
                    b_exact.cmp(&a_exact)
                        .then_with(|| {
                            // USDT pairs before others.
                            let a_usdt = a.name.contains("/USDT");
                            let b_usdt = b.name.contains("/USDT");
                            b_usdt.cmp(&a_usdt)
                        })
                        .then(a.symbol.cmp(&b.symbol))
                });
            }

            results.truncate(200);
            results
        } else {
            // No symbols loaded yet — return empty list.
            Vec::new()
        }
    }


    /// Alias for `build_symbol_search_results` — kept for compatibility with callers
    /// in `input.rs` that still reference the old name.
    #[inline]
    pub(crate) fn build_demo_symbol_results(
        query: &str,
        active_watchlist: Option<&mlc_core::WatchlistList>,
        exchange_symbols: &std::collections::HashMap<digdigdig3::ExchangeId, Vec<mlc_backend::CatalogueSymbol>>,
    ) -> Vec<mlc_core::state::modal_state::SearchResult> {
        Self::build_symbol_search_results(query, active_watchlist, exchange_symbols)
    }

    /// Apply `ChartOutputAction` results to the active `ChartContainer`.
    ///
    /// Mirrors `TerminalApp::process_output_actions` but simplified:
    /// no multi-window panel manager, no frame layout offset.
    pub(crate) fn process_output_actions(&mut self, actions: Vec<ChartOutputAction>) {
        fn calc_visible_price_range(
            window: &mut crate::chart_container::ChartContainer,
            series: crate::active_series::ActiveSeries<'_>,
        ) {
            window.calc_auto_scale(series);
        }
        // Snapshot bars for the active chart once (read-only borrow), then proceed
        // with all the mutable container access below. process_output_actions
        // targets the active window throughout.
        let active_bars: Vec<mlc_core::Bar> = self.active_chart_bars();
        // Audit D8: the ACTIVE chart_type's own array (renko/pnf/kagi/...)
        // for autoscale — `active_bars` above stays kline-scoped for the ts
        // math call sites below (drag anchors, magnet snap) that are out of
        // this arc's scope.
        let active_series_owned = self.active_chart_active_series();

        // Compute chart layout ONCE for coordinate conversion.
        // All actions from DefaultChartInputHandler use screen-absolute coords;
        // viewport/crosshair expect chart-local coords (0,0 = top-left of chart canvas).
        // Clone the frame-cached layout so the borrow ends here, allowing the mutable
        // &mut self calls (propagate_viewport_to_sync_group, etc.) that follow.
        let extended = self.active_frame_layout.clone()
            .unwrap_or_else(|| self.build_extended_layout());
        let chart_x = extended.main_chart.chart.x;
        let chart_y = extended.main_chart.chart.y;
        let chart_w = extended.main_chart.chart.width;
        let chart_h = extended.main_chart.chart.height;

        let drag_mode = self.input_handler.state.drag_mode;

        // Per-chart-type zoom-out fit cap (registry `viewport_fit_cap`,
        // owner spec 2026-07-13) for the ACTIVE chart — resolved once
        // before the loop (immutable read; the Zoom arm below holds the
        // container mutably).
        let active_fit_cap: Option<u32> = self.panel_app.panel_grid.active_container_id()
            .and_then(|cid| self.chart_type_for_chart(cid))
            .and_then(|ct| mlc_core::chart_type::by_id(ct))
            .and_then(|d| d.viewport_fit_cap);

        for action in actions {
            match action {
                ChartOutputAction::Pan { bar_delta, price_delta } => {
                    // Block viewport panning when a click-based drawing tool is active.
                    // This ensures accidental micro-drags don't move the chart,
                    // keeping mouse release position close enough for click detection.
                    if self.has_click_drawing_tool() { continue; }
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    // bar_delta is a PIXEL delta (drag distance). Viewport
                    // dispatches internally: time mode shifts the time
                    // anchor, index mode converts via bar_spacing.
                    window.pan(bar_delta);
                    if let mlc_core::engine::input::DragMode::SubPaneChart { pane_index } = drag_mode {
                        let instance_id = crate::chart_container::collect_inner_instance_ids(
                            &window.inner_docking,
                        ).into_iter().nth(pane_index);
                        let pane_height = self.active_frame_layout.as_ref()
                            .and_then(|ext| ext.sub_panes.get(pane_index))
                            .map(|sp| sp.content.height)
                            .unwrap_or(100.0);
                        if let Some(iid) = instance_id {
                            if let Some(s) = window.sub_pane_y_state.get_mut(&iid) {
                                if !s.auto_scale {
                                    let price_range = s.price_max - s.price_min;
                                    let price_delta_scaled = price_delta * price_range / pane_height;
                                    s.price_min += price_delta_scaled;
                                    s.price_max += price_delta_scaled;
                                }
                            }
                        }
                    } else if window.price_scale().scale_mode.is_auto_y() {
                        calc_visible_price_range(window, active_series_owned.as_active_series());
                    } else {
                        let price_range = window.price_scale().price_max - window.price_scale().price_min;
                        let price_delta_scaled = price_delta * price_range / window.viewport().chart_height;
                        window.set_price_range(
                            window.price_scale().price_min + price_delta_scaled,
                            window.price_scale().price_max + price_delta_scaled,
                        );
                    }
                    // Propagate viewport change to sync group.
                    let view_start = window.viewport().view_start;
                    let bar_spacing = window.viewport().bar_spacing;
                    // End the mutable borrow so we can call propagate_viewport_to_sync_group.
                    let _ = window;
                    let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                    if let Some(active_leaf) = active_leaf_opt {
                        self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, None);
                    }
                }
                ChartOutputAction::Zoom { center_x, factor_x, factor_y, .. } => {
                    // Convert screen-absolute center_x to chart-local
                    let local_center_x = center_x - chart_x;
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    if factor_x != 1.0 {
                        // zoom_horizontal dispatches the zoom then auto-scales
                        // when the mode is auto-Y (mirrors calc_visible_price_range).
                        window.zoom_horizontal_capped(local_center_x, factor_x, active_fit_cap, active_series_owned.as_active_series());
                    }
                    if factor_y != 1.0 {
                        let center = (window.price_scale().price_min + window.price_scale().price_max) / 2.0;
                        let half_range = (window.price_scale().price_max - window.price_scale().price_min) / 2.0 * factor_y;
                        window.set_price_range(center - half_range, center + half_range);
                    }
                    // Propagate viewport change to sync group.
                    let view_start = window.viewport().view_start;
                    let bar_spacing = window.viewport().bar_spacing;
                    // End the mutable borrow so we can call propagate_viewport_to_sync_group.
                    let _ = window;
                    if factor_x != 1.0 {
                        let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                        if let Some(active_leaf) = active_leaf_opt {
                            self.propagate_viewport_to_sync_group(active_leaf, view_start, bar_spacing, None);
                        }
                    }
                    if factor_y != 1.0 {
                        // Y-scale drag: mirror the A/M button click pattern — update
                        // group.scale_mode then propagate so sync-group peers switch to M.
                        let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                        if let Some(active_leaf) = active_leaf_opt {
                            let viewport_sync_on = self.panel_app.panel_grid
                                .container_id_for_leaf(active_leaf)
                                .and_then(|cid| self.bubble_for_chart(cid))
                                .and_then(|gid| {
                                    self.core.bubbles_mut().set_scale_mode(gid, ScaleMode::Manual);
                                    self.core.bubbles().sync_flags(gid).map(|sf| sf.sync_viewport)
                                })
                                .unwrap_or(false);
                            if viewport_sync_on {
                                self.propagate_viewport_to_sync_group(
                                    active_leaf,
                                    view_start,
                                    bar_spacing,
                                    Some(ScaleMode::Manual),
                                );
                            }
                        }
                    }
                }
                ChartOutputAction::ResetPriceScale => {
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    if let mlc_core::engine::input::DragMode::SubPanePriceScale { pane_index } = drag_mode {
                        let instance_id = crate::chart_container::collect_inner_instance_ids(
                            &window.inner_docking,
                        ).into_iter().nth(pane_index);
                        if let Some(iid) = instance_id {
                            window.sub_pane_y_state.entry(iid).or_default().auto_scale = true;
                        }
                        // Restore main chart to Auto so the A/M button reflects the true state.
                        window.set_scale_mode(ScaleMode::Auto);
                        let view_start = window.viewport().view_start;
                        let bar_spacing = window.viewport().bar_spacing;
                        let _ = window;
                        let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                        if let Some(active_leaf) = active_leaf_opt {
                            self.propagate_viewport_to_sync_group(
                                active_leaf,
                                view_start,
                                bar_spacing,
                                Some(ScaleMode::Auto),
                            );
                        }
                    } else {
                        window.set_scale_mode(ScaleMode::Auto);
                        calc_visible_price_range(window, active_series_owned.as_active_series());
                    }
                }
                ChartOutputAction::ResetTimeScale => {
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    window.reset_time_scale();
                    window.set_scale_mode(ScaleMode::Auto);
                    calc_visible_price_range(window, active_series_owned.as_active_series());
                }
                ChartOutputAction::TogglePriceScaleMode => {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.toggle_price_scale_mode();
                    }
                }
                ChartOutputAction::UpdateCrosshair { .. } => {
                    // Crosshair updates are handled by update_crosshair_from_global
                    // (called in on_mouse_move / on_drag_move), which is
                    // sub-pane-aware and uses the correct coordinate space.
                    // Applying main-chart-only coords here would conflict with
                    // that logic and produce wrong crosshair positions in sub-panes.
                }
                ChartOutputAction::HideCrosshair => {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.crosshair.visible = false;
                    }
                }

                // ── Drawing interaction ─────────────────────────────────────
                ChartOutputAction::SelectPrimitive { id } => {
                    let active_bid_sp = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    if let Some(bid) = active_bid_sp {
                        let mut drawings_sp = self.core.drawings_mut();
                        match id {
                            Some(primitive_id) => {
                                drawings_sp.select(bid, primitive_id);
                            }
                            None => {
                                drawings_sp.deselect(bid);
                            }
                        }
                    }
                }
                ChartOutputAction::StartPrimitiveDrag { id, bar: screen_x, price: screen_y } => {
                    let active_bid_pd = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    // `bar` and `price` from DefaultChartInputHandler are raw screen (x, y).
                    // Look up the primitive's pane_id to choose the right coordinate system.
                    let primitive_pane_id = active_bid_pd
                        .and_then(|bid| self.core.drawings().pane_id_of(bid, id));

                    let (data_bar, data_price) = self.screen_to_data_coords(
                        screen_x, screen_y,
                        primitive_pane_id,
                        &extended,
                        chart_x, chart_y, chart_h,
                    );

                    // Apply magnet snap for main-pane primitives when magnet is active.
                    // The window magnet can land on a NEIGHBOURING bar — `data_bar`
                    // must travel with it, or the primitive's timestamp stays on the
                    // bar under the mouse while its price jumps to the snapped pivot
                    // (see `calculate_magnet_snap`'s own doc).
                    let (data_bar, data_price) = if primitive_pane_id.is_none() {
                        if let Some(w) = self.panel_app.panel_grid.active_container() {
                            if w.crosshair.is_magnet() {
                                let local_x = screen_x - chart_x;
                                let bar_idx = w.viewport().x_to_bar(local_x);
                                let (snapped_price, _, snapped_bar_idx) = w.calculate_magnet_snap(
                                    &active_bars, bar_idx, data_price, chart_h,
                                    w.price_scale().price_min, w.price_scale().price_max,
                                );
                                let snapped_bar = snapped_bar_idx.map(|idx| idx as f64).unwrap_or(data_bar);
                                (snapped_bar, snapped_price)
                            } else {
                                (data_bar, data_price)
                            }
                        } else {
                            (data_bar, data_price)
                        }
                    } else {
                        (data_bar, data_price)
                    };

                    // Capture the primitive's points BEFORE drag.
                    let pre_drag = active_bid_pd
                        .and_then(|bid| {
                            let guard = self.core.drawings();
                            let pts = guard.get_points(bid, id)?;
                            Some((id, pts))
                        });
                    self.drag_start_points = pre_drag;

                    if let Some(bid) = active_bid_pd {
                        let drag_ts_ms = mlc_core::bar_f64_to_timestamp_ms(&active_bars[..], data_bar);
                        self.core.drawings_mut().start_drag(bid, id, drag_ts_ms, data_price);
                    }
                }
                ChartOutputAction::StartControlPointDrag { primitive_id, control_point, bar: screen_x, price: screen_y } => {
                    let active_bid_cpd = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    // Look up the primitive's pane_id to choose the right coordinate system.
                    let primitive_pane_id = active_bid_cpd
                        .and_then(|bid| self.core.drawings().pane_id_of(bid, primitive_id));

                    let (data_bar, data_price) = self.screen_to_data_coords(
                        screen_x, screen_y,
                        primitive_pane_id,
                        &extended,
                        chart_x, chart_y, chart_h,
                    );

                    // Apply magnet snap for main-pane primitives when magnet is active.
                    // Call calculate_magnet_snap() directly, like the terminal does.
                    // The window magnet can land on a NEIGHBOURING bar — `data_bar`
                    // must travel with it (see `calculate_magnet_snap`'s own doc).
                    let (data_bar, data_price) = if primitive_pane_id.is_none() {
                        if let Some(w) = self.panel_app.panel_grid.active_container() {
                            if w.crosshair.is_magnet() {
                                let local_x = screen_x - chart_x;
                                let bar_idx = w.viewport().x_to_bar(local_x);
                                let (snapped_price, _, snapped_bar_idx) = w.calculate_magnet_snap(
                                    &active_bars, bar_idx, data_price, chart_h,
                                    w.price_scale().price_min, w.price_scale().price_max,
                                );
                                let snapped_bar = snapped_bar_idx.map(|idx| idx as f64).unwrap_or(data_bar);
                                (snapped_bar, snapped_price)
                            } else {
                                (data_bar, data_price)
                            }
                        } else {
                            (data_bar, data_price)
                        }
                    } else {
                        (data_bar, data_price)
                    };

                    // Capture points before control-point reshape too.
                    let pre_drag = active_bid_cpd
                        .and_then(|bid| {
                            let guard = self.core.drawings();
                            let pts = guard.get_points(bid, primitive_id)?;
                            Some((primitive_id, pts))
                        });
                    self.drag_start_points = pre_drag;

                    // Use start_control_point_drag (not start_drag) so DragType is ControlPoint.
                    if let Some(bid) = active_bid_cpd {
                        let cp_ts_ms = mlc_core::bar_f64_to_timestamp_ms(&active_bars[..], data_bar);
                        self.core.drawings_mut().start_control_point_drag(bid, primitive_id, control_point, cp_ts_ms, data_price);
                    }
                }
                ChartOutputAction::UpdatePrimitiveDrag { bar: screen_x, price: screen_y } => {
                    let active_bid_ud = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    // Use the dragging primitive's pane_id for coordinate conversion.
                    let primitive_pane_id = active_bid_ud
                        .and_then(|bid| {
                            let guard = self.core.drawings();
                            let dragging_id = guard.dragging(bid)?;
                            guard.pane_id_of(bid, dragging_id)
                        });

                    let (data_bar, data_price) = self.screen_to_data_coords(
                        screen_x, screen_y,
                        primitive_pane_id,
                        &extended,
                        chart_x, chart_y, chart_h,
                    );

                    // Apply magnet snap for main-pane primitives when magnet is active.
                    // Call calculate_magnet_snap() directly on every drag update, like the terminal does.
                    // The window magnet can land on a NEIGHBOURING bar — `data_bar`
                    // must travel with it (see `calculate_magnet_snap`'s own doc).
                    let (data_bar, data_price) = if primitive_pane_id.is_none() {
                        if let Some(w) = self.panel_app.panel_grid.active_container() {
                            if w.crosshair.is_magnet() {
                                let local_x = screen_x - chart_x;
                                let bar_idx = w.viewport().x_to_bar(local_x);
                                let (snapped_price, _, snapped_bar_idx) = w.calculate_magnet_snap(
                                    &active_bars, bar_idx, data_price, chart_h,
                                    w.price_scale().price_min, w.price_scale().price_max,
                                );
                                let snapped_bar = snapped_bar_idx.map(|idx| idx as f64).unwrap_or(data_bar);
                                (snapped_bar, snapped_price)
                            } else {
                                (data_bar, data_price)
                            }
                        } else {
                            (data_bar, data_price)
                        }
                    } else {
                        (data_bar, data_price)
                    };

                    // Get the dragged primitive id BEFORE update_drag (id doesn't change).
                    let dragged_id = active_bid_ud
                        .and_then(|bid| self.core.drawings().dragging(bid));

                    if let Some(bid) = active_bid_ud {
                        let drag_ts_ms = mlc_core::bar_f64_to_timestamp_ms(&active_bars[..], data_bar);
                        // Resolve local (pane-relative) screen coords plus the
                        // viewport/price_scale scoped to the dragged primitive's
                        // pane, mirroring handle_drag_start's own control-point
                        // hit-test scoping (panel_grid.rs) — move_control_point_screen
                        // overrides (e.g. RotatedRectangle) do real pixel-space
                        // geometry and need the SAME pixel space screen_layout()/
                        // control_points() use, not the raw window-absolute coords
                        // screen_to_data_coords() takes above.
                        let sub_pane_scope = primitive_pane_id.and_then(|instance_id| {
                            extended.sub_panes.iter()
                                .find(|p| p.instance_id == instance_id)
                                .map(|p| (instance_id, p.content))
                        });
                        if let Some(window) = self.panel_app.panel_grid.active_container() {
                            let (local_x, local_y, drag_viewport, drag_price_scale) = match sub_pane_scope {
                                Some((instance_id, content)) => {
                                    let (price_min, price_max) = window.sub_pane_y_state
                                        .get(&instance_id)
                                        .map(|s| (s.price_min, s.price_max))
                                        .unwrap_or((0.0, 100.0));
                                    let mut vp = window.viewport().clone();
                                    vp.chart_height = content.height;
                                    (
                                        screen_x - content.x, screen_y - content.y,
                                        vp, mlc_core::PriceScale::new(price_min, price_max),
                                    )
                                }
                                None => {
                                    let mut vp = window.viewport().clone();
                                    vp.chart_width = chart_w;
                                    vp.chart_height = chart_h;
                                    (screen_x - chart_x, screen_y - chart_y, vp, window.price_scale().clone())
                                }
                            };
                            self.core.drawings_mut().update_drag_screen(
                                bid, local_x, local_y, drag_ts_ms, data_price,
                                &active_bars, &drag_viewport, &drag_price_scale,
                            );
                        }
                    }
                    // Propagate live drag position to sync-group peer leaves.
                    if let Some(prim_id) = dragged_id {
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            // TagManager path: update group in real-time during drag.
                            let group_updated = self.update_group_primitive_after_drag(active_leaf, prim_id);
                            if !group_updated {
                                self.propagate_primitive_update_to_sync_group(active_leaf, prim_id);
                            }
                        }
                    }
                }
                ChartOutputAction::EndPrimitiveDrag => {
                    let active_bid_ed = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    // Snapshot the dragged primitive's id BEFORE end_drag() clears it.
                    let dragged_prim_id = active_bid_ed
                        .and_then(|bid| self.core.drawings().dragging(bid));

                    // Collect move data BEFORE ending the drag.
                    let move_cmd = if let Some((prim_id, ref previous_points)) = self.drag_start_points {
                        active_bid_ed
                            .and_then(|bid| self.core.drawings().get_points(bid, prim_id))
                            .and_then(|new_points| {
                                if new_points != *previous_points {
                                    Some(Command::MovePrimitive {
                                        prim_id,
                                        previous_points: previous_points.clone(),
                                        new_points,
                                    })
                                } else {
                                    None
                                }
                            })
                    } else {
                        None
                    };
                    self.drag_start_points = None;

                    if let Some(bid) = active_bid_ed {
                        self.core.drawings_mut().end_drag(bid);
                    }
                    if let Some(cmd) = move_cmd {
                        self.push_undo_command(cmd);
                    }
                    // Propagate final primitive position to sync-group peer leaves.
                    if let Some(prim_id) = dragged_prim_id {
                        if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
                            // TagManager path: write updated primitive back to the group.
                            let group_updated = self.update_group_primitive_after_drag(active_leaf, prim_id);
                            if !group_updated {
                                // Legacy clone-based sync fallback.
                                self.propagate_primitive_update_to_sync_group(active_leaf, prim_id);
                            }
                        }
                        self.autosave_snapshot();
                    }
                }
                ChartOutputAction::FinishMultipointDrawing => {
                    let active_bid_fm = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    // Record CreatePrimitive if finish_multipoint creates a new primitive.
                    let prev_count = active_bid_fm
                        .map(|bid| self.core.drawings().count(bid))
                        .unwrap_or(0);

                    if let Some(bid) = active_bid_fm {
                        self.core.drawings_mut().finish_multipoint(bid);
                    }

                    let new_count = active_bid_fm
                        .map(|bid| self.core.drawings().count(bid))
                        .unwrap_or(0);

                    if new_count > prev_count {
                        let cmd = active_bid_fm.and_then(|bid| {
                            let prim_id = self.core.drawings().last_index(bid)?;
                            self.build_creation_command(bid, prim_id)
                        });
                        if let Some(cmd) = cmd {
                            self.push_undo_command(cmd);
                            self.autosave_snapshot();
                        }
                    }
                }
                ChartOutputAction::DrawingClick { bar, price, .. } => {
                    let active_bid_dc = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
                    // Record CreatePrimitive if the click completes a new primitive.
                    let prev_count = active_bid_dc
                        .map(|bid| self.core.drawings().count(bid))
                        .unwrap_or(0);

                    let click_ts_ms = self.panel_app.panel_grid.active_container()
                        .map(|_w| mlc_core::bar_f64_to_timestamp_ms(&active_bars[..], bar))
                        .unwrap_or(0);
                    if let Some(bid) = active_bid_dc {
                        self.core.drawings_mut().on_canvas_click(bid, click_ts_ms, price);
                    }

                    let new_count = active_bid_dc
                        .map(|bid| self.core.drawings().count(bid))
                        .unwrap_or(0);

                    if new_count > prev_count {
                        let cmd = active_bid_dc.and_then(|bid| {
                            let prim_id = self.core.drawings().last_index(bid)?;
                            self.build_creation_command(bid, prim_id)
                        });
                        if let Some(cmd) = cmd {
                            self.push_undo_command(cmd);
                            self.autosave_snapshot();
                        }
                    }
                }

                ChartOutputAction::ZoomSubPane { pane_index, delta_y } => {
                    let Some(window) = self.panel_app.panel_grid.active_container_mut() else { continue; };
                    {
                        let instance_id = crate::chart_container::collect_inner_instance_ids(
                            &window.inner_docking,
                        ).into_iter().nth(pane_index);
                        let pane_h = self.active_frame_layout.as_ref()
                            .and_then(|ext| ext.sub_panes.get(pane_index))
                            .map(|sp| sp.content.height)
                            .unwrap_or(100.0);
                        if let Some(iid) = instance_id {
                            let entry = window.sub_pane_y_state.entry(iid).or_default();
                            entry.auto_scale = false;
                            if pane_h > 0.0 {
                                let factor = 1.0 + delta_y / pane_h;
                                let center = (entry.price_min + entry.price_max) / 2.0;
                                let half_range = (entry.price_max - entry.price_min) / 2.0 * factor;
                                entry.price_min = center - half_range;
                                entry.price_max = center + half_range;
                            }
                        }
                    }
                    // Dragging a sub-pane price scale switches the whole window to
                    // Manual mode so the A/M button reflects the true state.
                    window.set_scale_mode(ScaleMode::Manual);
                    let view_start = window.viewport().view_start;
                    let bar_spacing = window.viewport().bar_spacing;
                    let _ = window;
                    let active_leaf_opt = self.panel_app.panel_grid.docking().active_leaf();
                    if let Some(active_leaf) = active_leaf_opt {
                        self.propagate_viewport_to_sync_group(
                            active_leaf,
                            view_start,
                            bar_spacing,
                            Some(ScaleMode::Manual),
                        );
                    }
                }

                // ── Tooltip / cursor / kinetic — not applicable in chart-app ──
                ChartOutputAction::ShowTooltip { .. }
                | ChartOutputAction::HideTooltip
                | ChartOutputAction::UpdateCursor(_)
                | ChartOutputAction::OpenContextMenu { .. }
                | ChartOutputAction::StartKinetic { .. }
                | ChartOutputAction::StopKinetic
                | ChartOutputAction::Repaint
                | ChartOutputAction::None => {
                    // These are either handled elsewhere or not applicable in the
                    // standalone chart-app context.
                }
            }
        }
    }

    /// Propagate viewport state to all leaves in the same sync color group.
    ///
    /// Propagate updated primitive points to sync-group peer leaves.
    ///
    /// Called after a drag move (`UpdatePrimitiveDrag`) and after drag end
    /// After a drag ends on a grouped window, replace the corresponding
    /// primitive in the TagManager group with the updated version from the
    /// window's drawing_manager.  Returns `true` if the window was grouped
    /// and the group was updated.
    pub(crate) fn update_group_primitive_after_drag(
        &mut self,
        source_leaf: mlc_core::LeafId,
        primitive_id: u64,
    ) -> bool {
        let group_id = self.panel_app.panel_grid
            .container_id_for_leaf(source_leaf)
            .and_then(|cid| self.bubble_for_chart(cid));
        let group_id = match group_id {
            Some(gid) => gid,
            None => return false,
        };

        // Respect the sync_drawings flag — if disabled, do not write drag result back to group.
        if !self.core.bubbles().sync_flags(group_id).map(|sf| sf.sync_drawings).unwrap_or(false) {
            return false;
        }

        // Clone the updated primitive from the window's drawing_manager.
        let chart_id = match self.panel_app.panel_grid.container_id_for_leaf(source_leaf) {
            Some(id) => id,
            None => return false,
        };
        let updated_prim = self.panel_app.panel_grid
            .containers()
            .get(&chart_id)
            .and_then(|w| {
                let guard = self.core.drawings();
                guard.primitives(w.current_tag_id).iter()
                    .find(|p| p.data().id == primitive_id)
                    .map(|p| p.clone_box())
            });
        let updated_prim = match updated_prim {
            Some(p) => p,
            None => return false,
        };

        // Replace the primitive in the group by id (round-trip through the
        // snapshot JSON shape — `replace_primitive_from_json` is the
        // Drawing facade's replace-by-id intent method).
        let snap = mlc_core::drawing::PrimitiveSnapshot::from_primitive(updated_prim.as_ref());
        self.core.drawings_mut().replace_primitive_from_json(group_id, primitive_id, &snap.type_id, &snap.json)
    }

    /// DEPRECATED — no-op under the per-bubble drawing model (Step 3.1).
    ///
    /// Previously cloned a dragged primitive's updated points into every
    /// peer leaf's separate `DrawingManager` copy. Peer leaves sharing a
    /// bubble now read the same `DataBubble.primitives` Vec directly — there
    /// is nothing left to propagate. Kept as a stub because
    /// `update_group_primitive_after_drag`'s call sites still reference it
    /// as a fallback path; safe to delete once those call sites are swept.
    pub(crate) fn propagate_primitive_update_to_sync_group(
        &mut self,
        _source_leaf: mlc_core::LeafId,
        _primitive_id: u64,
    ) {
    }

    /// `source_leaf` is the leaf whose viewport was just changed.
    /// All other leaves sharing the same color tag receive the same
    /// visible bar count, time-aligned `view_start`, and optionally `scale_mode`.
    ///
    /// Instead of copying raw `bar_spacing`, we sync the NUMBER of visible bars.
    /// Each peer recalculates its own `bar_spacing = peer.chart_width / visible_bars`
    /// so that a small 85px window and a large 271px window both show the same
    /// number of candles (just at different pixel densities).
    pub(crate) fn propagate_viewport_to_sync_group(
        &mut self,
        source_leaf: mlc_core::LeafId,
        view_start: f64,
        bar_spacing: f64,
        scale_mode: Option<mlc_core::ScaleMode>,
    ) {
        let should_sync = self.panel_app.panel_grid.container_id_for_leaf(source_leaf)
            .and_then(|cid| self.bubble_for_chart(cid))
            .and_then(|gid| self.with_bubble(gid, |g| g.sync_flags.sync_viewport))
            .unwrap_or(true);
        if !should_sync { return; }

        // Compute source visible bar count — the metric we actually sync.
        let source_bars: Vec<mlc_core::Bar> = self.bars_for_leaf(source_leaf);
        let (source_chart_width, anchor_ts, frac) = match self.panel_app.panel_grid.container_for_leaf(source_leaf) {
            Some(w) => {
                let ts = if source_bars.is_empty() {
                    None
                } else {
                    let last_idx = source_bars.len() - 1;
                    let fidx = view_start.floor();
                    if fidx > last_idx as f64 {
                        // Dragged past the last bar: extrapolate the anchor
                        // timestamp by the bar interval so sync peers follow
                        // into the future (find_bar_for_timestamp extrapolates
                        // right) instead of pinning at their last bar.
                        let interval = mlc_core::bar_interval_seconds(&source_bars[..]);
                        Some(source_bars[last_idx].timestamp
                            + (fidx as i64 - last_idx as i64) * interval)
                    } else {
                        let idx = (fidx.max(0.0) as usize).min(last_idx);
                        Some(source_bars[idx].timestamp)
                    }
                };
                (w.viewport().chart_width, ts, view_start - view_start.floor())
            }
            None => return,
        };
        let visible_bars = if bar_spacing > 0.0 { source_chart_width / bar_spacing } else { 100.0 };

        // Enumerate peers via TagManager group membership so untagged groups propagate too.
        let source_chart_id_vp = self.panel_app.panel_grid.container_id_for_leaf(source_leaf);
        let group_id_vp = source_chart_id_vp
            .and_then(|cid| self.bubble_for_chart(cid));
        let sync_leaves: Vec<mlc_core::LeafId> = if let Some(gid) = group_id_vp {
            self.core.tags()
                .chart_member_ids(gid)
                .into_iter()
                .filter_map(|cid| self.panel_app.panel_grid.leaf_for_container_id(cid))
                .filter(|&lid| lid != source_leaf)
                .collect()
        } else {
            match self.panel_app.leaf_color_tags.get(&source_leaf).copied() {
                Some(source_color) => self.panel_app.leaf_color_tags.iter()
                    .filter(|(&lid, &c)| lid != source_leaf && input::sync_colors_match(c, source_color))
                    .map(|(&lid, _)| lid)
                    .collect(),
                None => return,
            }
        };
        // Audit D8 (chart-types-xaxis-audit §3(b)): peers resolve THEIR OWN
        // active series for ts→index mapping — `bars_for_leaf` always
        // resolved `bubble.bars`, so a kline→renko sync computed a
        // kline-correct anchor and applied it to a viewport that indexes a
        // renko array ("well-formed math, meaningless target"). Snapshot
        // per-peer owned series up front (immutable borrows), then mutate.
        let peer_series: std::collections::HashMap<mlc_core::LeafId, crate::active_series::OwnedActiveSeries> =
            sync_leaves.iter()
                .map(|&lid| {
                    let owned = self.panel_app.panel_grid.container_id_for_leaf(lid)
                        .map(|cid| self.active_series_for_chart(cid))
                        .unwrap_or_default();
                    (lid, owned)
                })
                .collect();
        for leaf_id in sync_leaves {
            let this_peer_series = peer_series.get(&leaf_id)
                .map(|s| s.as_active_series())
                .unwrap_or(crate::active_series::ActiveSeries::Bars(&[]));
            if let Some(window) = self.panel_app.panel_grid.container_for_leaf_mut(leaf_id) {
                // Time-based sync: convert source timestamp to peer's own
                // active-series index so windows with different bar counts
                // (or entirely different array kinds) land on the same
                // calendar date / emission position.
                let peer_view_start = anchor_ts
                    .and_then(|ts| this_peer_series.idx_for_ts(ts * 1000))
                    .map(|idx| idx as f64 + frac)
                    .unwrap_or(view_start);
                // Sync visible bar count: peer gets the same number of bars in view,
                // but its own bar_spacing adapts to its chart_width.
                let peer_spacing = if visible_bars > 0.0 && window.viewport().chart_width > 0.0 {
                    (window.viewport().chart_width / visible_bars).clamp(
                        window.viewport().min_bar_spacing(),
                        window.viewport().max_bar_spacing(),
                    )
                } else {
                    bar_spacing
                };
                // set_from_sync only writes + rescales when the peer's viewport
                // actually moves. An unconditional per-frame write made
                // calc_auto_scale here race the per-tick TradeUpdate autoscale
                // on the same peer: the two paths compute slightly different
                // price ranges, the last-price projection lands alternately
                // inside/outside the range, and the line + scale chip blink
                // (owner report: right window of a 2-chart sync group ripples).
                window.set_from_sync(peer_view_start, peer_spacing, scale_mode, this_peer_series);
            }
        }
    }

    // -------------------------------------------------------------------------
    // User profile persistence
    // -------------------------------------------------------------------------

    /// Map a [`mlc_app_sidebar_content::state::RightSidebarPanel`] to a string name.
    /// Stamp the right-rail button id of the currently open sidebar panel
    /// into `toolbar_state` so the rail renders it accent-active (owner
    /// order 2026-07-31). NOTE: Monitoring's BUTTON id is "monitoring" —
    /// distinct from its persisted key "performance" (`panel_to_str`
    /// back-compat), so this cannot reuse `panel_to_str` directly.
    fn stamp_active_right_toolbar_id(&mut self) {
        use mlc_app_sidebar_content::state::RightSidebarPanel as P;
        self.panel_app.toolbar_state.active_right_toolbar_id =
            match self.sidebar_state.right_panel {
                P::None => None,
                P::Monitoring => Some("monitoring".to_string()),
                p => Self::panel_to_str(p),
            };
        // System event log (owner scope §8, 2026-09-05): the toolbar badge
        // flag and the panel's own row data are both cheap (ring-capped at
        // 200 events) and stamped unconditionally per frame here, same as
        // `active_right_toolbar_id` above — unlike `alert_items` et al. this
        // does NOT wait for `sidebar_data_dirty` so an event reported from a
        // background path (sync/tier/update) shows up without an unrelated
        // click first.
        self.panel_app.toolbar_state.system_events_unread = self.system_events.unread_count() > 0;
        self.sidebar_state.system_event_items = self.system_events.iter_newest_first().cloned().collect();
        // Community (Forum + Messages) envelope button's own unread dot
        // (community-modal-and-forum-ladder arc, piece 2) — same
        // unconditional per-frame stamp as `system_events_unread` above,
        // cheap (a bounded conversations vec), gated on `cloud` since
        // `community_state`/`DmState` only exist in cloud builds. Also
        // gated on `messages_tab_visible` (owner ruling 2026-09-09: Messages
        // is `paid`-and-above only) — a caller without the tab must never
        // see a badge pointing at content they cannot reach.
        #[cfg(feature = "cloud")]
        {
            self.panel_app.toolbar_state.community_unread = self.panel_app.community_state.messages_tab_visible()
                && self
                    .panel_app
                    .community_state
                    .dm
                    .conversations
                    .iter()
                    .any(|c| c.unread_count > 0);
        }
        self.stamp_settings_menu_state();
    }

    fn stamp_settings_menu_state(&mut self) {
        use mlc_core::chart::types::crosshair::CrosshairMode;
        use mlc_core::{HorzAlign, VertAlign};
        let mut toggles = std::collections::HashMap::new();
        let mut draft = None;
        let pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
        if let Some(pid) = pid {
            if let Some(panel) = self.panels_store.chart_panel.get(&pid) {
                let gv = panel.grid_options.vert_lines.visible;
                let gh = panel.grid_options.horz_lines.visible;
                toggles.insert("grid_toggle".into(), gv && gh);
                toggles.insert("grid_vert".into(), gv);
                toggles.insert("grid_horz".into(), gh);
                toggles.insert("legend_toggle".into(), panel.legend.visible);
                toggles.insert("tooltip_toggle".into(), panel.tooltip.visible);
                toggles.insert("tooltip_follow".into(), panel.tooltip.follow_cursor);
                let wm_on = panel.watermark.as_ref().map(|w| w.visible).unwrap_or(false);
                toggles.insert("watermark_toggle".into(), wm_on);
                if let Some(wm) = panel.watermark.as_ref() {
                    toggles.insert("watermark_pos_center".into(),
                        wm.horz_align == HorzAlign::Center && wm.vert_align == VertAlign::Center);
                    toggles.insert("watermark_pos_bl".into(),
                        wm.horz_align == HorzAlign::Left && wm.vert_align == VertAlign::Bottom);
                    toggles.insert("watermark_pos_br".into(),
                        wm.horz_align == HorzAlign::Right && wm.vert_align == VertAlign::Bottom);
                    draft = Some(wm.lines.first().map(|l| l.text.clone()).unwrap_or_default());
                }
            }
        }
        if let Some(w) = self.panel_app.panel_grid.active_container() {
            toggles.insert("crosshair_toggle".into(), w.crosshair.enabled);
            toggles.insert("ch_normal".into(), matches!(w.crosshair.mode, CrosshairMode::Normal));
            toggles.insert("ch_magnet".into(), matches!(w.crosshair.mode, CrosshairMode::Magnet));
            toggles.insert("ch_magnet_ohlc".into(), matches!(w.crosshair.mode, CrosshairMode::MagnetOHLC));
        }
        let theme = self.panel_app.theme_manager.preset_name().to_string();
        for name in ["dark", "light", "high_contrast", "high_contrast_mono", "mascot", "surf", "vaporwave", "terminal"] {
            toggles.insert(format!("theme_{name}"), theme == name);
        }
        let style = self.panel_app.theme_manager.current().style;
        toggles.insert("style_solid".into(), matches!(style, mlc_core::UIStyle::Solid));
        toggles.insert("style_glass".into(), matches!(style, mlc_core::UIStyle::Glass));
        toggles.insert("style_frosted_glass_flat".into(), matches!(style, mlc_core::UIStyle::FrostedGlassFlat));

        let wid = uzor::WidgetId::from(crate::text_input::WATERMARK_TEXT);
        let (focused, live_text, cursor, cursor_vis) = {
            let ic = self.input_coordinator.borrow();
            let tf = ic.text_fields();
            if tf.focused().map(|f| f.as_str()) == Some(crate::text_input::WATERMARK_TEXT) {
                let t = web_time::SystemTime::now().duration_since(web_time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
                (true, tf.text(&wid).to_string(), tf.cursor(&wid), tf.cursor_visible(t))
            } else {
                (false, String::new(), 0, false)
            }
        };
        let tb = &mut self.panel_app.toolbar_state;
        tb.settings_toggles = toggles;
        if focused {
            tb.watermark_edit_focused = true;
            tb.watermark_draft = live_text;
            tb.watermark_edit_cursor = cursor;
            tb.watermark_edit_cursor_visible = cursor_vis;
        } else {
            tb.watermark_edit_focused = false;
            if let Some(d) = draft {
                tb.watermark_draft = d;
            }
        }
    }

    pub fn panel_to_str(panel: mlc_app_sidebar_content::state::RightSidebarPanel) -> Option<String> {
        use mlc_app_sidebar_content::state::RightSidebarPanel;
        match panel {
            RightSidebarPanel::None => None,
            RightSidebarPanel::Watchlist => Some("watchlist".to_string()),
            RightSidebarPanel::Alerts => Some("alerts".to_string()),
            RightSidebarPanel::ObjectTree => Some("object_tree".to_string()),
            RightSidebarPanel::Signals => Some("signals".to_string()),
            // Wire/persisted key stays "performance" (pre-merge label) so
            // profiles saved before the Connectors+Performance merge
            // (2026-07-18) keep opening the same panel — see `str_to_panel`
            // below for the read-side back-compat mapping (old "connectors"
            // key also resolves to Monitoring).
            RightSidebarPanel::Monitoring => Some("performance".to_string()),
            RightSidebarPanel::Agents => Some("agents".to_string()),
            RightSidebarPanel::SlotTrading => Some("slot_trading".to_string()),
            RightSidebarPanel::SlotDom => Some("slot_dom".to_string()),
            RightSidebarPanel::SlotTape => Some("slot_tape".to_string()),
            RightSidebarPanel::SlotL2 => Some("slot_l2".to_string()),
            RightSidebarPanel::SlotBigTrades => Some("slot_big_trades".to_string()),
            RightSidebarPanel::SlotLiq => Some("slot_liq".to_string()),
            RightSidebarPanel::Screener => Some("screener".to_string()),
            RightSidebarPanel::Events => Some("events".to_string()),
        }
    }

    /// Parse a string name into a [`mlc_app_sidebar_content::state::RightSidebarPanel`].
    ///
    /// Accepts both the current typed-slot keys AND the legacy generic
    /// `slot1`..`slot4` keys written by pre-typed-slot profiles (owner
    /// redesign 2026-07-11, hard cutover — no new writes of legacy keys,
    /// but old saved profiles must still hydrate). Legacy mapping:
    /// `slot1`→Trading, `slot2`→Dom, `slot3`→Tape, `slot4`→L2. There is no
    /// legacy equivalent for the new `SlotBigTrades` slot.
    pub fn str_to_panel(s: &str) -> mlc_app_sidebar_content::state::RightSidebarPanel {
        use mlc_app_sidebar_content::state::RightSidebarPanel;
        match s {
            "watchlist" => RightSidebarPanel::Watchlist,
            "alerts" => RightSidebarPanel::Alerts,
            "object_tree" => RightSidebarPanel::ObjectTree,
            "signals" => RightSidebarPanel::Signals,
            // Both pre-merge keys ("connectors" and "performance") resolve to
            // the merged Monitoring panel — old saved profiles must still
            // hydrate (hard cutover, no dropped sessions per merge task spec).
            "connectors" | "performance" => RightSidebarPanel::Monitoring,
            "agents" => RightSidebarPanel::Agents,
            "slot_trading" | "slot1" => RightSidebarPanel::SlotTrading,
            "slot_dom" | "slot2" => RightSidebarPanel::SlotDom,
            "slot_tape" | "slot3" => RightSidebarPanel::SlotTape,
            "slot_l2" | "slot4" => RightSidebarPanel::SlotL2,
            "slot_big_trades" => RightSidebarPanel::SlotBigTrades,
            "slot_liq" => RightSidebarPanel::SlotLiq,
            "screener" => RightSidebarPanel::Screener,
            "events" => RightSidebarPanel::Events,
            _ => RightSidebarPanel::None,
        }
    }

    /// Build indicator display name with ALL numeric params in definition order.
    ///
    /// Examples: "SMA(20)", "RSI(14)", "MACD(26, 12, 9)", "BB(20, 2)"
    fn format_indicator_display_name(
        mgr: &mlc_backend::Indicators<'_>,
        inst: &mlc_backend_indicator_engine::IndicatorMgrInstance,
    ) -> String {
        use mlc_backend_indicator_engine::{IndicatorParamType, IndicatorParamValue};

        if let Some(def) = mgr.definition(&inst.type_id) {
            // Collect numeric param values in definition order
            let numeric_vals: Vec<String> = def.params.iter()
                .filter(|p| matches!(p.param_type, IndicatorParamType::Int { .. } | IndicatorParamType::Float { .. }))
                .filter_map(|p| {
                    inst.params.get(&p.name).map(|v| match v {
                        IndicatorParamValue::Int(i) => i.to_string(),
                        IndicatorParamValue::Float(f) => {
                            if f.fract() == 0.0 { format!("{:.0}", f) } else { format!("{}", f) }
                        }
                        _ => String::new(),
                    })
                })
                .filter(|s| !s.is_empty())
                .collect();

            if numeric_vals.is_empty() {
                def.short_name.clone()
            } else {
                format!("{}({})", def.short_name, numeric_vals.join(", "))
            }
        } else {
            inst.name.clone()
        }
    }

    /// Build a [`ChartSettingsData`] snapshot from the current chart state.
    ///
    /// This is the same logic used during rendering so that templates saved via
    /// the "Save As…" button capture exactly what the render pass would show.
    pub fn build_chart_settings_data(&self) -> mlc_app_layout::layout::modals::chart_settings::ChartSettingsData {
        use mlc_app_layout::layout::modals::chart_settings::{
            ChartSettingsData, InstrumentSettings, StatusLineSettings, ScalesLinesSettings,
        };

        let rt = self.panel_app.theme_manager.current();
        let series = &rt.series;

        // Phase 4 C: collect window-only fields first, then panel fields separately.
        let settings_pid = self.panel_app.panel_grid.active_container().map(|w| w.chart_panel_id);
        let (auto_scale,
             price_scale_width, time_scale_height,
             time_fmt_use_24h, time_fmt_show_dow, time_fmt_show_utc, tz_label, date_fmt_label, precision_lbl) =
            self.panel_app
            .panel_grid
            .active_container()
            .map(|w| {
                let tf = &w.scale_settings.time_format;
                (
                    w.price_scale().scale_mode.is_auto_y(),
                    w.scale_settings.price_scale_width,
                    w.scale_settings.time_scale_height,
                    tf.use_24h,
                    tf.show_day_of_week,
                    tf.show_utc_prefix,
                    tf.timezone_label(),
                    tf.date_format.label().to_string(),
                    mlc_core::scale_settings::precision_label(
                        w.scale_settings.user_precision,
                    ).to_string(),
                )
            })
            .unwrap_or_else(|| (
                true, 70.0, 30.0,
                true, false, true,
                mlc_core::TimeFormatSettings::default().timezone_label(),
                "21.01.2026".to_string(),
                mlc_core::scale_settings::precision_label(None).to_string(),
            ));
        // Panel-owned display fields.
        let settings_default_panel = settings_pid.map(crate::chart_panel_state::ChartPanelState::new);
        let settings_panel = settings_pid
            .and_then(|pid| self.panels_store.chart_panel.get(&pid))
            .or(settings_default_panel.as_ref());
        let (vert_lines, horz_lines, legend_show_ohlc, legend_show_change, legend_show_percent,
             tooltip_visible, tooltip_follow_cursor) = settings_panel
            .map(|p| (
                p.grid_options.vert_lines.visible,
                p.grid_options.horz_lines.visible,
                p.legend.show_ohlc,
                p.legend.show_change,
                p.legend.show_percent,
                p.tooltip.visible,
                p.tooltip.follow_cursor,
            ))
            .unwrap_or((true, true, true, true, true, false, false));

        let css = &self.panel_app.chart_settings_state;
        let settings_cid = self.panel_app.panel_grid.active_container_id();
        let active_chart_type = settings_cid
            .and_then(|cid| self.chart_type_for_chart(cid))
            .unwrap_or("candles")
            .to_string();
        let active_chart_type_param = settings_cid
            .and_then(|cid| self.chart_type_param_for_chart(cid))
            .unwrap_or(0.0);
        let active_chart_type_interval = settings_cid
            .and_then(|cid| self.chart_type_interval_for_chart(cid))
            .unwrap_or("1m")
            .to_string();

        ChartSettingsData {
            instrument: InstrumentSettings {
                chart_type: active_chart_type,
                chart_type_param: active_chart_type_param,
                chart_type_interval: active_chart_type_interval,
                use_prev_close_color: css.instrument_use_prev_close,
                body_enabled:   css.instrument_body_enabled,
                body_up_color:  series.candle_up_body.clone(),
                body_down_color: series.candle_down_body.clone(),
                border_enabled: css.instrument_border_enabled,
                border_up_color: series.candle_up_body.clone(),
                border_down_color: series.candle_down_body.clone(),
                wick_enabled:   css.instrument_wick_enabled,
                wick_up_color:  series.candle_up_wick.clone(),
                wick_down_color: series.candle_down_wick.clone(),
                candle_style_label: match rt.panel_style.candle_style {
                    mlc_core::theme::CandleStyle::Solid => "Solid",
                    mlc_core::theme::CandleStyle::ThinGlow => "Thin Glow",
                    mlc_core::theme::CandleStyle::Phosphor => "Phosphor",
                }
                .to_string(),
                footprint_show_abs_markers: css.footprint_show_abs_markers,
                footprint_show_volume_text: css.footprint_show_volume_text,
                footprint_buy_color: series.footprint_buy.clone(),
                footprint_sell_color: series.footprint_sell.clone(),
                renko_up_color: series.renko_up.clone(),
                renko_down_color: series.renko_down.clone(),
                pnf_x_color: series.pnf_x.clone(),
                pnf_o_color: series.pnf_o.clone(),
                tlb_up_color: series.tlb_up.clone(),
                tlb_down_color: series.tlb_down.clone(),
                kagi_yang_color: series.kagi_yang.clone(),
                kagi_yin_color: series.kagi_yin.clone(),
                hollow_up_color: series.hollow_up.clone(),
                hollow_down_color: series.hollow_down.clone(),
                line_color: series.line_color.clone(),
                line_width_label: settings_panel
                    .map(|p| p.line_family_display.line_width)
                    .unwrap_or(2.0)
                    .to_string(),
                step_line_color: series.step_line_color.clone(),
                step_line_width_label: settings_panel
                    .map(|p| p.line_family_display.step_line_width)
                    .unwrap_or(2.0)
                    .to_string(),
                area_line_color: series.area_line.clone(),
                area_top_color: series.area_top.clone(),
                area_bottom_color: series.area_bottom.clone(),
                area_line_width_label: settings_panel
                    .map(|p| p.line_family_display.area_line_width)
                    .unwrap_or(2.0)
                    .to_string(),
                baseline_top_line_color: series.baseline_top_line.clone(),
                baseline_top_fill_color: series.baseline_top_fill.clone(),
                baseline_bottom_line_color: series.baseline_bottom_line.clone(),
                baseline_bottom_fill_color: series.baseline_bottom_fill.clone(),
                baseline_line_width_label: settings_panel
                    .map(|p| p.line_family_display.baseline_line_width)
                    .unwrap_or(2.0)
                    .to_string(),
                bars_up_color: series.bar_up.clone(),
                bars_down_color: series.bar_down.clone(),
                line_markers_color: series.line_markers_color.clone(),
                line_markers_style_label: match settings_panel
                    .map(|p| p.line_family_display.marker_style)
                    .unwrap_or_default()
                {
                    mlc_core::chart::LineMarkerStyle::Circle => "Circle".to_string(),
                    mlc_core::chart::LineMarkerStyle::Square => "Square".to_string(),
                    mlc_core::chart::LineMarkerStyle::Triangle => "Triangle".to_string(),
                    mlc_core::chart::LineMarkerStyle::Diamond => "Diamond".to_string(),
                },
                line_markers_size_label: settings_panel
                    .map(|p| p.line_family_display.marker_size)
                    .unwrap_or(3.0)
                    .to_string(),
                footprint_heat_intensity_label: match settings_panel
                    .map(|p| p.footprint_display.heat_intensity)
                    .unwrap_or_default()
                {
                    mlc_core::chart::FootprintHeatIntensity::Low => "Low".to_string(),
                    mlc_core::chart::FootprintHeatIntensity::Normal => "Normal".to_string(),
                    mlc_core::chart::FootprintHeatIntensity::High => "High".to_string(),
                },
                footprint_row_grouping_label: match settings_panel
                    .map(|p| p.footprint_display.group_ticks)
                    .unwrap_or(0)
                {
                    0 => "Raw".to_string(),
                    n => format!("{n} ticks"),
                },
                footprint_imbalance_threshold_label: {
                    let ratio = settings_panel
                        .map(|p| p.footprint_display.imbalance_threshold)
                        .unwrap_or(1.5);
                    if ratio.fract() == 0.0 {
                        format!("{}x", ratio as i64)
                    } else {
                        format!("{ratio}x")
                    }
                },
                footprint_cell_mode_label: settings_panel
                    .map(|p| p.footprint_display.cell_mode)
                    .unwrap_or_default()
                    .label()
                    .to_string(),
                tick_intensity: css.tick_intensity,
                bar_family_wicks: css.bar_family_wicks,
                tpo_show_session_dates: settings_panel
                    .map(|p| p.tpo_display.show_session_dates)
                    .unwrap_or(false),
                tpo_show_poc: settings_panel
                    .map(|p| p.tpo_display.show_poc)
                    .unwrap_or(true),
                tpo_show_value_area: settings_panel
                    .map(|p| p.tpo_display.show_value_area)
                    .unwrap_or(true),
                tpo_show_single_prints: settings_panel
                    .map(|p| p.tpo_display.show_single_prints)
                    .unwrap_or(true),
                tpo_show_poor_hl: settings_panel
                    .map(|p| p.tpo_display.show_poor_hl)
                    .unwrap_or(true),
                tpo_show_initial_balance: settings_panel
                    .map(|p| p.tpo_display.show_initial_balance)
                    .unwrap_or(true),
                tpo_ib_periods_label: settings_panel
                    .map(|p| p.tpo_display.ib_periods)
                    .unwrap_or(2)
                    .to_string(),
                precision_label: precision_lbl.clone(),
                timezone_label: tz_label.clone(),
                use_24h: time_fmt_use_24h,
                show_utc_prefix: time_fmt_show_utc,
                date_format_label: date_fmt_label.clone(),
                show_day_of_week: time_fmt_show_dow,
                show_bar_countdown: self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.scale_settings.show_bar_countdown)
                    .unwrap_or(true),
                price_tick_style: self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.scale_settings.price_tick_style.clone())
                    .unwrap_or_else(|| "dotted".to_string()),
                price_tick_extend_right: self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.scale_settings.price_tick_extend_right)
                    .unwrap_or(true),
                price_tick_extend_left: self.panel_app.panel_grid
                    .active_container()
                    .map(|w| w.scale_settings.price_tick_extend_left)
                    .unwrap_or(true),
            },
            status_line: StatusLineSettings {
                legend_show_ohlc,
                legend_show_change,
                legend_show_percent,
                tooltip_visible,
                tooltip_follow_cursor,
                ..Default::default()
            },
            scales: ScalesLinesSettings {
                show_grid: vert_lines || horz_lines,
                vert_lines,
                horz_lines,
                auto_scale,
                price_scale_right: true,
                time_scale_bottom: true,
                price_scale_width,
                time_scale_height,
                crosshair_mode: "Normal".to_string(),
                crosshair_line_style: "Dashed".to_string(),
                crosshair_line_width: 1.0,
                crosshair_line_color: rt.chart.crosshair_line.clone(),
                price_scale_position: "right".to_string(),
                time_scale_position: "bottom".to_string(),
                corner_visibility: "on_hover".to_string(),
                date_format: "day_month_year".to_string(),
                use_24h: time_fmt_use_24h,
                show_day_of_week: time_fmt_show_dow,
                timezone_label: tz_label,
                ..Default::default()
            },
        }
    }

    /// Capture the current chart settings into the app-level snapshot store.
    ///
    /// Pushes a [`SnapshotAction::ChartSettings`] that is drained by App each
    /// frame and applied to the shared `AppState.snapshots`.
    pub fn snapshot_chart_settings_to_user_manager(&mut self) {
        let data = self.build_chart_settings_data();
        if let Ok(val) = serde_json::to_value(&data) {
            self.snapshot_actions.push(SnapshotAction::ChartSettings(val));
        }
    }

    /// Capture primitive settings for the active primitive into the app-level
    /// snapshot store, keyed by the primitive's `type_id`.
    pub fn snapshot_primitive_settings_to_user_manager(&mut self, prim_id: u64) {
        let active_bid_snap = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id);
        let data_opt = active_bid_snap
            .and_then(|bid| self.core.drawings().get_data(bid, prim_id));
        if let Some(data) = data_opt {
            let type_id = data.type_id.clone();
            if let Ok(val) = serde_json::to_value(&data) {
                self.snapshot_actions.push(SnapshotAction::PrimitiveSettings {
                    type_id: type_id.clone(),
                    data: val,
                });
            }
            // Remember this style (including extended style_properties) so the next
            // primitive of the same type is pre-populated with these settings.
            if let Some(bid) = active_bid_snap {
                // The write guard MUST end before the read below. It used to
                // be a plain `let` binding, which lives to the end of this
                // block, so `self.core.drawings()` took a read on the same
                // lock while the write was still held — on single-threaded
                // wasm32 that overlap does not panic and does not raise a JS
                // error, it just kills the frame loop where it stands. Live
                // symptom: pick a colour for a drawing and the whole chart
                // stops repainting, while the WS feed keeps running. Same
                // trap as the one documented at `input/sync_group.rs`'s
                // `sync_join_color_group`.
                {
                    let mut drawings_snap = self.core.drawings_mut();
                    drawings_snap.save_last_style_for_primitive(bid, prim_id);
                }
                // Persist the last-used style globally so it survives app restarts.
                let style = self.core.drawings().last_style_for(bid, &type_id);
                if let Some(style) = style {
                    if let Ok(style_val) = serde_json::to_value(&style) {
                        self.snapshot_actions.push(SnapshotAction::DrawingStyle {
                            type_id,
                            data: style_val,
                        });
                    }
                }
            }
        }
    }

    /// Capture indicator settings for the currently-open indicator into the
    /// app-level snapshot store, keyed by the indicator's `type_id`.
    pub fn snapshot_indicator_settings_to_user_manager(&mut self) {
        if let Some(ind_id) = self.panel_app.indicator_settings_state.indicator_id {
            // Bound first so the `indicators()` read guard drops before the body
            // runs (clippy::significant_drop_in_scrutinee).
            let cloned_inst = self.core.indicators().full_clone(ind_id);
            if let Some(inst) = cloned_inst {
                let type_id = inst.type_id.to_string();
                if let Ok(val) = serde_json::to_value(&inst) {
                    self.snapshot_actions.push(SnapshotAction::IndicatorSettings { type_id, data: val });
                }
            }
        }
    }

    /// Capture compare series settings for the currently-open compare modal into
    /// the app-level snapshot store.
    pub fn snapshot_compare_settings_to_user_manager(&mut self) {
        let idx = self.panel_app.compare_settings_state.series_index;
        if let Some(series) = self.panel_app.panel_grid.active_container()
            .and_then(|w| w.compare_overlay.series.get(idx))
        {
            if let Ok(val) = serde_json::to_value(series) {
                self.snapshot_actions.push(SnapshotAction::CompareSettings(val));
            }
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Default for ChartApp {
    fn default() -> Self {
        Self::new("Chart")
    }
}

#[cfg(test)]
mod typed_slot_hydration_tests {
    //! Coverage for `ChartApp::panel_to_str` / `str_to_panel` (T1a hydration
    //! mapping, owner redesign 2026-07-11): legacy `slot1..slot4` profile
    //! keys must still hydrate to the correct typed slot, and the current
    //! writer must only ever emit the new `slot_*` keys (hard cutover — no
    //! new writes of legacy keys, but old saved profiles must load).
    use mlc_app_sidebar_content::state::RightSidebarPanel;

    #[test]
    fn legacy_slot_keys_map_to_typed_slots() {
        assert_eq!(crate::ChartApp::str_to_panel("slot1"), RightSidebarPanel::SlotTrading);
        assert_eq!(crate::ChartApp::str_to_panel("slot2"), RightSidebarPanel::SlotDom);
        assert_eq!(crate::ChartApp::str_to_panel("slot3"), RightSidebarPanel::SlotTape);
        assert_eq!(crate::ChartApp::str_to_panel("slot4"), RightSidebarPanel::SlotL2);
    }

    #[test]
    fn current_slot_keys_map_to_typed_slots() {
        assert_eq!(crate::ChartApp::str_to_panel("slot_trading"), RightSidebarPanel::SlotTrading);
        assert_eq!(crate::ChartApp::str_to_panel("slot_dom"), RightSidebarPanel::SlotDom);
        assert_eq!(crate::ChartApp::str_to_panel("slot_tape"), RightSidebarPanel::SlotTape);
        assert_eq!(crate::ChartApp::str_to_panel("slot_l2"), RightSidebarPanel::SlotL2);
        assert_eq!(crate::ChartApp::str_to_panel("slot_big_trades"), RightSidebarPanel::SlotBigTrades);
    }

    /// `SlotBigTrades` has no legacy equivalent (it's a genuinely new 5th
    /// slot) — there is no `slot5` alias, only the current key.
    #[test]
    fn big_trades_slot_has_no_legacy_alias() {
        assert_eq!(crate::ChartApp::str_to_panel("slot5"), RightSidebarPanel::None);
    }

    /// Hard cutover: the writer (`panel_to_str`) never emits a legacy
    /// `slot1..slot4` key — only the current `slot_*` names.
    #[test]
    fn panel_to_str_never_writes_legacy_keys() {
        let typed_slots = [
            RightSidebarPanel::SlotTrading,
            RightSidebarPanel::SlotDom,
            RightSidebarPanel::SlotTape,
            RightSidebarPanel::SlotL2,
            RightSidebarPanel::SlotBigTrades,
            RightSidebarPanel::SlotLiq,
        ];
        for panel in typed_slots {
            let written = crate::ChartApp::panel_to_str(panel).expect("typed slot must serialize to Some");
            assert!(
                !matches!(written.as_str(), "slot1" | "slot2" | "slot3" | "slot4"),
                "panel_to_str({:?}) must not write a legacy key, got '{}'", panel, written,
            );
            assert!(written.starts_with("slot_"), "expected 'slot_*' key, got '{}'", written);
        }
    }

    /// Full round-trip: every typed slot survives `panel_to_str` →
    /// `str_to_panel` unchanged (current-key path, not the legacy alias).
    #[test]
    fn typed_slot_round_trip_through_current_keys() {
        let typed_slots = [
            RightSidebarPanel::SlotTrading,
            RightSidebarPanel::SlotDom,
            RightSidebarPanel::SlotTape,
            RightSidebarPanel::SlotL2,
            RightSidebarPanel::SlotBigTrades,
            RightSidebarPanel::SlotLiq,
        ];
        for panel in typed_slots {
            let written = crate::ChartApp::panel_to_str(panel).expect("typed slot must serialize to Some");
            let read_back = crate::ChartApp::str_to_panel(&written);
            assert_eq!(read_back, panel, "round-trip mismatch for {:?} via '{}'", panel, written);
        }
    }

    /// Unknown/garbage keys fall back to `None`, same as before the redesign.
    #[test]
    fn unknown_key_falls_back_to_none() {
        assert_eq!(crate::ChartApp::str_to_panel("bogus_panel_id"), RightSidebarPanel::None);
        assert_eq!(crate::ChartApp::str_to_panel(""), RightSidebarPanel::None);
    }

    /// Both pre-merge panel keys ("connectors" and "performance", written by
    /// profiles saved before the Connectors+Performance → Monitoring merge,
    /// 2026-07-18) must hydrate to the merged panel — no dropped sessions.
    #[test]
    fn legacy_connectors_and_performance_keys_map_to_monitoring() {
        assert_eq!(crate::ChartApp::str_to_panel("connectors"), RightSidebarPanel::Monitoring);
        assert_eq!(crate::ChartApp::str_to_panel("performance"), RightSidebarPanel::Monitoring);
    }

    /// The writer always emits the pre-existing "performance" key for the
    /// merged panel (not a new "monitoring" key) — avoids a migration for
    /// anything that persists this string (cheaper than introducing a new key).
    #[test]
    fn panel_to_str_writes_performance_key_for_monitoring() {
        assert_eq!(
            crate::ChartApp::panel_to_str(RightSidebarPanel::Monitoring),
            Some("performance".to_string()),
        );
    }
}
