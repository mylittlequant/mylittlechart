use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use mlc_core::chart::types::price_scale::ScaleMode;
use mlc_core::drawing::primitives_v2::config::TemplateStyle;
use mlc_core::ids::{BubbleId, MemberSyncOverride, SyncFlags, SyncMemberId};
use mlc_core::indicator_types::IndicatorGroupConfig;
use mlc_core::state::chart_container::ConnectionStatus;
use mlc_core::state::CommandHistory;
use mlc_core::state::Timeframe;

/// A group of chart windows and trading panels that synchronize shared state.
pub struct DataBubble {
    pub id: BubbleId,
    /// Display color used in the UI to identify this bubble.
    pub color: [f32; 4],

    // Owned state shared across all member windows
    pub primitives: Vec<Box<dyn mlc_core::drawing::primitives_v2::Primitive>>,
    pub indicator_configs: Vec<IndicatorGroupConfig>,
    pub symbol: String,
    /// Exchange name (e.g. `"binance"`).
    pub exchange: String,
    /// Account type short label (e.g. `"S"` for Spot, `"F"` for Futures).
    pub account_type: String,
    pub timeframe: Timeframe,

    /// Which properties are synchronized.
    pub sync_flags: SyncFlags,

    /// Bubble-persisted price-scale corner mode (Auto / Manual / Focus).
    ///
    /// Authoritative when `sync_flags.sync_viewport = true`. All members
    /// display this mode; clicking A/M/F on any peer updates it here and
    /// pushes to all peers via `propagate_viewport_to_sync_group`.
    /// When `sync_viewport = false` each window manages its own mode independently.
    pub scale_mode: ScaleMode,

    /// All members of this bubble: charts AND trading panels.
    pub members: std::collections::HashSet<SyncMemberId>,

    /// Per-member flag overrides. Absent = use bubble defaults.
    pub member_overrides: std::collections::HashMap<SyncMemberId, MemberSyncOverride>,

    /// Shared undo/redo history for operations that affect all members
    /// (primitives, indicator configs). Window-local operations (viewport,
    /// symbol, timeframe, chart type) are stored on the individual window's
    /// `command_history` instead.
    pub command_history: CommandHistory,

    /// Indicator instance ids that were active on the chart BEFORE it joined a
    /// color-tag sync group.  Populated at join time so desync can restore and
    /// unhide them.  Cleared after desync.
    pub pre_tag_indicator_ids: Vec<u64>,

    /// Invisible default bubble — auto-created so every window has a bubble.
    /// No color tag shown in UI. Set to false when user manually tags.
    pub auto_created: bool,

    /// Bubble-level last-used drawing style per tool type.
    ///
    /// Shared via `Arc` with every member window's `DrawingManager` so that a
    /// style change in *any* window of the bubble is immediately visible in all
    /// peers (fixes bugs B1 + D1).  The `Arc` handle in the DM is rebounded
    /// whenever a window joins or leaves this bubble.
    pub last_used_style: Arc<RwLock<HashMap<String, TemplateStyle>>>,

    /// Previous session close price for this bubble's instrument (`bars[0].open`).
    /// Populated by `BarsLoaded` handlers; read by the renderer for the
    /// price-change badge.
    pub prev_close_price: Option<f64>,

    /// Live / Delayed / Disconnected indicator for this bubble's data feed.
    pub connection_status: ConnectionStatus,

    // =========================================================================
    // === Per-Kind series storage (Phase X0, 2026-06-19) ======================
    // =========================================================================
    // Owner mandate: the bubble is a storage hub. Bridge writes raw via the
    // append_*/replace_*/clear_* methods below. Methods mutate ONLY the
    // storage — no viewport calls, no derived recompute, no callbacks.
    // Consumers (ChartContainer) PULL state during their own frame prepare.
    // See nemo/docs/mlc/plans/xaxis-phase-x0-ownership-refactor-2026-06-19.md
    // =========================================================================

    /// OHLC bars for the bubble's instrument. Written by `BarsLoaded`,
    /// `BackfillComplete`, `ScrollBarsLoaded`, `BarUpdate`, `TradeUpdate`.
    pub bars: Vec<mlc_core::Bar>,

    /// Footprint cluster bars. Written by `FootprintUpdate`.
    pub footprint_bars: Option<Vec<mlc_core::types::FootprintBar>>,

    /// Renko bricks. Written by `RenkoBarUpdate`.
    pub renko_bars: Option<Vec<mlc_core::types::RenkoBrick>>,

    /// Three Line Break lines. Written by `ThreeLineBreakUpdate`.
    pub three_line_break: Option<Vec<mlc_core::types::ThreeLineBreakLine>>,

    /// Point-and-Figure columns. Written by `PnfBarUpdate`.
    pub pnf_columns: Option<Vec<mlc_core::types::PnfColumn>>,

    /// Kagi segments. Written by `KagiBarUpdate`.
    pub kagi_segments: Option<Vec<mlc_core::types::KagiSegment>>,

    /// Cumulative Volume Delta samples. Written by `CvdLineUpdate`.
    pub cvd_line: Option<Vec<mlc_core::types::CvdSample>>,

    /// TPO Market Profile session snapshots. Written by `TpoProfileUpdate`.
    pub tpo_sessions: Option<Vec<mlc_core::types::TpoSession>>,

    /// Open Interest Δ bars. Written by `OpenInterestUpdate`.
    pub oi_delta: Option<Vec<mlc_core::types::OiDeltaBar>>,

    /// Funding Rate cells. Written by `FundingRateUpdate`.
    pub funding_rate: Option<Vec<mlc_core::types::FundingRateCell>>,

    /// DOM heatmap ring buffer. Written by `DomHeatmapSnapshot`.
    pub dom_heatmap: Option<mlc_core::chart::dom_heatmap::DomHeatmapBuffer>,

    /// DOM trade-circle ring overlay. Written by `DomTradeDot`.
    pub dom_trade_dots: Option<Vec<mlc_core::chart::dom_heatmap::DomTradeDot>>,

    /// Liquidation heatmap ring buffer. Written by `LiquidationBucketUpdate`
    /// (and aggregated from `LiquidationEvent`).
    pub liq_heatmap: Option<mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer>,

    /// Liquidation dot overlay. Written by `LiquidationEvent`.
    pub liq_dots: Option<Vec<mlc_core::chart::liquidation_heatmap::LiquidationDot>>,

    // =========================================================================
    // === Alert storage (Step 3.0 doctrine cutover) ===========================
    // =========================================================================
    // Alerts are bubble data: persisted user-authored rows + the per-bubble
    // identity allocator.  The tick-to-tick last-price cache used for
    // crossing detection is manager state — it lives on
    // `AlertManager.last_price`, keyed by `BubbleId` (see
    // `crates/mlc-backend-core/src/alert/manager.rs`).
    // =========================================================================

    /// Alerts attached to this bubble.  Each alert targets data inside this
    /// bubble (price levels, drawings, indicators).  Persisted with the bubble.
    pub alerts: Vec<mlc_core::alert::AlertItem>,

    /// Per-bubble next-id counter for alerts.
    pub alert_next_id: u64,
}


impl DataBubble {
    /// Reconstruct a [`DataBubble`] from a [`mlc_core::preset::snapshots::DataBubbleSnapshot`].
    ///
    /// The resulting bubble starts with an empty `primitives` list — primitives
    /// are restored separately via the `PrimitiveRegistry` and pushed in by the
    /// caller ([`crate::bubble::BubbleStore::restore_from_snapshots`]).
    pub fn from_snapshot(snap: &mlc_core::preset::snapshots::DataBubbleSnapshot) -> Self {
        let mut members = std::collections::HashSet::new();
        for &raw in &snap.members {
            members.insert(SyncMemberId::Chart(raw));
        }
        for &raw in &snap.panel_members {
            members.insert(SyncMemberId::Panel(raw));
        }

        let member_overrides: std::collections::HashMap<SyncMemberId, MemberSyncOverride> = snap
            .member_overrides
            .iter()
            .map(|o| {
                let member = if o.kind == 0 {
                    SyncMemberId::Chart(o.id)
                } else {
                    SyncMemberId::Panel(o.id)
                };
                let override_val = MemberSyncOverride {
                    sync_symbol: o.sync_symbol,
                    sync_crosshair: o.sync_crosshair,
                };
                (member, override_val)
            })
            .collect();

        let mut style_map = HashMap::new();
        for (type_id, raw) in &snap.last_used_style {
            if let Ok(style) = serde_json::from_value::<TemplateStyle>(raw.clone()) {
                style_map.insert(type_id.clone(), style);
            }
        }

        Self {
            id: BubbleId(snap.id),
            color: snap.color,
            primitives: Vec::new(),
            indicator_configs: snap.indicator_configs.clone(),
            symbol: snap.symbol.clone(),
            exchange: snap.exchange.clone(),
            account_type: snap.account_type.clone(),
            timeframe: snap.timeframe.clone(),
            sync_flags: snap.sync_flags.clone(),
            scale_mode: snap.scale_mode,
            members,
            member_overrides,
            command_history: snap.command_history.clone().unwrap_or_else(|| CommandHistory::new(250)),
            pre_tag_indicator_ids: Vec::new(),
            auto_created: snap.auto_created,
            last_used_style: Arc::new(RwLock::new(style_map)),
            prev_close_price: None,
            connection_status: ConnectionStatus::default(),
            bars: Vec::new(),
            footprint_bars: None,
            renko_bars: None,
            three_line_break: None,
            pnf_columns: None,
            kagi_segments: None,
            cvd_line: None,
            tpo_sessions: None,
            oi_delta: None,
            funding_rate: None,
            dom_heatmap: None,
            dom_trade_dots: None,
            liq_heatmap: None,
            liq_dots: None,
            alerts: snap.alerts.clone(),
            alert_next_id: snap.alerts.iter().map(|a| a.id).max().unwrap_or(0) + 1,
        }
    }

    /// Effective `sync_symbol` for a specific member.
    ///
    /// The per-member override (if present) wins over the bubble-level flag.
    pub fn effective_sync_symbol(&self, member: SyncMemberId) -> bool {
        self.member_overrides
            .get(&member)
            .and_then(|o| o.sync_symbol)
            .unwrap_or(self.sync_flags.sync_symbol)
    }

    /// Effective `sync_crosshair` for a specific member.
    pub fn effective_sync_crosshair(&self, member: SyncMemberId) -> bool {
        self.member_overrides
            .get(&member)
            .and_then(|o| o.sync_crosshair)
            .unwrap_or(self.sync_flags.sync_crosshair)
    }

    // =========================================================================
    // === Per-Kind storage mutators (Phase X0) ================================
    // =========================================================================
    // Mutate ONLY the storage field. No viewport calls, no derived recompute,
    // no callbacks. Consumers pull state and run their own reconcile.
    // =========================================================================

    pub fn replace_bars(&mut self, bars: Vec<mlc_core::Bar>) { self.bars = bars; }
    pub fn append_bar(&mut self, bar: mlc_core::Bar) { self.bars.push(bar); }
    pub fn last_bar_mut(&mut self) -> Option<&mut mlc_core::Bar> { self.bars.last_mut() }
    pub fn clear_bars(&mut self) { self.bars.clear(); }

    pub fn append_footprint(&mut self, bar: mlc_core::types::FootprintBar) {
        self.footprint_bars.get_or_insert_with(Vec::new).push(bar);
    }
    pub fn last_footprint_mut(&mut self) -> Option<&mut mlc_core::types::FootprintBar> {
        self.footprint_bars.as_mut().and_then(|v| v.last_mut())
    }
    pub fn clear_footprint(&mut self) { self.footprint_bars = None; }

    pub fn append_renko(&mut self, brick: mlc_core::types::RenkoBrick) {
        self.renko_bars.get_or_insert_with(Vec::new).push(brick);
    }
    pub fn clear_renko(&mut self) { self.renko_bars = None; }

    pub fn append_three_line_break(&mut self, line: mlc_core::types::ThreeLineBreakLine) {
        self.three_line_break.get_or_insert_with(Vec::new).push(line);
    }
    pub fn clear_three_line_break(&mut self) { self.three_line_break = None; }

    pub fn append_pnf(&mut self, col: mlc_core::types::PnfColumn) {
        self.pnf_columns.get_or_insert_with(Vec::new).push(col);
    }
    pub fn last_pnf_mut(&mut self) -> Option<&mut mlc_core::types::PnfColumn> {
        self.pnf_columns.as_mut().and_then(|v| v.last_mut())
    }
    pub fn clear_pnf(&mut self) { self.pnf_columns = None; }

    pub fn append_kagi(&mut self, seg: mlc_core::types::KagiSegment) {
        self.kagi_segments.get_or_insert_with(Vec::new).push(seg);
    }
    pub fn clear_kagi(&mut self) { self.kagi_segments = None; }

    pub fn append_cvd(&mut self, sample: mlc_core::types::CvdSample) {
        self.cvd_line.get_or_insert_with(Vec::new).push(sample);
    }
    pub fn clear_cvd(&mut self) { self.cvd_line = None; }

    pub fn append_tpo(&mut self, sess: mlc_core::types::TpoSession) {
        self.tpo_sessions.get_or_insert_with(Vec::new).push(sess);
    }
    pub fn last_tpo_mut(&mut self) -> Option<&mut mlc_core::types::TpoSession> {
        self.tpo_sessions.as_mut().and_then(|v| v.last_mut())
    }
    pub fn clear_tpo(&mut self) { self.tpo_sessions = None; }

    pub fn append_oi(&mut self, point: mlc_core::types::OiDeltaBar) {
        self.oi_delta.get_or_insert_with(Vec::new).push(point);
    }
    pub fn clear_oi(&mut self) { self.oi_delta = None; }

    pub fn append_funding(&mut self, point: mlc_core::types::FundingRateCell) {
        self.funding_rate.get_or_insert_with(Vec::new).push(point);
    }
    pub fn clear_funding(&mut self) { self.funding_rate = None; }

    pub fn dom_heatmap_mut(&mut self) -> &mut Option<mlc_core::chart::dom_heatmap::DomHeatmapBuffer> {
        &mut self.dom_heatmap
    }
    pub fn clear_dom_heatmap(&mut self) { self.dom_heatmap = None; }

    pub fn append_dom_trade_dot(&mut self, dot: mlc_core::chart::dom_heatmap::DomTradeDot) {
        self.dom_trade_dots.get_or_insert_with(Vec::new).push(dot);
    }
    pub fn dom_trade_dots_mut(&mut self) -> Option<&mut Vec<mlc_core::chart::dom_heatmap::DomTradeDot>> {
        self.dom_trade_dots.as_mut()
    }
    pub fn clear_dom_trade_dots(&mut self) { self.dom_trade_dots = None; }

    pub fn liq_heatmap_mut(&mut self) -> &mut Option<mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer> {
        &mut self.liq_heatmap
    }
    pub fn clear_liq_heatmap(&mut self) { self.liq_heatmap = None; }

    pub fn append_liq_dot(&mut self, dot: mlc_core::chart::liquidation_heatmap::LiquidationDot) {
        self.liq_dots.get_or_insert_with(Vec::new).push(dot);
    }
    pub fn liq_dots_mut(&mut self) -> Option<&mut Vec<mlc_core::chart::liquidation_heatmap::LiquidationDot>> {
        self.liq_dots.as_mut()
    }
    pub fn clear_liq_dots(&mut self) { self.liq_dots = None; }
}

impl Clone for DataBubble {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            color: self.color,
            primitives: self.primitives.iter().map(|p| p.clone_box()).collect(),
            indicator_configs: self.indicator_configs.clone(),
            symbol: self.symbol.clone(),
            exchange: self.exchange.clone(),
            account_type: self.account_type.clone(),
            timeframe: self.timeframe.clone(),
            sync_flags: self.sync_flags.clone(),
            scale_mode: self.scale_mode,
            members: self.members.clone(),
            member_overrides: self.member_overrides.clone(),
            command_history: CommandHistory::new(250),
            pre_tag_indicator_ids: self.pre_tag_indicator_ids.clone(),
            auto_created: self.auto_created,
            // Clone the Arc — both copies point to the same shared store.
            last_used_style: Arc::clone(&self.last_used_style),
            prev_close_price: self.prev_close_price,
            connection_status: self.connection_status,
            // === Per-Kind storage (Phase X0) ===
            bars: self.bars.clone(),
            footprint_bars: self.footprint_bars.clone(),
            renko_bars: self.renko_bars.clone(),
            three_line_break: self.three_line_break.clone(),
            pnf_columns: self.pnf_columns.clone(),
            kagi_segments: self.kagi_segments.clone(),
            cvd_line: self.cvd_line.clone(),
            tpo_sessions: self.tpo_sessions.clone(),
            oi_delta: self.oi_delta.clone(),
            funding_rate: self.funding_rate.clone(),
            dom_heatmap: self.dom_heatmap.clone(),
            dom_trade_dots: self.dom_trade_dots.clone(),
            liq_heatmap: self.liq_heatmap.clone(),
            liq_dots: self.liq_dots.clone(),
            alerts: self.alerts.clone(),
            alert_next_id: self.alert_next_id,
        }
    }
}

impl std::fmt::Debug for DataBubble {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DataBubble")
            .field("id", &self.id)
            .field("color", &self.color)
            .field("primitives_count", &self.primitives.len())
            .field("indicator_configs", &self.indicator_configs)
            .field("symbol", &self.symbol)
            .field("exchange", &self.exchange)
            .field("account_type", &self.account_type)
            .field("timeframe", &self.timeframe)
            .field("sync_flags", &self.sync_flags)
            .field("members", &self.members)
            .field("command_history_size", &self.command_history.size())
            .finish()
    }
}
