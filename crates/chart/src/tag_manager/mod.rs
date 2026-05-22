//! TagManager — synchronization group manager for chart panels.
//!
//! Manages `SyncGroup`s that bind multiple `ChartWindow`s together so they
//! share primitives, indicator configs, symbol, timeframe, and crosshair state.

pub mod member_id;
pub use member_id::{MemberSyncOverride, SyncMemberId};

use serde::{Deserialize, Serialize};

use crate::state::chart_window::ChartId;
use crate::state::{CommandHistory, Timeframe};

// =============================================================================
// SyncGroupId
// =============================================================================

/// Unique identifier for a synchronization group.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SyncGroupId(pub u64);

static NEXT_SYNC_GROUP_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl SyncGroupId {
    /// Generate a new unique `SyncGroupId`.
    pub fn generate() -> Self {
        SyncGroupId(NEXT_SYNC_GROUP_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }

    /// Advance the counter past `raw` so future `generate()` calls never
    /// collide with restored preset IDs.
    pub fn bump_past(raw: u64) {
        let next = raw + 1;
        NEXT_SYNC_GROUP_ID.fetch_max(next, std::sync::atomic::Ordering::SeqCst);
    }
}

// =============================================================================
// SyncFlags
// =============================================================================

/// Controls which properties are synchronized across windows in a `SyncGroup`.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SyncFlags {
    pub sync_crosshair: bool,
    pub sync_viewport: bool,
    pub sync_symbol: bool,
    pub sync_timeframe: bool,
    pub sync_drawings: bool,
    pub sync_indicators: bool,
    /// Sync the price-scale corner mode (Auto / Manual / Focus) across peers.
    /// Each peer keeps its own concrete min/max/zoom — only the discrete mode
    /// (A/M/F) follows the group.
    #[serde(default = "sync_flag_default_true")]
    pub sync_scale_mode: bool,
}

fn sync_flag_default_true() -> bool { true }

impl Default for SyncFlags {
    fn default() -> Self {
        Self {
            sync_crosshair: true,
            sync_viewport: true,
            sync_symbol: true,
            sync_timeframe: true,
            sync_drawings: true,
            sync_indicators: true,
            sync_scale_mode: true,
        }
    }
}

// =============================================================================
// IndicatorGroupConfig
// =============================================================================

/// Configuration for an indicator that belongs to a sync group.
///
/// Bound to the *group* (its windows), NOT to an instrument. Each peer window
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
// SyncGroup
// =============================================================================

/// A group of chart windows and trading panels that synchronize shared state.
pub struct SyncGroup {
    pub id: SyncGroupId,
    /// Display color used in the UI to identify this group.
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

    /// All members of this group: charts AND trading panels.
    pub members: std::collections::HashSet<SyncMemberId>,

    /// Per-member flag overrides. Absent = use group defaults.
    pub member_overrides: std::collections::HashMap<SyncMemberId, MemberSyncOverride>,

    /// Shared undo/redo history for operations that affect all members
    /// (primitives, indicator configs). Window-local operations (viewport,
    /// symbol, timeframe, chart type) are stored on the individual window's
    /// `command_history` instead.
    pub command_history: CommandHistory,

    /// Invisible default group — auto-created so every window has a group.
    /// No color tag shown in UI. Set to false when user manually tags.
    pub auto_created: bool,
}

impl SyncGroup {
    /// Effective `sync_symbol` for a specific member.
    ///
    /// The per-member override (if present) wins over the group-level flag.
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

    /// Effective `sync_scale_mode` for a specific member.
    ///
    /// Only the discrete A/M/F mode follows the group — min/max/zoom stay
    /// per-member.
    pub fn effective_scale_mode_sync(&self, member: SyncMemberId) -> bool {
        self.member_overrides
            .get(&member)
            .and_then(|o| o.sync_scale_mode)
            .unwrap_or(self.sync_flags.sync_scale_mode)
    }
}

impl Clone for SyncGroup {
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
            members: self.members.clone(),
            member_overrides: self.member_overrides.clone(),
            command_history: CommandHistory::new(250),
            auto_created: self.auto_created,
        }
    }
}

impl std::fmt::Debug for SyncGroup {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SyncGroup")
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
// TagManagerError
// =============================================================================

/// Errors returned by `TagManager` operations.
#[derive(Debug)]
pub enum TagManagerError {
    /// The given member is not registered in any sync group.
    MemberNotInGroup(SyncMemberId),
    GroupNotFound(SyncGroupId),
    PrimitiveNotFound(u64),
}

impl std::fmt::Display for TagManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MemberNotInGroup(id) => write!(f, "member {:?} is not in any sync group", id),
            Self::GroupNotFound(id) => write!(f, "group {:?} does not exist", id),
            Self::PrimitiveNotFound(id) => write!(f, "primitive {} not found in group", id),
        }
    }
}

impl std::error::Error for TagManagerError {}

// =============================================================================
// TagManager
// =============================================================================

/// Manages synchronization groups that bind multiple chart windows and panels.
pub struct TagManager {
    groups: std::collections::HashMap<SyncGroupId, SyncGroup>,
    /// Reverse index: member → group.
    member_to_group: std::collections::HashMap<SyncMemberId, SyncGroupId>,
    next_indicator_config_id: u64,
    /// The group that the currently-active chart belongs to.
    /// Updated whenever the active chart changes.
    pub active_chart_group: Option<SyncGroupId>,
    /// Panel members currently in Synced state.
    /// Their group membership is dynamic — reassigned when the active chart changes.
    synced_panels: std::collections::HashSet<SyncMemberId>,
}

impl TagManager {
    /// Create an empty `TagManager`.
    pub fn new() -> Self {
        Self {
            groups: std::collections::HashMap::new(),
            member_to_group: std::collections::HashMap::new(),
            next_indicator_config_id: 1,
            active_chart_group: None,
            synced_panels: std::collections::HashSet::new(),
        }
    }

    // =========================================================================
    // Group lifecycle
    // =========================================================================

    /// Create a new sync group with the given color, symbol, and timeframe.
    ///
    /// Returns the id of the newly created group.
    pub fn create_group(
        &mut self,
        color: [f32; 4],
        symbol: String,
        timeframe: Timeframe,
    ) -> SyncGroupId {
        let id = SyncGroupId::generate();
        let group = SyncGroup {
            id,
            color,
            primitives: Vec::new(),
            indicator_configs: Vec::new(),
            symbol,
            exchange: String::new(),
            account_type: String::new(),
            timeframe,
            sync_flags: SyncFlags::default(),
            members: std::collections::HashSet::new(),
            member_overrides: std::collections::HashMap::new(),
            command_history: CommandHistory::new(250),
            auto_created: false,
        };
        self.groups.insert(id, group);
        id
    }

    /// Create an invisible default group (no color tag in UI).
    pub fn create_group_auto(
        &mut self,
        color: [f32; 4],
        symbol: String,
        timeframe: Timeframe,
    ) -> SyncGroupId {
        let id = SyncGroupId::generate();
        let group = SyncGroup {
            id,
            color,
            primitives: Vec::new(),
            indicator_configs: Vec::new(),
            symbol,
            exchange: String::new(),
            account_type: String::new(),
            timeframe,
            sync_flags: SyncFlags {
                sync_crosshair: false,
                sync_viewport: false,
                sync_symbol: false,
                sync_timeframe: false,
                sync_drawings: false,
                sync_indicators: false,
                sync_scale_mode: false,
            },
            members: std::collections::HashSet::new(),
            member_overrides: std::collections::HashMap::new(),
            command_history: CommandHistory::new(250),
            auto_created: true,
        };
        self.groups.insert(id, group);
        id
    }

    /// Remove a sync group and unmap all its members.
    pub fn remove_group(&mut self, id: SyncGroupId) {
        if let Some(group) = self.groups.remove(&id) {
            for member_id in &group.members {
                self.member_to_group.remove(member_id);
            }
        }
    }

    /// Get an immutable reference to a group by id.
    pub fn group(&self, id: SyncGroupId) -> Option<&SyncGroup> {
        self.groups.get(&id)
    }

    /// Get a mutable reference to a group by id.
    pub fn group_mut(&mut self, id: SyncGroupId) -> Option<&mut SyncGroup> {
        self.groups.get_mut(&id)
    }

    /// Iterate over all sync groups.
    pub fn groups(&self) -> impl Iterator<Item = &SyncGroup> {
        self.groups.values()
    }

    /// Iterate over all sync groups mutably.
    pub fn groups_mut(&mut self) -> impl Iterator<Item = &mut SyncGroup> {
        self.groups.values_mut()
    }

    // =========================================================================
    // Membership — generic API
    // =========================================================================

    /// Connect any sync member to a sync group.
    ///
    /// If the member is already in another group it is disconnected first.
    pub fn connect(
        &mut self,
        member: SyncMemberId,
        group_id: SyncGroupId,
    ) -> Result<(), TagManagerError> {
        // Disconnect from current group first (ignore if not in any group)
        self.disconnect(member);

        let group = self
            .groups
            .get_mut(&group_id)
            .ok_or(TagManagerError::GroupNotFound(group_id))?;
        group.members.insert(member);
        self.member_to_group.insert(member, group_id);
        Ok(())
    }

    /// Disconnect a member from its current sync group.
    ///
    /// Returns the group id the member was removed from, or `None` if not
    /// in any group.
    pub fn disconnect(&mut self, member: SyncMemberId) -> Option<SyncGroupId> {
        if let Some(group_id) = self.member_to_group.remove(&member) {
            if let Some(group) = self.groups.get_mut(&group_id) {
                group.members.remove(&member);
            }
            Some(group_id)
        } else {
            None
        }
    }

    /// Return the sync group that contains the given member, if any.
    pub fn group_for_member(&self, member: SyncMemberId) -> Option<SyncGroupId> {
        self.member_to_group.get(&member).copied()
    }

    // =========================================================================
    // Membership — chart forwarding helpers (backward compat)
    // =========================================================================

    /// Connect a chart window to a sync group.
    ///
    /// Wraps `connect(SyncMemberId::Chart(...))` — all existing chart
    /// call sites compile unchanged.
    pub fn connect_chart(
        &mut self,
        chart_id: ChartId,
        group_id: SyncGroupId,
    ) -> Result<(), TagManagerError> {
        self.connect(SyncMemberId::Chart(chart_id.0), group_id)
    }

    /// Disconnect a chart window from its current sync group.
    pub fn disconnect_chart(&mut self, chart_id: ChartId) -> Option<SyncGroupId> {
        self.disconnect(SyncMemberId::Chart(chart_id.0))
    }

    /// Return the sync group that contains the given chart window, if any.
    pub fn group_for_window(&self, chart_id: ChartId) -> Option<SyncGroupId> {
        self.group_for_member(SyncMemberId::Chart(chart_id.0))
    }

    // =========================================================================
    // Membership — query helpers
    // =========================================================================

    /// Return all chart members of a group as `ChartId` values.
    pub fn chart_members(&self, group_id: SyncGroupId) -> Vec<ChartId> {
        self.groups
            .get(&group_id)
            .map(|g| {
                g.members
                    .iter()
                    .filter_map(|m| m.as_chart().map(ChartId))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Return all panel member IDs of a group.
    pub fn panel_members(&self, group_id: SyncGroupId) -> Vec<u64> {
        self.groups
            .get(&group_id)
            .map(|g| g.members.iter().filter_map(|m| m.as_panel()).collect())
            .unwrap_or_default()
    }

    /// Return the set of all member `SyncMemberId`s in a group, or `None` if
    /// the group does not exist.
    pub fn members(
        &self,
        group_id: SyncGroupId,
    ) -> Option<&std::collections::HashSet<SyncMemberId>> {
        self.groups.get(&group_id).map(|g| &g.members)
    }

    /// Return all peer chart windows of the given chart (every chart member except itself).
    pub fn peers(&self, chart_id: ChartId) -> Vec<ChartId> {
        let member = SyncMemberId::Chart(chart_id.0);
        if let Some(&group_id) = self.member_to_group.get(&member) {
            if let Some(group) = self.groups.get(&group_id) {
                return group
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
    /// the given group.
    pub fn set_synced(&mut self, panel: SyncMemberId, group_id: SyncGroupId) {
        self.synced_panels.insert(panel);
        let _ = self.connect(panel, group_id);
    }

    /// Put a panel in auto-created group state: remove from `synced_panels`
    /// and connect to its own private auto-created group.
    pub fn set_auto_group(&mut self, panel: SyncMemberId, private_group_id: SyncGroupId) {
        self.synced_panels.remove(&panel);
        let _ = self.connect(panel, private_group_id);
    }

    /// Returns `true` if the panel is currently in Synced state.
    pub fn is_synced(&self, panel: SyncMemberId) -> bool {
        self.synced_panels.contains(&panel)
    }

    /// Remove a member from `synced_panels` without affecting group membership.
    ///
    /// Call this when permanently removing a panel (e.g. user closes the panel).
    /// Pair with `disconnect` to fully deregister a panel from the tag system.
    pub fn synced_panels_remove(&mut self, member: SyncMemberId) {
        self.synced_panels.remove(&member);
    }

    /// Called when the active chart changes.
    ///
    /// Moves all Synced panels to the new group so they follow the active chart.
    pub fn reassign_synced_panels(&mut self, new_group_id: SyncGroupId) {
        let panels: Vec<SyncMemberId> = self.synced_panels.iter().copied().collect();
        for member in panels {
            self.disconnect(member);
            let _ = self.connect(member, new_group_id);
        }
        self.active_chart_group = Some(new_group_id);
    }

    // =========================================================================
    // Primitive operations
    // =========================================================================

    /// Return the primitives owned by the sync group that contains the window.
    pub fn primitives_for_window(
        &self,
        chart_id: ChartId,
    ) -> Option<&[Box<dyn crate::drawing::primitives_v2::Primitive>]> {
        let member = SyncMemberId::Chart(chart_id.0);
        let group_id = self.member_to_group.get(&member)?;
        let group = self.groups.get(group_id)?;
        Some(&group.primitives)
    }

    /// Add a primitive to the sync group that contains the given window.
    pub fn add_primitive(
        &mut self,
        chart_id: ChartId,
        prim: Box<dyn crate::drawing::primitives_v2::Primitive>,
    ) -> Result<(), TagManagerError> {
        let member = SyncMemberId::Chart(chart_id.0);
        let group_id = self
            .member_to_group
            .get(&member)
            .copied()
            .ok_or(TagManagerError::MemberNotInGroup(member))?;
        let group = self
            .groups
            .get_mut(&group_id)
            .ok_or(TagManagerError::GroupNotFound(group_id))?;
        group.primitives.push(prim);
        Ok(())
    }

    /// Remove a primitive by id from the sync group that contains the given window.
    pub fn remove_primitive(
        &mut self,
        chart_id: ChartId,
        prim_id: u64,
    ) -> Result<(), TagManagerError> {
        let member = SyncMemberId::Chart(chart_id.0);
        let group_id = self
            .member_to_group
            .get(&member)
            .copied()
            .ok_or(TagManagerError::MemberNotInGroup(member))?;
        let group = self
            .groups
            .get_mut(&group_id)
            .ok_or(TagManagerError::GroupNotFound(group_id))?;
        let len_before = group.primitives.len();
        group.primitives.retain(|p| p.data().id != prim_id);
        if group.primitives.len() == len_before {
            return Err(TagManagerError::PrimitiveNotFound(prim_id));
        }
        Ok(())
    }

    // =========================================================================
    // Indicator config operations
    // =========================================================================

    /// Add an indicator config to the sync group that contains the given window.
    ///
    /// Returns the newly assigned config id.
    pub fn add_indicator_config(
        &mut self,
        chart_id: ChartId,
        type_id: &str,
        name: &str,
        pane: u32,
    ) -> Result<u64, TagManagerError> {
        let member = SyncMemberId::Chart(chart_id.0);
        let group_id = self
            .member_to_group
            .get(&member)
            .copied()
            .ok_or(TagManagerError::MemberNotInGroup(member))?;
        let group = self
            .groups
            .get_mut(&group_id)
            .ok_or(TagManagerError::GroupNotFound(group_id))?;
        let config_id = self.next_indicator_config_id;
        self.next_indicator_config_id += 1;
        group.indicator_configs.push(IndicatorGroupConfig {
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

    /// Remove an indicator config from the sync group that contains the given window.
    pub fn remove_indicator_config(
        &mut self,
        chart_id: ChartId,
        config_id: u64,
    ) -> Result<(), TagManagerError> {
        let member = SyncMemberId::Chart(chart_id.0);
        let group_id = self
            .member_to_group
            .get(&member)
            .copied()
            .ok_or(TagManagerError::MemberNotInGroup(member))?;
        let group = self
            .groups
            .get_mut(&group_id)
            .ok_or(TagManagerError::GroupNotFound(group_id))?;
        group.indicator_configs.retain(|c| c.id != config_id);
        Ok(())
    }

    /// Return the indicator configs for the sync group that contains the window.
    pub fn indicator_configs_for_window(
        &self,
        chart_id: ChartId,
    ) -> Option<&[IndicatorGroupConfig]> {
        let member = SyncMemberId::Chart(chart_id.0);
        let group_id = self.member_to_group.get(&member)?;
        let group = self.groups.get(group_id)?;
        Some(&group.indicator_configs)
    }

    // =========================================================================
    // Symbol / Timeframe / Instrument
    // =========================================================================

    /// Update the symbol for the sync group that contains the given window.
    ///
    /// Returns a reference to the updated group, or `None` if the window is
    /// not in any group.
    pub fn set_symbol(&mut self, chart_id: ChartId, symbol: String) -> Option<&SyncGroup> {
        let member = SyncMemberId::Chart(chart_id.0);
        let group_id = self.member_to_group.get(&member).copied()?;
        let group = self.groups.get_mut(&group_id)?;
        group.symbol = symbol;
        Some(group)
    }

    /// Update the timeframe for the sync group that contains the given window.
    pub fn set_timeframe(&mut self, chart_id: ChartId, tf: Timeframe) -> Option<&SyncGroup> {
        let member = SyncMemberId::Chart(chart_id.0);
        let group_id = self.member_to_group.get(&member).copied()?;
        let group = self.groups.get_mut(&group_id)?;
        group.timeframe = tf;
        Some(group)
    }

    /// Update symbol, exchange, and account_type for the sync group that
    /// contains the given member.
    pub fn set_instrument(
        &mut self,
        member: SyncMemberId,
        symbol: String,
        exchange: String,
        account_type: String,
    ) -> Option<&SyncGroup> {
        let group_id = self.member_to_group.get(&member).copied()?;
        let group = self.groups.get_mut(&group_id)?;
        group.symbol = symbol;
        group.exchange = exchange;
        group.account_type = account_type;
        Some(group)
    }

    // =========================================================================
    // Color helpers
    // =========================================================================

    /// Set the display color of a sync group.
    pub fn set_color(&mut self, group_id: SyncGroupId, color: [f32; 4]) {
        if let Some(group) = self.groups.get_mut(&group_id) {
            group.color = color;
        }
    }

    /// Find a group by its display color.
    ///
    /// This is a bridge for UI code that identifies groups by color rather than id.
    pub fn find_group_by_color(&self, color: [f32; 4]) -> Option<SyncGroupId> {
        self.groups
            .values()
            .find(|g| {
                !g.auto_created
                    && (g.color[0] - color[0]).abs() < 0.01
                    && (g.color[1] - color[1]).abs() < 0.01
                    && (g.color[2] - color[2]).abs() < 0.01
            })
            .map(|g| g.id)
    }

    /// Pick the next color from the preset palette that is not already used by
    /// an existing sync group.
    ///
    /// Falls back to the first preset color if all are taken.
    pub fn next_unused_color(&self) -> [f32; 4] {
        use crate::ui::sync_color_grid::PRESET_COLORS;
        // Only visible (non-auto) groups occupy palette colors
        let used_colors: Vec<[f32; 4]> = self.groups.values()
            .filter(|g| !g.auto_created)
            .map(|g| g.color)
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

    /// Remove all sync groups and reset the member-to-group index.
    ///
    /// Used during preset restore to start from a clean slate before
    /// re-inserting groups reconstructed from snapshots.
    pub fn clear(&mut self) {
        self.groups.clear();
        self.member_to_group.clear();
        self.next_indicator_config_id = 1;
        self.active_chart_group = None;
        self.synced_panels.clear();
    }

    /// Insert a fully-constructed [`SyncGroup`] using its own id.
    ///
    /// No new id is generated. If a group with the same id already exists it
    /// is silently replaced. After inserting, also rebuilds the
    /// `member_to_group` reverse-index for all members already recorded on
    /// the group.
    pub fn insert_group_raw(&mut self, group: SyncGroup) {
        let group_id = group.id;
        for &member_id in &group.members {
            self.member_to_group.insert(member_id, group_id);
        }
        self.groups.insert(group_id, group);
    }

    /// Reconstruct a [`SyncGroup`] from a [`SyncGroupSnapshot`].
    ///
    /// The resulting group starts with an empty `primitives` list — primitives
    /// are restored separately via the `PrimitiveRegistry` and pushed in
    /// afterwards.
    pub fn group_from_snapshot(
        snap: &crate::preset::snapshots::SyncGroupSnapshot,
    ) -> SyncGroup {
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
                    sync_scale_mode: None,
                };
                (member, override_val)
            })
            .collect();

        SyncGroup {
            id: SyncGroupId(snap.id),
            color: snap.color,
            primitives: Vec::new(),
            indicator_configs: snap.indicator_configs.clone(),
            symbol: snap.symbol.clone(),
            exchange: snap.exchange.clone(),
            account_type: snap.account_type.clone(),
            timeframe: snap.timeframe.clone(),
            sync_flags: snap.sync_flags.clone(),
            members,
            member_overrides,
            command_history: snap.command_history.clone().unwrap_or_else(|| CommandHistory::new(250)),
            auto_created: snap.auto_created,
        }
    }
}

impl Default for TagManager {
    fn default() -> Self {
        Self::new()
    }
}
