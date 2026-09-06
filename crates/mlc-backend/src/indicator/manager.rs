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
    IndicatorOutputType, IndicatorParamType, IndicatorValue, RecalcMode,
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
    /// Next instance ID. Global, not per-bubble: instance identity can
    /// migrate across bubbles via `assign_bubble`/`reassign_bubble`, so a
    /// per-bubble counter would have to be re-keyed on every reassignment.
    /// Single global counter is the correct shape for an identity space
    /// instances can move across bubbles within (design:
    /// `docs/mlc/plans/step-3.3-indicator-watchlist-design.md` §6.1).
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

    }

    /// Normalize legacy type_ids at LOOKUP time — never stored in
    /// `definitions` (a stored alias entry duplicates the row in every
    /// definitions-driven UI list; owner report 2026-07-05: two identical
    /// "VOLUME" rows in the indicator picker).
    ///
    /// `builtin_volume` — pre-cutover hardcoded volume-histogram bypass,
    /// replaced by the real mli catalog entry (type_id `"volume"`).
    /// Persisted instances with the old id (old presets/snapshots) keep
    /// resolving through this normalization; `IndicatorInstance::new`
    /// copies `type_id` from the definition's OWN field ("volume"), so the
    /// legacy id disappears from live state on first restore.
    fn normalize_legacy_type_id(type_id: &str) -> &str {
        match type_id {
            "builtin_volume" => "volume",
            other => other,
        }
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
        let type_id = Self::normalize_legacy_type_id(type_id);
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

        let type_id = Self::normalize_legacy_type_id(type_id);
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
        // Stamp a display order the first time the instance lands on a real
        // bubble. Without this every instance carried `order = 0` and the
        // legend's sequence was whatever the storage happened to yield —
        // it reshuffled across reloads, and hiding a row could move it.
        // Order is the ADD ORDER and nothing else: it is assigned once here
        // and persisted, so visibility never touches it.
        let next_order = self
            .by_bubble_id
            .get(&new_key)
            .map(|ids| {
                ids.iter()
                    .filter_map(|i| self.instances.get(i))
                    .map(|inst| inst.order)
                    .max()
                    .unwrap_or(-1)
                    + 1
            })
            .unwrap_or(0);
        if let Some(inst) = self.instances.get_mut(&id) {
            inst.bubble_id = bubble_id;
            if inst.order == 0 {
                inst.order = next_order;
            }
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

    /// Remove ALL indicator instances belonging to a bubble. The bubble owns
    /// its instances, so this runs when the bubble itself goes away
    /// (`ChartApp::remove_tag`, daemon `DELETE /bubbles/{id}`). It does not
    /// run when a PANEL detaches — a panel moving between bubbles destroys
    /// nothing.
    ///
    /// Three narrower purges lived here and are gone: a clone-only variant
    /// (`origin_id.is_some()`) left every original behind on bubble deletion,
    /// a type-filtered group variant had no callers, and both carried
    /// `*_for_window` aliases from the pre-bubble id space.
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
    /// Replace an instance wholesale from its serialized form, keeping its id.
    ///
    /// The undo path for a parameter edit: the command carries the whole
    /// instance before and after, so applying it is a replace rather than a
    /// per-field write — a parameter set is edited as a group (reset to
    /// defaults and apply-template rewrite several at once).
    pub fn replace_from_json(&mut self, id: u64, json: &str) -> bool {
        let Ok(mut inst) = serde_json::from_str::<IndicatorInstance>(json) else {
            return false;
        };
        inst.id = id;
        self.instances.insert(id, inst);
        true
    }

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
        let mut out: Vec<&IndicatorInstance> = self
            .by_bubble_id
            .get(&bubble_id)
            .map(|ids| ids.iter().filter_map(|id| self.instances.get(id)).collect())
            .unwrap_or_default();
        // ADD ORDER, always — the legend, the object tree and the sub-pane
        // stack all read this list, and the owner's contract is that the
        // sequence is the order things were added and survives a reload.
        // `id` breaks ties so instances restored from a workspace written
        // before `order` was stamped still come out in a stable sequence
        // rather than a different one each run.
        out.sort_by_key(|inst| (inst.order, inst.id));
        out
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
    ///
    /// `deltas`, when present, must be the same length/order as `bars` —
    /// see `IndicatorCalculator::calculate_with_id` for the contract.
    pub fn calculate(&mut self, id: u64, bars: &[mlc_core::Bar]) {
        self.calculate_with_deltas(id, bars, None);
    }

    /// Same as [`Self::calculate`], but threads a real per-bar
    /// (buy_volume, sell_volume) delta series into the compute path for
    /// delta-aware indicator variants (CVD, VolumeDelta).
    pub fn calculate_with_deltas(&mut self, id: u64, bars: &[mlc_core::Bar], deltas: Option<&[(f64, f64)]>) {
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
                if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &instance.params, bars, deltas) {
                    for (output_name, output_values) in result.values {
                        instance.set_values(&output_name, output_values);
                    }
                    instance.signals = result.signals;
                }
            } else {
                // Calculate without signals (faster)
                if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &instance.params, bars, deltas) {
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
    ///
    /// `deltas`, when present, must be the same length/order as `bars` —
    /// see `IndicatorCalculator::calculate_with_id` for the contract.
    pub fn calculate_all_for_bubble(&mut self, bubble_id: u64, bars: &[mlc_core::Bar], deltas: Option<&[(f64, f64)]>) {
        use rayon::prelude::*;

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
                        if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &params, bars, deltas) {
                            return (id, Some(result.values), result.signals);
                        }
                    } else if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &params, bars, deltas) {
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
    ///
    /// `deltas`, when present, must be the same length/order as `bars` —
    /// see `IndicatorCalculator::calculate_with_id` for the contract. Applies
    /// to both the full-recalc and incremental paths below.
    pub fn calculate_for_bubble(&mut self, bubble_id: u64, bars: &[mlc_core::Bar], deltas: Option<&[(f64, f64)]>) {
        use rayon::prelude::*;

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

        // Owned copy so it can be shared into the rayon closure (Send bound).
        let owned_deltas: Option<Vec<(f64, f64)>> = deltas.map(|d| d.to_vec());

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
                    let deltas = owned_deltas.as_deref();
                    if let Some(indicator_id) = machine_id {
                        if signals_enabled {
                            if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &params, bars, deltas) {
                                return (id, Some(result.values), result.signals);
                            }
                        } else if let Some(values) = IndicatorCalculator::calculate_with_id(indicator_id, &params, bars, deltas) {
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
                            let delta = owned_deltas.as_ref().and_then(|d| d.get(i)).copied();
                            IndicatorCalculator::feed_bar_with_delta(&mut fresh_instance, bar, delta);
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
                    let last_delta = owned_deltas.as_ref()
                        .and_then(|d| d.get(current_bar_count.saturating_sub(1)))
                        .copied();
                    let value = IndicatorCalculator::feed_bar_with_delta(&mut working, &current_bar, last_delta);
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
                    let deltas = owned_deltas.as_deref();
                    let (values_opt, signals) = if signals_enabled {
                        if let Some(result) = IndicatorCalculator::calculate_with_signals(indicator_id, &params_clone, bars_slice, deltas) {
                            (Some(result.values), result.signals)
                        } else {
                            (None, Vec::new())
                        }
                    } else {
                        let v = IndicatorCalculator::calculate_with_id(indicator_id, &params_clone, bars_slice, deltas);
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
                            let delta = owned_deltas.as_ref().and_then(|d| d.get(i)).copied();
                            IndicatorCalculator::feed_bar_with_delta(&mut fresh_instance, bar, delta);
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

            let last_delta = owned_deltas.as_ref()
                .and_then(|d| d.get(current_bar_count.saturating_sub(1)))
                .copied();
            let value = IndicatorCalculator::feed_bar_with_delta(&mut cache.instance, &current_bar, last_delta);
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
                        IndicatorValue::StringList(_) => value.to_display_string(),
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
                        IndicatorValue::StringList(_) => v.to_display_string(),
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
                        IndicatorParamType::Venues => ChartParamType::Venues,
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

        // Special case: volume indicator bool param. "builtin_volume" is
        // not reachable as an instance type_id (IndicatorInstance::new
        // always copies type_id from the definition's own field, which is
        // "volume" even through the migration alias — see
        // `register_legacy_aliases`), so it is deliberately not matched here.
        //
        // Colors are NOT seeded here: absent up_color/down_color the
        // renderer falls back to the theme candle colors (+50% alpha), so
        // volume INHERITS the bar colors of the active theme/skin (owner
        // rule 2026-07-31). Only an explicit user color in the instance
        // params overrides that.
        if inst.type_id == "volume" || inst.type_id == "vol" {
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

    // =========================================================================
    // Scoped park/unpark — bubble-id-subset extraction (shared-BackendCore
    // arc, §5.3). Replaces whole-manager `swap()`: under a single
    // process-wide `BackendCore`, wholesale-swapping would rip every OTHER
    // window's indicator instances out too. `definitions` (the 480+ static
    // catalog) is process-wide config, not per-preset state — it is NOT
    // part of the extracted manager (nothing computes against a parked
    // manager; it just sits in `LivePresetState` until unparked).
    // `dirty_symbols`/`new_bar_symbols` are per-symbol recalc-scheduling
    // hints, not per-bubble state — they are naturally regenerated by future
    // ticks and are left untouched on `self`, not extracted either.
    // =========================================================================

    /// Remove and return only the indicator instances bound to `bubble_ids`
    /// (via `by_bubble_id`), plus their `compute_caches` entries, as a
    /// fresh, small `IndicatorManager`. The returned manager's `next_id`
    /// starts at 1 — safe because ids removed from `self` never get reused
    /// by `self` (its own `next_id` only grows) and the extracted instances
    /// keep their ORIGINAL ids (no renumbering), so there is no collision
    /// risk when they are merged back by [`Self::merge_for_bubbles`].
    ///
    /// Used exclusively by preset tab park (`mlc-app/src/persistence/
    /// profile.rs::park_active_preset`).
    pub fn extract_for_bubbles(&mut self, bubble_ids: &[u64]) -> IndicatorManager {
        let mut out = IndicatorManager {
            definitions: HashMap::new(),
            instances: HashMap::new(),
            by_bubble_id: HashMap::new(),
            next_id: 1,
            recalc_mode: self.recalc_mode,
            dirty_symbols: HashSet::new(),
            new_bar_symbols: HashSet::new(),
            compute_caches: HashMap::new(),
            last_incremental_count: 0,
            last_full_count: 0,
        };
        for &key in bubble_ids {
            let Some(ids) = self.by_bubble_id.remove(&key) else { continue };
            let mut moved_ids = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(inst) = self.instances.remove(&id) {
                    if let Some(cache) = self.compute_caches.remove(&id) {
                        out.compute_caches.insert(id, cache);
                    }
                    out.instances.insert(id, inst);
                    moved_ids.push(id);
                }
            }
            if !moved_ids.is_empty() {
                out.by_bubble_id.insert(key, moved_ids);
            }
        }
        out
    }

    /// Re-insert every instance, cache entry, and `by_bubble_id` bucket from
    /// `other` into `self`. Bumps `self.next_id` past the highest id merged
    /// back in, so future `create_instance` calls never collide with a
    /// restored id (mirrors `create_instance_with_id`'s bump rule).
    /// `other.definitions`/`dirty_symbols`/`new_bar_symbols` are discarded
    /// (always empty on an extracted manager — see
    /// [`Self::extract_for_bubbles`]).
    ///
    /// Used exclusively by preset tab unpark (`mlc-app/src/persistence/
    /// profile.rs::unpark_preset`).
    pub fn merge_for_bubbles(&mut self, other: IndicatorManager) {
        for (id, inst) in other.instances {
            if id >= self.next_id {
                self.next_id = id + 1;
            }
            self.instances.insert(id, inst);
        }
        for (id, cache) in other.compute_caches {
            self.compute_caches.insert(id, cache);
        }
        for (key, ids) in other.by_bubble_id {
            self.by_bubble_id.entry(key).or_default().extend(ids);
        }
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
            ..Default::default()
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

#[cfg(test)]
mod park_unpark_tests {
    //! Round-trip coverage for `extract_for_bubbles`/`merge_for_bubbles` —
    //! the shared-BackendCore-arc scoped-park replacement for whole-manager
    //! `swap()` (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §5.3).
    //! Also covers the §5.4 stop-condition concern this manager specifically
    //! raised: `next_id` must not collide after extraction + merge-back.

    use super::*;

    #[test]
    fn extract_then_merge_round_trips_to_identical_manager() {
        let mut mgr = IndicatorManager::new();
        let id_a = mgr.create_instance("sma").expect("create sma");
        mgr.assign_bubble(id_a, Some(1));
        let id_b = mgr.create_instance("ema").expect("create ema");
        mgr.assign_bubble(id_b, Some(2));
        let id_c = mgr.create_instance("rsi").expect("create rsi");
        mgr.assign_bubble(id_c, Some(3));

        let extracted = mgr.extract_for_bubbles(&[1, 2]);
        assert!(mgr.get_instance(id_a).is_none());
        assert!(mgr.get_instance(id_b).is_none());
        assert!(mgr.get_instance(id_c).is_some(), "non-extracted instance must remain in the source manager");
        assert_eq!(extracted.get_instances_for_bubble(1).len(), 1);
        assert_eq!(extracted.get_instances_for_bubble(2).len(), 1);
        assert!(extracted.get_instances_for_bubble(3).is_empty());

        mgr.merge_for_bubbles(extracted);
        assert!(mgr.get_instance(id_a).is_some());
        assert!(mgr.get_instance(id_b).is_some());
        assert!(mgr.get_instance(id_c).is_some());
        assert_eq!(mgr.get_instances_for_bubble(1).len(), 1);
        assert_eq!(mgr.get_instances_for_bubble(2).len(), 1);
    }

    #[test]
    fn merge_back_bumps_next_id_no_collision_with_instances_created_while_parked() {
        // Reproduces the exact §5.4 hazard this manager was flagged for:
        // while bubble 1's instances are parked (extracted), the SOURCE
        // manager keeps allocating new ids for OTHER bubbles. Merging the
        // parked instances back must never collide with ids allocated in
        // the meantime.
        let mut mgr = IndicatorManager::new();
        let parked_id = mgr.create_instance("sma").expect("create sma");
        mgr.assign_bubble(parked_id, Some(1));

        let extracted = mgr.extract_for_bubbles(&[1]);
        assert!(mgr.get_instance(parked_id).is_none());

        // Simulate other windows creating instances while this one is parked.
        let mut newly_created = Vec::new();
        for _ in 0..5 {
            let id = mgr.create_instance("ema").expect("create ema");
            mgr.assign_bubble(id, Some(2));
            newly_created.push(id);
        }

        mgr.merge_for_bubbles(extracted);

        // The originally-parked instance must be back, unrenamed.
        assert!(mgr.get_instance(parked_id).is_some());
        // None of the ids allocated while parked may have been clobbered.
        for id in &newly_created {
            assert!(mgr.get_instance(*id).is_some(), "instance {} created while bubble 1 was parked must survive merge-back", id);
        }
        // A fresh allocation after merge must not collide with any existing id.
        let post_merge_id = mgr.create_instance("rsi").expect("create rsi");
        assert!(mgr.get_instance(post_merge_id).is_some());
        assert_ne!(post_merge_id, parked_id);
        for id in &newly_created {
            assert_ne!(post_merge_id, *id);
        }
    }

    #[test]
    fn extract_other_windows_instances_are_untouched() {
        let mut mgr = IndicatorManager::new();
        let window_a_id = mgr.create_instance("sma").expect("create sma");
        mgr.assign_bubble(window_a_id, Some(1));
        let window_b_id = mgr.create_instance("ema").expect("create ema");
        mgr.assign_bubble(window_b_id, Some(2));

        let parked_b = mgr.extract_for_bubbles(&[2]);

        assert!(mgr.get_instance(window_a_id).is_some(), "window A's instance must survive parking window B's preset");
        assert!(parked_b.get_instance(window_a_id).is_none(), "extraction must not leak window A's instance into window B's payload");
    }
}
