use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use mlc_core::chart::types::price_scale::ScaleMode;
use mlc_core::ids::{BubbleId, SyncFlags, SyncMemberId};
use mlc_core::indicator_types::IndicatorGroupConfig;
use mlc_core::instrument::InstrumentAddress;
use mlc_core::state::chart_container::{ChartId, ConnectionStatus};
use mlc_core::state::{CommandHistory, Timeframe};

use super::data_bubble::DataBubble;

// =============================================================================
// BubbleStoreError
// =============================================================================

/// Errors returned by `BubbleStore` operations.
#[derive(Debug)]
pub enum BubbleStoreError {
    /// The given member is not registered in any data bubble.
    MemberNotInGroup(SyncMemberId),
    GroupNotFound(BubbleId),
    PrimitiveNotFound(u64),
}


impl std::fmt::Display for BubbleStoreError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemberNotInGroup(id) => write!(f, "member {:?} is not in any data bubble", id),
            Self::GroupNotFound(id) => write!(f, "bubble {:?} does not exist", id),
            Self::PrimitiveNotFound(id) => write!(f, "primitive {} not found in bubble", id),
        }
    }
}

impl std::error::Error for BubbleStoreError {}

// =============================================================================
// BubbleStore
// =============================================================================

/// Manages data bubbles that bind multiple chart windows and panels.
pub struct BubbleStore {
    bubbles: std::collections::HashMap<BubbleId, DataBubble>,
    /// Reverse index: member → bubble.
    member_to_bubble: std::collections::HashMap<SyncMemberId, BubbleId>,
    next_indicator_config_id: u64,
    /// The bubble that the currently-active chart belongs to.
    /// Updated whenever the active chart changes.
    pub active_chart_bubble: Option<BubbleId>,
    /// Panel members currently in Synced state.
    /// Their bubble membership is dynamic — reassigned when the active chart changes.
    synced_panels: std::collections::HashSet<SyncMemberId>,
}


impl BubbleStore {
    /// Create an empty `BubbleStore`.
    pub fn new() -> Self {
        Self {
            bubbles: std::collections::HashMap::new(),
            member_to_bubble: std::collections::HashMap::new(),
            next_indicator_config_id: 1,
            active_chart_bubble: None,
            synced_panels: std::collections::HashSet::new(),
        }
    }

    // =========================================================================
    // Bubble lifecycle
    // =========================================================================

    /// Create a new data bubble with the given color, symbol, and timeframe.
    ///
    /// Returns the id of the newly created bubble.
    pub fn create_group(
        &mut self,
        color: [f32; 4],
        symbol: String,
        timeframe: Timeframe,
    ) -> BubbleId {
        let id = BubbleId::generate();
        let bubble = DataBubble {
            id,
            color,
            primitives: Vec::new(),
            indicator_configs: Vec::new(),
            address: InstrumentAddress::new(String::new(), symbol, mlc_core::instrument::AccountType::default()),
            timeframe,
            sync_flags: SyncFlags::default(),
            scale_mode: ScaleMode::Auto,
            members: std::collections::HashSet::new(),
            member_overrides: std::collections::HashMap::new(),
            command_history: CommandHistory::new(250),
            pre_tag_indicator_ids: Vec::new(),
            auto_created: false,
            last_used_style: Arc::new(RwLock::new(HashMap::new())),
            prev_close_price: None,
            connection_status: ConnectionStatus::default(),
            // === Per-Kind storage (Phase X0) ===
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
            alerts: Vec::new(),
            alert_next_id: 1,
        };
        self.bubbles.insert(id, bubble);
        id
    }

    /// Create an invisible default bubble (no color tag in UI).
    pub fn create_group_auto(
        &mut self,
        color: [f32; 4],
        symbol: String,
        timeframe: Timeframe,
    ) -> BubbleId {
        let id = BubbleId::generate();
        let bubble = DataBubble {
            id,
            color,
            primitives: Vec::new(),
            indicator_configs: Vec::new(),
            address: InstrumentAddress::new(String::new(), symbol, mlc_core::instrument::AccountType::default()),
            timeframe,
            sync_flags: SyncFlags::default(),
            scale_mode: ScaleMode::Auto,
            members: std::collections::HashSet::new(),
            member_overrides: std::collections::HashMap::new(),
            command_history: CommandHistory::new(250),
            pre_tag_indicator_ids: Vec::new(),
            auto_created: true,
            last_used_style: Arc::new(RwLock::new(HashMap::new())),
            prev_close_price: None,
            connection_status: ConnectionStatus::default(),
            // === Per-Kind storage (Phase X0) ===
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
            alerts: Vec::new(),
            alert_next_id: 1,
        };
        self.bubbles.insert(id, bubble);
        id
    }

    /// Remove a data bubble and unmap all its members.
    pub fn remove_group(&mut self, id: BubbleId) {
        if let Some(bubble) = self.bubbles.remove(&id) {
            for member_id in &bubble.members {
                self.member_to_bubble.remove(member_id);
            }
        }
    }

    /// Get an immutable reference to a bubble by id.
    ///
    /// `pub(crate)` — external callers (outside `mlc-backend`) go
    /// through the `Bubbles`/`BubblesMut` facade instead (Step 3.2,
    /// `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A). Kept `pub` at
    /// the crate level so `tick_writers`, `Alerts`/`AlertsMut`, and
    /// `Drawings`/`DrawingsMut` can still reach it directly.
    pub(crate) fn bubble(&self, id: BubbleId) -> Option<&DataBubble> {
        self.bubbles.get(&id)
    }

    /// Get a mutable reference to a bubble by id. `pub(crate)` — see [`Self::bubble`].
    pub(crate) fn bubble_mut(&mut self, id: BubbleId) -> Option<&mut DataBubble> {
        self.bubbles.get_mut(&id)
    }

    /// Returns `true` if a bubble with this id exists.
    pub fn exists(&self, id: BubbleId) -> bool {
        self.bubbles.contains_key(&id)
    }

    /// Iterate over every bubble's id.
    pub fn bubble_ids(&self) -> impl Iterator<Item = BubbleId> + '_ {
        self.bubbles.keys().copied()
    }

    /// Call `f` once per bubble with a bundled read view. Replaces the raw
    /// `bubbles()` iterator for external callers that need more than just
    /// the id (Step 3.2 §2.1 #6).
    pub fn each_bubble<F: FnMut(super::facade::BubbleView<'_>)>(&self, mut f: F) {
        for bubble in self.bubbles.values() {
            f(super::facade::BubbleView::new(bubble));
        }
    }

    /// Iterate over all data bubbles. `pub(crate)` — see [`Self::bubble`].
    pub(crate) fn bubbles(&self) -> impl Iterator<Item = &DataBubble> {
        self.bubbles.values()
    }

    /// Iterate over all data bubbles mutably. `pub(crate)` — see [`Self::bubble`].
    pub(crate) fn bubbles_mut(&mut self) -> impl Iterator<Item = &mut DataBubble> {
        self.bubbles.values_mut()
    }

    // =========================================================================
    // Membership — generic API
    // =========================================================================

    /// Connect any sync member to a data bubble.
    ///
    /// If the member is already in another bubble it is disconnected first.
    pub fn connect(
        &mut self,
        member: SyncMemberId,
        bubble_id: BubbleId,
    ) -> Result<(), BubbleStoreError> {
        // Disconnect from current bubble first (ignore if not in any bubble)
        self.disconnect(member);

        let bubble = self
            .bubbles
            .get_mut(&bubble_id)
            .ok_or(BubbleStoreError::GroupNotFound(bubble_id))?;
        bubble.members.insert(member);
        self.member_to_bubble.insert(member, bubble_id);
        Ok(())
    }

    /// Disconnect a member from its current data bubble.
    ///
    /// Returns the bubble id the member was removed from, or `None` if not
    /// in any bubble.
    pub fn disconnect(&mut self, member: SyncMemberId) -> Option<BubbleId> {
        if let Some(bubble_id) = self.member_to_bubble.remove(&member) {
            if let Some(bubble) = self.bubbles.get_mut(&bubble_id) {
                bubble.members.remove(&member);
            }
            Some(bubble_id)
        } else {
            None
        }
    }

    /// Return the data bubble that contains the given member, if any.
    pub fn group_for_member(&self, member: SyncMemberId) -> Option<BubbleId> {
        self.member_to_bubble.get(&member).copied()
    }

    // =========================================================================
    // Membership — chart forwarding helpers (backward compat)
    // =========================================================================

    /// Connect a chart window to a data bubble.
    ///
    /// Wraps `connect(SyncMemberId::Chart(...))` — all existing chart
    /// call sites compile unchanged.
    pub fn attach_chart_to_bubble(
        &mut self,
        chart_id: ChartId,
        bubble_id: BubbleId,
    ) -> Result<(), BubbleStoreError> {
        self.connect(SyncMemberId::Chart(chart_id.0), bubble_id)
    }

    /// Disconnect a chart window from its current data bubble.
    pub fn detach_chart_from_bubble(&mut self, chart_id: ChartId) -> Option<BubbleId> {
        self.disconnect(SyncMemberId::Chart(chart_id.0))
    }

    /// Return the data bubble that contains the given chart window, if any.
    pub fn bubble_for_chart(&self, chart_id: ChartId) -> Option<BubbleId> {
        self.group_for_member(SyncMemberId::Chart(chart_id.0))
    }

    // =========================================================================
    // Instrument read helpers — convenience wrappers that replace read_* bridge fns
    // =========================================================================

    /// Symbol of the bubble that contains `chart_id`. `None` when no bubble or empty.
    pub fn symbol_for_chart(&self, chart_id: ChartId) -> Option<&str> {
        self.bubble_for_chart(chart_id)
            .and_then(|bid| self.bubble(bid))
            .map(|b| b.address.symbol.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Timeframe of the bubble that contains `chart_id`. `None` when no bubble.
    pub fn timeframe_for_chart(&self, chart_id: ChartId) -> Option<&Timeframe> {
        self.bubble_for_chart(chart_id)
            .and_then(|bid| self.bubble(bid))
            .map(|b| &b.timeframe)
    }

    /// Exchange of the bubble that contains `chart_id`. `None` when no bubble or empty.
    pub fn exchange_for_chart(&self, chart_id: ChartId) -> Option<&str> {
        self.bubble_for_chart(chart_id)
            .and_then(|bid| self.bubble(bid))
            .map(|b| b.address.exchange.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Account type of the bubble that contains `chart_id`. `None` when no bubble or empty.
    pub fn account_type_for_chart(&self, chart_id: ChartId) -> Option<&str> {
        self.bubble_for_chart(chart_id)
            .and_then(|bid| self.bubble(bid))
            .map(|b| b.address.account_type.as_label())
            .filter(|s| !s.is_empty())
    }

    /// Resolve the bubble that owns the given instrument coordinates.
    ///
    /// Reverse of `symbol_for_chart` / `exchange_for_chart` / `account_type_for_chart`:
    /// instead of "what does this chart show?", answers "which bubble holds the
    /// stream for `(exchange, account_type, symbol)`?". Used by the live-data
    /// bridge to route per-Kind series writes into bubble storage (Phase X0).
    ///
    /// Returns the first matching bubble. Instrument coordinates are intended
    /// to be unique per bubble.
    pub fn bubble_for_instrument(
        &self,
        exchange: &str,
        account_type: &str,
        symbol: &str,
    ) -> Option<BubbleId> {
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
        self.bubbles.iter().find_map(|(id, b)| {
            (b.address.exchange == exchange && b.address.account_type == at && b.address.symbol == symbol)
                .then_some(*id)
        })
    }

    /// All bubbles whose `(exchange, account_type, symbol)` matches.
    ///
    /// Required for live-data writers when multiple charts each hold their
    /// own auto-bubble pointing at the same instrument — writing only to
    /// the singular result of `bubble_for_instrument` would starve every
    /// chart except the first one (see `mlc-app-test-harness` unit tier).
    pub fn bubbles_for_instrument(
        &self,
        exchange: &str,
        account_type: &str,
        symbol: &str,
    ) -> Vec<BubbleId> {
        let at: mlc_core::instrument::AccountType = account_type.parse().unwrap_or_default();
        self.bubbles.iter()
            .filter_map(|(id, b)| {
                (b.address.exchange == exchange && b.address.account_type == at && b.address.symbol == symbol)
                    .then_some(*id)
            })
            .collect()
    }

    /// Previous close price of the bubble that contains `chart_id`. `None` when no bubble or unset.
    pub fn prev_close_price_for_chart(&self, chart_id: ChartId) -> Option<f64> {
        self.bubble_for_chart(chart_id)
            .and_then(|bid| self.bubble(bid))
            .and_then(|b| b.prev_close_price)
    }

    // =========================================================================
    // Membership — query helpers
    // =========================================================================

    /// Return all chart members of a bubble as `ChartId` values.
    pub fn chart_member_ids(&self, bubble_id: BubbleId) -> Vec<ChartId> {
        self.bubbles
            .get(&bubble_id)
            .map(|b| {
                b.members
                    .iter()
                    .filter_map(|m| m.as_chart().map(ChartId))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Return all panel member IDs of a bubble.
    pub fn panel_members(&self, bubble_id: BubbleId) -> Vec<u64> {
        self.bubbles
            .get(&bubble_id)
            .map(|b| b.members.iter().filter_map(|m| m.as_panel()).collect())
            .unwrap_or_default()
    }

    /// Return the set of all member `SyncMemberId`s in a bubble, or `None` if
    /// the bubble does not exist.
    pub fn members(
        &self,
        bubble_id: BubbleId,
    ) -> Option<&std::collections::HashSet<SyncMemberId>> {
        self.bubbles.get(&bubble_id).map(|b| &b.members)
    }

    /// Return all peer chart windows of the given chart (every chart member except itself).
    pub fn peers(&self, chart_id: ChartId) -> Vec<ChartId> {
        let member = SyncMemberId::Chart(chart_id.0);
        if let Some(&bubble_id) = self.member_to_bubble.get(&member) {
            if let Some(bubble) = self.bubbles.get(&bubble_id) {
                return bubble
                    .members
                    .iter()
                    .filter_map(|m| m.as_chart().map(ChartId))
                    .filter(|&id| id != chart_id)
                    .collect();
            }
        }
        Vec::new()
    }

    // =========================================================================
    // Synced panels — panel tracking
    // =========================================================================

    /// Put a panel in Synced state: register in `synced_panels` and connect to
    /// the given bubble.
    pub fn set_synced(&mut self, panel: SyncMemberId, bubble_id: BubbleId) {
        self.synced_panels.insert(panel);
        let _ = self.connect(panel, bubble_id);
    }

    /// Put a panel in auto-created bubble state: remove from `synced_panels`
    /// and connect to its own private auto-created bubble.
    pub fn set_auto_group(&mut self, panel: SyncMemberId, private_bubble_id: BubbleId) {
        self.synced_panels.remove(&panel);
        let _ = self.connect(panel, private_bubble_id);
    }

    /// Returns `true` if the panel is currently in Synced state.
    pub fn is_synced(&self, panel: SyncMemberId) -> bool {
        self.synced_panels.contains(&panel)
    }

    /// Snapshot the synced_panels set (for persistence).
    pub fn synced_panels_snapshot(&self) -> Vec<SyncMemberId> {
        self.synced_panels.iter().copied().collect()
    }

    /// Remove a member from `synced_panels` without affecting bubble membership.
    ///
    /// Call this when permanently removing a panel (e.g. user closes the panel).
    /// Pair with `disconnect` to fully deregister a panel from the bubble system.
    pub fn synced_panels_remove(&mut self, member: SyncMemberId) {
        self.synced_panels.remove(&member);
    }

    /// Called when the active chart changes.
    ///
    /// Moves all Synced panels to the new bubble so they follow the active chart.
    pub fn reassign_synced_panels(&mut self, new_bubble_id: BubbleId) {
        let panels: Vec<SyncMemberId> = self.synced_panels.iter().copied().collect();
        for member in panels {
            self.disconnect(member);
            let _ = self.connect(member, new_bubble_id);
        }
        self.active_chart_bubble = Some(new_bubble_id);
    }

    // =========================================================================
    // Primitive operations
    // =========================================================================

    /// Return the primitives owned by the data bubble that contains the window.
    pub fn primitives_for_window(
        &self,
        chart_id: ChartId,
    ) -> Option<&[Box<dyn mlc_core::drawing::primitives_v2::Primitive>]> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self.member_to_bubble.get(&member)?;
        let bubble = self.bubbles.get(bubble_id)?;
        Some(&bubble.primitives)
    }

    /// Add a primitive to the data bubble that contains the given window.
    pub fn add_primitive(
        &mut self,
        chart_id: ChartId,
        prim: Box<dyn mlc_core::drawing::primitives_v2::Primitive>,
    ) -> Result<(), BubbleStoreError> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self
            .member_to_bubble
            .get(&member)
            .copied()
            .ok_or(BubbleStoreError::MemberNotInGroup(member))?;
        let bubble = self
            .bubbles
            .get_mut(&bubble_id)
            .ok_or(BubbleStoreError::GroupNotFound(bubble_id))?;
        bubble.primitives.push(prim);
        Ok(())
    }

    /// Remove a primitive by id from the data bubble that contains the given window.
    pub fn remove_primitive(
        &mut self,
        chart_id: ChartId,
        prim_id: u64,
    ) -> Result<(), BubbleStoreError> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self
            .member_to_bubble
            .get(&member)
            .copied()
            .ok_or(BubbleStoreError::MemberNotInGroup(member))?;
        let bubble = self
            .bubbles
            .get_mut(&bubble_id)
            .ok_or(BubbleStoreError::GroupNotFound(bubble_id))?;
        let len_before = bubble.primitives.len();
        bubble.primitives.retain(|p| p.data().id != prim_id);
        if bubble.primitives.len() == len_before {
            return Err(BubbleStoreError::PrimitiveNotFound(prim_id));
        }
        Ok(())
    }

    // =========================================================================
    // Indicator config operations
    // =========================================================================

    /// Add an indicator config to the data bubble that contains the given window.
    ///
    /// Returns the newly assigned config id.
    pub fn add_indicator_config(
        &mut self,
        chart_id: ChartId,
        type_id: &str,
        name: &str,
        pane: u32,
    ) -> Result<u64, BubbleStoreError> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self
            .member_to_bubble
            .get(&member)
            .copied()
            .ok_or(BubbleStoreError::MemberNotInGroup(member))?;
        let bubble = self
            .bubbles
            .get_mut(&bubble_id)
            .ok_or(BubbleStoreError::GroupNotFound(bubble_id))?;
        let config_id = self.next_indicator_config_id;
        self.next_indicator_config_id += 1;
        bubble.indicator_configs.push(IndicatorGroupConfig {
            id: config_id,
            type_id: type_id.to_string(),
            name: name.to_string(),
            params: std::collections::HashMap::new(),
            pane,
            visible: true,
            symbol: String::new(),
            exchange: String::new(),
            account_type: String::new(),
        });
        Ok(config_id)
    }

    /// Remove an indicator config from the data bubble that contains the given window.
    pub fn remove_indicator_config(
        &mut self,
        chart_id: ChartId,
        config_id: u64,
    ) -> Result<(), BubbleStoreError> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self
            .member_to_bubble
            .get(&member)
            .copied()
            .ok_or(BubbleStoreError::MemberNotInGroup(member))?;
        let bubble = self
            .bubbles
            .get_mut(&bubble_id)
            .ok_or(BubbleStoreError::GroupNotFound(bubble_id))?;
        bubble.indicator_configs.retain(|c| c.id != config_id);
        Ok(())
    }

    /// Return the indicator configs for the data bubble that contains the window.
    pub fn indicator_configs_for_window(
        &self,
        chart_id: ChartId,
    ) -> Option<&[IndicatorGroupConfig]> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self.member_to_bubble.get(&member)?;
        let bubble = self.bubbles.get(bubble_id)?;
        Some(&bubble.indicator_configs)
    }

    // =========================================================================
    // Symbol / Timeframe / Instrument
    // =========================================================================

    /// Update the symbol for the data bubble that contains the given window.
    ///
    /// Returns `true` if the window is in a bubble and the write happened.
    pub fn set_symbol(&mut self, chart_id: ChartId, symbol: String) -> bool {
        let member = SyncMemberId::Chart(chart_id.0);
        let Some(bubble_id) = self.member_to_bubble.get(&member).copied() else { return false };
        let Some(bubble) = self.bubbles.get_mut(&bubble_id) else { return false };
        bubble.address.symbol = symbol;
        true
    }

    /// Update the timeframe for the data bubble that contains the given window.
    pub fn set_timeframe(&mut self, chart_id: ChartId, tf: Timeframe) -> bool {
        let member = SyncMemberId::Chart(chart_id.0);
        let Some(bubble_id) = self.member_to_bubble.get(&member).copied() else { return false };
        let Some(bubble) = self.bubbles.get_mut(&bubble_id) else { return false };
        bubble.timeframe = tf;
        true
    }

    /// Update symbol, exchange, and account_type for the data bubble that
    /// contains the given member.
    pub fn set_instrument(
        &mut self,
        member: SyncMemberId,
        symbol: String,
        exchange: String,
        account_type: String,
    ) -> bool {
        let Some(bubble_id) = self.member_to_bubble.get(&member).copied() else { return false };
        let Some(bubble) = self.bubbles.get_mut(&bubble_id) else { return false };
        bubble.address.symbol = symbol;
        bubble.address.exchange = exchange;
        bubble.address.account_type = account_type.parse().unwrap_or_default();
        true
    }

    /// Update exchange and account_type for a bubble known by id directly
    /// (no chart member required). Used by bare/headless bubble creation —
    /// e.g. the daemon's `POST /v1/bubbles` route, which creates a bubble
    /// with no chart attached yet.
    pub fn set_bubble_instrument(&mut self, bid: BubbleId, exchange: String, account_type: String) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.address.exchange = exchange;
        bubble.address.account_type = account_type.parse().unwrap_or_default();
        true
    }

    /// Update symbol/exchange/account_type/timeframe for a bubble known by id
    /// directly. Covers the "assign full instrument to a freshly-created
    /// bubble" shape used by chart-seed / new-window flows.
    pub fn set_bid_instrument_full(
        &mut self,
        bid: BubbleId,
        symbol: String,
        exchange: String,
        account_type: String,
        timeframe: Timeframe,
    ) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.address.symbol = symbol;
        bubble.address.exchange = exchange;
        bubble.address.account_type = account_type.parse().unwrap_or_default();
        bubble.timeframe = timeframe;
        true
    }

    /// Update just the exchange field for a bubble known by id directly.
    pub fn set_exchange_for_bid(&mut self, bid: BubbleId, exchange: String) -> bool {
        let Some(bubble) = self.bubbles.get_mut(&bid) else { return false };
        bubble.address.exchange = exchange;
        true
    }

    // =========================================================================
    // Color helpers
    // =========================================================================

    /// Set the display color of a data bubble.
    pub fn set_color(&mut self, bubble_id: BubbleId, color: [f32; 4]) {
        if let Some(bubble) = self.bubbles.get_mut(&bubble_id) {
            bubble.color = color;
        }
    }

    /// Promote an auto-created (untagged) bubble to a visible tagged bubble.
    ///
    /// Assigns the next unused color from the palette and clears the
    /// `auto_created` flag so the bubble appears in UI color overlays.
    /// No-op if the bubble is not auto-created.
    ///
    /// Returns the assigned color, or `None` if the bubble does not exist
    /// or was already tagged.
    pub fn promote_to_tagged(&mut self, bubble_id: BubbleId) -> Option<[f32; 4]> {
        let color = self.next_unused_color();
        let bubble = self.bubbles.get_mut(&bubble_id)?;
        if !bubble.auto_created {
            return None;
        }
        bubble.auto_created = false;
        bubble.color = color;
        Some(color)
    }

    /// Find a bubble by its display color.
    ///
    /// This is a bridge for UI code that identifies bubbles by color rather than id.
    pub fn find_group_by_color(&self, color: [f32; 4]) -> Option<BubbleId> {
        self.bubbles
            .values()
            .find(|b| {
                !b.auto_created
                    && (b.color[0] - color[0]).abs() < 0.01
                    && (b.color[1] - color[1]).abs() < 0.01
                    && (b.color[2] - color[2]).abs() < 0.01
            })
            .map(|b| b.id)
    }

    /// Pick the next color from the preset palette that is not already used by
    /// an existing data bubble.
    ///
    /// Falls back to the first preset color if all are taken.
    pub fn next_unused_color(&self) -> [f32; 4] {
        use mlc_core::state::toolbar_geometry::PRESET_COLORS;
        // Only visible (non-auto) bubbles occupy palette colors
        let used_colors: Vec<[f32; 4]> = self.bubbles.values()
            .filter(|b| !b.auto_created)
            .map(|b| b.color)
            .collect();
        for &preset in PRESET_COLORS.iter() {
            let is_used = used_colors.iter().any(|uc| {
                (uc[0] - preset[0]).abs() < 0.01
                    && (uc[1] - preset[1]).abs() < 0.01
                    && (uc[2] - preset[2]).abs() < 0.01
            });
            if !is_used {
                return preset;
            }
        }
        // All preset colors are in use — return the first one
        PRESET_COLORS[0]
    }

    // =========================================================================
    // Preset restore helpers
    // =========================================================================

    /// Remove all data bubbles and reset the member-to-bubble index.
    ///
    /// Used during preset restore to start from a clean slate before
    /// re-inserting bubbles reconstructed from snapshots.
    pub fn clear(&mut self) {
        self.bubbles.clear();
        self.member_to_bubble.clear();
        self.next_indicator_config_id = 1;
        self.active_chart_bubble = None;
        self.synced_panels.clear();
    }

    /// Insert a fully-constructed [`DataBubble`] using its own id.
    ///
    /// No new id is generated. If a bubble with the same id already exists it
    /// is silently replaced. After inserting, also rebuilds the
    /// `member_to_bubble` reverse-index for all members already recorded on
    /// the bubble.
    ///
    /// Private — the only caller is [`Self::restore_from_snapshots`]. External
    /// callers use that instead (Step 3.2, doctrine-cutover-2026-06-27.md §1A).
    fn insert_group_raw(&mut self, bubble: DataBubble) {
        let bubble_id = bubble.id;
        for &member_id in &bubble.members {
            self.member_to_bubble.insert(member_id, bubble_id);
        }
        self.bubbles.insert(bubble_id, bubble);
    }

    /// Rebuild the entire bubble registry from persisted snapshots.
    ///
    /// For each snapshot: constructs a [`DataBubble`] via
    /// [`DataBubble::from_snapshot`], backfills primitive `symbol`/`exchange`/
    /// `account_type` from the bubble's own instrument fields (for primitives
    /// saved before those fields existed), inserts the bubble, and invokes
    /// `synced_panel_restorer` once per restored `SyncMemberId::Panel` member
    /// that was in the Synced set at save time.
    ///
    /// Does NOT call [`Self::clear`] — callers that want a full wipe-and-
    /// rebuild call `clear()` first.
    ///
    /// Returns the ids of every bubble restored, in snapshot order.
    pub fn restore_from_snapshots(
        &mut self,
        snaps: &[mlc_core::preset::snapshots::DataBubbleSnapshot],
        mut synced_panel_restorer: impl FnMut(SyncMemberId, BubbleId),
    ) -> Vec<BubbleId> {
        let mut restored_ids = Vec::with_capacity(snaps.len());
        for snap in snaps {
            let mut group = DataBubble::from_snapshot(snap);
            if let Ok(reg) = mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry::global().read() {
                for ps in &snap.primitives {
                    if let Some(mut prim) = reg.from_json(&ps.type_id, &ps.json) {
                        // Backfill symbol/exchange/account_type for primitives
                        // saved before these fields existed (serde(default)
                        // gives empty string). Use the group's shared
                        // instrument as source of truth.
                        if prim.data().symbol.is_empty() && !snap.symbol.is_empty() {
                            prim.data_mut().symbol = snap.symbol.clone();
                        }
                        if prim.data().exchange.is_empty() && !snap.exchange.is_empty() {
                            prim.data_mut().exchange = snap.exchange.clone();
                        }
                        if prim.data().account_type.is_empty() && !snap.account_type.is_empty() {
                            prim.data_mut().account_type = snap.account_type.clone();
                        }
                        group.primitives.push(prim);
                    }
                }
            }
            let gid = group.id;
            self.insert_group_raw(group);
            for &pid in &snap.synced_panel_ids {
                synced_panel_restorer(SyncMemberId::Panel(pid), gid);
            }
            restored_ids.push(gid);
        }
        restored_ids
    }

    // =========================================================================
    // Bid-keyed read intents (Step 3.2 §2.2 — the `Bubbles`/`BubblesMut`
    // facade forwards to these).
    // =========================================================================

    /// OHLC bars for a bubble. Empty slice if the bubble doesn't exist.
    pub fn bars(&self, bid: BubbleId) -> &[mlc_core::Bar] {
        self.bubbles.get(&bid).map(|b| b.bars.as_slice()).unwrap_or(&[])
    }

    /// Footprint cluster bars, if any have been written.
    pub fn footprint_bars(&self, bid: BubbleId) -> Option<&[mlc_core::types::FootprintBar]> {
        self.bubbles.get(&bid)?.footprint_bars.as_deref()
    }

    /// Renko bricks, if any have been written.
    pub fn renko_bars(&self, bid: BubbleId) -> Option<&[mlc_core::types::RenkoBrick]> {
        self.bubbles.get(&bid)?.renko_bars.as_deref()
    }

    /// Three Line Break lines, if any have been written.
    pub fn three_line_break(&self, bid: BubbleId) -> Option<&[mlc_core::types::ThreeLineBreakLine]> {
        self.bubbles.get(&bid)?.three_line_break.as_deref()
    }

    /// Point-and-Figure columns, if any have been written.
    pub fn pnf_columns(&self, bid: BubbleId) -> Option<&[mlc_core::types::PnfColumn]> {
        self.bubbles.get(&bid)?.pnf_columns.as_deref()
    }

    /// Kagi segments, if any have been written.
    pub fn kagi_segments(&self, bid: BubbleId) -> Option<&[mlc_core::types::KagiSegment]> {
        self.bubbles.get(&bid)?.kagi_segments.as_deref()
    }

    /// Cumulative Volume Delta samples, if any have been written.
    pub fn cvd_line(&self, bid: BubbleId) -> Option<&[mlc_core::types::CvdSample]> {
        self.bubbles.get(&bid)?.cvd_line.as_deref()
    }

    /// TPO Market Profile session snapshots, if any have been written.
    pub fn tpo_sessions(&self, bid: BubbleId) -> Option<&[mlc_core::types::TpoSession]> {
        self.bubbles.get(&bid)?.tpo_sessions.as_deref()
    }

    /// Open Interest Δ bars, if any have been written.
    pub fn oi_delta(&self, bid: BubbleId) -> Option<&[mlc_core::types::OiDeltaBar]> {
        self.bubbles.get(&bid)?.oi_delta.as_deref()
    }

    /// Funding Rate cells, if any have been written.
    pub fn funding_rate(&self, bid: BubbleId) -> Option<&[mlc_core::types::FundingRateCell]> {
        self.bubbles.get(&bid)?.funding_rate.as_deref()
    }

    /// DOM heatmap ring buffer, if any has been written.
    pub fn dom_heatmap(&self, bid: BubbleId) -> Option<&mlc_core::chart::dom_heatmap::DomHeatmapBuffer> {
        self.bubbles.get(&bid)?.dom_heatmap.as_ref()
    }

    /// DOM trade-circle ring overlay, if any has been written.
    pub fn dom_trade_dots(&self, bid: BubbleId) -> Option<&[mlc_core::chart::dom_heatmap::DomTradeDot]> {
        self.bubbles.get(&bid)?.dom_trade_dots.as_deref()
    }

    /// Liquidation heatmap ring buffer, if any has been written.
    pub fn liq_heatmap(&self, bid: BubbleId) -> Option<&mlc_core::chart::liquidation_heatmap::LiquidationHeatmapBuffer> {
        self.bubbles.get(&bid)?.liq_heatmap.as_ref()
    }

    /// Liquidation dot overlay, if any has been written.
    pub fn liq_dots(&self, bid: BubbleId) -> Option<&[mlc_core::chart::liquidation_heatmap::LiquidationDot]> {
        self.bubbles.get(&bid)?.liq_dots.as_deref()
    }

    /// Symbol of a bubble known by id directly. `None` if missing or empty.
    pub fn symbol(&self, bid: BubbleId) -> Option<&str> {
        self.bubbles.get(&bid).map(|b| b.address.symbol.as_str()).filter(|s| !s.is_empty())
    }

    /// Exchange of a bubble known by id directly. `None` if missing or empty.
    pub fn exchange(&self, bid: BubbleId) -> Option<&str> {
        self.bubbles.get(&bid).map(|b| b.address.exchange.as_str()).filter(|s| !s.is_empty())
    }

    /// Account type of a bubble known by id directly. `None` if missing or empty.
    pub fn account_type(&self, bid: BubbleId) -> Option<&str> {
        self.bubbles.get(&bid).map(|b| b.address.account_type.as_label()).filter(|s| !s.is_empty())
    }

    /// Timeframe of a bubble known by id directly.
    pub fn timeframe(&self, bid: BubbleId) -> Option<&Timeframe> {
        self.bubbles.get(&bid).map(|b| &b.timeframe)
    }

    /// Bundled instrument-address + timeframe read — the shape most DTO
    /// builders (e.g. the daemon's bubble snapshot route) actually want.
    pub fn instrument(&self, bid: BubbleId) -> Option<InstrumentKey> {
        let b = self.bubbles.get(&bid)?;
        Some(InstrumentKey {
            address: b.address.clone(),
            timeframe: b.timeframe.clone(),
        })
    }

    /// Display color of a bubble known by id directly.
    pub fn color(&self, bid: BubbleId) -> Option<[f32; 4]> {
        self.bubbles.get(&bid).map(|b| b.color)
    }

    /// Price-scale corner mode of a bubble known by id directly.
    pub fn scale_mode(&self, bid: BubbleId) -> Option<ScaleMode> {
        self.bubbles.get(&bid).map(|b| b.scale_mode)
    }

    /// Sync flags of a bubble known by id directly.
    pub fn sync_flags(&self, bid: BubbleId) -> Option<SyncFlags> {
        self.bubbles.get(&bid).map(|b| b.sync_flags.clone())
    }

    /// Shared undo/redo history of a bubble known by id directly.
    pub fn command_history(&self, bid: BubbleId) -> Option<&CommandHistory> {
        self.bubbles.get(&bid).map(|b| &b.command_history)
    }

    /// Previous session close price of a bubble known by id directly.
    pub fn prev_close(&self, bid: BubbleId) -> Option<f64> {
        self.bubbles.get(&bid).and_then(|b| b.prev_close_price)
    }

    /// Live/Delayed/Disconnected status of a bubble known by id directly.
    pub fn connection_status(&self, bid: BubbleId) -> Option<ConnectionStatus> {
        self.bubbles.get(&bid).map(|b| b.connection_status)
    }

    /// Indicator configs of a bubble known by id directly. Empty slice if missing.
    pub fn indicator_configs(&self, bid: BubbleId) -> &[IndicatorGroupConfig] {
        self.bubbles.get(&bid).map(|b| b.indicator_configs.as_slice()).unwrap_or(&[])
    }

    /// Whether a bubble known by id directly is an invisible auto-created
    /// bubble (no user color tag). `false` if the bubble doesn't exist.
    pub fn auto_created(&self, bid: BubbleId) -> bool {
        self.bubbles.get(&bid).map(|b| b.auto_created).unwrap_or(false)
    }

    /// Indicator instance ids hidden pre-tag (desync/rejoin bookkeeping).
    /// Empty slice if missing.
    pub fn pre_tag_indicator_ids(&self, bid: BubbleId) -> &[u64] {
        self.bubbles.get(&bid).map(|b| b.pre_tag_indicator_ids.as_slice()).unwrap_or(&[])
    }

    /// The bubble that the currently-active chart belongs to.
    pub fn active_chart_bubble(&self) -> Option<BubbleId> {
        self.active_chart_bubble
    }

    // =========================================================================
    // Bid-keyed write intents (Step 3.2 §2.4)
    // =========================================================================

    /// Append one bar to a bubble's OHLC series.
    pub fn append_bar(&mut self, bid: BubbleId, bar: mlc_core::Bar) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.append_bar(bar);
        }
    }

    /// Replace a bubble's entire OHLC series.
    pub fn replace_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.replace_bars(bars);
        }
    }

    /// Merge a page of scroll-loaded bars into the existing series by
    /// timestamp (last-write-wins on collision — the live ring's version of
    /// any overlapping bar takes priority), sorted oldest→newest. Returns the
    /// merged series so the caller can build the matching `BubbleEvent`.
    pub fn merge_scroll_bars(&mut self, bid: BubbleId, bars: &[mlc_core::Bar]) -> Vec<mlc_core::Bar> {
        let Some(b) = self.bubbles.get_mut(&bid) else { return Vec::new() };
        let mut by_ts: std::collections::BTreeMap<i64, mlc_core::Bar> = std::collections::BTreeMap::new();
        for bar in bars { by_ts.insert(bar.timestamp, *bar); }
        for bar in &b.bars { by_ts.insert(bar.timestamp, *bar); }
        let merged: Vec<mlc_core::Bar> = by_ts.into_values().collect();
        b.replace_bars(merged.clone());
        merged
    }

    /// Replace a bubble's entire OHLC series (restore/hydration path — same
    /// underlying write as `replace_bars`, distinct name for call-site intent
    /// clarity between "live tick" and "restore").
    pub fn set_bars(&mut self, bid: BubbleId, bars: Vec<mlc_core::Bar>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.replace_bars(bars);
        }
    }

    /// Set the price-scale corner mode of a bubble known by id directly.
    pub fn set_scale_mode(&mut self, bid: BubbleId, mode: ScaleMode) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.scale_mode = mode;
        }
    }

    /// Set the previous session close price of a bubble known by id directly.
    pub fn set_prev_close(&mut self, bid: BubbleId, price: Option<f64>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.prev_close_price = price;
        }
    }

    /// Set the Live/Delayed/Disconnected status of a bubble known by id directly.
    pub fn set_connection_status(&mut self, bid: BubbleId, status: ConnectionStatus) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.connection_status = status;
        }
    }

    /// Set the pre-tag indicator ids of a bubble known by id directly
    /// (desync-join bookkeeping write).
    pub fn set_pre_tag_indicator_ids(&mut self, bid: BubbleId, ids: Vec<u64>) {
        if let Some(b) = self.bubbles.get_mut(&bid) {
            b.pre_tag_indicator_ids = ids;
        }
    }
}

/// Bundled `{ address, timeframe }` read — see [`BubbleStore::instrument`].
#[derive(Debug, Clone)]
pub struct InstrumentKey {
    pub address: InstrumentAddress,
    pub timeframe: Timeframe,
}

impl Default for BubbleStore {
    fn default() -> Self {
        Self::new()
    }
}
