//! TagManager — synchronization group manager for chart panels.
//!
//! Manages `SyncGroup`s that bind multiple `ChartWindow`s together so they
//! share primitives, indicator configs, symbol, timeframe, and crosshair state.

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

/// Configuration for an indicator that belongs to a sync group.
///
/// All windows in the group share these configs so that adding an indicator
/// to one window can optionally propagate it to peers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IndicatorGroupConfig {
    pub id: u64,
    pub type_id: String,
    pub name: String,
    /// Simplified string parameters — typed params can be added later.
    pub params: std::collections::HashMap<String, String>,
    pub pane: u32,
    pub visible: bool,
    /// The symbol this indicator was created for.
    pub symbol: String,
}

// =============================================================================
// SyncGroup
// =============================================================================

/// A group of chart windows that synchronize shared state.
pub struct SyncGroup {
    pub id: SyncGroupId,
    /// Display color used in the UI to identify this group.
    pub color: [f32; 4],

    // Owned state shared across all member windows
    pub primitives: Vec<Box<dyn crate::drawing::primitives_v2::Primitive>>,
    pub indicator_configs: Vec<IndicatorGroupConfig>,
    pub symbol: String,
    pub timeframe: Timeframe,

    /// Which properties are synchronized.
    pub sync_flags: SyncFlags,

    /// The set of `ChartId`s that belong to this group.
    pub members: std::collections::HashSet<ChartId>,

    /// Shared undo/redo history for operations that affect all members
    /// (primitives, indicator configs). Window-local operations (viewport,
    /// symbol, timeframe, chart type) are stored on the individual window's
    /// `command_history` instead.
    pub command_history: CommandHistory,

    /// Invisible default group — auto-created so every window has a group.
    /// No color tag shown in UI. Set to false when user manually tags.
    pub auto_created: bool,
}

impl Clone for SyncGroup {
    fn clone(&self) -> Self {
        Self {
            id: self.id,
            color: self.color,
            primitives: self.primitives.iter().map(|p| p.clone_box()).collect(),
            indicator_configs: self.indicator_configs.clone(),
            symbol: self.symbol.clone(),
            timeframe: self.timeframe.clone(),
            sync_flags: self.sync_flags.clone(),
            members: self.members.clone(),
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
    WindowNotInGroup(ChartId),
    GroupNotFound(SyncGroupId),
    PrimitiveNotFound(u64),
}

impl std::fmt::Display for TagManagerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WindowNotInGroup(id) => write!(f, "window {:?} is not in any sync group", id),
            Self::GroupNotFound(id) => write!(f, "group {:?} does not exist", id),
            Self::PrimitiveNotFound(id) => write!(f, "primitive {} not found in group", id),
        }
    }
}

impl std::error::Error for TagManagerError {}

// =============================================================================
// TagManager
// =============================================================================

/// Manages synchronization groups that bind multiple chart windows together.
pub struct TagManager {
    groups: std::collections::HashMap<SyncGroupId, SyncGroup>,
    window_to_group: std::collections::HashMap<ChartId, SyncGroupId>,
    next_indicator_config_id: u64,
}

impl TagManager {
    /// Create an empty `TagManager`.
    pub fn new() -> Self {
        Self {
            groups: std::collections::HashMap::new(),
            window_to_group: std::collections::HashMap::new(),
            next_indicator_config_id: 1,
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
            timeframe,
            sync_flags: SyncFlags::default(),
            members: std::collections::HashSet::new(),
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
            timeframe,
            sync_flags: SyncFlags {
                sync_crosshair: false,
                sync_viewport: false,
                sync_symbol: false,
                sync_timeframe: false,
                sync_drawings: false,
                sync_indicators: false,
            },
            members: std::collections::HashSet::new(),
            command_history: CommandHistory::new(250),
            auto_created: true,
        };
        self.groups.insert(id, group);
        id
    }

    /// Remove a sync group and unmap all its member windows.
    pub fn remove_group(&mut self, id: SyncGroupId) {
        if let Some(group) = self.groups.remove(&id) {
            for chart_id in &group.members {
                self.window_to_group.remove(chart_id);
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
    // Membership
    // =========================================================================

    /// Connect a chart window to a sync group.
    ///
    /// If the window is already in another group it is disconnected first.
    pub fn connect(
        &mut self,
        chart_id: ChartId,
        group_id: SyncGroupId,
    ) -> Result<(), TagManagerError> {
        // Disconnect from current group first (ignore if not in any group)
        self.disconnect(chart_id);

        let group = self
            .groups
            .get_mut(&group_id)
            .ok_or(TagManagerError::GroupNotFound(group_id))?;
        group.members.insert(chart_id);
        self.window_to_group.insert(chart_id, group_id);
        Ok(())
    }

    /// Disconnect a chart window from its current sync group.
    ///
    /// Returns the group id the window was removed from, or `None` if the
    /// window was not in any group.
    pub fn disconnect(&mut self, chart_id: ChartId) -> Option<SyncGroupId> {
        if let Some(group_id) = self.window_to_group.remove(&chart_id) {
            if let Some(group) = self.groups.get_mut(&group_id) {
                group.members.remove(&chart_id);
            }
            Some(group_id)
        } else {
            None
        }
    }

    /// Return the sync group that contains the given window, if any.
    pub fn group_for_window(&self, chart_id: ChartId) -> Option<SyncGroupId> {
        self.window_to_group.get(&chart_id).copied()
    }

    /// Return the set of all member `ChartId`s in a group, or `None` if the
    /// group does not exist.
    pub fn members(
        &self,
        group_id: SyncGroupId,
    ) -> Option<&std::collections::HashSet<ChartId>> {
        self.groups.get(&group_id).map(|g| &g.members)
    }

    /// Return all peer windows of the given window (every member except itself).
    pub fn peers(&self, chart_id: ChartId) -> Vec<ChartId> {
        if let Some(&group_id) = self.window_to_group.get(&chart_id) {
            if let Some(group) = self.groups.get(&group_id) {
                return group
                    .members
                    .iter()
                    .filter(|&&id| id != chart_id)
                    .copied()
                    .collect();
            }
        }
        Vec::new()
    }

    // =========================================================================
    // Primitive operations
    // =========================================================================

    /// Return the primitives owned by the sync group that contains the window.
    pub fn primitives_for_window(
        &self,
        chart_id: ChartId,
    ) -> Option<&[Box<dyn crate::drawing::primitives_v2::Primitive>]> {
        let group_id = self.window_to_group.get(&chart_id)?;
        let group = self.groups.get(group_id)?;
        Some(&group.primitives)
    }

    /// Add a primitive to the sync group that contains the given window.
    pub fn add_primitive(
        &mut self,
        chart_id: ChartId,
        prim: Box<dyn crate::drawing::primitives_v2::Primitive>,
    ) -> Result<(), TagManagerError> {
        let group_id = self
            .window_to_group
            .get(&chart_id)
            .copied()
            .ok_or(TagManagerError::WindowNotInGroup(chart_id))?;
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
        let group_id = self
            .window_to_group
            .get(&chart_id)
            .copied()
            .ok_or(TagManagerError::WindowNotInGroup(chart_id))?;
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
        symbol: &str,
    ) -> Result<u64, TagManagerError> {
        let group_id = self
            .window_to_group
            .get(&chart_id)
            .copied()
            .ok_or(TagManagerError::WindowNotInGroup(chart_id))?;
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
            symbol: symbol.to_string(),
        });
        Ok(config_id)
    }

    /// Remove an indicator config from the sync group that contains the given window.
    pub fn remove_indicator_config(
        &mut self,
        chart_id: ChartId,
        config_id: u64,
    ) -> Result<(), TagManagerError> {
        let group_id = self
            .window_to_group
            .get(&chart_id)
            .copied()
            .ok_or(TagManagerError::WindowNotInGroup(chart_id))?;
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
        let group_id = self.window_to_group.get(&chart_id)?;
        let group = self.groups.get(group_id)?;
        Some(&group.indicator_configs)
    }

    // =========================================================================
    // Symbol / Timeframe
    // =========================================================================

    /// Update the symbol for the sync group that contains the given window.
    ///
    /// Returns a reference to the updated group, or `None` if the window is
    /// not in any group.
    pub fn set_symbol(&mut self, chart_id: ChartId, symbol: String) -> Option<&SyncGroup> {
        let group_id = self.window_to_group.get(&chart_id).copied()?;
        let group = self.groups.get_mut(&group_id)?;
        group.symbol = symbol;
        Some(group)
    }

    /// Update the timeframe for the sync group that contains the given window.
    pub fn set_timeframe(&mut self, chart_id: ChartId, tf: Timeframe) -> Option<&SyncGroup> {
        let group_id = self.window_to_group.get(&chart_id).copied()?;
        let group = self.groups.get_mut(&group_id)?;
        group.timeframe = tf;
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

    /// Remove all sync groups and reset the window-to-group index.
    ///
    /// Used during preset restore to start from a clean slate before
    /// re-inserting groups reconstructed from snapshots.
    pub fn clear(&mut self) {
        self.groups.clear();
        self.window_to_group.clear();
        self.next_indicator_config_id = 1;
    }

    /// Insert a fully-constructed [`SyncGroup`] using its own id.
    ///
    /// No new id is generated. If a group with the same id already exists it
    /// is silently replaced. After inserting, also rebuilds the
    /// `window_to_group` reverse-index for all members already recorded on
    /// the group.
    pub fn insert_group_raw(&mut self, group: SyncGroup) {
        let group_id = group.id;
        for &chart_id in &group.members {
            self.window_to_group.insert(chart_id, group_id);
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
        use crate::state::chart_window::ChartId;

        let members = snap
            .members
            .iter()
            .map(|&raw| ChartId(raw))
            .collect::<std::collections::HashSet<ChartId>>();

        SyncGroup {
            id: SyncGroupId(snap.id),
            color: snap.color,
            primitives: Vec::new(),
            indicator_configs: snap.indicator_configs.clone(),
            symbol: snap.symbol.clone(),
            timeframe: snap.timeframe.clone(),
            sync_flags: snap.sync_flags.clone(),
            members,
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
