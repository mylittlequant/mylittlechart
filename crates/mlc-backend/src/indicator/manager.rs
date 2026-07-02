//! `IndicatorManager` — the stateful indicator registry.
//!
//! Owns indicator instances, the catalog of definitions, per-bubble
//! assignment, recalculation scheduling (dirty/new-bar flags, recalc mode),
//! and the incremental compute-cache index. This is the doctrine "manager"
//! half of the indicator system (§1A/§1B of
//! `docs/mlc/plans/doctrine-cutover-2026-06-27.md`) — the pure compute
//! primitives it calls into (`IndicatorCalculator`, `IndicatorBridge`,
//! `ComputeCache`, the `mli` catalog, `SignalEngine`) stay in
//! `mlc-backend-indicator-engine`, which this crate already depends on.
//!
//! App code never constructs or holds this type directly — access goes
//! through `mlc_backend::indicator::{Indicators, IndicatorsMut}`
//! (`BackendCore::indicators` / `indicators_mut`).

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use mlc_backend_indicator_engine::managers::indicator_bridge::IndicatorBridge;
use mlc_backend_indicator_engine::managers::indicator_calculator::IndicatorCalculator;
use mlc_backend_indicator_engine::managers::indicator_manager::{
    ComputeCache, HistogramStyle, IndicatorCategory, IndicatorDefinition, IndicatorInstance,
    IndicatorOutput, IndicatorOutputType, IndicatorParamType, IndicatorValue, RecalcMode,
};

/// Manages indicator definitions and instances
pub struct IndicatorManager {
    /// Available indicator definitions
    definitions: HashMap<String, IndicatorDefinition>,
    /// Active indicator instances
    instances: HashMap<u64, IndicatorInstance>,
    /// Instances by bubble_id (= color tag / sync group id / home auto-bubble id).
    /// Instances with `bubble_id = None` (legacy/unscoped) are indexed under key 0.
    by_bubble_id: HashMap<u64, Vec<u64>>,
    /// Next instance ID. Global, not per-bubble — unlike `AlertManager.alert_next_id`
    /// (per-bubble, alerts never move bubbles), instance identity can migrate
    /// across bubbles via `assign_bubble`/`reassign_bubble`, so a per-bubble
    /// counter would have to be re-keyed on every reassignment. Single global
    /// counter is the correct shape for an identity space instances can move
    /// across bubbles within (design: `docs/mlc/plans/step-3.3-indicator-watchlist-design.md` §6.1).
    next_id: u64,

    // ── Recalculation scheduling ──────────────────────────────────────────────

    /// Current recalculation mode (controls when indicator values are refreshed).
    pub recalc_mode: RecalcMode,
    /// Symbols that received at least one trade update since the last flush.
    /// Used by PerFrame mode — all dirty symbols are recalculated once per frame.
    dirty_symbols: HashSet<String>,
    /// Symbols where a new bar formed since the last flush.
    /// Used by PerBar mode — only these symbols trigger recalculation.
    new_bar_symbols: HashSet<String>,

    // ── Incremental computation cache ─────────────────────────────────────────

    /// Per-instance stateful compute caches for incremental indicator updates.
    /// Keyed by `IndicatorInstance.id`. Cleared when an instance is removed,
    /// its params change, or bar history resets.
    compute_caches: HashMap<u64, ComputeCache>,

    // ── Profiling counters (updated each calculate_for_bubble call) ───────────

    /// Number of indicator instances that used the O(1) incremental path
    /// in the most recent `calculate_for_bubble` call.
    pub last_incremental_count: u32,
    /// Number of indicator instances that used the O(N) full-recalc path
    /// in the most recent `calculate_for_bubble` call.
    pub last_full_count: u32,
}

impl IndicatorManager {
    /// Create a new indicator manager with the full catalog (480+ indicators)
    pub fn new() -> Self {
        let mut manager = Self {
            definitions: HashMap::new(),
            instances: HashMap::new(),
            by_bubble_id: HashMap::new(),
            next_id: 1,
            recalc_mode: RecalcMode::default(),
            dirty_symbols: HashSet::new(),
            new_bar_symbols: HashSet::new(),
            compute_caches: HashMap::new(),
            last_incremental_count: 0,
            last_full_count: 0,
        };
        manager.register_from_catalog();
        manager
    }

    /// Remove all indicator instances. Used during preset restore.
    ///
    /// Clears active instances and per-symbol indices, then resets the ID
    /// counter to 1 so freshly inserted indicators receive predictable IDs.
    /// Catalog definitions are preserved — they are static and do not need
    /// to be rebuilt after a clear.
    pub fn clear_all(&mut self) {
        self.instances.clear();
        self.by_bubble_id.clear();
        self.next_id = 1;
        self.dirty_symbols.clear();
        self.new_bar_symbols.clear();
        self.compute_caches.clear();
    }

    /// Register all indicators from the unified catalog (480+ indicators)
    fn register_from_catalog(&mut self) {
        let bridge = IndicatorBridge::new();
        let definitions = bridge.get_all_definitions();

        for definition in definitions {
            self.definitions.insert(definition.type_id.clone(), definition);
        }

        self.register_builtins();
    }

    /// Register built-in indicator overlays that are computed directly from
    /// bar data without going through the mli catalog.
    ///
    /// `builtin_volume` — volume histogram strip pinned to the bottom 18 % of
    /// the main chart area.  Uses `IndicatorOutputRenderType::Histogram` so
    /// `draw_volume_overlay` in `render_chart.rs` handles the actual draw.
    fn register_builtins(&mut self) {
        let volume_overlay = IndicatorDefinition {
            type_id: "builtin_volume".to_string(),
            name: "Volume".to_string(),
            short_name: "Volume".to_string(),
            description: "Volume histogram — bars proportional to trade volume, pinned to the bottom of the chart.".to_string(),
            category: IndicatorCategory::Volume,
            params: vec![],
            outputs: vec![IndicatorOutput::histogram("volume", "Volume", "#26A69A80")],
            overlay: true,
            precision: 0,
            bounds: None,
            zero_baseline: true,
            histogram_style: HistogramStyle::FromBottom,
            machine_id: None,
        };
        self.definitions.insert(volume_overlay.type_id.clone(), volume_overlay);
    }

    /// Return the total number of active indicator instances.
    pub fn instances_count(&self) -> usize {
        self.instances.len()
    }

    /// Register a new indicator definition
    pub fn register_definition(&mut self, definition: IndicatorDefinition) {
        self.definitions.insert(definition.type_id.clone(), definition);
    }

    /// Get all available definitions
    pub fn get_definitions(&self) -> Vec<&IndicatorDefinition> {
        self.definitions.values().collect()
    }

    /// Get definition by type ID
    pub fn get_definition(&self, type_id: &str) -> Option<&IndicatorDefinition> {
        self.definitions.get(type_id)
    }

    /// Get definitions by category
    pub fn get_by_category(&self, category: IndicatorCategory) -> Vec<&IndicatorDefinition> {
        self.definitions
            .values()
            .filter(|d| d.category == category)
            .collect()
    }

    /// Create a new indicator instance. `bubble_id = None` keeps the instance
    /// unscoped (legacy / unbound).  Caller usually follows up with
    /// `assign_bubble` to bind it to a bubble — until then it lives in the
    /// "no-bubble" bucket (key 0).
    pub fn create_instance(&mut self, type_id: &str) -> Option<u64> {
        let definition = self.definitions.get(type_id)?.clone();
        let id = self.next_id;
        self.next_id += 1;

        let instance = IndicatorInstance::new(id, &definition);

        self.instances.insert(id, instance);
        self.by_bubble_id.entry(0).or_default().push(id);

        Some(id)
    }

    /// Create instance with a specific ID (for undo/redo)
    /// Returns true if created successfully, false if definition not found or ID already exists
    pub fn create_instance_with_id(&mut self, id: u64, type_id: &str) -> bool {
        // Check if ID already exists
        if self.instances.contains_key(&id) {
            return false;
        }

        let definition = match self.definitions.get(type_id) {
            Some(d) => d.clone(),
            None => return false,
        };

        let instance = IndicatorInstance::new(id, &definition);

        self.instances.insert(id, instance);
        self.by_bubble_id.entry(0).or_default().push(id);

        // Update next_id if needed
        if id >= self.next_id {
            self.next_id = id + 1;
        }

        true
    }

    /// Bind / rebind an existing indicator instance to a bubble.
    /// Updates `instance.bubble_id` and the `by_bubble_id` index so future
    /// queries route the instance to the correct bucket.
    pub fn assign_bubble(&mut self, id: u64, bubble_id: Option<u64>) {
        let prev = match self.instances.get(&id) {
            Some(inst) => inst.bubble_id,
            None => return,
        };
        let prev_key = prev.unwrap_or(0);
        let new_key = bubble_id.unwrap_or(0);
        if prev_key == new_key {
            // Still update the field in case bubble_id changed Some↔None at same numeric value (rare).
            if let Some(inst) = self.instances.get_mut(&id) {
                inst.bubble_id = bubble_id;
            }
            return;
        }
        if let Some(ids) = self.by_bubble_id.get_mut(&prev_key) {
            ids.retain(|&i| i != id);
        }
        self.by_bubble_id.entry(new_key).or_default().push(id);
        if let Some(inst) = self.instances.get_mut(&id) {
            inst.bubble_id = bubble_id;
        }
    }

    /// Remove an instance
    pub fn remove_instance(&mut self, id: u64) -> Option<IndicatorInstance> {
        if let Some(instance) = self.instances.remove(&id) {
            let key = instance.bubble_id.unwrap_or(0);
            if let Some(ids) = self.by_bubble_id.get_mut(&key) {
                ids.retain(|&i| i != id);
            }
            self.compute_caches.remove(&id);
            Some(instance)
        } else {
            None
        }
    }

    /// Remove an instance (alias for remove_instance)
    pub fn remove(&mut self, id: u64) -> Option<IndicatorInstance> {
        self.remove_instance(id)
    }

    /// Remove all cloned indicator instances for a specific bubble (used on desync).
    /// DEPRECATED: Only used by legacy origin_id-based sync. TagManager uses
    /// pre_tag_indicator_ids filtering instead.
    pub fn purge_synced_instances_for_bubble(&mut self, target_bubble_id: u64) {
        let to_remove: Vec<u64> = self.instances.values()
            .filter(|inst| inst.origin_id.is_some() && inst.bubble_id == Some(target_bubble_id))
            .map(|inst| inst.id)
            .collect();

        for id in to_remove {
            if let Some(inst) = self.instances.remove(&id) {
                let key = inst.bubble_id.unwrap_or(0);
                if let Some(ids) = self.by_bubble_id.get_mut(&key) {
                    ids.retain(|&i| i != id);
                }
            }
            self.compute_caches.remove(&id);
        }
    }

    /// Legacy alias for `purge_group_indicators_for_bubble`.
    pub fn purge_group_indicators_for_window(&mut self, target_bubble_id: u64, group_type_ids: &[String]) {
        self.purge_group_indicators_for_bubble(target_bubble_id, group_type_ids)
    }

    /// Remove only indicator instances whose type_id is in `group_type_ids` for the given bubble.
    /// Used when disconnecting from a TagManager group — removes group-owned indicators
    /// but keeps any pre-existing indicators the bubble had before joining.
    pub fn purge_group_indicators_for_bubble(&mut self, target_bubble_id: u64, group_type_ids: &[String]) {
        let to_remove: Vec<u64> = self.instances.values()
            .filter(|inst| {
                inst.bubble_id == Some(target_bubble_id)
                    && group_type_ids.contains(&inst.type_id)
            })
            .map(|inst| inst.id)
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            if let Some(inst) = self.instances.remove(&id) {
                let key = inst.bubble_id.unwrap_or(0);
                if let Some(ids) = self.by_bubble_id.get_mut(&key) {
                    ids.retain(|&i| i != id);
                }
            }
            self.compute_caches.remove(&id);
        }
        if count > 0 {
            eprintln!("[IndicatorManager] Purged {} group indicators for bubble {}", count, target_bubble_id);
        }
    }

    /// Legacy alias for `purge_all_instances_for_bubble`.
    pub fn purge_all_instances_for_window(&mut self, target_bubble_id: u64) {
        self.purge_all_instances_for_bubble(target_bubble_id)
    }

    /// Remove ALL indicator instances belonging to a bubble (regardless of origin_id).
    /// Used when disconnecting from a TagManager group — the group owns the indicators.
    pub fn purge_all_instances_for_bubble(&mut self, target_bubble_id: u64) {
        let to_remove: Vec<u64> = self.instances.values()
            .filter(|inst| inst.bubble_id == Some(target_bubble_id))
            .map(|inst| inst.id)
            .collect();

        let count = to_remove.len();
        for id in to_remove {
            if let Some(inst) = self.instances.remove(&id) {
                let key = inst.bubble_id.unwrap_or(0);
                if let Some(ids) = self.by_bubble_id.get_mut(&key) {
                    ids.retain(|&i| i != id);
                }
            }
            self.compute_caches.remove(&id);
        }
        if count > 0 {
            eprintln!("[IndicatorManager] Purged all {} instances for bubble {}", count, target_bubble_id);
        }
    }

    /// Toggle visibility of an instance
    /// Returns the new visibility state or None if instance not found
    pub fn toggle_visibility(&mut self, id: u64) -> Option<bool> {
        if let Some(instance) = self.instances.get_mut(&id) {
            instance.visible = !instance.visible;
            Some(instance.visible)
        } else {
            None
        }
    }

    /// Set timeframe visibility config for an instance
    pub fn set_timeframe_visibility(&mut self, id: u64, config: mlc_core::drawing::TimeframeVisibilityConfig) {
        if let Some(instance) = self.instances.get_mut(&id) {
            instance.set_timeframe_visibility(config);
        }
    }

    /// Get instance by ID
    pub fn get_instance(&self, id: u64) -> Option<&IndicatorInstance> {
        self.instances.get(&id)
    }

    /// Get mutable instance by ID
    pub fn get_instance_mut(&mut self, id: u64) -> Option<&mut IndicatorInstance> {
        self.instances.get_mut(&id)
    }

    /// Get indicator instances bound to a specific bubble.
    /// Only returns instances whose `bubble_id` exactly matches `bubble_id`.
    /// Instances with `bubble_id = None` are NOT included to prevent cross-tab leakage.
    pub fn get_instances_for_bubble(&self, bubble_id: u64) -> Vec<&IndicatorInstance> {
        self.by_bubble_id
            .get(&bubble_id)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.instances.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Swap the display order of two sub-pane indicator instances within a bubble.
    ///
    /// Finds the positions of `id_a` and `id_b` in the `by_bubble_id[bubble_id]`
    /// insertion-order Vec and swaps them.  `sync_sub_panes_from_manager` reads
    /// that Vec as `desired_ids` on every tick, so the swap persists across
    /// re-syncs without reverting.
    ///
    /// Returns `true` if both ids were found and swapped, `false` otherwise.
    pub fn swap_pane_order(&mut self, bubble_id: u64, id_a: u64, id_b: u64) -> bool {
        let ids = match self.by_bubble_id.get_mut(&bubble_id) {
            Some(v) => v,
            None => return false,
        };
        let pos_a = ids.iter().position(|&x| x == id_a);
        let pos_b = ids.iter().position(|&x| x == id_b);
        match (pos_a, pos_b) {
            (Some(i), Some(j)) => {
                ids.swap(i, j);
                true
            }
            _ => false,
        }
    }

    /// Pick a distinct palette color for a new overlay indicator instance.
    ///
    /// Returns `Some(color)` when:
    /// - The indicator type is an overlay (`definition.overlay == true`)
    /// - At least one instance of the same `type_id` already exists in `bubble_id`
    ///
    /// The returned color is the first entry from the fixed palette that is not
    /// already in use by existing same-type instances in that bubble.  The
    /// definition's own default output colors are also counted as "in use" so
    /// the very first (default-colored) instance is never given a duplicate color.
    ///
    /// Returns `None` when no auto-color is needed (non-overlay, no existing
    /// instances of the same type, or the palette is exhausted).
    pub fn pick_overlay_color(&self, type_id: &str, bubble_id: u64) -> Option<String> {
        /// Colors evenly spread across the visible spectrum, ordered so that
        /// successive duplicates contrast clearly with each other and with the
        /// typical default blue (#2196F3).
        const OVERLAY_PALETTE: &[&str] = &[
            "#F44336", // red
            "#4CAF50", // green
            "#FF9800", // orange
            "#9C27B0", // purple
            "#00BCD4", // cyan
            "#E91E63", // pink
            "#FFEB3B", // yellow
            "#009688", // teal
            "#3F51B5", // indigo
            "#03A9F4", // light blue
            "#FF5722", // deep orange
            "#CDDC39", // lime
            "#FFC107", // amber
            "#9E9E9E", // gray
            "#607D8B", // blue gray
            "#2196F3", // blue (last resort — same as many defaults)
        ];

        let definition = self.definitions.get(type_id)?;
        if !definition.overlay {
            return None;
        }

        let existing_same_type: Vec<&IndicatorInstance> = self
            .get_instances_for_bubble(bubble_id)
            .into_iter()
            .filter(|i| i.type_id == type_id)
            .collect();

        if existing_same_type.is_empty() {
            return None;
        }

        // Collect colors that are already visually in use.
        // An instance with `color = None` renders with the definition's default
        // output color, so those defaults count as "used" too.
        let def_colors: std::collections::HashSet<String> = definition
            .outputs
            .iter()
            .map(|o| o.color.clone())
            .collect();

        let mut used: std::collections::HashSet<String> = existing_same_type
            .iter()
            .flat_map(|i| i.outputs.values())
            .filter_map(|oc| oc.color.clone())
            .collect();
        used.extend(def_colors);

        OVERLAY_PALETTE
            .iter()
            .find(|&&c| !used.contains(c))
            .map(|c| c.to_string())
    }

    /// Get all instances
    pub fn get_all_instances(&self) -> Vec<&IndicatorInstance> {
        self.instances.values().collect()
    }

    /// Get count of instances
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    /// Search definitions by name
    pub fn search_definitions(&self, query: &str) -> Vec<&IndicatorDefinition> {
        let query_lower = query.to_lowercase();
        self.definitions
            .values()
            .filter(|d| {
                d.name.to_lowercase().contains(&query_lower) ||
                d.short_name.to_lowercase().contains(&query_lower)
            })
            .collect()
    }

    /// Calculate indicator values for an instance using bar data
    ///
    /// Uses the full indicator catalog from zengeld-chart-indicators for computation.
    /// Also generates signals if signals_enabled is true.
    ///
    /// Accepts `mlc_core::Bar` (which has `timestamp`) and converts internally.
    pub fn calculate(&mut self, id: u64, bars: &[mlc_core::Bar]) {
        // Convert mlc_core::Bar to indicators Bar
        let bars: Vec<mlc_backend_indicator_engine::Bar> = bars.iter().map(|b| mlc_backend_indicator_engine::Bar {
            time: b.timestamp,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        }).collect();
        let bars: &[mlc_backend_indicator_engine::Bar] = &bars;

        // Get machine_id and signals_enabled from definition/instance
        let (machine_id, signals_enabled) = {
            let instance = match self.instances.get(&id) {
                Some(i) => i,
                None => return,
            };
            let definition = match self.definitions.get(&instance.type_id) {
                Some(d) => d,
                None => return,
            };
            (definition.machine_id, instance.signals_enabled)
        };

        let instance = match self.instances.get_mut(&id) {
            Some(i) => i,
            None => return,
        };

        // Use machine_id directly (no string parsing!)
        if let Some(indicator_id) = machine_id {
            if signals_enabled {
                // Calculate with signals
                if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &instance.params, bars) {
                    for (output_name, output_values) in result.values {
                        instance.set_values(&output_name, output_values);
                    }
                    instance.signals = result.signals;
                }
            } else {
                // Calculate without signals (faster)
                if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &instance.params, bars) {
                    for (output_name, output_values) in values {
                        instance.set_values(&output_name, output_values);
                    }
                }
            }
        }
    }

    /// Calculate all instances bound to a bubble in parallel using rayon.
    ///
    /// Each indicator computation is independent (reads params, writes values),
    /// so all instances can be computed concurrently on the rayon thread pool.
    /// Results are written back sequentially after the parallel phase.
    pub fn calculate_all_for_bubble(&mut self, bubble_id: u64, bars: &[mlc_core::Bar]) {
        use rayon::prelude::*;

        // Built-ins are handled separately (direct bar-data copy, no mli catalog).
        self.calculate_builtins_for_bubble(bubble_id, bars);

        let ids: Vec<u64> = self
            .by_bubble_id
            .get(&bubble_id)
            .cloned()
            .unwrap_or_default();

        if ids.is_empty() {
            return;
        }

        // Convert bars once, shared across all parallel tasks.
        let converted_bars: Vec<mlc_backend_indicator_engine::Bar> = bars.iter().map(|b| mlc_backend_indicator_engine::Bar {
            time: b.timestamp,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        }).collect();

        // Collect all inputs needed for parallel computation (cheap clones of params).
        // This phase is sequential and cheap — just field reads and small HashMap clones.
        let tasks: Vec<(u64, Option<mlc_backend_indicator_engine::BarIndicatorId>, bool, HashMap<String, IndicatorValue>)> = ids
            .iter()
            .filter_map(|&id| {
                let inst = self.instances.get(&id)?;
                let def = self.definitions.get(&inst.type_id)?;
                Some((id, def.machine_id, inst.signals_enabled, inst.params.clone()))
            })
            .collect();

        // Parallel computation phase — no shared mutable state.
        // Each task produces an (id, values, signals) result.
        type TaskResult = (u64, Option<HashMap<String, Vec<f64>>>, Vec<mlc_backend_indicator_engine::signals::signal::Signal>);
        let results: Vec<TaskResult> = tasks
            .into_par_iter()
            .map(|(id, machine_id, signals_enabled, params)| {
                let bars: &[mlc_backend_indicator_engine::Bar] = &converted_bars;
                if let Some(indicator_id) = machine_id {
                    if signals_enabled {
                        if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &params, bars) {
                            return (id, Some(result.values), result.signals);
                        }
                    } else if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &params, bars) {
                        return (id, Some(values), Vec::new());
                    }
                }
                (id, None, Vec::new())
            })
            .collect();

        // Write-back phase — sequential, updates self.instances with computed results.
        for (id, values_opt, signals) in results {
            if let Some(instance) = self.instances.get_mut(&id) {
                if let Some(values) = values_opt {
                    for (output_name, output_values) in values {
                        instance.set_values(&output_name, output_values);
                    }
                }
                if instance.signals_enabled {
                    instance.signals = signals;
                }
            }
        }
    }

    /// Compute built-in indicators (those with `machine_id == None`) for a
    /// bubble.  Currently the only built-in is `builtin_volume`, which copies
    /// `bar.volume` directly into its output Vec — no mli catalog needed.
    ///
    /// Called from both `calculate_all_for_bubble` and `calculate_for_bubble`
    /// so that built-ins always run regardless of which recalc path is active.
    fn calculate_builtins_for_bubble(&mut self, bubble_id: u64, bars: &[mlc_core::Bar]) {
        let ids: Vec<u64> = self
            .by_bubble_id
            .get(&bubble_id)
            .cloned()
            .unwrap_or_default();

        for id in ids {
            let type_id = match self.instances.get(&id) {
                Some(inst) => inst.type_id.clone(),
                None => continue,
            };

            if type_id == "builtin_volume" {
                let volumes: Vec<f64> = bars.iter().map(|b| b.volume).collect();
                if let Some(inst) = self.instances.get_mut(&id) {
                    inst.set_values("volume", volumes);
                }
            }
        }
    }

    /// Calculate only the instances for a symbol that belong to a specific window.
    ///
    /// Uses an **incremental path** (O(1) per indicator) when:
    /// - A `ComputeCache` exists for the instance,
    /// - The params fingerprint matches (no parameter changes),
    /// - Exactly one new bar was appended since the last call, AND
    /// - The last bar's OHLCV is unchanged.
    ///
    /// Falls back to a **full recalc** (rayon parallel, O(N)) for all other
    /// cases: first call, parameter changes, scroll prepend, last-bar update.
    ///
    /// Instances whose `bubble_id` is `None` (unscoped / legacy) are skipped;
    /// they should be handled by `calculate_all_for_bubble(0, …)` when appropriate.
    pub fn calculate_for_bubble(&mut self, bubble_id: u64, bars: &[mlc_core::Bar]) {
        use rayon::prelude::*;

        // Built-ins are handled separately (direct bar-data copy, no mli catalog).
        self.calculate_builtins_for_bubble(bubble_id, bars);

        let ids: Vec<u64> = self
            .by_bubble_id
            .get(&bubble_id)
            .cloned()
            .unwrap_or_default();

        if ids.is_empty() {
            return;
        }

        // Convert bars once, shared across all computation paths.
        let converted_bars: Vec<mlc_backend_indicator_engine::Bar> = bars.iter().map(|b| mlc_backend_indicator_engine::Bar {
            time: b.timestamp,
            open: b.open,
            high: b.high,
            low: b.low,
            close: b.close,
            volume: b.volume,
        }).collect();

        let current_bar_count = converted_bars.len();

        // ── Classify each instance into incremental or full-recalc bucket ──────

        // (id, machine_id, signals_enabled, params_clone)
        type TaskMeta = (u64, Option<mlc_backend_indicator_engine::BarIndicatorId>, bool, HashMap<String, IndicatorValue>);

        let mut incremental_ids: Vec<u64> = Vec::new();
        let mut full_recalc_tasks: Vec<TaskMeta> = Vec::new();

        for &id in &ids {
            let inst = match self.instances.get(&id) {
                Some(i) => i,
                None => continue,
            };
            if inst.bubble_id != Some(bubble_id) {
                continue;
            }
            let def = match self.definitions.get(&inst.type_id) {
                Some(d) => d,
                None => continue,
            };
            let machine_id = def.machine_id;
            let signals_enabled = inst.signals_enabled;
            let params = &inst.params;

            // Check if we can use the incremental path.
            // bar_diff == 1: one new confirmed bar → feed it, append output.
            // bar_diff == 0: same bar count but live bar updated by tick → restore
            //               from checkpoint, feed updated bar, replace last output.
            let can_increment = if let (Some(mid), Some(cache)) = (machine_id, self.compute_caches.get(&id)) {
                let fingerprint = IndicatorCalculator::params_fingerprint(mid, params);
                let fingerprint_ok = cache.config_fingerprint == fingerprint;
                let bar_diff = current_bar_count.wrapping_sub(cache.fed_bar_count);
                // One new bar OR same bar count (live-bar tick update), fingerprint ok.
                fingerprint_ok && (bar_diff == 1 || bar_diff == 0) && current_bar_count > 0
            } else {
                false
            };

            if can_increment {
                incremental_ids.push(id);
            } else {
                full_recalc_tasks.push((id, machine_id, signals_enabled, params.clone()));
            }
        }

        // Update profiling counters after classification.
        self.last_incremental_count = incremental_ids.len() as u32;
        self.last_full_count = full_recalc_tasks.len() as u32;

        // ── Full-recalc path (rayon parallel) ──────────────────────────────────

        if !full_recalc_tasks.is_empty() {
            type TaskResult = (u64, Option<HashMap<String, Vec<f64>>>, Vec<mlc_backend_indicator_engine::signals::signal::Signal>);
            let results: Vec<TaskResult> = full_recalc_tasks
                .into_par_iter()
                .map(|(id, machine_id, signals_enabled, params)| {
                    let bars: &[mlc_backend_indicator_engine::Bar] = &converted_bars;
                    if let Some(indicator_id) = machine_id {
                        if signals_enabled {
                            if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &params, bars) {
                                return (id, Some(result.values), result.signals);
                            }
                        } else if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &params, bars) {
                            return (id, Some(values), Vec::new());
                        }
                    }
                    (id, None, Vec::new())
                })
                .collect();

            // Write-back + build ComputeCache for each full-recalc result.
            for (id, values_opt, signals) in results {
                if let Some(instance) = self.instances.get_mut(&id) {
                    if let Some(values) = values_opt {
                        for (output_name, output_values) in values {
                            instance.set_values(&output_name, output_values);
                        }
                    }
                    if instance.signals_enabled {
                        instance.signals = signals;
                    }
                }

                // Rebuild ComputeCache so future ticks can use the incremental path.
                let (machine_id, signals_enabled, params_ref) = match self.instances.get(&id) {
                    Some(inst) => {
                        let def = self.definitions.get(&inst.type_id);
                        (def.and_then(|d| d.machine_id), inst.signals_enabled, inst.params.clone())
                    }
                    None => continue,
                };

                if let Some(indicator_id) = machine_id {
                    if let Some(mut fresh_instance) = IndicatorCalculator::build_compute_instance(indicator_id, &params_ref) {
                        // Feed all bars; capture checkpoint at N-1 (before last bar).
                        let mut checkpoint: Option<mlc_backend_indicator_engine::IndicatorInstance> = None;
                        let bar_count = converted_bars.len();
                        for (i, bar) in converted_bars.iter().enumerate() {
                            if i + 1 == bar_count {
                                // About to feed the last bar — save N-1 state.
                                checkpoint = Some(fresh_instance.clone());
                            }
                            IndicatorCalculator::feed_bar(&mut fresh_instance, bar);
                        }
                        let fingerprint = IndicatorCalculator::params_fingerprint(indicator_id, &params_ref);
                        let last_bar = converted_bars.last().cloned();

                        // Build optional signal engine for incremental signal path.
                        let signal_engine = if signals_enabled {
                            mlc_backend_indicator_engine::signals::rules::default_profile(indicator_id)
                                .map(|p| mlc_backend_indicator_engine::signals::rules::SignalEngine::from_profile(&p))
                        } else {
                            None
                        };

                        self.compute_caches.insert(id, ComputeCache {
                            instance: fresh_instance,
                            checkpoint,
                            fed_bar_count: current_bar_count,
                            config_fingerprint: fingerprint,
                            last_fed_bar: last_bar,
                            signal_engine,
                        });
                    }
                }
            }
        }

        // ── Incremental path (sequential, O(1) per instance) ───────────────────

        for id in incremental_ids {
            // Resolve bar_diff and last_bar_changed from cache.
            let current_bar = match converted_bars.last() {
                Some(b) => b.clone(),
                None => continue,
            };

            let (bar_diff, last_bar_changed) = {
                let cache = match self.compute_caches.get(&id) {
                    Some(c) => c,
                    None => continue,
                };
                let diff = current_bar_count.wrapping_sub(cache.fed_bar_count);
                let changed = match &cache.last_fed_bar {
                    Some(prev) => {
                        prev.close != current_bar.close
                            || prev.high  != current_bar.high
                            || prev.low   != current_bar.low
                            || prev.open  != current_bar.open
                    }
                    None => true,
                };
                (diff, changed)
            };

            // ── Case 1: bar_diff == 0, same bar count but last bar updated by tick ──
            // Use checkpoint (N-1 state) to replay only the updated last bar.
            if bar_diff == 0 {
                if !last_bar_changed {
                    // Nothing changed — skip entirely.
                    continue;
                }

                // Checkpoint path: restore N-1 state, feed the updated last bar.
                let (machine_id_opt, signals_enabled) = match self.instances.get(&id) {
                    Some(inst) => {
                        let def = self.definitions.get(&inst.type_id);
                        (def.and_then(|d| d.machine_id), inst.signals_enabled)
                    }
                    None => continue,
                };

                let indicator_id = match machine_id_opt {
                    Some(mid) => mid,
                    None => continue,
                };

                let cache = match self.compute_caches.get_mut(&id) {
                    Some(c) => c,
                    None => continue,
                };

                if let Some(cp) = &cache.checkpoint {
                    // Clone the N-1 checkpoint and feed the updated last bar.
                    let mut working = cp.clone();
                    let value = IndicatorCalculator::feed_bar(&mut working, &current_bar);
                    let outputs = IndicatorCalculator::extract_single_outputs(indicator_id, value);

                    // Replace the last value in each output series (not append).
                    cache.instance = working;
                    cache.last_fed_bar = Some(current_bar.clone());

                    if let Some(instance) = self.instances.get_mut(&id) {
                        let values_map = Arc::make_mut(&mut instance.values);
                        for (key, val) in outputs {
                            if let Some(vec) = values_map.get_mut(&key) {
                                if let Some(last) = vec.last_mut() {
                                    *last = val;
                                }
                            }
                        }
                        // Signals are only generated on bar close — skip for tick updates.
                        let _ = signals_enabled;
                    }
                } else {
                    // No checkpoint available (e.g. single-bar history) — no update needed.
                }

                continue;
            }

            // ── Case 2: bar_diff == 1, last bar is genuinely new ──
            // Sub-case 2a: the new bar slot has OHLCV changes from the prev last bar.
            // This can happen in PerTick mode when the live bar count just grew by 1.
            // Fall back to full recalc for correctness.
            if last_bar_changed {
                let (machine_id_opt, signals_enabled, params_clone) = match self.instances.get(&id) {
                    Some(inst) => {
                        let def = self.definitions.get(&inst.type_id);
                        (def.and_then(|d| d.machine_id), inst.signals_enabled, inst.params.clone())
                    }
                    None => continue,
                };

                if let Some(indicator_id) = machine_id_opt {
                    let bars_slice: &[mlc_backend_indicator_engine::Bar] = &converted_bars;
                    let (values_opt, signals) = if signals_enabled {
                        if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &params_clone, bars_slice) {
                            (Some(result.values), result.signals)
                        } else {
                            (None, Vec::new())
                        }
                    } else {
                        let v = IndicatorCalculator::calculate_with_id(indicator_id, &params_clone, bars_slice);
                        (v, Vec::new())
                    };

                    if let Some(instance) = self.instances.get_mut(&id) {
                        if let Some(values) = values_opt {
                            for (output_name, output_values) in values {
                                instance.set_values(&output_name, output_values);
                            }
                        }
                        if instance.signals_enabled {
                            instance.signals = signals;
                        }
                    }

                    // Rebuild cache after fallback recalc.
                    if let Some(mut fresh_instance) = IndicatorCalculator::build_compute_instance(indicator_id, &params_clone) {
                        let mut checkpoint: Option<mlc_backend_indicator_engine::IndicatorInstance> = None;
                        let bar_count = converted_bars.len();
                        for (i, bar) in converted_bars.iter().enumerate() {
                            if i + 1 == bar_count {
                                checkpoint = Some(fresh_instance.clone());
                            }
                            IndicatorCalculator::feed_bar(&mut fresh_instance, bar);
                        }
                        let signal_engine = if signals_enabled {
                            mlc_backend_indicator_engine::signals::rules::default_profile(indicator_id)
                                .map(|p| mlc_backend_indicator_engine::signals::rules::SignalEngine::from_profile(&p))
                        } else {
                            None
                        };
                        self.compute_caches.insert(id, ComputeCache {
                            instance: fresh_instance,
                            checkpoint,
                            fed_bar_count: current_bar_count,
                            config_fingerprint: IndicatorCalculator::params_fingerprint(indicator_id, &params_clone),
                            last_fed_bar: converted_bars.last().cloned(),
                            signal_engine,
                        });
                    }
                }

                continue;
            }

            // ── Case 2b: bar_diff == 1, clean new bar — happy path ──
            // Save current instance as the new checkpoint (N-1 state for the next
            // tick update), then feed the new bar and append its output values.
            let (machine_id_opt, signals_enabled, indicator_name) = match self.instances.get(&id) {
                Some(inst) => {
                    let def = self.definitions.get(&inst.type_id);
                    (
                        def.and_then(|d| d.machine_id),
                        inst.signals_enabled,
                        format!("{}", inst.name),
                    )
                }
                None => continue,
            };

            let indicator_id = match machine_id_opt {
                Some(mid) => mid,
                None => continue,
            };

            let cache = match self.compute_caches.get_mut(&id) {
                Some(c) => c,
                None => continue,
            };

            // Save current instance (N-1) as the checkpoint before feeding the new bar.
            cache.checkpoint = Some(cache.instance.clone());

            let value = IndicatorCalculator::feed_bar(&mut cache.instance, &current_bar);
            let outputs = IndicatorCalculator::extract_single_outputs(indicator_id, value.clone());

            // Process signals on confirmed bar close.
            let new_signals: Vec<mlc_backend_indicator_engine::signals::signal::Signal> = if signals_enabled {
                if let Some(ref mut engine) = cache.signal_engine {
                    let is_last_bar = true;
                    engine.process(
                        &value,
                        current_bar.open,
                        current_bar.high,
                        current_bar.low,
                        current_bar.close,
                        current_bar.volume,
                        current_bar.time,
                        &indicator_name,
                        is_last_bar,
                    )
                } else {
                    Vec::new()
                }
            } else {
                Vec::new()
            };

            // Update cache metadata.
            cache.fed_bar_count = current_bar_count;
            cache.last_fed_bar = Some(current_bar);

            // Append the new output values to the instance's value Vecs.
            if let Some(instance) = self.instances.get_mut(&id) {
                let values_map = Arc::make_mut(&mut instance.values);
                for (key, val) in outputs {
                    values_map.entry(key).or_default().push(val);
                }
                if signals_enabled && !new_signals.is_empty() {
                    instance.signals.extend(new_signals);
                }
            }
        }
    }

    // ── Recalculation scheduling helpers ─────────────────────────────────────

    /// Mark a symbol as needing indicator recalculation.
    ///
    /// In PerTick mode this is a no-op (callers handle recalc inline).
    /// In PerFrame and PerBar modes, the symbol is queued for the next flush.
    pub fn mark_dirty(&mut self, symbol: &str) {
        self.dirty_symbols.insert(symbol.to_string());
    }

    /// Mark a symbol as having formed a new bar, and also mark it dirty.
    ///
    /// In PerBar mode, only symbols marked via this method are recalculated
    /// during the next flush.
    pub fn mark_new_bar(&mut self, symbol: &str) {
        self.new_bar_symbols.insert(symbol.to_string());
        self.dirty_symbols.insert(symbol.to_string());
    }

    /// Clear all pending dirty flags without triggering recalculation.
    ///
    /// Call this when the recalc mode changes so that stale queue entries from
    /// the previous mode do not leak into the first flush of the new mode.
    pub fn clear_pending(&mut self) {
        self.dirty_symbols.clear();
        self.new_bar_symbols.clear();
    }

    /// Drain pending recalculation requests and return the symbols to recalculate.
    ///
    /// Behaviour depends on the current `recalc_mode`:
    /// - `PerTick`: returns an empty list (callers handle recalc inline, not here).
    /// - `PerFrame`: returns all dirty symbols and clears the dirty set.
    /// - `PerBar`: returns only symbols that formed a new bar; also clears dirty set.
    ///
    /// After calling this, both internal sets are empty.
    pub fn drain_pending_recalc(&mut self) -> Vec<String> {
        let result = match self.recalc_mode {
            RecalcMode::PerTick => {
                // PerTick is handled inline — nothing to drain.
                Vec::new()
            }
            RecalcMode::PerFrame => {
                self.dirty_symbols.drain().collect()
            }
            RecalcMode::PerBar => {
                let new_bars: Vec<String> = self.new_bar_symbols.drain().collect();
                self.dirty_symbols.clear();
                new_bars
            }
        };
        // Always clear both sets so stale entries never accumulate.
        self.new_bar_symbols.clear();
        result
    }

    /// Calculate the Y-axis range (min, max) for an indicator instance.
    /// This centralizes the range calculation logic that was previously in native.
    ///
    /// Returns `(min, max)` tuple, with appropriate padding applied.
    /// For bounded indicators (0-100), returns fixed bounds.
    /// For unbounded indicators, calculates from visible data.
    pub fn calculate_pane_range(
        &self,
        id: u64,
        visible_start: usize,
        visible_end: usize,
    ) -> Option<(f64, f64)> {
        let instance = self.instances.get(&id)?;
        let definition = self.definitions.get(&instance.type_id)?;

        // Check if this indicator has fixed bounds (e.g., RSI/Stoch 0-100)
        if let Some(bounds) = definition.bounds {
            return Some(bounds);
        }

        // Calculate range from visible data
        let mut min_val = f64::INFINITY;
        let mut max_val = f64::NEG_INFINITY;

        // Iterate through all output values
        for output in &definition.outputs {
            if let Some(values) = instance.values.get(&output.name) {
                if values.is_empty() {
                    continue;
                }
                let start = visible_start.min(values.len().saturating_sub(1));
                // Clamp `end` to the slice length AND make sure it is not below
                // `start` — without the lower clamp a sub-pane separator drag
                // can call this with visible_end=0 (the legacy `.min(0usize)`
                // bug in sync_sub_panes), and `&values[start..0]` panics with
                // "slice index starts at N but ends at 0".
                let end = visible_end.min(values.len()).max(start);

                for &v in &values[start..end] {
                    if !v.is_nan() && !v.is_infinite() {
                        min_val = min_val.min(v);
                        max_val = max_val.max(v);
                    }
                }
            }
        }

        if min_val.is_infinite() || max_val.is_infinite() || min_val >= max_val {
            // No valid data, return default range
            return Some((-1.0, 1.0));
        }

        // Check if indicator has zero baseline (extends Y-range to include zero)
        if definition.zero_baseline && min_val > 0.0 {
            // Extend down to zero for histogram-type indicators
            min_val = 0.0;
        }

        // Return raw min/max — callers are responsible for applying padding
        Some((min_val, max_val))
    }

    /// Get the preferred pane index for a new indicator instance.
    /// Overlay indicators go to pane 0 (main chart), others get their own pane.
    pub fn get_preferred_pane(&self, type_id: &str) -> usize {
        if let Some(def) = self.definitions.get(type_id) {
            if def.overlay { 0 } else { 1 }
        } else {
            1
        }
    }

    /// Check if indicator type is an overlay (renders on main chart)
    pub fn is_overlay(&self, type_id: &str) -> bool {
        self.definitions.get(type_id).map(|d| d.overlay).unwrap_or(false)
    }

    /// Check if indicator type uses a bounded range (has fixed bounds)
    pub fn is_bounded(&self, type_id: &str) -> bool {
        self.definitions.get(type_id).map(|d| d.bounds.is_some()).unwrap_or(false)
    }

    /// Hit test for overlay indicators (pane == 0).
    /// Returns the indicator instance ID if the point (screen_x, screen_y) is close to any indicator line.
    pub fn hit_test_overlay(
        &self,
        screen_x: f64,
        screen_y: f64,
        bubble_id: u64,
        viewport: &mlc_core::Viewport,
        price_scale: &mlc_core::PriceScale,
        chart_height: f64,
        hit_distance: f64,
    ) -> Option<u64> {
        let instances = self.get_instances_for_bubble(bubble_id);

        // Convert screen X to bar index
        let bar_idx = viewport.x_to_bar_f64(screen_x);
        let bar_int = bar_idx.round() as i64;

        // Convert screen Y to price
        let price_range = price_scale.price_max - price_scale.price_min;
        let _screen_price = if chart_height > 0.0 && price_range > 0.0 {
            price_scale.price_max - (screen_y / chart_height) * price_range
        } else {
            return None;
        };

        // Check each overlay indicator
        for instance in instances {
            // Only check overlay indicators (pane == 0) that are visible
            if instance.pane != 0 || !instance.visible {
                continue;
            }

            // Check each output line
            for (_output_name, values) in instance.values.iter() {
                if values.is_empty() {
                    continue;
                }

                // Get the value at the current bar position
                if bar_int < 0 || bar_int as usize >= values.len() {
                    continue;
                }

                let value = values[bar_int as usize];
                if value.is_nan() || value.is_infinite() {
                    continue;
                }

                // Convert indicator value to screen Y
                let indicator_screen_y = if price_range > 0.0 {
                    ((price_scale.price_max - value) / price_range) * chart_height
                } else {
                    continue;
                };

                // Check if cursor is close to this indicator line
                let y_distance = (screen_y - indicator_screen_y).abs();
                if y_distance <= hit_distance {
                    return Some(instance.id);
                }

                // Also check interpolated value between bars for smoother hit detection
                // Check previous bar if available
                if bar_int > 0 {
                    let prev_idx = (bar_int - 1) as usize;
                    if prev_idx < values.len() {
                        let prev_value = values[prev_idx];
                        if !prev_value.is_nan() && !prev_value.is_infinite() {
                            // Linear interpolation
                            let frac = bar_idx - bar_idx.floor();
                            let interp_value = prev_value + (value - prev_value) * frac;
                            let interp_screen_y = ((price_scale.price_max - interp_value) / price_range) * chart_height;
                            let interp_distance = (screen_y - interp_screen_y).abs();
                            if interp_distance <= hit_distance {
                                return Some(instance.id);
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Hit test for sub-pane indicators (pane > 0).
    /// Returns the indicator instance ID if the point is within a sub-pane.
    pub fn hit_test_sub_pane(
        &self,
        instance_id: u64,
        screen_x: f64,
        screen_y: f64,
        viewport: &mlc_core::Viewport,
        price_min: f64,
        price_max: f64,
        pane_height: f64,
        hit_distance: f64,
    ) -> bool {
        let instance = match self.instances.get(&instance_id) {
            Some(i) => i,
            None => return false,
        };

        if !instance.visible {
            return false;
        }

        // Convert screen X to bar index
        let bar_idx = viewport.x_to_bar_f64(screen_x);
        let bar_int = bar_idx.round() as i64;

        // Convert screen Y to indicator value
        let price_range = price_max - price_min;
        let _screen_value = if pane_height > 0.0 && price_range > 0.0 {
            price_max - (screen_y / pane_height) * price_range
        } else {
            return false;
        };

        // Check each output line
        for (_, values) in instance.values.iter() {
            if values.is_empty() {
                continue;
            }

            if bar_int < 0 || bar_int as usize >= values.len() {
                continue;
            }

            let value = values[bar_int as usize];
            if value.is_nan() || value.is_infinite() {
                continue;
            }

            // Convert indicator value to screen Y for comparison
            let indicator_screen_y = if price_range > 0.0 {
                ((price_max - value) / price_range) * pane_height
            } else {
                continue;
            };

            let y_distance = (screen_y - indicator_screen_y).abs();
            if y_distance <= hit_distance {
                return true;
            }
        }

        false
    }

    /// Get display name for an indicator instance (e.g., "RSI (14)")
    pub fn get_instance_display_name(&self, instance: &IndicatorInstance) -> String {
        // Get the definition to find the main param
        if let Some(def) = self.definitions.get(&instance.type_id) {
            // Try to find a "period" or "length" param to include in name
            let main_param = instance.params.get("period")
                .or_else(|| instance.params.get("length"))
                .or_else(|| instance.params.get("Period"))
                .or_else(|| instance.params.get("Length"));

            if let Some(param_val) = main_param {
                let param_str = match param_val {
                    IndicatorValue::Int(v) => v.to_string(),
                    IndicatorValue::Float(v) => format!("{:.0}", v),
                    _ => String::new(),
                };
                if !param_str.is_empty() {
                    return format!("{} ({})", def.short_name, param_str);
                }
            }

            def.short_name.clone()
        } else {
            instance.name.clone()
        }
    }

    /// Expose instances map for use by core-side extensions
    pub fn instances_iter(&self) -> impl Iterator<Item = &IndicatorInstance> {
        self.instances.values()
    }
}

impl Default for IndicatorManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// IndicatorSource-shaped inherent methods (id-keyed, no bubble scoping needed)
// =============================================================================
//
// `IndicatorManager` no longer implements `mlc_core::indicator_source::
// IndicatorSource` directly — the two symbol-scoped trait methods
// (`get_instances_for_symbol` / `get_render_instances_for_symbol`) require a
// `BubbleId` to scope correctly, and the manager itself no longer carries the
// `current_render_bubble_id` scoping cursor that used to stand in for that
// parameter (design: `docs/mlc/plans/step-3.3-indicator-watchlist-design.md`
// §1.1). The bubble-scoped trait impl now lives on
// `mlc_backend::indicator::facade::BubbleIndicatorView`, constructed via
// `Indicators::with_bubble(bid)` — mirrors Drawing's `BubbleDrawingView`
// (`mlc_backend::drawing::facade`).
//
// The id-keyed methods below (`calculate_pane_range`, `get_render_instance`,
// `get_settings_data`) don't need bubble scoping at all and stay directly
// callable; `BubbleIndicatorView` forwards to them for its own trait impl.

impl IndicatorManager {
    /// Get settings-level data for the indicator settings modal. Id-keyed —
    /// no bubble scoping needed.
    pub fn get_settings_data(
        &self,
        instance_id: u64,
    ) -> Option<mlc_core::indicator_source::IndicatorSettingsData> {
        use mlc_core::indicator_source::IndicatorSettingsData;
        use mlc_core::state::modal_data::{
            IndicatorDisplayInfo, IndicatorParamDef,
            IndicatorParamType as ChartParamType,
            IndicatorOutputDef, IndicatorOutputType as ChartOutputType,
        };

        let inst = self.get_instance(instance_id)?;
        let definition = self.get_definition(&inst.type_id);

        // Convert params to (name, value) pairs using definition order for stable UI ordering
        let params: Vec<(String, String)> = if let Some(def) = definition.as_ref() {
            def.params.iter()
                .filter_map(|param_def| {
                    let value = inst.params.get(&param_def.name)
                        .unwrap_or(&param_def.default_value);
                    let value_str = match value {
                        IndicatorValue::Int(i) => i.to_string(),
                        IndicatorValue::Float(f) => format!("{:.2}", f),
                        IndicatorValue::Bool(b) => b.to_string(),
                        IndicatorValue::String(s) => s.clone(),
                        IndicatorValue::Color(c) => c.clone(),
                    };
                    Some((param_def.name.clone(), value_str))
                })
                .collect()
        } else {
            // Fallback: no definition, use HashMap order (unstable but better than nothing)
            inst.params.iter()
                .map(|(k, v)| {
                    let value_str = match v {
                        IndicatorValue::Int(i) => i.to_string(),
                        IndicatorValue::Float(f) => format!("{:.2}", f),
                        IndicatorValue::Bool(b) => b.to_string(),
                        IndicatorValue::String(s) => s.clone(),
                        IndicatorValue::Color(c) => c.clone(),
                    };
                    (k.clone(), value_str)
                })
                .collect()
        };

        // Convert outputs to (name, color) pairs using definition order for stable ordering
        let outputs: Vec<(String, String)> = if let Some(def) = definition.as_ref() {
            def.outputs.iter()
                .map(|output_def| {
                    let color = inst.outputs.get(&output_def.name)
                        .and_then(|cfg| cfg.color.clone())
                        .unwrap_or_else(|| output_def.color.clone());
                    (output_def.name.clone(), color)
                })
                .collect()
        } else {
            inst.outputs.iter()
                .map(|(k, v)| {
                    let color = v.color.clone().unwrap_or_else(|| "#2962ff".to_string());
                    (k.clone(), color)
                })
                .collect()
        };

        // Convert IndicatorDefinition → IndicatorDisplayInfo (chart-owned type)
        let display_info: Option<IndicatorDisplayInfo> = definition.map(|def| {
            IndicatorDisplayInfo {
                name: def.name.clone(),
                short_name: def.short_name.clone(),
                description: def.description.clone(),
                overlay: def.overlay,
                bounds: def.bounds,
                category_name: def.category.display_name().to_string(),
                params: def.params.iter().map(|p| IndicatorParamDef {
                    name: p.name.clone(),
                    param_type: match &p.param_type {
                        IndicatorParamType::Int { .. } => ChartParamType::Int,
                        IndicatorParamType::Float { .. } => ChartParamType::Float,
                        IndicatorParamType::Bool => ChartParamType::Bool,
                        IndicatorParamType::Source => ChartParamType::Source,
                        IndicatorParamType::Select { options } => ChartParamType::Select { options: options.clone() },
                        IndicatorParamType::Color => ChartParamType::Color,
                    },
                }).collect(),
                outputs: def.outputs.iter().map(|o| IndicatorOutputDef {
                    display_name: o.display_name.clone(),
                    output_type: match o.output_type {
                        IndicatorOutputType::Line => ChartOutputType::Line,
                        IndicatorOutputType::Histogram => ChartOutputType::Histogram,
                        IndicatorOutputType::Area => ChartOutputType::Other("Area".to_string()),
                        IndicatorOutputType::Band => ChartOutputType::Other("Band".to_string()),
                        IndicatorOutputType::Dots => ChartOutputType::Other("Dots".to_string()),
                        IndicatorOutputType::Background => ChartOutputType::Other("Background".to_string()),
                    },
                }).collect(),
            }
        });

        Some(IndicatorSettingsData {
            name: inst.name.clone(),
            params,
            outputs,
            display_info,
            signals_enabled: inst.signals_enabled,
            signal_display: inst.signal_display.clone(),
            timeframe_visibility: inst.timeframe_visibility.clone(),
        })
    }

    /// Get full render data for a single indicator instance by ID. Id-keyed —
    /// no bubble scoping needed.
    pub fn get_render_instance(
        &self,
        instance_id: u64,
    ) -> Option<mlc_core::indicator_source::IndicatorRenderInstance> {
        let inst = self.get_instance(instance_id)?;
        self.build_render_instance(inst)
    }
}

impl IndicatorManager {
    /// Convert an `IndicatorInstance` + its `IndicatorDefinition` into a chart-side
    /// `IndicatorRenderInstance`. Returns `None` if the definition is missing.
    pub fn build_render_instance(
        &self,
        inst: &IndicatorInstance,
    ) -> Option<mlc_core::indicator_source::IndicatorRenderInstance> {
        use mlc_core::indicator_source::{
            IndicatorRenderInstance, IndicatorOutputRenderDef, IndicatorOutputRenderType,
            OutputRenderConfig, HistogramStyle as ChartHistogramStyle, SignalRenderData,
        };

        let def = self.get_definition(&inst.type_id)?;

        let output_defs: Vec<IndicatorOutputRenderDef> = def.outputs.iter().map(|o| {
            let render_type = match o.output_type {
                IndicatorOutputType::Line       => IndicatorOutputRenderType::Line,
                IndicatorOutputType::Histogram  => IndicatorOutputRenderType::Histogram,
                IndicatorOutputType::Band       => IndicatorOutputRenderType::Band,
                IndicatorOutputType::Area       => IndicatorOutputRenderType::Area,
                IndicatorOutputType::Dots       => IndicatorOutputRenderType::Dots,
                IndicatorOutputType::Background => IndicatorOutputRenderType::Background,
            };
            IndicatorOutputRenderDef {
                name: o.name.clone(),
                output_type: render_type,
                color: o.color.clone(),
                line_width: o.line_width,
            }
        }).collect();

        let output_configs: std::collections::HashMap<String, OutputRenderConfig> = inst.outputs
            .iter()
            .map(|(name, cfg)| {
                (name.clone(), OutputRenderConfig {
                    visible: cfg.visible,
                    color: cfg.color.clone(),
                    line_width: cfg.line_width,
                })
            })
            .collect();

        let histogram_style = match def.histogram_style {
            HistogramStyle::FromBottom => ChartHistogramStyle::FromBottom,
            HistogramStyle::Centered   => ChartHistogramStyle::Centered,
        };

        let signals: Vec<SignalRenderData> = inst.signals.iter().map(|s| SignalRenderData {
            bar_index: s.bar_index,
            direction: s.direction.as_i8() as i32,
            price: s.price,
            confirmation: match s.confirmation {
                mlc_backend_indicator_engine::signals::signal::BarConfirmation::Pending => 0,
                mlc_backend_indicator_engine::signals::signal::BarConfirmation::Closed => 1,
                mlc_backend_indicator_engine::signals::signal::BarConfirmation::WickOnly => 2,
            },
            kind_name: s.kind.description().to_string(),
        }).collect();

        // Collect color_params and bool_params from indicator params
        let mut color_params = std::collections::HashMap::new();
        let mut bool_params = std::collections::HashMap::new();

        for (key, value) in &inst.params {
            match value {
                IndicatorValue::Color(c) => { color_params.insert(key.clone(), c.clone()); }
                IndicatorValue::Bool(b)  => { bool_params.insert(key.clone(), *b); }
                _ => {}
            }
        }

        // Special case: volume indicator color params (including builtin_volume overlay)
        if inst.type_id == "volume" || inst.type_id == "vol" || inst.type_id == "builtin_volume" {
            color_params.entry("up_color".to_string())
                .or_insert_with(|| "#26A69A80".to_string());
            color_params.entry("down_color".to_string())
                .or_insert_with(|| "#EF535080".to_string());
            bool_params.entry("color_by_direction".to_string())
                .or_insert(true);
        }

        Some(IndicatorRenderInstance {
            id: inst.id,
            type_id: inst.type_id.clone(),
            pane: inst.pane,
            visible: inst.visible,
            title: inst.title(),
            output_defs,
            output_configs,
            // Arc::clone bumps the reference count (O(1)) instead of deep-copying
            // potentially millions of f64 values on every render frame.
            values: Arc::clone(&inst.values),
            histogram_style,
            signals,
            signals_enabled: inst.signals_enabled,
            signal_display: inst.signal_display.clone(),
            color_params,
            bool_params,
            timeframe_visibility: inst.timeframe_visibility.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_instance() {
        let mut manager = IndicatorManager::new();

        let id = manager.create_instance("sma").unwrap();
        let instance = manager.get_instance(id).unwrap();

        assert_eq!(instance.type_id, "sma");
        assert!(instance.visible);
    }

    #[test]
    fn test_modify_params() {
        let mut manager = IndicatorManager::new();

        let id = manager.create_instance("rsi").unwrap();
        let instance = manager.get_instance_mut(id).unwrap();

        instance.set_param("period", IndicatorValue::Int(21));

        let period = instance.get_param("period").unwrap().as_int().unwrap();
        assert_eq!(period, 21);
    }

    #[test]
    fn test_full_catalog() {
        let manager = IndicatorManager::new();

        // Should have 400+ indicators from catalog
        let all_defs = manager.get_definitions();
        assert!(all_defs.len() > 400, "Expected 400+ indicators, got {}", all_defs.len());

        // Check some specific indicators exist
        assert!(manager.get_definition("sma").is_some());
        assert!(manager.get_definition("ema").is_some());
        assert!(manager.get_definition("rsi").is_some());
        assert!(manager.get_definition("macd").is_some());
        assert!(manager.get_definition("bb").is_some());
        assert!(manager.get_definition("atr").is_some());

        // Check overlay vs sub-pane
        let sma = manager.get_definition("sma").unwrap();
        assert!(sma.overlay, "SMA should be overlay");

        let rsi = manager.get_definition("rsi").unwrap();
        assert!(!rsi.overlay, "RSI should not be overlay");
        assert_eq!(rsi.bounds, Some((0.0, 100.0)), "RSI should have 0-100 bounds");
    }

    #[test]
    fn test_category_filter() {
        let manager = IndicatorManager::new();

        let trend = manager.get_by_category(IndicatorCategory::Trend);
        assert!(trend.iter().any(|d| d.type_id == "sma"));
        assert!(trend.iter().any(|d| d.type_id == "ema"));

        let momentum = manager.get_by_category(IndicatorCategory::Momentum);
        assert!(momentum.iter().any(|d| d.type_id == "rsi"));
        assert!(momentum.iter().any(|d| d.type_id == "macd"));
    }

    #[test]
    fn test_instance_title() {
        let mut manager = IndicatorManager::new();

        let id = manager.create_instance("sma").unwrap();
        let instance = manager.get_instance(id).unwrap();

        // Title format depends on params from catalog
        assert!(instance.title().contains("SMA"));
    }

    #[test]
    fn test_calculate_rsi_values() {
        let mut manager = IndicatorManager::new();

        // Create RSI instance
        let id = manager.create_instance("rsi").unwrap();

        // Create test bars (using mlc_core::Bar which has `timestamp`)
        let bars: Vec<mlc_core::Bar> = (0..100).map(|i| mlc_core::Bar {
            timestamp: i as i64,
            open: 100.0 + (i as f64 * 0.1).sin() * 5.0,
            high: 101.0 + (i as f64 * 0.1).sin() * 5.0,
            low: 99.0 + (i as f64 * 0.1).sin() * 5.0,
            close: 100.0 + (i as f64 * 0.1).sin() * 5.0,
            volume: 1000.0,
        }).collect();

        // Calculate
        manager.calculate(id, &bars);

        // Check values were populated
        let instance = manager.get_instance(id).unwrap();
        let definition = manager.get_definition("rsi").unwrap();

        // RSI should have "rsi" output from rendering catalog
        assert!(!instance.values.is_empty(), "RSI values should not be empty");

        // Check the output name matches between definition and values
        for output in &definition.outputs {
            assert!(instance.values.contains_key(&output.name),
                    "Instance should have values for output '{}', but has keys: {:?}",
                    output.name, instance.values.keys().collect::<Vec<_>>());
        }
    }
}
