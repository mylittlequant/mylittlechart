//! HeadlessChartApp — full ChartApp lifecycle without the GPU/window layer.
//!
//! Wraps `mlc_app::ChartApp::new(symbol)` (the same entry point the live
//! binary uses), drives `tick()` + `prepare_frame()` on demand, exposes
//! inspection helpers for the bubble/chart state, and lets tests dispatch
//! the same `ChartOutEvent`s that user clicks produce in the binary.
//!
//! No winit, no wgpu, no vello. All other layers (DataBridge, tokio runtime,
//! managers, BubbleStore, panels_store, persistence, presets, drawing,
//! alerts, signals) are the real production code.
//!
//! ## Use cases
//!
//! - Reproduce live-binary bugs without spending 13 minutes on the release
//!   build for each iteration.
//! - Drive scripted scenarios: change_symbol → wait_for_bars → change_timeframe
//!   → split → assert bubble layout.
//! - Foundation for an eventual backend/CLI binary.

use std::sync::Arc;
use std::time::Duration;
use web_time::Instant;

use mlc_app::ChartApp;
use mlc_core::state::ChartId;
use mlc_core::tag_manager::BubbleId;
use mlc_live_data::DataBridge;

/// Headless wrapper over `mlc_app::ChartApp`.
pub struct HeadlessChartApp {
    pub app: ChartApp,
    pub bridge: Arc<DataBridge>,
    /// Wall-clock time the harness started — used to feed `current_time_ms` to tick().
    started: Instant,
    /// Virtual viewport width fed into resize/prepare_frame.
    width: u32,
    /// Virtual viewport height fed into resize/prepare_frame.
    height: u32,
}

impl HeadlessChartApp {
    /// Construct via the same `ChartApp::new(symbol)` entry point the live
    /// binary's bootstrap calls. The bridge, runtime, managers, presets,
    /// persistence and BubbleStore are all real.
    pub fn new(symbol: &str) -> Self {
        let app = ChartApp::new(symbol);
        let bridge = app.bridge_arc();
        let started = Instant::now();
        Self {
            app,
            bridge,
            started,
            width: 1280,
            height: 800,
        }
    }

    /// Apply a virtual resize so prepare_frame computes real chart dimensions.
    /// Required before any wait_for_bars: deferred-snap path needs a non-zero
    /// chart_width to consume the BarsLoaded.
    pub fn resize(&mut self, width: u32, height: u32) {
        self.width = width;
        self.height = height;
        self.app.resize(width, height);
    }

    /// One full frame cycle: tick + prepare_frame + drain_pending_cold_apply.
    ///
    /// `prepare_frame` flushes the deferred viewport snap once chart_width is
    /// known. `drain_pending_cold_apply` runs the deferred preset apply body
    /// — the live binary calls this from its GPU-done hook (one frame after
    /// LoadPreset stashed), so headless must do the equivalent or the chart
    /// stays stuck on `BubbleId(0)` sentinel forever.
    pub fn pump_once(&mut self) {
        let now_ms = self.started.elapsed().as_millis() as u64;
        self.app.tick(now_ms);
        self.app.prepare_frame(self.width as f64, self.height as f64);
        self.app.drain_pending_cold_apply();
        self.app.drain_pending_cure_after_preset_load();
    }

    /// Pump frames at ~60Hz until `predicate` returns true or `timeout` elapses.
    /// Returns `true` if the predicate became true before the timeout.
    pub fn pump_until<F: FnMut(&Self) -> bool>(&mut self, mut predicate: F, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        while Instant::now() < deadline {
            self.pump_once();
            if predicate(self) {
                return true;
            }
            std::thread::sleep(Duration::from_millis(16));
        }
        false
    }

    // ── Inspection helpers ───────────────────────────────────────────────

    /// All `ChartId`s currently in the panel grid.
    pub fn chart_ids(&self) -> Vec<ChartId> {
        self.app.panel_app.panel_grid.containers().keys().copied().collect()
    }

    /// The active chart's `ChartId`, if any.
    pub fn active_chart_id(&self) -> Option<ChartId> {
        self.app.panel_app.panel_grid.active_container_id()
    }

    /// The bubble the renderer reads for this chart (via current_bubble_id).
    pub fn chart_bubble(&self, cid: ChartId) -> Option<BubbleId> {
        self.app.panel_app.panel_grid.containers()
            .get(&cid)
            .map(|w| w.current_bubble_id)
    }

    /// Number of bars in the renderer-read bubble for this chart.
    pub fn chart_bar_count(&self, cid: ChartId) -> usize {
        self.chart_bubble(cid)
            .and_then(|bid| self.app.with_bubble(bid, |b| b.bars.len()))
            .unwrap_or(0)
    }

    /// `(chart_id, current_bubble_id, bubble.bars.len)` for every chart.
    pub fn dump_chart_bubbles(&self) -> Vec<(ChartId, BubbleId, usize)> {
        self.chart_ids().into_iter().map(|cid| {
            let bid = self.chart_bubble(cid).unwrap_or(BubbleId(0));
            let len = self.chart_bar_count(cid);
            (cid, bid, len)
        }).collect()
    }

    /// `(bubble_id, exchange, account_type, symbol, tf, bars.len, attached_charts)`
    /// for every bubble in the store.
    pub fn dump_all_bubbles(&self) -> Vec<BubbleSummary> {
        let guard = self.app.core.bubble();
        guard.bubbles().map(|b| BubbleSummary {
            id: b.id,
            exchange: b.exchange.clone(),
            account_type: b.account_type.clone(),
            symbol: b.symbol.clone(),
            timeframe: b.timeframe.name.clone(),
            bars_len: b.bars.len(),
            attached_charts: guard.chart_member_ids(b.id),
        }).collect()
    }

    /// Resolve the bubble `bubble_for_instrument` would pick for this triplet.
    pub fn bubble_for_instrument(&self, exchange: &str, account_type: &str, symbol: &str) -> Option<BubbleId> {
        self.app.core.bubble()
            .bubble_for_instrument(exchange, account_type, symbol)
    }

    /// Borrow the inner `ChartApp` for advanced manipulation (sending out-events,
    /// driving the input handler, etc).
    pub fn inner_mut(&mut self) -> &mut ChartApp {
        &mut self.app
    }

    /// Element counts for every Phase X0 derived series on this chart's
    /// bubble. Mirrors `mlc_agentic_server::state::DerivedSeriesCounts` so
    /// HeadlessChartApp asserts and the agent-server HTTP snapshot agree.
    pub fn derived_counts(&self, cid: ChartId) -> DerivedCounts {
        let Some(bid) = self.chart_bubble(cid) else { return DerivedCounts::default(); };
        self.app.with_bubble(bid, |b| DerivedCounts {
            bars:                b.bars.len(),
            footprint_bars:      b.footprint_bars.as_ref().map(|v| v.len()).unwrap_or(0),
            renko_bars:          b.renko_bars.as_ref().map(|v| v.len()).unwrap_or(0),
            three_line_break:    b.three_line_break.as_ref().map(|v| v.len()).unwrap_or(0),
            pnf_columns:         b.pnf_columns.as_ref().map(|v| v.len()).unwrap_or(0),
            kagi_segments:       b.kagi_segments.as_ref().map(|v| v.len()).unwrap_or(0),
            cvd_line:            b.cvd_line.as_ref().map(|v| v.len()).unwrap_or(0),
            tpo_sessions:        b.tpo_sessions.as_ref().map(|v| v.len()).unwrap_or(0),
            oi_delta:            b.oi_delta.as_ref().map(|v| v.len()).unwrap_or(0),
            funding_rate:        b.funding_rate.as_ref().map(|v| v.len()).unwrap_or(0),
            dom_heatmap_columns: b.dom_heatmap.as_ref().map(|h| h.bucket_count()).unwrap_or(0),
            dom_trade_dots:      b.dom_trade_dots.as_ref().map(|v| v.len()).unwrap_or(0),
            liq_heatmap_columns: b.liq_heatmap.as_ref().map(|h| h.bucket_count()).unwrap_or(0),
            liq_dots:            b.liq_dots.as_ref().map(|v| v.len()).unwrap_or(0),
        }).unwrap_or_default()
    }
}

/// Same shape as `mlc_agentic_server::state::DerivedSeriesCounts` plus the
/// primary `bars` count. Returned by `HeadlessChartApp::derived_counts`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DerivedCounts {
    pub bars: usize,
    pub footprint_bars: usize,
    pub renko_bars: usize,
    pub three_line_break: usize,
    pub pnf_columns: usize,
    pub kagi_segments: usize,
    pub cvd_line: usize,
    pub tpo_sessions: usize,
    pub oi_delta: usize,
    pub funding_rate: usize,
    pub dom_heatmap_columns: usize,
    pub dom_trade_dots: usize,
    pub liq_heatmap_columns: usize,
    pub liq_dots: usize,
}

/// Flat summary of one bubble for test assertions / debug dumps.
#[derive(Clone, Debug)]
pub struct BubbleSummary {
    pub id: BubbleId,
    pub exchange: String,
    pub account_type: String,
    pub symbol: String,
    pub timeframe: String,
    pub bars_len: usize,
    pub attached_charts: Vec<ChartId>,
}
