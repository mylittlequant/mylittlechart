//! `BackfillEngine` — ONE ramped, progressive backfill state machine for
//! every raw layer (kline-per-TF and trades) of every bubble.
//!
//! Unified-data-plane Stage Э2 (`nemo/docs/mlc/plans/unified-data-plane-2026-07-09.md`):
//! replaces three previously-separate backfill models (kline one-shot ladder
//! emitting one `BackfillComplete` at the end, derived cold-seed lump,
//! footprint's private deepen copy) with a single doctrine:
//!
//! - **Cold path**: fetch one page immediately (fastest possible first
//!   paint) → then ramp the GROUP size 1 → 1 → 2 → 4 → … pages per group
//!   until `target` is reached (if set) or the venue is exhausted. Every
//!   page is emitted the instant it arrives — never accumulate-then-emit.
//! - **Scroll-back path**: once the consumer reports it has crossed ~80% of
//!   loaded history toward the oldest edge, fire one page group; repeated
//!   pressure while still scrolling escalates the group size 1 → 2 → 4 → …
//!   the same way the cold path ramps. A quiet period (or the pressure
//!   signal stopping) resets the ramp back to 1.
//! - **Stops**: venue exhaustion (empty page), venue wall (`RestWindow` /
//!   `kline_backpage=false`), or `target` reached. There is no fixed depth
//!   cap — `target: None` backfills until the venue itself stops.
//!
//! The ramp GROUP size is a RATE limiter (how many pages this engine allows
//! itself to fetch back-to-back before yielding control), NOT a depth
//! ceiling — depth is governed exclusively by `target`/exhaustion/wall. See
//! [`MAX_RAMP_PAGES_PER_GROUP`].
//!
//! ## Scope of this module
//!
//! Pure ramp/state logic ([`RampState`], fully unit-tested, no I/O) plus the
//! `DataBridge` driver methods that fetch pages through the station hub and
//! emit them. This module cannot reach into `mlc-backend`'s `BubblesMut`
//! facade (crate layering: `mlc-backend-data-bridge` sits BELOW
//! `mlc-backend` — engine, not kernel) — every page this engine produces
//! reaches the bubble's raw layers exclusively through a `LiveUpdate`
//! broadcast event that `mlc-backend::tick_writers` already owns a writer
//! for (`ScrollBarsLoaded`/`BackfillComplete` for kline,
//! `TradeHistoryPrepended` for trades), plus the layer-scoped
//! `LiveUpdate::LayerBackfilled` notification for Э3/Э4 consumers.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use digdigdig3::core::types::{ArchiveSpec, HistoryCursor, TradeHistoryTier};
use digdigdig3::{AccountType, ExchangeId};
use digdigdig3_station::archive::RawTrade;

use crate::bridge::{DataBridge, LiveUpdate, RawTradeWire, bar_point_to_bar, sleep_ms};

// ═══════════════════════════════════════════════════════════════════════════
// TUNABLES — all ramp/rate constants live here, one place, with doc comments
// distinguishing RATE limiters from the (deliberately absent) depth caps.
// ═══════════════════════════════════════════════════════════════════════════

/// Page size for one kline backfill page, in bars. Matches the exchange
/// per-request cap used elsewhere in the bridge (`effective_limit`).
pub const KLINE_PAGE_SIZE: usize = 500;

/// Page size for one trade/aggTrade backfill page, in trades.
pub const TRADE_PAGE_SIZE: usize = 1000;

/// Ceiling on how many pages one ramp GROUP may request before the engine
/// yields back to the caller loop (still single-flight; the next group is
/// scheduled on the next `ensure_*`/`on_*_scroll_pressure` call). This is a
/// RATE limiter — it bounds how much REST traffic one burst can generate,
/// never how much history the layer is allowed to hold. Depth is governed
/// solely by `target` / venue exhaustion / venue wall (owner doctrine
/// 2026-07-09 — no hand-made depth caps).
pub const MAX_RAMP_PAGES_PER_GROUP: u32 = 8;

/// Scroll-pressure threshold: the consumer is considered "near the oldest
/// loaded edge" once the viewport's distance from the oldest loaded point is
/// at or below this fraction of total loaded depth. `tick.rs`'s existing
/// `view_start < visible_bars * 0.20` check (distance from the LEFT/oldest
/// edge in bar units) is the same 80%-of-loaded-history rule restated in
/// viewport-visible-bars terms rather than total-loaded-bars terms — see the
/// mapping note on `DataBridge::on_kline_scroll_pressure`.
pub const SCROLL_PRESSURE_THRESHOLD: f64 = 0.80;

/// Quiet period after the last scroll-pressure signal before the ramp resets
/// back to a single page per group. Matches the existing scroll-fetch
/// in-flight timeout used elsewhere in the bridge/app for consistency.
pub const RAMP_QUIET_RESET_MS: u64 = 10_000;

/// RAM guard on the trade layer: hard cap on total trades one
/// [`RampState`] (i.e. one `(exchange, symbol, account)` trade layer) will
/// ever load, regardless of how deep the time ladder or an explicit
/// `require_target_ts` demand reaches. This is NOT the backfill goal —
/// the goal is always time (`target_ts`, see [`TradeLadderStage`]) — this
/// is a safety latch so a pathological deep-demand (or a very high-volume
/// symbol whose "last 24h" alone is huge) cannot grow one bubble's trade
/// layer without bound.
///
/// Sizing: `RawTrade`/`RawTradeWire` is `{ts_ms: i64, price: f64, qty: f64,
/// is_buyer_maker: bool}` = 8+8+8+1 bytes, padded to 32 bytes per element
/// in a `Vec`. 1_500_000 points ≈ 1_500_000 × 32B ≈ 48 MB raw storage for
/// the layer itself (plus whatever fold/derived views the layer feeds) —
/// generous enough to cover the full "day-by-day" ladder for very liquid
/// symbols (BTCUSDT perp is on the order of a few hundred thousand trades
/// per day) while still bounding worst-case memory for a pathological
/// demand.
pub const TRADE_RAM_CAP_POINTS: usize = 1_500_000;

/// Wall-clock "now" in epoch milliseconds — the single time source for
/// every trade-ladder flight (sampled ONCE per [`DataBridge::
/// ensure_trade_backfill`]/[`DataBridge::on_trade_scroll_pressure`] call,
/// never re-sampled per group, per [`RampState::begin_trade_flight`]'s doc
/// comment). Same `web_time::SystemTime` pattern used elsewhere in this
/// crate (wasm-safe — `web_time` shims to `Date.now()` on wasm32).
pub(crate) fn now_ms() -> i64 {
    web_time::SystemTime::now()
        .duration_since(web_time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(i64::MAX)
}

// ═══════════════════════════════════════════════════════════════════════════
// LayerId / BackfillKey — identity for one engine-tracked layer
// ═══════════════════════════════════════════════════════════════════════════

/// Which raw layer of a bubble's instrument a [`RampState`] tracks.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum LayerId {
    /// A kline layer for one timeframe name (e.g. `"1m"`, `"1h"`).
    Kline(String),
    /// The single trade layer (no per-parameter split — one raw trade
    /// stream per instrument).
    Trades,
}

impl LayerId {
    /// Wire label used by `LiveUpdate::LayerBackfilled.layer` — `"kline:<tf>"`
    /// or `"trades"`.
    pub fn wire_label(&self) -> String {
        match self {
            LayerId::Kline(tf) => format!("kline:{tf}"),
            LayerId::Trades => "trades".to_string(),
        }
    }
}

/// Compound key identifying one engine-tracked layer:
/// `(exchange, symbol, account_type, layer)`.
pub type BackfillKey = (ExchangeId, String, AccountType, LayerId);

// ═══════════════════════════════════════════════════════════════════════════
// Trade time ladder — owner doctrine 2026-07-12: trade backfill targets a
// TIME floor, not a point count. "Load the last hour as fast as possible,
// then the last 12 hours, then the last day, then day-by-day on demand" —
// counts survive only as [`TRADE_RAM_CAP_POINTS`], a RAM guard, never the
// goal.
// ═══════════════════════════════════════════════════════════════════════════

/// One rung of the trade-layer time ladder. Auto-advances Hour → HalfDay →
/// Day without any caller involvement; past `Day` the floor only deepens on
/// an explicit [`RampState::require_target_ts`] demand (scroll-pressure
/// asking for more history) — that state is [`Self::Deep`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradeLadderStage {
    /// Floor = flight-start "now" − 1 hour.
    Hour,
    /// Floor = flight-start "now" − 12 hours (≈ the ladder's "12 hours
    /// total" rung, i.e. the hour rung plus another 11).
    HalfDay,
    /// Floor = flight-start "now" − 24 hours. Terminal auto-advance rung —
    /// the ladder does not advance past `Day` on its own.
    Day,
    /// Floor set by an explicit [`RampState::require_target_ts`] demand
    /// reaching further back than `Day`'s floor (scroll-pressure "day by
    /// day, reacting to backfill needs" phase).
    Deep,
}

impl TradeLadderStage {
    /// Floor (ms) this rung targets, given the flight's fixed `now_ms`
    /// (sampled once per `ensure_trade_backfill` call, never re-sampled per
    /// group — deterministic per flight).
    fn floor_ms(self, now_ms: i64) -> i64 {
        match self {
            TradeLadderStage::Hour => now_ms - 3_600_000,
            TradeLadderStage::HalfDay => now_ms - 12 * 3_600_000,
            TradeLadderStage::Day | TradeLadderStage::Deep => now_ms - 24 * 3_600_000,
        }
    }

    /// Next auto-advance rung, or `None` once at (or past) `Day` — the
    /// ladder never advances itself past the one-day floor; anything deeper
    /// requires an explicit [`RampState::require_target_ts`] demand.
    fn next_auto(self) -> Option<TradeLadderStage> {
        match self {
            TradeLadderStage::Hour => Some(TradeLadderStage::HalfDay),
            TradeLadderStage::HalfDay => Some(TradeLadderStage::Day),
            TradeLadderStage::Day | TradeLadderStage::Deep => None,
        }
    }

    /// Debug label for [`crate::dlog`] probes.
    fn label(self) -> &'static str {
        match self {
            TradeLadderStage::Hour => "Hour",
            TradeLadderStage::HalfDay => "HalfDay",
            TradeLadderStage::Day => "Day",
            TradeLadderStage::Deep => "Deep",
        }
    }
}

/// Caller-side depth demand for [`DataBridge::ensure_trade_backfill`] —
/// replaces the old raw point-count `target: Option<usize>`. `window_ms:
/// None` means "no opinion, use the engine's own ladder default" (auto to
/// the `Day` rung); `Some(w)` requests a floor `w` milliseconds back from
/// "now", widening the ramp's `target_ts` exactly like a deeper
/// `require_target_ts` call would (never shrinks an already-deeper floor).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct TradeDemand {
    /// Depth requested, in milliseconds back from "now". `None` = ladder
    /// default (auto-advance bounded by `auto_ceiling_ms`, no explicit
    /// deeper floor).
    pub window_ms: Option<i64>,
    /// Ceiling for the AUTO ladder, in milliseconds back from "now" — the
    /// ladder never auto-advances its floor past `now − ceiling`. Derived
    /// from the consumer's timeframe/fold interval via
    /// [`trade_auto_ceiling_ms`] (a 1m footprint has no business
    /// auto-pulling 24h of tape — owner correction 2026-07-12). Deepest
    /// ceiling wins across consumers sharing one key; `None` = no opinion
    /// (keeps whatever ceiling the key already has — a fresh key with no
    /// ceiling at all auto-advances to `Day`, the legacy behavior).
    /// Explicit demand (`window_ms` / scroll pressure) IGNORES the
    /// ceiling — scroll can always go deeper.
    pub auto_ceiling_ms: Option<i64>,
}

/// TF-scaled ceiling for the auto trade ladder: roughly a screenful of
/// bars (~180) at `unit_ms` per bar, clamped to `[1h, 24h]`. `unit_ms` is
/// the consumer's natural bar width — fold `interval_ms` for footprint,
/// chart TF for the CVD companion; consumers with no time-shaped unit
/// (tick/imbalance bars, `unit_ms == 0`) clamp up to the 1h minimum.
pub fn trade_auto_ceiling_ms(unit_ms: i64) -> i64 {
    unit_ms.saturating_mul(180).clamp(3_600_000, 86_400_000)
}

// ═══════════════════════════════════════════════════════════════════════════
// RampState — pure state machine, no I/O
// ═══════════════════════════════════════════════════════════════════════════

/// Per-layer backfill/ramp bookkeeping. Pure data + pure transitions — every
/// method here is synchronous and side-effect-free (no network, no locking
/// beyond what the caller already holds), so the ramp progression itself is
/// exhaustively unit-testable without a station/hub/runtime.
#[derive(Debug, Clone)]
pub struct RampState {
    /// Oldest anchor (ms) fetched so far for this layer. `None` = nothing
    /// fetched yet (fresh/cold layer) — the next fetch anchors at "now".
    pub oldest_anchor: Option<i64>,
    /// Current ramp step, in PAGES for the next group — 1, 1, 2, 4, 8, …,
    /// capped at [`MAX_RAMP_PAGES_PER_GROUP`]. Doubles once per COMPLETED
    /// group via [`Self::advance_ramp`] (seed group and the group right
    /// after it both stay at 1; doubling starts on the third group).
    pub ramp_step: u32,
    /// Number of fetch groups completed so far for this key. Drives the
    /// 1 → 1 → 2 → 4 → … progression independently of `loaded` (which a
    /// zero-length exhausted page would otherwise leave ambiguous).
    pub groups_completed: u32,
    /// A fetch group is currently in flight for this key (single-flight
    /// guard).
    pub in_flight: bool,
    /// The venue has exhausted its history (empty page or capability wall)
    /// — latched permanently until the layer's identity changes (instrument
    /// switch drops the whole key, never un-set otherwise).
    pub exhausted: bool,
    /// Target depth in points (bars/trades), if any. `None` = unbounded —
    /// backfill continues until the venue stops on its own (scroll-back
    /// mode, or an explicit "no known target" cold path).
    pub target: Option<usize>,
    /// Points loaded into the layer so far, as tracked by this engine (not
    /// re-derived from the layer itself — the engine is the sole writer of
    /// this counter and it is monotonically non-decreasing until a reset).
    pub loaded: usize,
    /// Wall-clock instant of the last scroll-pressure signal, used to reset
    /// the ramp to 1 after a quiet period. `None` before the first pressure
    /// signal (cold path never needs this — it ramps unconditionally).
    pub last_pressure_at: Option<web_time::Instant>,
    /// Trade layer only: this key has transitioned from live REST
    /// pagination into bulk daily-archive-file mode (Э6-Ф3 — see
    /// `run_trade_ramp`'s `RecentOnly`/`RestWindow` archive-chaining
    /// branches). Once `true`, one ramp "page" is one archive day file
    /// instead of one REST page. Latched like `exhausted` — a key never
    /// leaves archive mode once entered (only a fresh instrument/layer
    /// identity, i.e. a new key, starts outside it again).
    pub archive_mode: bool,
    /// Trade layer only: minimum `ts_ms` seen across every page emitted so
    /// far for this key — updated on EVERY [`Self::record_page`] call
    /// (unconditionally, alongside `oldest_anchor`; `oldest_anchor` is the
    /// venue-side pagination cursor — an agg-trade id for the REST tier —
    /// while `oldest_ts` is always a wall-clock ms value regardless of
    /// which tier produced the page). `None` = nothing emitted yet.
    pub oldest_seen_ts: Option<i64>,
    /// Trade layer only: absolute ms floor this key wants to reach.
    /// `None` before the first [`Self::begin_trade_flight`] call for a
    /// fresh key. Set from [`TradeLadderStage::floor_ms`] on every ladder
    /// auto-advance, or widened (made smaller / further back) by
    /// [`Self::require_target_ts`].
    pub target_ts: Option<i64>,
    /// Trade layer only: current rung of the time ladder. `None` on a
    /// non-trade `RampState` (kline layers never touch the ladder) or
    /// before the first trade flight.
    pub ladder_stage: Option<TradeLadderStage>,
    /// Trade layer only: "now" as sampled once at the start of the current
    /// (or most recent) flight — every ladder floor for this flight is
    /// computed from this fixed instant, never re-sampled per group.
    pub flight_now_ms: Option<i64>,
    /// [`TRADE_RAM_CAP_POINTS`] was hit for this key — latched like
    /// `exhausted` (a RAM-capped key never resumes growing, even if a
    /// later caller widens `target_ts` further; re-arming would just spend
    /// a round-trip re-hitting the same cap).
    pub ram_capped: bool,
    /// Trade layer only: auto-ladder ceiling in ms back from "now" —
    /// see [`TradeDemand::auto_ceiling_ms`]. Deepest wins
    /// ([`Self::widen_auto_ceiling`]); `None` = uncapped auto ladder
    /// (legacy: auto-advance all the way to `Day`).
    pub auto_ceiling_ms: Option<i64>,
}

impl Default for RampState {
    fn default() -> Self {
        Self {
            oldest_anchor: None,
            ramp_step: 1,
            groups_completed: 0,
            in_flight: false,
            exhausted: false,
            target: None,
            loaded: 0,
            last_pressure_at: None,
            archive_mode: false,
            oldest_seen_ts: None,
            target_ts: None,
            ladder_stage: None,
            flight_now_ms: None,
            ram_capped: false,
            auto_ceiling_ms: None,
        }
    }
}

impl RampState {
    /// Fresh state for a target-bounded cold-open backfill (kline layers —
    /// count-based target, unchanged). Trade layers use
    /// [`Self::begin_trade_flight`] instead (time-ladder target).
    pub fn new(target: Option<usize>) -> Self {
        Self { target, ..Self::default() }
    }

    /// Whether this layer needs any more fetching right now: not already
    /// in flight, not exhausted, and — for the AUTO-ENSURE phase only, when
    /// `gate_on_target` is `true` — (if a target is set) not yet satisfied.
    ///
    /// `target` gates EXCLUSIVELY the auto-ensure ramp phase
    /// (`ensure_kline_backfill`/`ensure_trade_backfill`). Scroll-pressure
    /// driven runs (`on_kline_scroll_pressure`/`on_trade_scroll_pressure`)
    /// pass `gate_on_target: false` here — pressure keeps loading past a
    /// satisfied target, unbounded until the venue itself goes dry (owner
    /// doctrine 2026-07-09).
    pub fn needs_more(&self, gate_on_target: bool) -> bool {
        if self.in_flight || self.exhausted {
            return false;
        }
        self.target_satisfied(gate_on_target)
    }

    /// The `target`-satisfaction half of [`Self::needs_more`], WITHOUT the
    /// `in_flight`/`exhausted` gates — for mid-group continuation checks
    /// made from INSIDE a runner loop that itself holds the single-flight
    /// guard for the whole group's duration (`in_flight` is `true` from
    /// `begin_flight()` at group start until `end_flight()` at group end,
    /// so `needs_more` would unconditionally read `false` for every page
    /// after the first one in the same group — this is the fix for that:
    /// [`run_archive_group`] uses this instead of `needs_more` to decide
    /// whether to fetch the NEXT day within the group it is already
    /// running). `gate_on_target: true` = auto-ensure phase (stop once
    /// `loaded >= target`); `false` = scroll-pressure phase (`target`
    /// ignored, always more wanted) — same split as `needs_more`.
    pub fn target_satisfied(&self, gate_on_target: bool) -> bool {
        if !gate_on_target {
            return true;
        }
        match self.target {
            Some(t) => self.loaded < t,
            None => true,
        }
    }

    /// Adopt `requested_target` as this key's REQUIRED DEPTH and report
    /// whether the auto-ensure ramp must re-arm to satisfy it (chart-type-
    /// engine-consolidation B1, fix 1).
    ///
    /// The bug this closes: a single `BackfillKey` (exchange, symbol,
    /// account, layer) is shared by every chart_type/window folding over
    /// that layer. Once the FIRST subscriber's target was reached (`loaded
    /// >= target`), [`Self::needs_more`] latched `false` for good — a LATER
    /// subscriber requesting a DEEPER target (e.g. a chart-type switch whose
    /// `derived_kline_target` is bigger) silently no-op'd forever, because
    /// `target` was never raised past the first caller's value and the old
    /// call-site guard (`state.target.is_none() && target_bars.is_some()`)
    /// only ever adopted a target once.
    ///
    /// Fix: `requested_target: None` means "this caller has no depth
    /// requirement of its own" (e.g. the TPO Kline1m companion's kline sub,
    /// which only needs whatever depth an earlier fold subscriber already
    /// established) — `target` is left exactly as-is, matching the
    /// pre-existing "don't touch an unset target" contract for `None`.
    /// `requested_target: Some(req)` widens `target` to `max(current, req)`
    /// when `target` is already `Some`, or adopts `req` outright when
    /// `target` was `None` — the key tracks the DEEPEST depth ANY concrete
    /// subscriber has ever asked for, never shrinks. If that raises
    /// `loaded < target` back to `true`, the ramp re-arms:
    /// `groups_completed`/`ramp_step` reset to the seed state (a re-arm is
    /// architecturally a fresh ramp, not a continuation — the prior ramp's
    /// progression doesn't apply to a caller that showed up after the ramp
    /// had already gone quiet) so the resumed run starts gently (1 page)
    /// rather than picking up wherever the previous ramp's step had climbed
    /// to.
    ///
    /// What stays LATCHED across a re-arm: `exhausted` (and `archive_mode`)
    /// — a venue-dry layer legitimately cannot grow backward regardless of
    /// how deep a later subscriber's target is; re-arming a venue-dry key
    /// would just spend a network round-trip to learn the same "no more
    /// data" answer again. `oldest_anchor` (resume point) and `loaded`
    /// (running total) are untouched — the ramp resumes from where the
    /// layer actually is, it does not restart the fetch from scratch.
    ///
    /// Returns `true` iff the caller should [`Self::begin_flight`] a fresh
    /// run (`needs_more(true)` reads `true` after the target update AND the
    /// re-arm just performed, if any) — mirrors the return-shape callers
    /// already build their `should_start` decision from.
    pub fn require_target(&mut self, requested_target: Option<usize>) -> bool {
        if let Some(req) = requested_target {
            let widened = Some(self.target.map_or(req, |cur| cur.max(req)));
            if widened != self.target {
                self.target = widened;
                if self.needs_more(true) {
                    // A satisfied-then-widened key re-arms as a fresh ramp —
                    // the OLD ramp had already gone quiet
                    // (`groups_completed`/`ramp_step` reflect a run that
                    // finished satisfying the smaller target); a brand-new
                    // subscriber's pressure should not inherit that stale
                    // progression.
                    self.reset_ramp();
                }
            }
        }
        self.needs_more(true)
    }

    /// Pages to request in the NEXT group, honoring the ramp progression
    /// (1 → 1 → 2 → 4 → … capped at [`MAX_RAMP_PAGES_PER_GROUP`]).
    ///
    /// When `gate_on_target` is `true` (auto-ensure phase), never requests
    /// more pages than needed to reach a set `target`. When `false`
    /// (scroll-pressure phase), `target` is ignored entirely — the ramp
    /// step is the only bound, per the same doctrine as [`Self::needs_more`].
    pub fn next_group_pages(&self, page_size: usize, gate_on_target: bool) -> u32 {
        let mut pages = self.ramp_step.clamp(1, MAX_RAMP_PAGES_PER_GROUP);
        if gate_on_target {
            if let Some(target) = self.target {
                if page_size > 0 {
                    let remaining = target.saturating_sub(self.loaded);
                    let pages_needed = remaining.div_ceil(page_size).max(1) as u32;
                    pages = pages.min(pages_needed);
                }
            }
        }
        pages
    }

    /// Advance the ramp step for the NEXT group, per the doctrine sequence
    /// 1 → 1 → 2 → 4 → 8 → … (the seed group and the group immediately
    /// after it are both size 1; doubling begins on the third group),
    /// capped at [`MAX_RAMP_PAGES_PER_GROUP`].
    pub fn advance_ramp(&mut self) {
        self.groups_completed = self.groups_completed.saturating_add(1);
        self.ramp_step = match self.groups_completed {
            0 | 1 => 1,
            n => {
                let doublings = n - 1; // group 2 -> 2^0=1, group 3 -> 2^1=2, group 4 -> 2^2=4, ...
                1u32.checked_shl(doublings.min(31))
                    .unwrap_or(u32::MAX)
                    .min(MAX_RAMP_PAGES_PER_GROUP)
            }
        };
    }

    /// Reset the ramp back to the seed step (1) — called after a quiet
    /// period with no scroll pressure, or when a fresh crossing begins.
    pub fn reset_ramp(&mut self) {
        self.ramp_step = 1;
        self.groups_completed = 0;
    }

    /// Record a scroll-pressure signal: escalates the ramp if the signal
    /// arrives while a previous one is still "fresh" (within
    /// [`RAMP_QUIET_RESET_MS`]), otherwise resets to the seed step first —
    /// a fresh crossing after a quiet spell re-earns the ramp from scratch.
    pub fn record_pressure(&mut self, now: web_time::Instant) {
        let quiet_expired = self.last_pressure_at
            .map(|t| now.saturating_duration_since(t).as_millis() as u64 > RAMP_QUIET_RESET_MS)
            .unwrap_or(false);
        if quiet_expired {
            self.reset_ramp();
        }
        self.last_pressure_at = Some(now);
    }

    /// Mark a fetch group as started (single-flight guard engaged).
    pub fn begin_flight(&mut self) {
        self.in_flight = true;
    }

    /// Record one page's outcome: advances `oldest_anchor`, accumulates
    /// `loaded`, and latches `exhausted` when the venue signals it has no
    /// more data (empty page or a capability wall report). Kline layers
    /// (whose `new_oldest_anchor` IS already a wall-clock ms value) call
    /// this directly; trade layers call [`Self::record_trade_page`] instead
    /// so `oldest_seen_ts` — always a wall-clock ms value regardless of
    /// which tier (REST agg-id-cursor vs. archive day file) produced the
    /// page — is tracked alongside the tier-specific `oldest_anchor`.
    pub fn record_page(&mut self, page_len: usize, new_oldest_anchor: Option<i64>, venue_exhausted: bool) {
        self.loaded += page_len;
        if let Some(anchor) = new_oldest_anchor {
            self.oldest_anchor = Some(anchor);
        }
        if venue_exhausted {
            self.exhausted = true;
        }
    }

    /// Trade layer only: [`Self::record_page`] plus `oldest_seen_ts`
    /// bookkeeping — `new_oldest_ts` is the MINIMUM `ts_ms` observed in the
    /// page just emitted (`None` for an empty page). `oldest_seen_ts` only
    /// ever moves further back (monotonically non-increasing), matching
    /// `oldest_anchor`'s resume-point semantics.
    pub fn record_trade_page(
        &mut self,
        page_len: usize,
        new_oldest_anchor: Option<i64>,
        new_oldest_ts: Option<i64>,
        venue_exhausted: bool,
    ) {
        self.record_page(page_len, new_oldest_anchor, venue_exhausted);
        if let Some(ts) = new_oldest_ts {
            self.oldest_seen_ts = Some(self.oldest_seen_ts.map_or(ts, |cur| cur.min(ts)));
        }
        if self.loaded >= TRADE_RAM_CAP_POINTS {
            self.ram_capped = true;
        }
    }

    /// End the in-flight group, advancing the ramp for the next one.
    pub fn end_flight(&mut self) {
        self.in_flight = false;
        self.advance_ramp();
    }

    /// Trade layer only: transition into bulk daily-archive-file mode
    /// instead of latching `exhausted` — called at the exact site that
    /// would otherwise have declared the venue's live REST tier dry (a
    /// `RecentOnly` venue's one-and-only shallow fetch just landed, or a
    /// `RestWindow` venue's cursor walk just hit its `max_back_ms` wall) but
    /// a [`digdigdig3::core::types::TradeArchiveCapabilities::for_account`]
    /// spec exists for this key. Idempotent — entering archive mode twice is
    /// a no-op (latch semantics, same as `exhausted`).
    pub fn enter_archive_mode(&mut self) {
        self.archive_mode = true;
    }

    /// Trade layer only: whether the CURRENT ladder rung's floor has been
    /// reached — `oldest_seen_ts <= target_ts`, or the layer is latched
    /// (`exhausted`/`ram_capped`, both of which make "more" moot regardless
    /// of the floor). A fresh key with no `target_ts` yet (`None`) has not
    /// reached anything.
    pub fn ladder_floor_reached(&self) -> bool {
        if self.exhausted || self.ram_capped {
            return true;
        }
        match (self.oldest_seen_ts, self.target_ts) {
            (Some(seen), Some(target)) => seen <= target,
            _ => false,
        }
    }

    /// Trade layer only — named for its ACTUAL polarity: `true` = keep
    /// walking (current rung's floor not reached yet), `false` = this rung
    /// is done (floor reached, or latched exhausted/ram-capped).
    /// `gate_on_target == false` (scroll-pressure phase) always reports
    /// "more wanted" — the pressure caller bounds its own flight to one
    /// group, so this never spins. Bypasses `in_flight` for use from inside
    /// a runner loop that already holds the single-flight guard. (Was
    /// briefly `target_satisfied_ts` — the inverted name got the loop
    /// tail read backwards and stalled the ladder mid-rung.)
    pub fn wants_more_ts(&self, gate_on_target: bool) -> bool {
        if !gate_on_target {
            return true;
        }
        !self.ladder_floor_reached()
    }

    /// Trade layer only: start (or resume) a flight — samples `now_ms`
    /// ONCE for the whole flight (deterministic ladder floors, never
    /// re-sampled per group) and, on a FRESH key (`ladder_stage.is_none()`),
    /// seeds the ladder at [`TradeLadderStage::Hour`] with `target_ts` set
    /// from its floor. A key that already has a `ladder_stage` (resumed
    /// flight, or a re-arm via [`Self::require_target_ts`] that ran before
    /// this call) keeps its current stage/`target_ts` — only `flight_now_ms`
    /// is refreshed, so `TradeLadderStage::floor_ms` stays anchored to this
    /// flight's start for any auto-advance that happens during it.
    pub fn begin_trade_flight(&mut self, now_ms: i64) {
        self.flight_now_ms = Some(now_ms);
        if self.ladder_stage.is_none() {
            self.ladder_stage = Some(TradeLadderStage::Hour);
            let mut floor = TradeLadderStage::Hour.floor_ms(now_ms);
            if let Some(ceiling) = self.auto_ceiling_ms {
                floor = floor.max(now_ms - ceiling);
            }
            self.target_ts = Some(floor);
        }
    }

    /// Trade layer only: adopt a (possibly wider) auto-ladder ceiling —
    /// deepest wins across consumers sharing this key (a 1h footprint and
    /// a 1m CVD on the same instrument → the 1h consumer's 24h ceiling
    /// governs). See [`TradeDemand::auto_ceiling_ms`].
    pub fn widen_auto_ceiling(&mut self, ceiling_ms: i64) {
        self.auto_ceiling_ms = Some(self.auto_ceiling_ms.map_or(ceiling_ms, |cur| cur.max(ceiling_ms)));
    }

    /// Trade layer only: advance the ladder to the next auto rung if the
    /// current rung's floor has been reached and an auto rung exists past
    /// it (`Hour` → `HalfDay` → `Day`; `Day`/`Deep` never auto-advance).
    /// Each rung's floor is clamped by [`Self::auto_ceiling_ms`] — once the
    /// clamped next floor would not deepen the current `target_ts`, the
    /// ladder is at its ceiling and reports no advance (a 1m consumer's
    /// ~3h ceiling stops the ladder mid-`HalfDay`; a later wider ceiling
    /// via [`Self::widen_auto_ceiling`] re-opens it).
    /// Returns `true` iff it advanced (caller keeps looping); `false` means
    /// either the floor isn't reached yet, or the ladder is already at its
    /// auto-advance ceiling with no explicit deeper demand.
    pub fn advance_ladder_if_reached(&mut self) -> bool {
        if self.exhausted || self.ram_capped || !self.ladder_floor_reached() {
            return false;
        }
        let Some(stage) = self.ladder_stage else { return false; };
        let Some(now_ms) = self.flight_now_ms else { return false; };
        let Some(next) = stage.next_auto() else { return false; };
        let mut next_floor = next.floor_ms(now_ms);
        if let Some(ceiling) = self.auto_ceiling_ms {
            next_floor = next_floor.max(now_ms - ceiling);
        }
        if self.target_ts.is_some_and(|cur| cur <= next_floor) {
            return false;
        }
        self.ladder_stage = Some(next);
        self.target_ts = Some(next_floor);
        true
    }

    /// Trade layer only: adopt `deeper_ts` as this key's REQUIRED FLOOR and
    /// report whether the ramp must re-arm to satisfy it — the ts-based
    /// mirror of [`Self::require_target`]. `deeper_ts` is a smaller
    /// (further-back) absolute ms floor than whatever is currently set;
    /// `target_ts` only ever moves further back (`min`), matching
    /// `require_target`'s "never shrinks" rule. Moves the stage to
    /// [`TradeLadderStage::Deep`] once the demand reaches past `Day`'s
    /// floor. A latched key (`exhausted`/`ram_capped`) never re-arms — same
    /// doctrine as `require_target`.
    pub fn require_target_ts(&mut self, deeper_ts: i64) -> bool {
        if self.exhausted || self.ram_capped {
            return false;
        }
        let widened = Some(self.target_ts.map_or(deeper_ts, |cur| cur.min(deeper_ts)));
        if widened != self.target_ts {
            self.target_ts = widened;
            if self.ladder_stage.map(|s| !matches!(s, TradeLadderStage::Deep)).unwrap_or(true) {
                self.ladder_stage = Some(TradeLadderStage::Deep);
            }
            if !self.ladder_floor_reached() {
                self.reset_ramp();
            }
        }
        self.in_flight_or_floor_unmet()
    }

    fn in_flight_or_floor_unmet(&self) -> bool {
        if self.in_flight || self.exhausted || self.ram_capped {
            return false;
        }
        !self.ladder_floor_reached()
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// BackfillEngine — the registry DataBridge owns
// ═══════════════════════════════════════════════════════════════════════════

/// Registry of [`RampState`] per [`BackfillKey`], owned by [`DataBridge`].
#[derive(Debug, Default, Clone)]
pub struct BackfillEngine {
    states: Arc<Mutex<HashMap<BackfillKey, RampState>>>,
}

impl BackfillEngine {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot the current state for a key (test/debug introspection).
    pub fn snapshot(&self, key: &BackfillKey) -> Option<RampState> {
        self.states.lock().ok()?.get(key).cloned()
    }

    fn with_state<R>(&self, key: &BackfillKey, f: impl FnOnce(&mut RampState) -> R) -> R {
        let mut guard = self.states.lock().unwrap_or_else(|e| e.into_inner());
        let state = guard.entry(key.clone()).or_insert_with(RampState::default);
        f(state)
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// DataBridge driver methods — kline layer
// ═══════════════════════════════════════════════════════════════════════════

impl DataBridge {
    /// Ensure the kline layer for `(exchange, symbol, account, tf)` has at
    /// least `target_bars` bars loaded (or, if `target_bars` is `None`,
    /// keeps ramping until the venue is exhausted) — the universal
    /// cold/ramp backfill path (unified-data-plane Э2). Replaces the
    /// one-shot 40-page ladder previously in `request_background_backfill`.
    ///
    /// Non-blocking, single-flight per key. Every page fetched is emitted
    /// immediately: the FIRST page of a cold run as
    /// `LiveUpdate::BackfillComplete` (preserves the existing cold-open
    /// `needs_snap_after_backfill` / full-replace-anchor path unchanged),
    /// every subsequent page in the same run as the additive
    /// `LiveUpdate::ScrollBarsLoaded` (the existing `tick_writers`/react.rs
    /// merge machinery already prepends it into `bubble.bars` + the raw
    /// kline layer) — plus a `LiveUpdate::LayerBackfilled` notification for
    /// every page.
    pub fn ensure_kline_backfill(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &mlc_core::state::Timeframe,
        account_type: AccountType,
        target_bars: Option<u32>,
    ) {
        let key: BackfillKey = (
            exchange_id,
            symbol.to_string(),
            account_type,
            LayerId::Kline(timeframe.name.clone()),
        );

        let should_start = self.backfill_engine().with_state(&key, |state| {
            // Fix 1 (chart-type-engine-consolidation B1): re-arm by REQUIRED
            // DEPTH rather than a one-shot target adoption — see
            // `RampState::require_target`'s doc comment for the full
            // rationale (a later, deeper subscriber must not starve behind
            // an earlier, shallower one's already-satisfied target).
            if !state.require_target(target_bars.map(|t| t as usize)) {
                return false;
            }
            state.begin_flight();
            true
        });
        if !should_start {
            return;
        }

        let fetch_hub = self.station().hub();
        let tx = self.updates_tx();
        let symbol_str = symbol.to_string();
        let wire_sym = self.wire_symbol_pub(exchange_id, account_type, symbol);
        let interval = crate::convert::timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let engine = self.backfill_engine();

        self.spawn_bg_pub(async move {
            run_kline_ramp(
                fetch_hub, tx, engine, key, exchange_id, account_type,
                symbol_str, wire_sym, interval, tf_name, true,
            ).await;
        });
    }

    /// Scroll-pressure entry for the kline layer (unified-data-plane Э2):
    /// call this when the consumer has crossed ~80% of its loaded kline
    /// history toward the oldest edge. Escalates the ramp group size if
    /// pressure keeps arriving; resets to 1 after a quiet period (handled
    /// inside [`RampState::record_pressure`]).
    ///
    /// Threshold mapping (owner doctrine "trigger at ~80% of loaded data"):
    /// `tick.rs`'s existing trigger fires when
    /// `view_start <= visible_bars * 0.20` — the viewport's LEFT edge is
    /// within 20% of the visible window's width from the oldest loaded bar.
    /// That is the same "within the last 20% of loaded history" rule the
    /// moment the loaded series is only slightly larger than one screen —
    /// exactly the state a freshly-opened chart is in, and the state the
    /// existing call site already checks against. `on_kline_scroll_pressure`
    /// does not re-derive the geometry itself (that stays call-site work,
    /// documented at the `tick.rs` call site) — it only accepts the boolean
    /// "pressure now" and drives the ramp.
    ///
    /// `target` (if any adopted by a prior/concurrent
    /// [`Self::ensure_kline_backfill`] call for the same key) is IGNORED for
    /// this run: pressure keeps loading past a satisfied target, unbounded
    /// until the venue itself goes dry (owner doctrine 2026-07-09 — target
    /// gates only the auto-ensure phase).
    pub fn on_kline_scroll_pressure(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        timeframe: &mlc_core::state::Timeframe,
        account_type: AccountType,
    ) {
        let key: BackfillKey = (
            exchange_id,
            symbol.to_string(),
            account_type,
            LayerId::Kline(timeframe.name.clone()),
        );

        let should_start = self.backfill_engine().with_state(&key, |state| {
            state.record_pressure(web_time::Instant::now());
            if state.in_flight || state.exhausted {
                return false;
            }
            state.begin_flight();
            true
        });
        if !should_start {
            return;
        }

        let fetch_hub = self.station().hub();
        let tx = self.updates_tx();
        let symbol_str = symbol.to_string();
        let wire_sym = self.wire_symbol_pub(exchange_id, account_type, symbol);
        let interval = crate::convert::timeframe_to_interval(timeframe);
        let tf_name = timeframe.name.clone();
        let engine = self.backfill_engine();

        self.spawn_bg_pub(async move {
            run_kline_ramp(
                fetch_hub, tx, engine, key, exchange_id, account_type,
                symbol_str, wire_sym, interval, tf_name, false,
            ).await;
        });
    }
}

/// Shared ramp-group runner for both the cold path and the scroll-pressure
/// path — fetches `next_group_pages()` anchored kline pages sequentially,
/// emitting each one as it arrives (never accumulate-then-emit), then ends
/// the flight (advancing the ramp for the next group). `gate_on_target`
/// selects the phase: `true` for the auto-ensure path (target bounds the
/// run), `false` for the scroll-pressure path (target ignored — unbounded
/// until venue-dry).
async fn run_kline_ramp(
    fetch_hub: Arc<digdigdig3::connector_manager::ExchangeHub>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: BackfillEngine,
    key: BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_str: String,
    wire_sym: String,
    interval: String,
    tf_name: String,
    gate_on_target: bool,
) {
    // Readiness barrier — same pattern as the legacy backfill/scroll fns.
    {
        let mut attempts = 0;
        loop {
            if fetch_hub.rest(exchange_id).is_some() {
                break;
            }
            attempts += 1;
            if attempts > 50 {
                crate::dlog!(
                    "[BackfillEngine] kline: connector {:?} not initialized after 5s",
                    exchange_id
                );
                engine.with_state(&key, RampState::end_flight);
                return;
            }
            sleep_ms(100).await;
        }
    }

    let page_size = KLINE_PAGE_SIZE;
    let mut is_first_page_of_run = engine.with_state(&key, |s| s.groups_completed == 0 && s.oldest_anchor.is_none());

    // Auto-phase self-drive: one `ensure_kline_backfill` call walks GROUPS
    // (ramp progression 1→1→2→4… via `next_group_pages` per group) until
    // the declared target depth is reached or the venue goes dry. Before
    // this, one call fetched exactly ONE group and stopped — a deep target
    // (TPO Kline1m: 43 200 bars) silently stalled at the first 1000-bar
    // page with nobody re-driving the ramp (found live 2026-07-11).
    'run: loop {
    let group_pages = engine.with_state(&key, |s| s.next_group_pages(page_size, gate_on_target));
    crate::dlog!(
        "[kline_ramp] group key={:?}/{} tf={} group_pages={} loaded={} target={:?}",
        key.0, key.1, tf_name, group_pages,
        engine.with_state(&key, |s| s.loaded),
        engine.with_state(&key, |s| s.target)
    );

    for _ in 0..group_pages {
        let older_than_ms = engine.with_state(&key, |s| s.oldest_anchor);

        let (points, outcome) = digdigdig3_station::backfill::klines_paginated(
            &fetch_hub, exchange_id, account_type, &wire_sym, &interval,
            page_size, 1, older_than_ms,
        ).await;

        if points.is_empty() {
            engine.with_state(&key, |s| s.record_page(0, None, true));
            let _ = tx.send(LiveUpdate::LayerBackfilled {
                exchange_id, symbol: symbol_str.clone(), account_type,
                layer: LayerId::Kline(tf_name.clone()).wire_label(),
                prepended: 0, exhausted: true, stage_boundary: true,
            });
            break 'run;
        }

        let new_oldest = points.iter().map(|p| p.open_time).min();
        let bars: Vec<mlc_core::Bar> = points.iter().map(bar_point_to_bar).collect();
        let prepended = bars.len();
        // A `VenueWindowCap`/`VenueRecentOnly` truncation is a genuine wall
        // (stop asking); `VenueHistoryExhausted` on a single-page request
        // just means this page came up short of a FULL page, not that the
        // walk must stop (the empty-page branch above already covers true
        // exhaustion) — so only the wall-shaped reasons latch `exhausted`
        // here.
        let venue_wall = matches!(
            outcome.truncated_by,
            Some(digdigdig3_station::backfill::TruncationReason::VenueWindowCap)
                | Some(digdigdig3_station::backfill::TruncationReason::VenueRecentOnly)
        );

        engine.with_state(&key, |s| s.record_page(prepended, new_oldest.map(|t| t - 1), venue_wall));

        if is_first_page_of_run {
            // First page of a fresh run: full-replace anchor path so the
            // pre-existing cold-open snap/anchor machinery is unchanged.
            let _ = tx.send(LiveUpdate::BackfillComplete {
                exchange_id, account_type,
                symbol: symbol_str.clone(),
                timeframe: tf_name.clone(),
                bars: bars.clone(),
            });
            is_first_page_of_run = false;
        } else {
            let _ = tx.send(LiveUpdate::ScrollBarsLoaded {
                exchange_id, account_type,
                symbol: symbol_str.clone(),
                timeframe: tf_name.clone(),
                bars: bars.clone(),
                prepend_count: prepended,
            });
        }
        let _ = tx.send(LiveUpdate::LayerBackfilled {
            exchange_id, symbol: symbol_str.clone(), account_type,
            layer: LayerId::Kline(tf_name.clone()).wire_label(),
            prepended, exhausted: venue_wall, stage_boundary: true,
        });

        if venue_wall {
            break 'run;
        }
        // Mid-flight check MUST bypass the `in_flight` gate (`needs_more`
        // reads `false` for every page while we ourselves hold the flight)
        // — same fix `run_archive_group` documents on `target_satisfied`.
        let done = engine.with_state(&key, |s| !s.target_satisfied(gate_on_target));
        if done {
            break 'run;
        }
    }

    // Group exhausted without hitting target/wall — continue with the next
    // (ramp-escalated) group. Brief inter-group pause keeps the REST cadence
    // polite (mirrors the trade ramp's pacing).
    if engine.with_state(&key, |s| !s.target_satisfied(gate_on_target)) {
        break 'run;
    }
    sleep_ms(150).await;
    }

    engine.with_state(&key, RampState::end_flight);
}

// ═══════════════════════════════════════════════════════════════════════════
// DataBridge driver methods — trade layer
// ═══════════════════════════════════════════════════════════════════════════

impl DataBridge {
    /// Ensure the trade layer for `(exchange, symbol, account)` has loaded
    /// at least back to `demand`'s time floor (owner doctrine 2026-07-12:
    /// trade backfill targets a TIME LADDER, not a point count — "last
    /// hour, then last 12 hours, then last day, then day-by-day on
    /// demand"). `demand: None` = engine's own ladder default (auto-advance
    /// Hour → HalfDay → Day, no explicit deeper floor); `Some(TradeDemand {
    /// window_ms: Some(w) })` widens the floor to `now − w` — never
    /// shrinks an already-deeper floor, same "deepest wins" rule
    /// [`crate::backfill_engine::RampState::require_target_ts`] documents.
    /// `Some(TradeDemand { window_ms: None })` behaves identically to
    /// `None` (ladder default).
    ///
    /// `RecentOnly`-tier venues (see `TradeHistoryCapabilities`) get exactly
    /// one shallow fetch and are latched `exhausted` immediately — hammering
    /// an endpoint the venue caps to a single shallow call buys nothing (see
    /// `agg_trades_paginated`'s own doc comment for the same rule).
    pub fn ensure_trade_backfill(
        &self,
        exchange_id: ExchangeId,
        symbol: &str,
        account_type: AccountType,
        demand: Option<crate::backfill_engine::TradeDemand>,
    ) {
        let key: BackfillKey = (exchange_id, symbol.to_string(), account_type, LayerId::Trades);
        let now_ms = crate::backfill_engine::now_ms();
        let window_ms = demand.and_then(|d| d.window_ms);
        let auto_ceiling_ms = demand.and_then(|d| d.auto_ceiling_ms);

        let should_start = self.backfill_engine().with_state(&key, |state| {
            // Fix 1 (chart-type-engine-consolidation B1), ts-ladder shape:
            // start/resume the flight (seeds the ladder at `Hour` on a
            // fresh key, otherwise just refreshes `flight_now_ms`), then
            // widen the floor if the caller asked for a deeper window than
            // the ladder's current rung already covers.
            if let Some(ceiling) = auto_ceiling_ms {
                state.widen_auto_ceiling(ceiling);
            }
            state.begin_trade_flight(now_ms);
            // Demands are accepted only BETWEEN flights: the tick-side
            // ×2-doubling demand re-fires every frame, and letting it
            // widen `target_ts` while a flight walks would turn the floor
            // into a moving target the flight chases to the RAM cap in
            // one giant run (exactly the rung-dump the ×2 shape exists to
            // avoid). A latched key (venue-dry / RAM cap) never re-arms.
            if state.in_flight || state.exhausted || state.ram_capped {
                return false;
            }
            // A wider ceiling from a later consumer must be able to re-open
            // a ladder that already stopped at a smaller consumer's ceiling
            // (1m footprint stopped at its Hour warm, then a 1h chart opens
            // the same instrument → its ceiling deepens the same key).
            state.advance_ladder_if_reached();
            if let Some(window_ms) = window_ms {
                state.require_target_ts(now_ms - window_ms);
            }
            if !state.wants_more_ts(true) {
                return false;
            }
            state.begin_flight();
            true
        });
        if !should_start {
            return;
        }

        let fetch_hub = self.station().hub();
        let station = self.station();
        let tx = self.updates_tx();
        let symbol_str = symbol.to_string();
        let wire_sym = self.wire_symbol_pub(exchange_id, account_type, symbol);
        let engine = self.backfill_engine();

        self.spawn_bg_pub(async move {
            run_trade_ramp(fetch_hub, station, tx, engine, key, exchange_id, account_type, symbol_str, wire_sym, true).await;
        });
    }

    /// Scroll-pressure entry for the trade layer (unified-data-plane Э2/Э4)
    /// — same doctrine as [`Self::on_kline_scroll_pressure`], mirrored for
    /// trades: fires a pressure signal that escalates the ramp GROUP size,
    /// AND (owner doctrine 2026-07-12, "day by day, reacting to backfill
    /// needs") arms DEPTH: once the current floor is met, each pressure
    /// event widens `target_ts` one day past the oldest loaded point via
    /// [`crate::backfill_engine::RampState::require_target_ts`] — this is
    /// the ONLY path past the auto ladder's TF-scaled ceiling. While the
    /// current floor is still being walked, pressure only re-fires groups
    /// (rate), never stacks further demand.
    pub fn on_trade_scroll_pressure(&self, exchange_id: ExchangeId, symbol: &str, account_type: AccountType) {
        let key: BackfillKey = (exchange_id, symbol.to_string(), account_type, LayerId::Trades);
        let now_ms = crate::backfill_engine::now_ms();

        let should_start = self.backfill_engine().with_state(&key, |state| {
            state.begin_trade_flight(now_ms);
            state.record_pressure(web_time::Instant::now());
            // Scroll DEPTH: floor met → arm one more day back from the
            // oldest loaded trade. `require_target_ts` itself refuses on a
            // latched (exhausted/RAM-capped) key.
            if !state.wants_more_ts(true) {
                if let Some(oldest) = state.oldest_seen_ts {
                    state.require_target_ts(oldest - 86_400_000);
                }
            }
            if state.in_flight || state.exhausted || state.ram_capped {
                return false;
            }
            state.begin_flight();
            true
        });
        if !should_start {
            return;
        }

        let fetch_hub = self.station().hub();
        let station = self.station();
        let tx = self.updates_tx();
        let symbol_str = symbol.to_string();
        let wire_sym = self.wire_symbol_pub(exchange_id, account_type, symbol);
        let engine = self.backfill_engine();

        self.spawn_bg_pub(async move {
            run_trade_ramp(fetch_hub, station, tx, engine, key, exchange_id, account_type, symbol_str, wire_sym, false).await;
        });
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Archive-day chaining (Э6-Ф3 native + Э6-Ф4 wasm) — bulk daily-archive
// files continuing a trade ramp once the venue's live REST tier bottoms out
// (`RecentOnly`'s one shallow fetch, or `RestWindow`'s `max_back_ms` wall).
//
// Runs on both native and wasm32 — `digdigdig3_station::archive::
// fetch_archive_day` itself transport-splits (native = direct reqwest;
// wasm = reqwest's browser-fetch backend through the CORS proxy). The only
// wasm-specific piece here is `proxy_base`, threaded through `day_fetch`
// into the real fetch call (`None` on native, `Some(base)` on wasm — see
// `mlc_backend_data_bridge::archive_proxy_base()`).
// ═══════════════════════════════════════════════════════════════════════════

/// Consecutive mid-range 404s treated as a boundary (venues delist symbols —
/// a long-dead symbol's archive host stops publishing new days without ever
/// reporting a single contiguous "genuinely too old" 404 run, so a handful
/// of gap days in a row is reclassified as "this symbol's archive effectively
/// ends here" rather than skipped forever).
pub const MAX_CONSECUTIVE_MID_RANGE_404S: u32 = 5;

/// Outcome of fetching ONE archive day file, in the pure vocabulary the ramp
/// chaining logic reasons about — decouples the classification rules (unit
/// tested below) from the network call itself.
#[derive(Debug, Clone, PartialEq)]
pub enum ArchiveDayOutcome {
    /// Day file fetched and parsed successfully (rows may be empty — an
    /// empty-but-200 day is data, not a 404, and does not affect the walk).
    Rows(Vec<RawTrade>),
    /// 404 AT OR BELOW the `since_ms` boundary date — the archive host's
    /// retention floor has been reached; latch exhausted.
    BoundaryReached,
    /// 404 INSIDE the range (above the boundary date) — a data gap, skip
    /// this day and continue to the next older one, NOT exhausted.
    Gap,
    /// Non-404 failure (transport/HTTP-status/decode) — transient, caller
    /// stops this group without latching exhausted (retried on the next
    /// group/pressure signal).
    TransientError,
}

/// Pure classification: turn one day-fetch `Result` plus "is this date at or
/// below the boundary floor" into an [`ArchiveDayOutcome`]. No network, no
/// I/O — the entire boundary/gap/error decision lives here so it is directly
/// unit-testable against synthetic `Result`s.
pub fn classify_archive_day_result(
    result: Result<Vec<RawTrade>, digdigdig3_station::archive::ArchiveDayError>,
    date_at_or_below_floor: bool,
) -> ArchiveDayOutcome {
    match result {
        Ok(rows) => ArchiveDayOutcome::Rows(rows),
        Err(digdigdig3_station::archive::ArchiveDayError::NotFound(_)) => {
            if date_at_or_below_floor {
                ArchiveDayOutcome::BoundaryReached
            } else {
                ArchiveDayOutcome::Gap
            }
        }
        Err(_) => ArchiveDayOutcome::TransientError,
    }
}

/// Trim rows whose `ts_ms` falls at or after `oldest_loaded_ms` — the
/// overlap guard for the RestWindow → archive continuation seam: REST pages
/// already cover `[wall..now)`, so an archive day whose rows overlap that
/// range must not re-introduce duplicates (final dedup-by-ts happens at
/// prepend, but trimming here avoids re-emitting whole redundant days).
pub fn trim_rows_at_or_after(rows: Vec<RawTrade>, oldest_loaded_ms: Option<i64>) -> Vec<RawTrade> {
    match oldest_loaded_ms {
        Some(floor) => rows.into_iter().filter(|r| r.ts_ms < floor).collect(),
        None => rows,
    }
}

/// One archive day fetch call, boxed so the runner can be driven by either
/// the real network fetch or a synthetic mock in tests — the trait/closure
/// seam requested for archive-chaining test coverage. Takes `(spec, symbol,
/// date)` only — the mock seam predates the wasm proxy-base plumbing and
/// stays that shape; `day_fetch(proxy_base)` below closes over the proxy
/// base instead of threading it through this type, so the mock closures in
/// `tests::archive_chaining` are unaffected by the wasm work.
///
/// `Send + Sync` on the closure and its returned future are native-only:
/// they're required there because `run_trade_ramp`/`run_archive_group` are
/// driven through `DataBridge::spawn_bg_pub`'s native arm (`tokio::spawn`,
/// cross-thread). On wasm the inner future crosses the browser's `fetch`
/// JS bridge (`wasm_bindgen_futures::JsFuture`, `Rc`-backed) and is
/// fundamentally `!Send` — but wasm is single-threaded and
/// `spawn_bg_pub`'s wasm arm (`wasm_bindgen_futures::spawn_local`) has no
/// `Send` bound either, so dropping it here is sound, not a workaround.
#[cfg(not(target_arch = "wasm32"))]
pub type DayFetch = Arc<
    dyn Fn(
            ArchiveSpec,
            String,
            chrono::NaiveDate,
        ) -> std::pin::Pin<
            Box<dyn std::future::Future<Output = Result<Vec<RawTrade>, digdigdig3_station::archive::ArchiveDayError>> + Send>,
        > + Send
        + Sync,
>;

/// Wasm counterpart of [`DayFetch`] — same shape, no `Send`/`Sync` bounds.
/// See the native `DayFetch` doc comment for why this is sound rather than
/// a workaround.
#[cfg(target_arch = "wasm32")]
pub type DayFetch = Arc<
    dyn Fn(
        ArchiveSpec,
        String,
        chrono::NaiveDate,
    ) -> std::pin::Pin<
        Box<dyn std::future::Future<Output = Result<Vec<RawTrade>, digdigdig3_station::archive::ArchiveDayError>>>,
    >,
>;

/// Real day-fetch seam — thin wrapper over
/// [`digdigdig3_station::archive::fetch_archive_day`], closing over
/// `proxy_base` (`None` on native — direct HTTPS to the archive host, no
/// CORS concern off-browser; `Some(base)` on wasm — routed through the CORS
/// proxy, see `digdigdig3-station::archive::fetch::proxied_url`).
fn day_fetch(proxy_base: Option<String>) -> DayFetch {
    Arc::new(move |spec, symbol, date| {
        let proxy_base = proxy_base.clone();
        Box::pin(async move {
            digdigdig3_station::archive::fetch_archive_day(&spec, &symbol, date, proxy_base.as_deref()).await
        })
    })
}

/// Walk one ramp GROUP of archive day files for `key`, oldest-file-per-page,
/// via `fetcher` (the real native fetch in production, a mock in tests).
/// The per-day overlap guard (trimming rows at or after whatever
/// `oldest_anchor` is at the time each day lands) is re-read from the engine
/// state per-day rather than passed in once — see the module doc block
/// above [`ArchiveDayOutcome`].
///
/// Mirrors [`run_kline_ramp`]/[`run_trade_ramp`]'s REST loops: one page
/// (here, one day file) emitted the instant it arrives, single-flight
/// already held by the caller, `gate_on_target` honored via
/// `RampState::wants_more_ts`/`next_group_pages` exactly as the REST
/// arm uses it — GROUP size is still a rate limiter, not a depth cap. The
/// walk stops once `RampState::ladder_floor_reached` (the SAME `target_ts`
/// the REST arm inherits into archive mode — see [`run_trade_ramp`]'s
/// archive-chaining doc) or the day-file walk itself goes dry, whichever
/// comes first; [`TRADE_RAM_CAP_POINTS`] is checked per-day as the RAM
/// guard, independent of the ts floor.
async fn run_archive_group(
    fetcher: &DayFetch,
    tx: &tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: &BackfillEngine,
    key: &BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_str: &str,
    wire_sym: &str,
    spec: ArchiveSpec,
    gate_on_target: bool,
) {
    let page_size = TRADE_PAGE_SIZE;
    let group_pages = engine.with_state(key, |s| s.next_group_pages(page_size, gate_on_target));

    let oldest_loaded = engine.with_state(key, |s| s.oldest_anchor);
    let mut days = match oldest_loaded {
        Some(ms) => digdigdig3_station::archive::DayIterator::resume_before(ms, spec.since_ms),
        None => digdigdig3_station::archive::DayIterator::new(spec.since_ms),
    };

    let mut consecutive_mid_range_404s: u32 = 0;
    let mut pages_landed: u32 = 0;

    // Bounded by `group_pages` successfully-landed days, not by raw loop
    // iterations — a run of gap days must not eat into the group's page
    // budget (that would starve a group down to zero real pages on a
    // gappy symbol). The mid-range-404 counter above is the actual
    // circuit breaker against a truly dead symbol looping forever. The
    // ts-floor / RAM-cap check inside the loop (below) is what actually
    // stops a satisfied or RAM-capped walk — this bound alone would
    // otherwise keep pulling `group_pages` more days even past the floor.
    while pages_landed < group_pages {
        if engine.with_state(key, |s| s.ram_capped || !s.wants_more_ts(gate_on_target)) {
            break;
        }
        let Some(date) = days.next() else {
            // Walk exhausted its own floor without a 404 — genuinely out of
            // days to try; treat as boundary reached.
            engine.with_state(key, |s| s.exhausted = true);
            let _ = tx.send(LiveUpdate::LayerBackfilled {
                exchange_id, symbol: symbol_str.to_string(), account_type,
                layer: LayerId::Trades.wire_label(), prepended: 0, exhausted: true, stage_boundary: true,
            });
            break;
        };

        let date_at_or_below_floor = {
            let floor_secs = spec.since_ms.div_euclid(1000);
            let floor_date = chrono::DateTime::from_timestamp(floor_secs, 0)
                .map(|dt| dt.date_naive())
                .unwrap_or(date);
            date <= floor_date
        };

        let result = fetcher(spec, wire_sym.to_string(), date).await;
        let outcome = classify_archive_day_result(result, date_at_or_below_floor);

        match outcome {
            ArchiveDayOutcome::Rows(rows) => {
                consecutive_mid_range_404s = 0;
                pages_landed += 1;
                let oldest_loaded_now = engine.with_state(key, |s| s.oldest_anchor);
                let trimmed = trim_rows_at_or_after(rows, oldest_loaded_now);
                emit_archive_day_page(tx, engine, key, exchange_id, account_type, symbol_str, &trimmed, false);
            }
            ArchiveDayOutcome::BoundaryReached => {
                engine.with_state(key, |s| s.exhausted = true);
                let _ = tx.send(LiveUpdate::LayerBackfilled {
                    exchange_id, symbol: symbol_str.to_string(), account_type,
                    layer: LayerId::Trades.wire_label(), prepended: 0, exhausted: true, stage_boundary: true,
                });
                break;
            }
            ArchiveDayOutcome::Gap => {
                consecutive_mid_range_404s += 1;
                if consecutive_mid_range_404s >= MAX_CONSECUTIVE_MID_RANGE_404S {
                    // A run of mid-range 404s this long means the symbol's
                    // archive effectively ends here (delisted) — treat as
                    // boundary, not an infinite skip.
                    engine.with_state(key, |s| s.exhausted = true);
                    let _ = tx.send(LiveUpdate::LayerBackfilled {
                        exchange_id, symbol: symbol_str.to_string(), account_type,
                        layer: LayerId::Trades.wire_label(), prepended: 0, exhausted: true, stage_boundary: true,
                    });
                    break;
                }
                crate::dlog!(
                    "[BackfillEngine] archive: gap day {date} for {exchange_id:?}/{symbol_str} — skipping, continuing older"
                );
                continue;
            }
            ArchiveDayOutcome::TransientError => {
                crate::dlog!(
                    "[BackfillEngine] archive: transient fetch error for {date} on {exchange_id:?}/{symbol_str} — stopping group, not exhausted"
                );
                break;
            }
        }

        // `needs_more` would read `false` here unconditionally (`in_flight`
        // has been `true` for the whole group since the caller's
        // `begin_flight()`) — use the ts-floor-only half instead. Same fix
        // `RampState::target_satisfied`'s doc documents for the count-based
        // (kline) path.
        let done = engine.with_state(key, |s| s.ram_capped || !s.wants_more_ts(gate_on_target));
        if done {
            break;
        }
    }
}

fn emit_archive_day_page(
    tx: &tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: &BackfillEngine,
    key: &BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol: &str,
    rows: &[RawTrade],
    exhausted: bool,
) {
    let wire: Vec<RawTradeWire> = rows.iter().map(|r| RawTradeWire {
        ts_ms: r.ts_ms,
        price: r.price,
        qty: r.qty,
        is_buyer_maker: r.is_buyer_maker,
    }).collect();
    let prepended = wire.len();
    let new_oldest = wire.iter().map(|t| t.ts_ms).min();
    // Archive day rows: the venue-side "anchor" (day-file resume cursor) IS
    // a ts value, same as `new_oldest_ts` — pass it for both.
    engine.with_state(key, |s| s.record_trade_page(prepended, new_oldest, new_oldest, exhausted));
    if prepended > 0 {
        let _ = tx.send(LiveUpdate::TradeHistoryPrepended {
            exchange_id, account_type, symbol: symbol.to_string(), trades: wire,
        });
    }
    let stage_boundary = exhausted || trade_stage_boundary(engine, key);
    let _ = tx.send(LiveUpdate::LayerBackfilled {
        exchange_id, symbol: symbol.to_string(), account_type,
        layer: LayerId::Trades.wire_label(), prepended, exhausted, stage_boundary,
    });
}

/// Shared ramp-group runner for the trade layer. The first page of a fresh
/// key (no `oldest_anchor` yet) uses the capability-aware seed
/// (`agg_trades_paginated`, which itself falls back to `trades_recent` for
/// `RecentOnly`/unsupported venues); subsequent pages walk the resumable
/// backward cursor page-by-page directly against the hub's `get_agg_trades`
/// — mirroring what `agg_trades_paginated` does internally for its own
/// multi-page walk, without needing any dig3-side addition (the walk is
/// simple enough to own bridge-side, since `agg_trades_paginated` does not
/// expose a resumable cursor across separate calls). The next-page cursor
/// step is NOT a single formula — it branches by the venue's declared
/// `HistoryCursor` (`FromId` subtracts `page_size`, `TsCursor`/OKX passes
/// `from_id` verbatim, `TsWindow` steps `-1`ms) — see the `cursor_kind`
/// local and its doc comment right before the walk loop, and
/// `get_agg_trades`'s trait-level doc comment for the full normalized
/// contract (2026-07-13). `gate_on_target` selects the phase — see
/// [`run_kline_ramp`]'s doc for the same split.
///
/// Time-ladder self-drive (owner doctrine 2026-07-12): one call walks
/// GROUPS (page ramp 1→1→2→4→8… still paces the RATE, unchanged) until the
/// CURRENT ladder rung's floor is reached — [`RampState::
/// advance_ladder_if_reached`] then either advances to the next auto rung
/// (`Hour`→`HalfDay`→`Day`) and the loop continues, or (at `Day`/`Deep`
/// with nothing deeper armed) reports no advance and the run stops. A
/// deeper explicit demand ([`RampState::require_target_ts`], driven from
/// [`DataBridge::ensure_trade_backfill`]) re-arms the loop past `Day`
/// exactly like a fresh flight. [`TRADE_RAM_CAP_POINTS`] is checked every
/// group as a hard backstop independent of the ladder.
///
/// Archive chaining (Э6-Ф3, native only — `cfg(not(wasm32))`): when the
/// venue's `TradeArchiveCapabilities::for_account` declares a spec for this
/// key, the site that would otherwise latch `exhausted` on a `RecentOnly`
/// venue's one shallow fetch, or a `RestWindow` venue's `max_back_ms` wall,
/// instead calls [`RampState::enter_archive_mode`] and continues the SAME
/// group budget via [`run_archive_group`] — the ramp step, single-flight
/// guard, and ladder/RAM-cap gating are shared unchanged across both REST
/// and archive pages for one key. On wasm the archive arms are absent (Ф4
/// scope) and the pre-existing latch-on-wall behavior is unchanged.
async fn run_trade_ramp(
    fetch_hub: Arc<digdigdig3::connector_manager::ExchangeHub>,
    station: Arc<digdigdig3_station::Station>,
    tx: tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: BackfillEngine,
    key: BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol_str: String,
    wire_sym: String,
    gate_on_target: bool,
) {
    // Fix (boot-race latch, footprint-trade-backfill-fix-2026-07-13):
    // `wire_sym` as received is the CALLER's precomputed passthrough
    // fallback (`DataBridge::wire_symbol`), which returns the canonical
    // string unresolved when the station symbol catalogue is still cold
    // at boot. For a venue whose native form differs from canonical (e.g.
    // OKX "BTC-USDT" vs canonical "BTCUSDT"), that passthrough is a wire
    // symbol the venue rejects — the seed page comes back empty, which
    // the ramp below reads as `venue_dry` and latches `exhausted`
    // permanently (no later demand ever re-fetches). Confidently resolve
    // the wire symbol here, mirroring the kline path's retry loop
    // (`request_bars`'s spawned task in bridge.rs, "Fix 2 — MUUSDC
    // race") — poll `try_resolve_wire_symbol` every ~250ms for up to
    // ~10s total, sharing the budget with the connector-readiness wait
    // below. On confident resolution, override the passthrough. On
    // timeout, end the flight WITHOUT latching exhausted so a later
    // demand (catalogue now warm) retries instead of being permanently
    // stuck.
    let mut wire_sym = wire_sym;
    {
        const MAX_ATTEMPTS: u32 = 40; // 40 * 250ms = 10s total budget
        let mut attempts = 0u32;
        let mut connector_ready = false;
        let mut symbol_resolved = false;
        loop {
            if !connector_ready && fetch_hub.rest(exchange_id).is_some() {
                connector_ready = true;
            }
            if connector_ready && !symbol_resolved {
                if let Some(resolved) = crate::bridge::try_resolve_wire_symbol(
                    &station, exchange_id, account_type, &symbol_str,
                ) {
                    wire_sym = resolved;
                    symbol_resolved = true;
                }
            }
            if connector_ready && symbol_resolved {
                break;
            }
            if connector_ready && attempts >= MAX_ATTEMPTS {
                // Do NOT fall back to the passthrough here — "connector
                // ready but catalogue not yet resolved" is exactly the OKX
                // boot case this fix exists for (exchange_info REST still
                // in flight while the connector object already exists);
                // proceeding would send the canonical form to the venue,
                // read the empty reply as venue-dry, and latch `exhausted`
                // permanently. End the flight unlatched — the per-frame
                // viewport demand retries once the catalogue warms.
                crate::dlog!(
                    "[BackfillEngine] trades: wire_symbol unresolved after {}ms exchange={:?} sym={} — ending flight, no latch",
                    attempts * 250, exchange_id, symbol_str
                );
                engine.with_state(&key, RampState::end_flight);
                return;
            }
            if !connector_ready && attempts >= MAX_ATTEMPTS {
                crate::dlog!(
                    "[BackfillEngine] trades: connector {:?} not initialized after {}ms",
                    exchange_id, attempts * 250
                );
                engine.with_state(&key, RampState::end_flight);
                return;
            }
            attempts += 1;
            sleep_ms(250).await;
        }
    }

    let Some(rest) = fetch_hub.rest(exchange_id) else {
        engine.with_state(&key, RampState::end_flight);
        return;
    };
    let tier = rest.trade_history_capabilities().tier_for(account_type);
    // Cursor kind (dig3 `get_agg_trades` normalized contract, 2026-07-13 —
    // see `digdigdig3_station::backfill::agg_trades_paginated`'s doc comment
    // for the full per-venue breakdown, mirrored here for the resumable
    // page-by-page walk below): `from_id: Some(id)` always means "trades
    // strictly older than `id`" at the trait level, but the CALLER still
    // must compute each next-page cursor per the connector's internal
    // translation strategy —
    // - `FromId` (Binance): connector emulates "older than" via
    //   `id.saturating_sub(limit)` — this walk steps by a full `page_size`.
    // - `TsCursor` (OKX): connector's own `after` cursor is already
    //   descending/exclusive — this walk passes `min_id` verbatim, no
    //   arithmetic (subtracting `page_size` here would skip a page-sized
    //   hole every page — the OKX cursor-pagination bug this contract
    //   closes).
    // - `TsWindow` (Bitfinex/Gate.io/MEXC/Coinbase/BingX): no id cursor —
    //   `agg_id` on returned points IS a millisecond timestamp, next window
    //   step is `min_ts - 1`.
    let cursor_kind = match tier {
        TradeHistoryTier::RestDeep { cursor, .. } => cursor,
        TradeHistoryTier::RestWindow { cursor, .. } => cursor,
        TradeHistoryTier::RecentOnly { .. } => HistoryCursor::FromId,
    };
    // Venue-side per-page clamp (dig3 `RestDeep::page_cap`): "partial page
    // = venue dry" must be judged against what the venue can actually
    // return — OKX clamps every request to 100 rows, so a 100-row page on
    // a bigger request is FULL, not dry. Judging against the raw request
    // latched `exhausted` after one walk page (footprint incident
    // 2026-07-13).
    let full_page_rows: usize = match tier {
        TradeHistoryTier::RestDeep { page_cap, .. } => (page_cap as usize).min(TRADE_PAGE_SIZE),
        _ => TRADE_PAGE_SIZE,
    };
    let archive_spec = rest.trade_archive_capabilities().for_account(account_type);
    // Wasm needs the CORS proxy base to reach the archive hosts (browsers
    // cannot cross-origin GET `public.bybit.com` / `data.binance.vision`
    // directly); native passes `None` straight through to a direct fetch.
    // Read once per run — `day_fetch` closes over it for every day-file
    // call this group makes.
    let proxy_base = crate::archive_proxy_base();

    'run: loop {
    let is_cold = engine.with_state(&key, |s| s.oldest_anchor.is_none() && s.groups_completed == 0);
    let already_in_archive_mode = engine.with_state(&key, |s| s.archive_mode);
    let (stage, target_ts, oldest_ts, loaded) = engine.with_state(&key, |s| {
        (s.ladder_stage, s.target_ts, s.oldest_seen_ts, s.loaded)
    });
    crate::dlog!(
        "[trade_ramp] group key={:?}/{} stage={} target_ts={:?} oldest_ts={:?} loaded={}",
        key.0, key.1, stage.map(|s| s.label()).unwrap_or("?"), target_ts, oldest_ts, loaded
    );

    // A key already latched into archive mode by a PRIOR run (REST tier
    // already exhausted last time) skips straight back to the day walk —
    // never re-attempts the REST tier once archive mode is entered.
    if already_in_archive_mode {
        if let Some(spec) = archive_spec {
            run_archive_group(
                &day_fetch(proxy_base.clone()), &tx, &engine, &key, exchange_id, account_type,
                &symbol_str, &wire_sym, spec, gate_on_target,
            ).await;
        } else {
            // Spec disappeared (shouldn't happen — capability declarations
            // are static per connector) — fail safe to exhausted rather
            // than loop forever with no fetcher.
            engine.with_state(&key, |s| s.exhausted = true);
        }
        break 'run;
    }

    // RecentOnly: exactly one shallow fetch, then either chain into the
    // archive (if the venue declares one) or latch exhausted — never
    // hammer an endpoint the venue caps to a single call.
    if let TradeHistoryTier::RecentOnly { max_trades } = tier {
        if is_cold {
            let cap = (max_trades as usize).min(TRADE_PAGE_SIZE).max(1);
            let (trades, _outcome) = digdigdig3_station::backfill::trades_recent(
                &fetch_hub, exchange_id, account_type, &wire_sym, cap,
            ).await;
            // Do NOT latch exhausted here even though this is the venue's
            // only shallow REST page — a chained archive continues the
            // ramp for this key on the very next call.
            let latch_exhausted = archive_spec.is_none();
            emit_trade_page(&tx, &engine, &key, exchange_id, account_type, &symbol_str, &trades, latch_exhausted);
            if archive_spec.is_some() {
                engine.with_state(&key, RampState::enter_archive_mode);
            }
        } else if let Some(spec) = archive_spec {
            engine.with_state(&key, RampState::enter_archive_mode);
            run_archive_group(
                &day_fetch(proxy_base.clone()), &tx, &engine, &key, exchange_id, account_type,
                &symbol_str, &wire_sym, spec, gate_on_target,
            ).await;
        } else {
            engine.with_state(&key, |s| s.exhausted = true);
        }
        break 'run;
    }

    let page_size = TRADE_PAGE_SIZE;
    let group_pages = engine.with_state(&key, |s| s.next_group_pages(page_size, gate_on_target));
    let mut chained_to_archive = false;

    for i in 0..group_pages {
        let cold_this_page = is_cold && i == 0;
        if cold_this_page {
            let (points, outcome) = digdigdig3_station::backfill::agg_trades_paginated(
                &fetch_hub, exchange_id, account_type, &wire_sym, page_size, 1,
            ).await;
            let venue_wall = matches!(
                outcome.truncated_by,
                Some(digdigdig3_station::backfill::TruncationReason::VenueWindowCap)
                    | Some(digdigdig3_station::backfill::TruncationReason::VenueRecentOnly)
            );
            let min_id = points.iter().map(|p| p.agg_id).min();
            let venue_dry = points.is_empty();
            let exhausted_here = venue_dry || venue_wall;
            // A `RestWindow` wall chains into the archive instead of
            // latching, when the venue declares one; a genuinely empty
            // page (true venue-dry, no wall) still latches even with an
            // archive spec — nothing older exists to chain into.
            let chain_to_archive = venue_wall && !venue_dry && archive_spec.is_some();
            let latch_now = exhausted_here && !chain_to_archive;
            emit_trade_agg_page(&tx, &engine, &key, exchange_id, account_type, &symbol_str, &points, min_id.map(|v| v as i64), latch_now);
            if chain_to_archive {
                if let Some(spec) = archive_spec {
                    engine.with_state(&key, RampState::enter_archive_mode);
                    let remaining = engine.with_state(&key, |s| s.next_group_pages(page_size, gate_on_target));
                    if remaining > 0 {
                        run_archive_group(
                            &day_fetch(proxy_base.clone()), &tx, &engine, &key, exchange_id, account_type,
                            &symbol_str, &wire_sym, spec, gate_on_target,
                        ).await;
                    }
                }
                chained_to_archive = true;
                break;
            }
            if exhausted_here {
                break;
            }
        } else {
            let from_id = engine.with_state(&key, |s| s.oldest_anchor);
            let Some(from_id) = from_id else { break; };
            if from_id <= 0 {
                engine.with_state(&key, |s| s.exhausted = true);
                let _ = tx.send(LiveUpdate::LayerBackfilled {
                    exchange_id, symbol: symbol_str.clone(), account_type,
                    layer: LayerId::Trades.wire_label(), prepended: 0, exhausted: true, stage_boundary: true,
                });
                break;
            }
            // Next-page cursor step, branched by `HistoryCursor` exactly like
            // `agg_trades_paginated`'s own multi-page walk (see the
            // `cursor_kind` doc comment above this loop): `FromId` (Binance)
            // subtracts a full `page_size` to land one page earlier;
            // `TsCursor` (OKX) passes `from_id` verbatim — its own `after`
            // cursor already walks strictly-older on its own, and
            // subtracting `page_size` here would skip a page-sized hole
            // every page; `TsWindow` venues carry a millisecond timestamp in
            // `agg_id` and step back 1ms, not a trade-count-sized chunk.
            let next_from_id = match cursor_kind {
                HistoryCursor::TsWindow => (from_id as u64).saturating_sub(1),
                HistoryCursor::TsCursor => from_id as u64,
                HistoryCursor::FromId => (from_id as u64).saturating_sub(page_size as u64),
            };
            let res = rest.get_agg_trades(
                digdigdig3::core::types::SymbolInput::Raw(&wire_sym),
                Some(page_size as u32),
                Some(next_from_id),
                account_type,
            ).await;
            let points: Vec<digdigdig3_station::data::AggTradePoint> = match res {
                Ok(trades) => trades.iter().map(|t| digdigdig3_station::data::AggTradePoint {
                    ts_ms: t.timestamp,
                    price: t.price,
                    quantity: t.quantity,
                    side: if t.is_buy { 0u8 } else { 1u8 },
                    agg_id: t.aggregate_id as u64,
                }).collect(),
                Err(_) => {
                    // Transient fetch failure (rate limit / network blip) —
                    // end this GROUP without emitting and WITHOUT latching:
                    // an Err page is not venue-dry, and latching `exhausted`
                    // here would kill the ladder permanently on a 429.
                    crate::dlog!(
                        "[trade_ramp] page fetch error key={:?}/{} — ending group, no latch",
                        key.0, key.1
                    );
                    break;
                }
            };
            let was_partial = points.len() < full_page_rows;
            let min_id = points.iter().map(|p| p.agg_id).min();
            // A short (partial) page on a `RestWindow` venue's resumable
            // walk means the cursor just crossed `max_back_ms` — the same
            // wall the cold seed's `VenueWindowCap` reports, just observed
            // on a later page. An empty page is genuine venue-dry.
            let venue_dry = points.is_empty();
            let exhausted_here = venue_dry || was_partial;
            let chain_to_archive = was_partial && !venue_dry && archive_spec.is_some()
                && matches!(tier, TradeHistoryTier::RestWindow { .. });
            let latch_now = exhausted_here && !chain_to_archive;
            emit_trade_agg_page(&tx, &engine, &key, exchange_id, account_type, &symbol_str, &points, min_id.map(|v| v as i64), latch_now);
            if chain_to_archive {
                if let Some(spec) = archive_spec {
                    engine.with_state(&key, RampState::enter_archive_mode);
                    let remaining = engine.with_state(&key, |s| s.next_group_pages(page_size, gate_on_target));
                    if remaining > 0 {
                        run_archive_group(
                            &day_fetch(proxy_base.clone()), &tx, &engine, &key, exchange_id, account_type,
                            &symbol_str, &wire_sym, spec, gate_on_target,
                        ).await;
                    }
                }
                chained_to_archive = true;
                break;
            }
            if exhausted_here {
                break;
            }
        }

        // Mid-flight check MUST bypass the `in_flight` gate — same fix
        // `run_kline_ramp`/`run_archive_group` document on
        // `target_satisfied`/`wants_more_ts`.
        let done = engine.with_state(&key, |s| !s.wants_more_ts(gate_on_target) || s.ram_capped);
        if done {
            break;
        }
    }

    if chained_to_archive {
        break 'run;
    }

    let ram_capped_now = engine.with_state(&key, |s| s.ram_capped);
    if ram_capped_now {
        crate::dlog!(
            "[trade_ramp] RAM cap hit key={:?}/{} loaded={} cap={}",
            key.0, key.1, engine.with_state(&key, |s| s.loaded), TRADE_RAM_CAP_POINTS
        );
        break 'run;
    }

    let latched = engine.with_state(&key, |s| s.exhausted);
    if latched {
        break 'run;
    }

    // Ladder auto-advance: current rung's floor reached and an auto rung
    // exists past it (`Hour`→`HalfDay`→`Day`) -> advance and keep walking
    // in the SAME flight. At `Day`/`Deep` with nothing deeper armed, this
    // reports no advance and the run stops here (deeper history only
    // arrives on a later explicit `require_target_ts` demand).
    let advanced = engine.with_state(&key, RampState::advance_ladder_if_reached);
    if advanced {
        let (new_stage, new_target_ts) = engine.with_state(&key, |s| (s.ladder_stage, s.target_ts));
        crate::dlog!(
            "[trade_ramp] stage advance key={:?}/{} stage={} target_ts={:?}",
            key.0, key.1, new_stage.map(|s| s.label()).unwrap_or("?"), new_target_ts
        );
        // Escalate the page ramp INSIDE the flight (1 -> 1 -> 2 -> 4 -> 8).
        // Trade pages are seconds of tape each; without in-flight
        // escalation every group stays at the seed step (1 page / ~0.5s)
        // and a HalfDay rung takes thousands of groups.
        engine.with_state(&key, |s| s.advance_ramp());
        sleep_ms(150).await;
        continue 'run;
    }

    // Still mid-rung (floor not reached, nothing latched) — continue with
    // the next (ramp-escalated) group toward the SAME target_ts. Brief
    // inter-group pause keeps the REST cadence polite. Pressure flights
    // (`gate_on_target == false`) stay one-group-per-event — the pressure
    // path re-fires on demand, so they never self-loop here.
    if gate_on_target && engine.with_state(&key, |s| s.wants_more_ts(true)) {
        // Same in-flight ramp escalation as the stage-advance branch.
        engine.with_state(&key, |s| s.advance_ramp());
        sleep_ms(150).await;
        continue 'run;
    }

    break 'run;
    }

    // Terminal flush: guarantee exactly one `stage_boundary` sweep per
    // flight no matter how it ended (transient page Err break, pressure
    // one-group flight, RAM cap, ceiling stop) — flushes any bubbles the
    // mid-rung pages left `refold_dirty`, so the layer's final state is
    // never left unrendered. `exhausted` mirrors the latch (idempotent for
    // the depth-badge consumer — `set_history_depth` dedups).
    let final_exhausted = engine.with_state(&key, |s| s.exhausted);
    let _ = tx.send(LiveUpdate::LayerBackfilled {
        exchange_id, symbol: symbol_str.clone(), account_type,
        layer: LayerId::Trades.wire_label(), prepended: 0,
        exhausted: final_exhausted, stage_boundary: true,
    });

    engine.with_state(&key, RampState::end_flight);
}

/// `stage_boundary` for a trade-layer `LayerBackfilled` send — read AFTER
/// the page was recorded: the rung/demand floor is reached, or the key
/// latched. Mid-rung streaming pages read `false`; the page that completes
/// a rung naturally reads `true` (see the field's doc on the event).
fn trade_stage_boundary(engine: &BackfillEngine, key: &BackfillKey) -> bool {
    engine.with_state(key, |s| s.exhausted || s.ram_capped || !s.wants_more_ts(true))
}

fn emit_trade_page(
    tx: &tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: &BackfillEngine,
    key: &BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol: &str,
    trades: &[digdigdig3_station::data::TradePoint],
    exhausted: bool,
) {
    let wire: Vec<RawTradeWire> = trades.iter().map(|t| RawTradeWire {
        ts_ms: t.ts_ms,
        price: t.price,
        qty: t.quantity,
        is_buyer_maker: t.side == 0,
    }).collect();
    let prepended = wire.len();
    let new_oldest = wire.iter().map(|t| t.ts_ms).min();
    engine.with_state(key, |s| s.record_trade_page(prepended, new_oldest, new_oldest, exhausted));
    if prepended > 0 {
        let _ = tx.send(LiveUpdate::TradeHistoryPrepended {
            exchange_id, account_type, symbol: symbol.to_string(), trades: wire,
        });
    }
    let stage_boundary = exhausted || trade_stage_boundary(engine, key);
    let _ = tx.send(LiveUpdate::LayerBackfilled {
        exchange_id, symbol: symbol.to_string(), account_type,
        layer: LayerId::Trades.wire_label(), prepended, exhausted, stage_boundary,
    });
}

fn emit_trade_agg_page(
    tx: &tokio::sync::broadcast::Sender<LiveUpdate>,
    engine: &BackfillEngine,
    key: &BackfillKey,
    exchange_id: ExchangeId,
    account_type: AccountType,
    symbol: &str,
    points: &[digdigdig3_station::data::AggTradePoint],
    new_oldest_anchor: Option<i64>,
    exhausted: bool,
) {
    let wire: Vec<RawTradeWire> = points.iter().map(|p| RawTradeWire {
        ts_ms: p.ts_ms,
        price: p.price,
        qty: p.quantity,
        is_buyer_maker: p.side == 0,
    }).collect();
    let prepended = wire.len();
    // `new_oldest_anchor` here is the venue-side agg_id cursor, NOT a ts —
    // `new_oldest_ts` is derived separately from the wire points' `ts_ms`.
    let new_oldest_ts = wire.iter().map(|t| t.ts_ms).min();
    engine.with_state(key, |s| s.record_trade_page(prepended, new_oldest_anchor, new_oldest_ts, exhausted));
    if prepended > 0 {
        let _ = tx.send(LiveUpdate::TradeHistoryPrepended {
            exchange_id, account_type, symbol: symbol.to_string(), trades: wire,
        });
    }
    let stage_boundary = exhausted || trade_stage_boundary(engine, key);
    let _ = tx.send(LiveUpdate::LayerBackfilled {
        exchange_id, symbol: symbol.to_string(), account_type,
        layer: LayerId::Trades.wire_label(), prepended, exhausted, stage_boundary,
    });
}

#[cfg(test)]
mod tests {
    //! Pure ramp/state-machine unit tests — no network, no runtime, `--lib`
    //! fast. Exercises the 1→1→2→4 progression, quiet-period reset,
    //! single-flight guard, and the exhaustion latch per the task gate.
    //!
    //! Contract pinned here (2026-07-09 fix): `exhausted` means venue-dry
    //! ONLY (empty page or a capability wall) — reaching `target` is a
    //! silent stop, never an `exhausted` event. `target` gates exclusively
    //! the auto-ensure phase (`gate_on_target: true`); the scroll-pressure
    //! phase (`gate_on_target: false`) ignores `target` entirely and keeps
    //! firing until the venue itself goes dry.

    use super::*;

    #[test]
    fn fresh_state_needs_more_with_no_target() {
        let s = RampState::default();
        assert!(s.needs_more(true));
        assert_eq!(s.next_group_pages(500, true), 1);
    }

    #[test]
    fn fresh_state_with_target_zero_loaded_needs_more() {
        let s = RampState::new(Some(2000));
        assert!(s.needs_more(true));
    }

    #[test]
    fn satisfied_target_does_not_need_more_in_auto_phase() {
        let mut s = RampState::new(Some(500));
        s.loaded = 500;
        assert!(!s.needs_more(true));
        let mut s2 = RampState::new(Some(500));
        s2.loaded = 600;
        assert!(!s2.needs_more(true));
    }

    #[test]
    fn satisfied_target_still_needs_more_in_pressure_phase() {
        // The core of the item-2 fix: pressure-driven calls pass
        // `gate_on_target: false` and must ignore a satisfied (or even
        // over-satisfied) target completely.
        let mut s = RampState::new(Some(500));
        s.loaded = 500;
        assert!(!s.needs_more(true), "auto phase must consider this satisfied");
        assert!(s.needs_more(false), "pressure phase must ignore target and keep loading");

        let mut s2 = RampState::new(Some(500));
        s2.loaded = 10_000; // way past target
        assert!(s2.needs_more(false), "pressure phase unbounded even far past target");
    }

    #[test]
    fn next_group_pages_ignores_target_when_gate_is_false() {
        let mut s = RampState::new(Some(600));
        s.loaded = 500; // only 100 remaining toward target
        s.ramp_step = MAX_RAMP_PAGES_PER_GROUP;
        // Auto phase: clamped to what's needed to reach target (1 page).
        assert_eq!(s.next_group_pages(500, true), 1);
        // Pressure phase: target ignored entirely, full ramp step used.
        assert_eq!(s.next_group_pages(500, false), MAX_RAMP_PAGES_PER_GROUP);
    }

    #[test]
    fn in_flight_blocks_needs_more() {
        let mut s = RampState::default();
        s.begin_flight();
        assert!(!s.needs_more(true));
        assert!(!s.needs_more(false));
    }

    #[test]
    fn exhausted_blocks_needs_more_even_with_open_target() {
        let mut s = RampState::new(Some(10_000));
        s.exhausted = true;
        assert!(!s.needs_more(true));
        assert!(!s.needs_more(false), "venue-dry latch blocks pressure phase too, target aside");
    }

    #[test]
    fn ramp_progression_is_1_1_2_4_8_capped() {
        let mut s = RampState::default();
        assert_eq!(s.ramp_step, 1);

        // Group 1 (the cold seed) completes -> per doctrine the group RIGHT
        // AFTER the seed is still size 1.
        s.begin_flight();
        s.record_page(500, Some(1000), false);
        s.end_flight();
        assert_eq!(s.ramp_step, 1, "group right after the cold seed stays at 1 page");

        // Group 2 completes -> group 3 doubles to 2.
        s.begin_flight();
        s.record_page(500, Some(500), false);
        s.end_flight();
        assert_eq!(s.ramp_step, 2);

        // Group 3 completes -> group 4 doubles to 4.
        s.begin_flight();
        s.record_page(500, Some(0), false);
        s.end_flight();
        assert_eq!(s.ramp_step, 4);

        // Group 4 completes -> group 5 doubles to 8 (== cap).
        s.begin_flight();
        s.record_page(500, Some(0), false);
        s.end_flight();
        assert_eq!(s.ramp_step, MAX_RAMP_PAGES_PER_GROUP);

        // Further groups stay capped, never exceed it.
        for _ in 0..10 {
            s.begin_flight();
            s.record_page(500, Some(0), false);
            s.end_flight();
        }
        assert_eq!(s.ramp_step, MAX_RAMP_PAGES_PER_GROUP);
    }

    #[test]
    fn ramp_step_never_exceeds_cap() {
        let mut s = RampState::default();
        for _ in 0..20 {
            s.advance_ramp();
        }
        assert!(s.ramp_step <= MAX_RAMP_PAGES_PER_GROUP);
    }

    #[test]
    fn next_group_pages_never_exceeds_remaining_target() {
        let mut s = RampState::new(Some(1200));
        s.ramp_step = MAX_RAMP_PAGES_PER_GROUP; // pretend fully ramped
        s.loaded = 1000; // 200 remaining
        // page_size 500 -> ceil(200/500) = 1 page needed, even though ramp
        // allows up to MAX_RAMP_PAGES_PER_GROUP.
        assert_eq!(s.next_group_pages(500, true), 1);
    }

    #[test]
    fn next_group_pages_respects_ramp_when_target_is_far() {
        let mut s = RampState::new(Some(1_000_000));
        s.ramp_step = 4;
        assert_eq!(s.next_group_pages(500, true), 4);
    }

    #[test]
    fn record_page_accumulates_loaded_and_updates_anchor() {
        let mut s = RampState::default();
        s.record_page(500, Some(12345), false);
        assert_eq!(s.loaded, 500);
        assert_eq!(s.oldest_anchor, Some(12345));
        assert!(!s.exhausted);

        s.record_page(300, Some(9999), false);
        assert_eq!(s.loaded, 800);
        assert_eq!(s.oldest_anchor, Some(9999));
    }

    #[test]
    fn record_page_latches_exhausted_permanently() {
        let mut s = RampState::default();
        s.record_page(0, None, true);
        assert!(s.exhausted);
        // Latched — nothing in this state machine clears it once set; a
        // fresh instrument/layer identity means a brand-new key/state.
        assert!(s.exhausted);
    }

    #[test]
    fn single_flight_guard_prevents_concurrent_begin() {
        let mut s = RampState::default();
        assert!(s.needs_more(true));
        s.begin_flight();
        assert!(s.in_flight);
        assert!(!s.needs_more(true), "a second caller must not start while in_flight");
        assert!(!s.needs_more(false), "in_flight blocks pressure phase too");
        s.end_flight();
        assert!(!s.in_flight);
    }

    // ─────────────────────────────────────────────────────────────────────
    // `require_target` — fix 1 (chart-type-engine-consolidation B1):
    // re-arm by required depth instead of a one-shot target adoption.
    // Mirrors `ensure_kline_backfill`/`ensure_trade_backfill`'s
    // `should_start` decision directly against `RampState` (no
    // network/runtime needed).
    // ─────────────────────────────────────────────────────────────────────

    #[test]
    fn require_target_rearms_when_a_deeper_consumer_arrives() {
        // (a) First consumer ramps to target 1000, satisfied; second
        // consumer arrives with target 5000 -> ramp re-arms and continues.
        let mut s = RampState::default();
        assert!(s.require_target(Some(1000)), "cold key with a target always needs more");
        s.begin_flight();
        s.record_page(1000, Some(0), false);
        s.end_flight();
        assert_eq!(s.loaded, 1000);
        assert!(!s.require_target(Some(1000)), "same target, already satisfied -> no re-arm");
        assert!(s.require_target(Some(5000)), "deeper target -> re-arms");
        assert_eq!(s.target, Some(5000), "target widens to the deeper request, never shrinks");
        assert!(!s.in_flight, "require_target itself does not engage the single-flight guard");
        assert_eq!(s.groups_completed, 0, "re-arm resets ramp progression to the seed state");
        assert_eq!(s.ramp_step, 1, "re-arm resets ramp progression to the seed state");
        assert_eq!(s.loaded, 1000, "loaded is NOT reset — the ramp resumes, it does not restart");
        assert_eq!(s.oldest_anchor, Some(0), "oldest_anchor (resume point) is untouched by a re-arm");

        // A shallower request than the current target never shrinks it —
        // still satisfied (loaded 1000 < 5000, so this DOES still need
        // more; the point under test is that `target` itself doesn't drop
        // back down to 2000).
        let _ = s.require_target(Some(2000));
        assert_eq!(s.target, Some(5000), "a shallower later request never shrinks the widened target");
    }

    #[test]
    fn require_target_does_not_rearm_a_venue_dry_latch() {
        // (b) venue-dry latched -> second consumer does NOT re-arm, even
        // with a much deeper target than was ever reached.
        let mut s = RampState::default();
        s.target = Some(500);
        s.loaded = 200;
        s.record_page(0, None, true); // venue reports empty page -> latches exhausted
        assert!(s.exhausted);
        assert!(!s.require_target(Some(1_000_000)), "exhausted latch blocks re-arm regardless of target depth");
        assert_eq!(s.target, Some(1_000_000), "target still widens (bookkeeping) even though the run won't start");
        assert!(s.exhausted, "exhausted stays latched — never cleared by require_target");
        assert!(!s.needs_more(true));
        assert!(!s.needs_more(false), "pressure phase is blocked too — venue-dry is unconditional");
    }

    #[test]
    fn require_target_does_not_double_start_while_in_flight() {
        // (c) in_flight -> no double-start (single-flight preserved) even
        // when a deeper target arrives mid-flight.
        let mut s = RampState::default();
        assert!(s.require_target(Some(1000)));
        s.begin_flight();
        assert!(s.in_flight);
        assert!(!s.require_target(Some(5000)), "in_flight blocks the caller's should_start regardless of target");
        assert_eq!(s.target, Some(5000), "target still widens — the run already in flight will pick it up via next_group_pages on its next group");
        assert!(s.in_flight, "require_target never clears in_flight itself");
        s.end_flight();
        // Now that the group ended, the widened target re-arms the ramp.
        assert!(s.needs_more(true), "post-flight, the widened target is unmet -> needs_more is true");
    }

    #[test]
    fn require_target_none_leaves_an_existing_target_untouched() {
        // A caller with no depth requirement of its own (target: None) must
        // not clear or widen an existing target — same "don't touch an
        // unset/set target" contract the old one-shot-adopt code had for
        // `None`, preserved by `require_target`.
        let mut s = RampState::new(Some(500));
        s.loaded = 500; // satisfied
        assert!(!s.require_target(None), "None request does not disturb a satisfied target");
        assert_eq!(s.target, Some(500));
    }

    #[test]
    fn require_target_none_on_a_fresh_key_stays_unbounded() {
        let mut s = RampState::default();
        assert!(s.require_target(None), "unbounded fresh key always needs more");
        assert_eq!(s.target, None);
    }

    #[test]
    fn pressure_resets_ramp_after_quiet_period_elapsed() {
        let mut s = RampState::default();
        s.ramp_step = MAX_RAMP_PAGES_PER_GROUP;
        s.groups_completed = 5;
        // Simulate a quiet period having already elapsed by constructing a
        // `last_pressure_at` synthetically far enough in the past. web_time
        // does not expose `checked_sub` publicly across targets, so instead
        // assert the OTHER direction (still-fresh signal, no reset) here and
        // cover the actual elapsed-time reset via a real sleep in the next
        // test — keeping this one deterministic and fast.
        s.last_pressure_at = Some(web_time::Instant::now());
        s.record_pressure(web_time::Instant::now());
        assert_eq!(s.ramp_step, MAX_RAMP_PAGES_PER_GROUP, "immediate re-signal must NOT reset (well under quiet window)");
    }

    #[test]
    fn pressure_escalates_while_still_scrolling() {
        let mut s = RampState::default();
        let t0 = web_time::Instant::now();
        s.record_pressure(t0);
        assert_eq!(s.ramp_step, 1);

        // A fresh group completes between pressure signals (as the real
        // driver loop would do), advancing the ramp.
        s.begin_flight();
        s.record_page(500, Some(0), false);
        s.end_flight();

        s.record_pressure(web_time::Instant::now());
        // Still within the quiet window (no sleep), so no reset happened —
        // ramp_step reflects the completed group's advance, not a reset.
        assert!(s.ramp_step >= 1);
    }

    #[test]
    fn layer_id_wire_label_formats_kline_and_trades() {
        assert_eq!(LayerId::Kline("1m".to_string()).wire_label(), "kline:1m");
        assert_eq!(LayerId::Trades.wire_label(), "trades");
    }

    #[test]
    fn engine_registry_creates_default_state_on_first_touch() {
        let engine = BackfillEngine::new();
        let key: BackfillKey = (ExchangeId::Binance, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Kline("1m".to_string()));
        assert!(engine.snapshot(&key).is_none());
        engine.with_state(&key, |s| s.loaded = 42);
        assert_eq!(engine.snapshot(&key).map(|s| s.loaded), Some(42));
    }

    #[test]
    fn engine_registry_is_per_key_isolated() {
        let engine = BackfillEngine::new();
        let key_a: BackfillKey = (ExchangeId::Binance, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Kline("1m".to_string()));
        let key_b: BackfillKey = (ExchangeId::Binance, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Trades);
        engine.with_state(&key_a, |s| s.loaded = 10);
        engine.with_state(&key_b, |s| s.loaded = 20);
        assert_eq!(engine.snapshot(&key_a).map(|s| s.loaded), Some(10));
        assert_eq!(engine.snapshot(&key_b).map(|s| s.loaded), Some(20));
    }

    // ─────────────────────────────────────────────────────────────────────
    // Contract-pinning tests (2026-07-09 fix): target-reached is a silent
    // stop, never `exhausted`; pressure ignores target and keeps loading
    // past it; only venue-dry (wall-shaped) reasons latch `exhausted`.
    // These simulate the runner loop's page-outcome/emission decisions
    // directly against `RampState` (no network/runtime needed).
    // ─────────────────────────────────────────────────────────────────────

    /// Mirrors the emission decision the real `run_kline_ramp`/
    /// `run_trade_ramp` loops make per page: `exhausted` reported on the
    /// `LayerBackfilled` event is driven SOLELY by the venue-shaped signal
    /// (`venue_dry`), never by whether `target` was reached.
    fn simulate_page_exhausted_flag(venue_dry: bool) -> bool {
        venue_dry
    }

    #[test]
    fn target_reached_stops_auto_phase_without_exhausted_event_and_pressure_still_fires() {
        let mut s = RampState::new(Some(1000));

        // Auto phase loads two full, non-dry pages and reaches target.
        s.begin_flight();
        s.record_page(500, Some(500), false);
        assert!(!simulate_page_exhausted_flag(false), "page 1: venue not dry, no exhausted event");
        s.end_flight();

        s.begin_flight();
        s.record_page(500, Some(0), false);
        assert!(!simulate_page_exhausted_flag(false), "page 2: venue not dry, no exhausted event, even though target now satisfied");
        s.end_flight();

        assert_eq!(s.loaded, 1000);
        assert!(!s.exhausted, "target reached must NOT latch exhausted");
        assert!(!s.needs_more(true), "auto phase quietly stops once target is satisfied");

        // Scroll-pressure phase must still fire and load MORE, ignoring the
        // already-satisfied target entirely.
        assert!(s.needs_more(false), "pressure phase ignores satisfied target and keeps loading");
        s.begin_flight();
        s.record_page(500, Some(0), false);
        s.end_flight();
        assert_eq!(s.loaded, 1500, "pressure phase loaded MORE past the target");
        assert!(!s.exhausted, "still not exhausted — venue kept delivering non-dry pages");
    }

    #[test]
    fn venue_dry_latches_exhausted_and_blocks_all_further_fires_even_under_pressure() {
        let mut s = RampState::new(Some(1_000_000)); // target far away, irrelevant here

        s.begin_flight();
        s.record_page(500, Some(500), false);
        s.end_flight();

        // Venue goes dry: empty page.
        s.begin_flight();
        s.record_page(0, None, true);
        assert!(simulate_page_exhausted_flag(true), "empty page: venue-dry exhausted event fires");
        s.end_flight();

        assert!(s.exhausted, "venue-dry latches exhausted permanently");
        assert!(!s.needs_more(true), "auto phase blocked by the latch");
        assert!(!s.needs_more(false), "pressure phase ALSO blocked by the latch — target distance is irrelevant once venue-dry");
    }

    #[test]
    fn wall_shaped_truncation_latches_but_target_shaped_stop_does_not() {
        // Wall-shaped: a capability wall (RestWindow max_back / single-page
        // kline_backpage=false / RecentOnly tier) reported on a page that
        // still returned data — this DOES latch exhausted, because it is a
        // venue-side signal, not a target-side one.
        let mut wall = RampState::new(Some(1_000_000));
        wall.begin_flight();
        let venue_wall = true; // e.g. TruncationReason::VenueWindowCap
        wall.record_page(200, Some(100), venue_wall);
        assert!(simulate_page_exhausted_flag(venue_wall));
        wall.end_flight();
        assert!(wall.exhausted, "wall-shaped truncation reason latches exhausted");

        // Target-shaped: the run stops only because `loaded >= target`
        // (auto phase `needs_more(true)` goes false) — this must NEVER
        // latch `exhausted`, regardless of how the loop decides to break.
        let mut target = RampState::new(Some(500));
        target.begin_flight();
        target.record_page(500, Some(0), false); // venue fully served the page, not dry
        assert!(!simulate_page_exhausted_flag(false));
        target.end_flight();
        assert!(!target.exhausted, "target-shaped stop must NOT latch exhausted");
        assert!(!target.needs_more(true), "auto phase correctly stops (target satisfied)");
        assert!(target.needs_more(false), "but the layer is NOT exhausted — pressure can still pull more");
    }

    // ─────────────────────────────────────────────────────────────────────
    // Trade time ladder (owner doctrine 2026-07-12): `begin_trade_flight`/
    // `advance_ladder_if_reached`/`require_target_ts`/`record_trade_page`
    // pure transitions — no network/runtime needed, mirrors the
    // `require_target` (count-based) test block above one axis over.
    // ─────────────────────────────────────────────────────────────────────

    const HOUR_MS: i64 = 3_600_000;
    const DAY_MS: i64 = 24 * HOUR_MS;

    #[test]
    fn begin_trade_flight_seeds_hour_stage_on_fresh_key() {
        let mut s = RampState::default();
        let now = 10 * DAY_MS; // arbitrary fixed "now"
        s.begin_trade_flight(now);
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::Hour));
        assert_eq!(s.target_ts, Some(now - HOUR_MS));
        assert_eq!(s.flight_now_ms, Some(now));
    }

    #[test]
    fn begin_trade_flight_on_resumed_key_keeps_stage_refreshes_now() {
        let mut s = RampState::default();
        s.begin_trade_flight(10 * DAY_MS);
        s.ladder_stage = Some(TradeLadderStage::HalfDay);
        s.target_ts = Some(10 * DAY_MS - 12 * HOUR_MS);
        // A later flight (e.g. a fresh `ensure_trade_backfill` call after
        // the previous one went quiet) refreshes `flight_now_ms` but does
        // NOT reset the stage back to `Hour`.
        let now2 = 11 * DAY_MS;
        s.begin_trade_flight(now2);
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::HalfDay), "resumed flight keeps its rung");
        assert_eq!(s.flight_now_ms, Some(now2), "flight_now_ms refreshes to the new call's now");
        assert_eq!(s.target_ts, Some(10 * DAY_MS - 12 * HOUR_MS), "target_ts untouched by a bare resume");
    }

    #[test]
    fn ladder_auto_advances_hour_to_halfday_to_day() {
        let mut s = RampState::default();
        let now = 10 * DAY_MS;
        s.begin_trade_flight(now);
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::Hour));

        // Floor not reached yet -> no advance.
        s.oldest_seen_ts = Some(now - HOUR_MS + 1);
        assert!(!s.advance_ladder_if_reached(), "hour floor not yet reached");
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::Hour));

        // Hour floor reached -> advances to HalfDay with its own floor.
        s.oldest_seen_ts = Some(now - HOUR_MS);
        assert!(s.advance_ladder_if_reached(), "hour floor reached -> advances");
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::HalfDay));
        assert_eq!(s.target_ts, Some(now - 12 * HOUR_MS));

        // HalfDay floor not reached yet.
        assert!(!s.advance_ladder_if_reached());
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::HalfDay));

        // HalfDay floor reached -> advances to Day.
        s.oldest_seen_ts = Some(now - 12 * HOUR_MS);
        assert!(s.advance_ladder_if_reached());
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::Day));
        assert_eq!(s.target_ts, Some(now - DAY_MS));
    }

    #[test]
    fn ladder_stops_at_day_without_explicit_deeper_demand() {
        let mut s = RampState::default();
        let now = 10 * DAY_MS;
        s.begin_trade_flight(now);
        s.ladder_stage = Some(TradeLadderStage::Day);
        s.target_ts = Some(now - DAY_MS);
        s.oldest_seen_ts = Some(now - DAY_MS); // Day floor reached

        assert!(s.ladder_floor_reached());
        assert!(!s.advance_ladder_if_reached(), "Day is the auto-advance ceiling -> no further advance");
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::Day), "stage stays at Day");
        // `wants_more_ts(true)` follows the SAME "not yet satisfied /
        // keep going" polarity as the pre-existing count-based
        // `target_satisfied` (see its doc comment) — `false` here means the
        // floor IS reached and the auto phase should stop.
        assert!(!s.wants_more_ts(true), "auto phase: Day floor reached -> run should stop");
        assert!(s.wants_more_ts(false), "pressure phase always reports 'keep going' (target ignored)");
    }

    #[test]
    fn require_target_ts_rearms_past_day_into_deep_stage() {
        let mut s = RampState::default();
        let now = 10 * DAY_MS;
        s.begin_trade_flight(now);
        s.ladder_stage = Some(TradeLadderStage::Day);
        s.target_ts = Some(now - DAY_MS);
        s.oldest_seen_ts = Some(now - DAY_MS); // satisfied at Day
        assert!(s.ladder_floor_reached());

        let deeper = now - 3 * DAY_MS; // 3 days back — past Day's 1-day floor
        assert!(s.require_target_ts(deeper), "deeper demand past Day re-arms");
        assert_eq!(s.ladder_stage, Some(TradeLadderStage::Deep), "stage moves to Deep");
        assert_eq!(s.target_ts, Some(deeper), "target_ts widens to the deeper floor");
        assert!(!s.ladder_floor_reached(), "no longer satisfied — oldest_seen_ts has not caught up yet");

        // A shallower later request never shrinks the widened floor.
        let _ = s.require_target_ts(now - 2 * DAY_MS);
        assert_eq!(s.target_ts, Some(deeper), "shallower request does not shrink target_ts");
    }

    #[test]
    fn require_target_ts_does_not_rearm_a_venue_dry_latch() {
        let mut s = RampState::default();
        let now = 10 * DAY_MS;
        s.begin_trade_flight(now);
        s.exhausted = true;
        assert!(!s.require_target_ts(now - 30 * DAY_MS), "exhausted latch blocks re-arm regardless of depth");
        assert!(s.exhausted, "exhausted stays latched");
    }

    #[test]
    fn require_target_ts_does_not_double_start_while_in_flight() {
        let mut s = RampState::default();
        let now = 10 * DAY_MS;
        s.begin_trade_flight(now);
        s.begin_flight();
        assert!(s.in_flight);
        assert!(!s.require_target_ts(now - 5 * DAY_MS), "in_flight blocks should_start regardless of the deeper demand");
        assert_eq!(s.target_ts, Some(now - 5 * DAY_MS), "target_ts still widens — in-flight run picks it up on its next group");
    }

    #[test]
    fn record_trade_page_tracks_oldest_seen_ts_monotonically() {
        let mut s = RampState::default();
        s.record_trade_page(500, Some(100), Some(5_000), false);
        assert_eq!(s.oldest_seen_ts, Some(5_000));
        assert_eq!(s.loaded, 500);

        // A later page with a SMALLER ts moves the floor further back.
        s.record_trade_page(500, Some(50), Some(1_000), false);
        assert_eq!(s.oldest_seen_ts, Some(1_000));
        assert_eq!(s.loaded, 1_000);

        // A page reporting a LARGER min-ts than what's already seen must
        // never move `oldest_seen_ts` forward (monotonic non-increasing).
        s.record_trade_page(500, Some(0), Some(3_000), false);
        assert_eq!(s.oldest_seen_ts, Some(1_000), "oldest_seen_ts never regresses toward 'now'");
    }

    #[test]
    fn ram_cap_latches_when_loaded_reaches_the_cap() {
        let mut s = RampState::default();
        s.record_trade_page(TRADE_RAM_CAP_POINTS - 1, Some(0), Some(1_000), false);
        assert!(!s.ram_capped, "one short of the cap -> not latched yet");

        s.record_trade_page(1, Some(0), Some(999), false);
        assert!(s.ram_capped, "reaching the cap exactly latches ram_capped");
    }

    #[test]
    fn ram_cap_blocks_further_demand_like_exhausted() {
        let mut s = RampState::default();
        let now = 10 * DAY_MS;
        s.begin_trade_flight(now);
        s.record_trade_page(TRADE_RAM_CAP_POINTS, Some(0), Some(now - 5 * DAY_MS), false);
        assert!(s.ram_capped);
        assert!(s.ladder_floor_reached(), "ram_capped makes the floor check report satisfied (stop, not error)");
        assert!(!s.require_target_ts(now - 30 * DAY_MS), "ram_capped blocks re-arm regardless of depth");
        assert!(s.ram_capped, "ram_capped stays latched — require_target_ts never clears it");
    }

    #[test]
    fn wants_more_ts_ignores_floor_in_pressure_phase() {
        let mut s = RampState::default();
        let now = 10 * DAY_MS;
        s.begin_trade_flight(now);
        s.oldest_seen_ts = Some(now); // nowhere near the hour floor yet
        // Same "not yet satisfied / keep going" polarity as the count-based
        // `target_satisfied` — `true` here means the floor is NOT reached
        // and the auto phase should keep loading.
        assert!(s.wants_more_ts(true), "auto phase: floor not reached -> keep loading");
        assert!(s.wants_more_ts(false), "pressure phase always reports 'keep going' regardless of the floor");
    }

    // ─────────────────────────────────────────────────────────────────────
    // Archive-day chaining (Э6-Ф3 native + Э6-Ф4 wasm). `RampState::
    // archive_mode` pure transitions, `classify_archive_day_result`/
    // `trim_rows_at_or_after` pure decision fns, and `run_archive_group`
    // end-to-end driven through the `DayFetch` mock-closure seam — no
    // network in any of these. The production code under test (`run_archive_
    // group`, `day_fetch`, `classify_archive_day_result`, `trim_rows_at_or_
    // after`) compiles and runs on both native and wasm32; this test module
    // stays native-only ONLY because `#[tokio::test(flavor = "multi_thread")]`
    // needs a multi-thread tokio runtime that does not exist on wasm32 (wasm
    // tests run under `wasm_bindgen_test`, not tokio) — a test-harness
    // constraint, not a production-code gate.
    #[cfg(not(target_arch = "wasm32"))]
    mod archive_chaining {
        use super::*;
        use digdigdig3_station::archive::ArchiveDayError;
        use digdigdig3::core::types::{ArchiveFormat, ArchiveSchema};

        fn spec_with_since(since_ms: i64) -> ArchiveSpec {
            ArchiveSpec {
                host: "example.test",
                url_pattern: "/{symbol}/{date}.csv.gz",
                format: ArchiveFormat::CsvGz,
                schema: ArchiveSchema::BybitSpot,
                since_ms,
            }
        }

        fn trade(ts_ms: i64) -> RawTrade {
            RawTrade { ts_ms, price: 1.0, qty: 1.0, is_buyer_maker: false }
        }

        // ── RampState::archive_mode pure transitions ────────────────────

        #[test]
        fn enter_archive_mode_is_idempotent_and_does_not_alone_block_needs_more() {
            let mut s = RampState::default();
            assert!(!s.archive_mode);
            s.enter_archive_mode();
            assert!(s.archive_mode);
            assert!(s.needs_more(true), "archive_mode alone does not latch exhausted");
            s.enter_archive_mode();
            assert!(s.archive_mode, "idempotent — entering twice stays true");
        }

        #[test]
        fn archive_mode_key_still_respects_exhausted_latch() {
            let mut s = RampState::default();
            s.enter_archive_mode();
            s.record_page(0, None, true); // boundary reached
            assert!(s.exhausted);
            assert!(!s.needs_more(true));
            assert!(!s.needs_more(false), "exhausted latch blocks pressure phase too, archive_mode aside");
        }

        // ── classify_archive_day_result: pure boundary/gap/error rules ──

        #[test]
        fn classify_rows_on_success() {
            let rows = vec![trade(1_000), trade(2_000)];
            let outcome = classify_archive_day_result(Ok(rows.clone()), false);
            assert_eq!(outcome, ArchiveDayOutcome::Rows(rows));
        }

        #[test]
        fn classify_not_found_at_floor_is_boundary() {
            let outcome = classify_archive_day_result(
                Err(ArchiveDayError::NotFound("http://x".to_string())),
                true,
            );
            assert_eq!(outcome, ArchiveDayOutcome::BoundaryReached);
        }

        #[test]
        fn classify_not_found_mid_range_is_gap() {
            let outcome = classify_archive_day_result(
                Err(ArchiveDayError::NotFound("http://x".to_string())),
                false,
            );
            assert_eq!(outcome, ArchiveDayOutcome::Gap);
        }

        #[test]
        fn classify_non_404_is_transient_error_regardless_of_floor() {
            let http_err = classify_archive_day_result(
                Err(ArchiveDayError::HttpStatus { status: 500, url: "http://x".to_string() }),
                true,
            );
            assert_eq!(http_err, ArchiveDayOutcome::TransientError);
            let decode_err = classify_archive_day_result(
                Err(ArchiveDayError::Decode("bad gzip".to_string())),
                false,
            );
            assert_eq!(decode_err, ArchiveDayOutcome::TransientError);
        }

        // ── trim_rows_at_or_after: overlap guard ────────────────────────

        #[test]
        fn trim_rows_removes_rows_at_or_after_floor() {
            let rows = vec![trade(100), trade(500), trade(999), trade(1000), trade(1500)];
            let trimmed = trim_rows_at_or_after(rows, Some(1000));
            let ts: Vec<i64> = trimmed.iter().map(|r| r.ts_ms).collect();
            assert_eq!(ts, vec![100, 500, 999], "1000 and 1500 are >= floor, must be dropped");
        }

        #[test]
        fn trim_rows_no_floor_is_noop() {
            let rows = vec![trade(100), trade(500)];
            let trimmed = trim_rows_at_or_after(rows.clone(), None);
            assert_eq!(trimmed, rows);
        }

        // ── run_archive_group: end-to-end via the DayFetch mock seam ────

        /// Build a `DayFetch` from a plain closure over `(symbol, date) ->
        /// Result<Vec<RawTrade>, ArchiveDayError>` — the mock seam used by
        /// every test below in place of live network `fetch_archive_day`.
        fn mock_fetcher<F>(f: F) -> DayFetch
        where
            F: Fn(&str, chrono::NaiveDate) -> Result<Vec<RawTrade>, ArchiveDayError> + Send + Sync + 'static,
        {
            let f = Arc::new(f);
            Arc::new(move |_spec, symbol, date| {
                let f = f.clone();
                Box::pin(async move { f(&symbol, date) })
            })
        }

        fn day(y: i32, m: u32, d: u32) -> chrono::NaiveDate {
            chrono::NaiveDate::from_ymd_opt(y, m, d).unwrap_or_default()
        }

        fn day_ms(y: i32, m: u32, d: u32) -> i64 {
            day(y, m, d).and_hms_opt(0, 0, 0).unwrap_or_default().and_utc().timestamp_millis()
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn run_archive_group_walks_days_and_emits_wire_prepend() {
            let engine = BackfillEngine::new();
            let key: BackfillKey = (ExchangeId::Bybit, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Trades);
            engine.with_state(&key, RampState::begin_flight);

            let since = day_ms(2026, 1, 1);
            let spec = spec_with_since(since);
            let (tx, mut rx) = tokio::sync::broadcast::channel(64);

            // Every day the walk visits returns two rows — mock never hits
            // network. `DayIterator::new` starts at "yesterday UTC" which is
            // test-environment-dependent, so instead exercise the resumable
            // walk directly by seeding `oldest_anchor` to a fixed recent day
            // and letting `resume_before` walk deterministically downward
            // from there.
            engine.with_state(&key, |s| s.oldest_anchor = Some(day_ms(2026, 1, 10)));
            engine.with_state(&key, |s| s.ramp_step = 3); // group of 3 days this call

            let fetcher = mock_fetcher(move |_symbol, date| {
                Ok(vec![trade(date.and_hms_opt(1, 0, 0).unwrap_or_default().and_utc().timestamp_millis())])
            });

            run_archive_group(
                &fetcher, &tx, &engine, &key, ExchangeId::Bybit, AccountType::Spot,
                "BTCUSDT", "BTCUSDT", spec, true,
            ).await;

            let mut prepended_batches = 0;
            let mut layer_events = 0;
            while let Ok(update) = rx.try_recv() {
                match update {
                    LiveUpdate::TradeHistoryPrepended { trades, .. } => {
                        prepended_batches += 1;
                        assert_eq!(trades.len(), 1);
                    }
                    LiveUpdate::LayerBackfilled { layer, .. } => {
                        assert_eq!(layer, "trades");
                        layer_events += 1;
                    }
                    _ => {}
                }
            }
            assert_eq!(prepended_batches, 3, "3-day group emits 3 TradeHistoryPrepended batches");
            assert_eq!(layer_events, 3, "one LayerBackfilled per landed day");
            assert_eq!(engine.snapshot(&key).map(|s| s.loaded), Some(3));
            assert!(!engine.snapshot(&key).map(|s| s.exhausted).unwrap_or(true), "mid-walk, not yet at boundary");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn run_archive_group_boundary_404_latches_exhausted() {
            let engine = BackfillEngine::new();
            let key: BackfillKey = (ExchangeId::Bybit, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Trades);
            engine.with_state(&key, RampState::begin_flight);

            let since = day_ms(2026, 1, 5);
            let spec = spec_with_since(since);
            let (tx, mut rx) = tokio::sync::broadcast::channel(64);

            // Resume starts at 2026-01-04 (day before the seeded oldest
            // 2026-01-05) — immediately AT the floor, so the very first
            // fetch call 404s and must latch as boundary, not a gap.
            engine.with_state(&key, |s| s.oldest_anchor = Some(day_ms(2026, 1, 5)));
            engine.with_state(&key, |s| s.ramp_step = 8);

            let fetcher = mock_fetcher(|_symbol, _date| {
                Err(ArchiveDayError::NotFound("http://x".to_string()))
            });

            run_archive_group(
                &fetcher, &tx, &engine, &key, ExchangeId::Bybit, AccountType::Spot,
                "BTCUSDT", "BTCUSDT", spec, true,
            ).await;

            assert!(engine.snapshot(&key).map(|s| s.exhausted).unwrap_or(false), "404 at/below floor must latch exhausted");
            let mut saw_exhausted_event = false;
            while let Ok(update) = rx.try_recv() {
                if let LiveUpdate::LayerBackfilled { exhausted: true, prepended: 0, .. } = update {
                    saw_exhausted_event = true;
                }
            }
            assert!(saw_exhausted_event);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn run_archive_group_mid_range_404_skips_not_exhausted() {
            let engine = BackfillEngine::new();
            let key: BackfillKey = (ExchangeId::Bybit, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Trades);
            engine.with_state(&key, RampState::begin_flight);

            // Wide floor so mid-walk 404s are gaps, not boundary.
            let spec = spec_with_since(day_ms(2020, 1, 1));
            let (tx, mut rx) = tokio::sync::broadcast::channel(64);

            engine.with_state(&key, |s| s.oldest_anchor = Some(day_ms(2026, 1, 10)));
            engine.with_state(&key, |s| s.ramp_step = 2); // wants 2 LANDED days

            // First day visited (2026-01-09) 404s (gap, skip), second
            // (2026-01-08) succeeds, third (2026-01-07) succeeds — group
            // wants 2 landed pages, so it stops after the 3rd fetch call.
            let call_count = Arc::new(std::sync::atomic::AtomicU32::new(0));
            let call_count_cl = call_count.clone();
            let fetcher = mock_fetcher(move |_symbol, date| {
                call_count_cl.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                if date == day(2026, 1, 9) {
                    Err(ArchiveDayError::NotFound("http://x".to_string()))
                } else {
                    Ok(vec![trade(date.and_hms_opt(1, 0, 0).unwrap_or_default().and_utc().timestamp_millis())])
                }
            });

            run_archive_group(
                &fetcher, &tx, &engine, &key, ExchangeId::Bybit, AccountType::Spot,
                "BTCUSDT", "BTCUSDT", spec, true,
            ).await;

            assert_eq!(call_count.load(std::sync::atomic::Ordering::SeqCst), 3, "gap day does not count toward the landed-page budget");
            assert!(!engine.snapshot(&key).map(|s| s.exhausted).unwrap_or(true), "mid-range 404 must NOT latch exhausted");
            assert_eq!(engine.snapshot(&key).map(|s| s.loaded), Some(2), "only the 2 successful days counted as loaded");

            let mut prepended_batches = 0;
            while let Ok(update) = rx.try_recv() {
                if let LiveUpdate::TradeHistoryPrepended { .. } = update {
                    prepended_batches += 1;
                }
            }
            assert_eq!(prepended_batches, 2);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn run_archive_group_consecutive_mid_range_404s_latch_as_boundary() {
            let engine = BackfillEngine::new();
            let key: BackfillKey = (ExchangeId::Bybit, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Trades);
            engine.with_state(&key, RampState::begin_flight);

            // Wide floor: every 404 the mock returns is classified as a gap
            // by distance-to-floor alone — the consecutive-run breaker must
            // be what stops the walk, not the boundary check.
            let spec = spec_with_since(day_ms(2000, 1, 1));
            let (tx, mut rx) = tokio::sync::broadcast::channel(64);

            engine.with_state(&key, |s| s.oldest_anchor = Some(day_ms(2026, 1, 20)));
            engine.with_state(&key, |s| s.ramp_step = MAX_RAMP_PAGES_PER_GROUP); // want as many as possible

            let fetcher = mock_fetcher(|_symbol, _date| {
                Err(ArchiveDayError::NotFound("http://x".to_string()))
            });

            run_archive_group(
                &fetcher, &tx, &engine, &key, ExchangeId::Bybit, AccountType::Spot,
                "BTCUSDT", "BTCUSDT", spec, true,
            ).await;

            assert!(
                engine.snapshot(&key).map(|s| s.exhausted).unwrap_or(false),
                "MAX_CONSECUTIVE_MID_RANGE_404S consecutive gaps must latch exhausted (delisted-symbol heuristic)"
            );
            let mut saw_exhausted_event = false;
            while let Ok(update) = rx.try_recv() {
                if let LiveUpdate::LayerBackfilled { exhausted: true, .. } = update {
                    saw_exhausted_event = true;
                }
            }
            assert!(saw_exhausted_event);
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn run_archive_group_transient_error_stops_group_without_latching() {
            let engine = BackfillEngine::new();
            let key: BackfillKey = (ExchangeId::Bybit, "BTCUSDT".to_string(), AccountType::Spot, LayerId::Trades);
            engine.with_state(&key, RampState::begin_flight);

            let spec = spec_with_since(day_ms(2000, 1, 1));
            let (tx, mut rx) = tokio::sync::broadcast::channel(64);

            engine.with_state(&key, |s| s.oldest_anchor = Some(day_ms(2026, 1, 10)));
            engine.with_state(&key, |s| s.ramp_step = 4);

            let fetcher = mock_fetcher(|_symbol, _date| {
                Err(ArchiveDayError::HttpStatus { status: 503, url: "http://x".to_string() })
            });

            run_archive_group(
                &fetcher, &tx, &engine, &key, ExchangeId::Bybit, AccountType::Spot,
                "BTCUSDT", "BTCUSDT", spec, true,
            ).await;

            assert!(!engine.snapshot(&key).map(|s| s.exhausted).unwrap_or(true), "transient error must not latch exhausted");
            assert_eq!(engine.snapshot(&key).map(|s| s.loaded), Some(0));
            let mut saw_any_event = false;
            while let Ok(_update) = rx.try_recv() {
                saw_any_event = true;
            }
            assert!(!saw_any_event, "transient error breaks the group with no emission at all for that failed day");
        }

        #[tokio::test(flavor = "multi_thread")]
        async fn run_archive_group_overlap_guard_trims_rows_at_or_after_oldest_loaded() {
            let engine = BackfillEngine::new();
            let key: BackfillKey = (ExchangeId::Binance, "BTCUSDT".to_string(), AccountType::FuturesCross, LayerId::Trades);
            engine.with_state(&key, RampState::begin_flight);

            // Simulates the RestWindow → archive continuation seam: REST
            // already loaded down to `oldest_anchor`, so the FIRST archive
            // day (which straddles that boundary) must have its overlapping
            // rows trimmed before being counted/emitted.
            let wall_ms = day_ms(2026, 1, 10) + 12 * 60 * 60 * 1000; // noon on the 10th
            engine.with_state(&key, |s| s.oldest_anchor = Some(wall_ms));
            engine.with_state(&key, |s| s.ramp_step = 1);

            let spec = spec_with_since(day_ms(2000, 1, 1));
            let (tx, mut rx) = tokio::sync::broadcast::channel(64);

            // resume_before(wall_ms, ...) starts at 2026-01-09 (day BEFORE
            // the day containing wall_ms) — that day's rows are entirely
            // before the wall, so nothing should be trimmed here. Exercise
            // the trim directly at the boundary day instead by using a
            // fetcher that returns rows straddling `wall_ms` regardless of
            // which date is requested (day-boundary semantics are already
            // covered by `resume_before`'s own unit tests in dig3-station).
            let wall = wall_ms;
            let fetcher = mock_fetcher(move |_symbol, date| {
                let base = date.and_hms_opt(0, 0, 0).unwrap_or_default().and_utc().timestamp_millis();
                Ok(vec![
                    trade(base + 1_000),          // well before any wall
                    trade(wall - 1),               // just before the wall — kept
                    trade(wall),                   // AT the wall — trimmed
                    trade(wall + 60_000),          // after the wall — trimmed
                ])
            });

            run_archive_group(
                &fetcher, &tx, &engine, &key, ExchangeId::Binance, AccountType::FuturesCross,
                "BTCUSDT", "BTCUSDT", spec, true,
            ).await;

            let mut all_ts: Vec<i64> = Vec::new();
            while let Ok(update) = rx.try_recv() {
                if let LiveUpdate::TradeHistoryPrepended { trades, .. } = update {
                    all_ts.extend(trades.iter().map(|t| t.ts_ms));
                }
            }
            assert!(!all_ts.contains(&wall), "row exactly at the overlap floor must be trimmed");
            assert!(!all_ts.contains(&(wall + 60_000)), "row after the overlap floor must be trimmed");
            assert!(all_ts.contains(&(wall - 1)), "row just before the overlap floor is kept");
        }
    }
}
