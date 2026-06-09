//! BubbleStore — synchronization group manager for chart panels.
//!
//! Manages `DataBubble`s that bind multiple `ChartContainer`s together so they
//! share primitives, indicator configs, symbol, timeframe, and crosshair state.

pub mod member_id;
pub use member_id::{MemberSyncOverride, SyncMemberId};

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use serde::{Deserialize, Serialize};

use crate::chart::annotations::markers::MarkerManager;
use crate::chart::types::price_scale::ScaleMode;
use crate::drawing::primitives_v2::config::TemplateStyle;
use crate::drawing::primitives_v2::{SystemSignal, StrategySignalConfig};
use crate::drawing::Trade;
use crate::state::chart_container::{ChartId, ConnectionStatus};
use crate::state::{CommandHistory, Timeframe};

// =============================================================================
// BubbleId
// =============================================================================

/// Unique identifier for a data bubble (synchronization group).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BubbleId(pub u64);

static NEXT_BUBBLE_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl BubbleId {
    /// Generate a new unique `BubbleId`.
    pub fn generate() -> Self {
        BubbleId(NEXT_BUBBLE_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }

    /// Advance the counter past `raw` so future `generate()` calls never
    /// collide with restored preset IDs.
    pub fn bump_past(raw: u64) {
        let next = raw + 1;
        NEXT_BUBBLE_ID.fetch_max(next, std::sync::atomic::Ordering::SeqCst);
    }
}


// =============================================================================
// SyncFlags
// =============================================================================

/// Controls which properties are synchronized across windows in a `DataBubble`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncFlags {
    pub sync_crosshair: bool,
    pub sync_viewport: bool,
    pub sync_symbol: bool,
    pub sync_timeframe: bool,
    pub sync_drawings: bool,
    pub sync_indicators: bool,
}

impl Default for SyncFlags {
    fn default() -> Self {
        Self {
            sync_crosshair: true,
            sync_viewport: true,
            sync_symbol: true,
            sync_timeframe: true,
            sync_drawings: true,
            sync_indicators: true,
        }
    }
}

// =============================================================================
// IndicatorGroupConfig
// =============================================================================

/// Configuration for an indicator that belongs to a data bubble.
///
/// Bound to the *bubble* (its windows), NOT to an instrument. Each peer window
/// recalculates the indicator against whatever bars its current symbol
/// produces — switching symbol/exchange/account_type does not migrate the
/// indicator.
///
/// Legacy `symbol` / `exchange` / `account_type` fields are kept on disk via
/// `#[serde(default)]` for backwards-compatible profile loading, but they are
/// never read by runtime code.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndicatorGroupConfig {
    pub id: u64,
    pub type_id: String,
    pub name: String,
    /// Simplified string parameters — typed params can be added later.
    pub params: std::collections::HashMap<String, String>,
    pub pane: u32,
    pub visible: bool,
    /// Legacy: symbol this indicator was created for. Unused at runtime —
    /// indicators are window-scoped, not symbol-scoped.
    #[serde(default, skip_serializing)]
    pub symbol: String,
    /// Legacy: exchange. Unused at runtime.
    #[serde(default, skip_serializing)]
    pub exchange: String,
    /// Legacy: account type. Unused at runtime.
    #[serde(default, skip_serializing)]
    pub account_type: String,
}

// =============================================================================
// DataBubble
// =============================================================================

/// A group of chart windows and trading panels that synchronize shared state.
pub struct DataBubble {
    pub id: BubbleId,
    /// Display color used in the UI to identify this bubble.
    pub color: [f32; 4],

    // Owned state shared across all member windows
    pub primitives: Vec<Box<dyn crate::drawing::primitives_v2::Primitive>>,
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

    /// Strategy-generated signal annotations keyed by signal ID.
    ///
    /// Written exclusively by [`crate::managers::Managers::signal`].
    pub signals: std::collections::HashMap<u64, SystemSignal>,

    /// Next signal ID. Advanced by `SignalManager::add_signal`.
    /// Starts at 1; repaired after snapshot restore via `SignalManager::repair_next_id`.
    pub signal_next_id: u64,

    /// Per-strategy rendering configuration (color, size, visibility).
    pub signal_strategies: std::collections::HashMap<String, StrategySignalConfig>,

    /// Global on/off toggle for all signals on this bubble. Default: `false`.
    pub signals_visible: bool,

    /// Trade fill / position overlays keyed by trade ID.
    ///
    /// Written exclusively by [`crate::managers::Managers::trade`].
    pub trades: std::collections::HashMap<u64, Trade>,

    /// Next trade ID. Advanced by `TradeManager::add_trade`.
    /// Starts at 1; repaired after snapshot restore.
    pub trade_next_id: u64,

    /// Global on/off toggle for all trades on this bubble. Default: `true`.
    pub trades_visible: bool,

    /// Show trade connector lines on chart. Default: `true`.
    pub trade_show_lines: bool,

    /// Price / time marker annotations rendered on every member chart.
    pub marker_manager: MarkerManager,
}


impl DataBubble {
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
            signals: self.signals.clone(),
            signal_next_id: self.signal_next_id,
            signal_strategies: self.signal_strategies.clone(),
            signals_visible: self.signals_visible,
            trades: self.trades.clone(),
            trade_next_id: self.trade_next_id,
            trades_visible: self.trades_visible,
            trade_show_lines: self.trade_show_lines,
            marker_manager: self.marker_manager.clone(),
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
            symbol,
            exchange: String::new(),
            account_type: String::new(),
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
            signals: std::collections::HashMap::new(),
            signal_next_id: 1,
            signal_strategies: std::collections::HashMap::new(),
            signals_visible: false,
            trades: std::collections::HashMap::new(),
            trade_next_id: 1,
            trades_visible: true,
            trade_show_lines: true,
            marker_manager: MarkerManager::default(),
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
            symbol,
            exchange: String::new(),
            account_type: String::new(),
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
            signals: std::collections::HashMap::new(),
            signal_next_id: 1,
            signal_strategies: std::collections::HashMap::new(),
            signals_visible: false,
            trades: std::collections::HashMap::new(),
            trade_next_id: 1,
            trades_visible: true,
            trade_show_lines: true,
            marker_manager: MarkerManager::default(),
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
    pub fn bubble(&self, id: BubbleId) -> Option<&DataBubble> {
        self.bubbles.get(&id)
    }

    /// Get a mutable reference to a bubble by id.
    pub fn bubble_mut(&mut self, id: BubbleId) -> Option<&mut DataBubble> {
        self.bubbles.get_mut(&id)
    }

    /// Iterate over all data bubbles.
    pub fn bubbles(&self) -> impl Iterator<Item = &DataBubble> {
        self.bubbles.values()
    }

    /// Iterate over all data bubbles mutably.
    pub fn bubbles_mut(&mut self) -> impl Iterator<Item = &mut DataBubble> {
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
            .map(|b| b.symbol.as_str())
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
            .map(|b| b.exchange.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Account type of the bubble that contains `chart_id`. `None` when no bubble or empty.
    pub fn account_type_for_chart(&self, chart_id: ChartId) -> Option<&str> {
        self.bubble_for_chart(chart_id)
            .and_then(|bid| self.bubble(bid))
            .map(|b| b.account_type.as_str())
            .filter(|s| !s.is_empty())
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
    ) -> Option<&[Box<dyn crate::drawing::primitives_v2::Primitive>]> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self.member_to_bubble.get(&member)?;
        let bubble = self.bubbles.get(bubble_id)?;
        Some(&bubble.primitives)
    }

    /// Add a primitive to the data bubble that contains the given window.
    pub fn add_primitive(
        &mut self,
        chart_id: ChartId,
        prim: Box<dyn crate::drawing::primitives_v2::Primitive>,
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
    /// Returns a reference to the updated bubble, or `None` if the window is
    /// not in any bubble.
    pub fn set_symbol(&mut self, chart_id: ChartId, symbol: String) -> Option<&DataBubble> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self.member_to_bubble.get(&member).copied()?;
        let bubble = self.bubbles.get_mut(&bubble_id)?;
        bubble.symbol = symbol;
        Some(bubble)
    }

    /// Update the timeframe for the data bubble that contains the given window.
    pub fn set_timeframe(&mut self, chart_id: ChartId, tf: Timeframe) -> Option<&DataBubble> {
        let member = SyncMemberId::Chart(chart_id.0);
        let bubble_id = self.member_to_bubble.get(&member).copied()?;
        let bubble = self.bubbles.get_mut(&bubble_id)?;
        bubble.timeframe = tf;
        Some(bubble)
    }

    /// Update symbol, exchange, and account_type for the data bubble that
    /// contains the given member.
    pub fn set_instrument(
        &mut self,
        member: SyncMemberId,
        symbol: String,
        exchange: String,
        account_type: String,
    ) -> Option<&DataBubble> {
        let bubble_id = self.member_to_bubble.get(&member).copied()?;
        let bubble = self.bubbles.get_mut(&bubble_id)?;
        bubble.symbol = symbol;
        bubble.exchange = exchange;
        bubble.account_type = account_type;
        Some(bubble)
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
        use crate::ui::sync_color_grid::PRESET_COLORS;
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
    pub fn insert_group_raw(&mut self, bubble: DataBubble) {
        let bubble_id = bubble.id;
        for &member_id in &bubble.members {
            self.member_to_bubble.insert(member_id, bubble_id);
        }
        self.bubbles.insert(bubble_id, bubble);
    }

    /// Reconstruct a [`DataBubble`] from a [`DataBubbleSnapshot`].
    ///
    /// The resulting bubble starts with an empty `primitives` list — primitives
    /// are restored separately via the `PrimitiveRegistry` and pushed in
    /// afterwards.
    pub fn group_from_snapshot(
        snap: &crate::preset::snapshots::DataBubbleSnapshot,
    ) -> DataBubble {
        // Restore chart members (backward compat — stored as bare u64)
        let mut members = std::collections::HashSet::new();
        for &raw in &snap.members {
            members.insert(SyncMemberId::Chart(raw));
        }
        // Restore panel members (new field — defaults to empty in old presets)
        for &raw in &snap.panel_members {
            members.insert(SyncMemberId::Panel(raw));
        }

        // Restore per-member overrides
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

        // Restore per-tool last-used styles from the snapshot.
        // Old presets have no `last_used_style` field → start empty; the first
        // style change after load will populate it.
        let mut style_map = HashMap::new();
        for (type_id, raw) in &snap.last_used_style {
            if let Ok(style) = serde_json::from_value::<TemplateStyle>(raw.clone()) {
                style_map.insert(type_id.clone(), style);
            }
        }

        let mut bubble = DataBubble {
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
            signals: snap.signals.clone(),
            signal_next_id: 1,
            signal_strategies: snap.signal_strategies.clone(),
            signals_visible: snap.signals_visible,
            trades: snap.trades.clone(),
            trade_next_id: 1,
            trades_visible: snap.trades_visible,
            trade_show_lines: snap.trade_show_lines,
            marker_manager: MarkerManager::default(),
        };

        // Advance ID counters past any restored IDs so new entries never
        // collide with persisted ones.
        if let Some(&max_id) = bubble.signals.keys().max() {
            bubble.signal_next_id = max_id + 1;
        }
        if let Some(&max_id) = bubble.trades.keys().max() {
            bubble.trade_next_id = max_id + 1;
        }

        bubble
    }
}

impl Default for BubbleStore {
    fn default() -> Self {
        Self::new()
    }
}
