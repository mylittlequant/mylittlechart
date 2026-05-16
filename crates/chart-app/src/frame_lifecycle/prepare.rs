//! prepare_frame: pre-render state sync — runs before each render to align
//! cached state, layout dirty flags, and downstream consumers with the
//! latest tick() output.

use crate::ChartApp;
use uzor::WidgetId;
use zengeld_chart::{
    ChartId, ChartPanelLayout, LayoutRect,
};
use zengeld_chart::ui::modal_state::OpenModal;
use zengeld_terminal_indicators::RecalcMode;

impl ChartApp {
    /// Pre-render mutations — call once per frame on the mutable self BEFORE
    /// calling `render_to_scene`.
    ///
    /// Handles:
    /// - Layout computation and `content_rect` / `right_toolbar_left_x` sync
    /// - `indicator_manager.recalc_mode_label` sync
    /// - `diagnostics_enabled` sync
    /// - Viewport dimensions sync via `sync_viewport_from_layout()`
    /// - Alert-settings modal sync
    /// - Sidebar data rebuild (when `sidebar_data_dirty` is set)
    pub fn prepare_frame(&mut self, width: f64, height: f64) {
        // Advance the text-field store's frame counter so stale field geometry
        // from a previous frame is expired before new update_field calls arrive.
        // NOTE: render_to_scene also calls coordinator.begin_frame() which calls
        // text_fields.begin_frame() internally.  We call it here too so that
        // prepare_frame (called before render) stamps the correct frame on
        // update_field calls that follow render.
        self.input_coordinator.borrow_mut().text_fields_mut().begin_frame();

        // Sync text-field cursor → picker.hex_cursor before rendering.
        // The renderer reads hex_cursor from ColorPickerState, but the text-field
        // store owns the authoritative cursor position after mouse/keyboard events.
        let hex_id = WidgetId::from(crate::text_input::HEX_COLOR);
        if self.input_coordinator.borrow().text_fields().is_focused(&hex_id) {
            let coord = self.input_coordinator.borrow();
            let tf = coord.text_fields();
            let cursor = tf.cursor(&hex_id);
            let text = tf.text(&hex_id).to_string();
            let sel = tf.selection_range(&hex_id);
            drop(coord);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let cursor_vis = self.input_coordinator.borrow().text_fields().cursor_visible(now_ms);
            for picker in [
                &mut self.panel_app.primitive_settings_state.color_picker,
                &mut self.panel_app.indicator_settings_state.color_picker,
                &mut self.panel_app.chart_settings_state.color_picker,
                &mut self.panel_app.compare_settings_state.color_picker,
                &mut self.panel_app.panel_color_picker,
            ] {
                if picker.hex_editing {
                    picker.hex_cursor = cursor;
                    picker.hex_input = text.clone();
                    picker.hex_selection_start = sel.map(|(s, _)| s);
                    picker.hex_selection_end = sel.map(|(_, e)| e);
                    picker.hex_cursor_visible = cursor_vis;
                }
            }
        }

        // Sync text-field store → sidebar_state for agent chat input rendering.
        let chat_id = WidgetId::from(crate::text_input::AGENT_CHAT);
        if self.input_coordinator.borrow().text_fields().is_focused(&chat_id) {
            let coord = self.input_coordinator.borrow();
            let tf = coord.text_fields();
            let cursor = tf.cursor(&chat_id);
            let text = tf.text(&chat_id).to_string();
            let sel = tf.selection_range(&chat_id);
            drop(coord);
            let now_ms = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64;
            let cursor_vis = self.input_coordinator.borrow().text_fields().cursor_visible(now_ms);
            self.sidebar_state.agent_input_cursor_visible = cursor_vis;
            self.sidebar_state.agent_input_focused_leaf = self.sidebar_state.focused_agent_leaf;
            if let Some(leaf_id) = self.sidebar_state.focused_agent_leaf {
                self.sidebar_state.agent_input_buffers.insert(leaf_id, text);
                self.sidebar_state.agent_input_cursors.insert(leaf_id, cursor);
                self.sidebar_state.agent_input_selections.insert(
                    leaf_id, (sel.map(|(s, _)| s), sel.map(|(_, e)| e))
                );
            }
        } else {
            self.sidebar_state.agent_input_focused_leaf = None;
        }

        let sidebar_w = self.sidebar_state.right_width();
        let window_rect = LayoutRect::new(0.0, 0.0, width, height);
        let panel_layout = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);

        // Sync content_rect and right_toolbar_left_x so input handlers have
        // correct coordinates before the frame is rendered.
        let content_rect = {
            let mut r = panel_layout.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };
        self.content_rect = content_rect;
        self.right_toolbar_left_x = panel_layout.right_toolbar_rect.x;

        // Clamp open color picker popups to content_rect so they never
        // overlap toolbars or the right sidebar.
        {
            let cr = &content_rect;
            let margin = 4.0;
            for picker in [
                &mut self.panel_app.primitive_settings_state.color_picker,
                &mut self.panel_app.indicator_settings_state.color_picker,
                &mut self.panel_app.chart_settings_state.color_picker,
                &mut self.panel_app.compare_settings_state.color_picker,
                &mut self.panel_app.panel_color_picker,
            ] {
                if !picker.is_open() { continue; }
                let (pw, ph) = match picker.level {
                    zengeld_chart::ui::color_picker_state::ColorPickerLevel::L1 => picker.l1_config().calculate_size(),
                    zengeld_chart::ui::color_picker_state::ColorPickerLevel::L2 => picker.l2_config().calculate_size(),
                    _ => continue,
                };
                let min_x = cr.x + margin;
                let min_y = cr.y + margin;
                let max_x = (cr.x + cr.width - pw - margin).max(min_x);
                let max_y = (cr.y + cr.height - ph - margin).max(min_y);
                picker.origin.0 = picker.origin.0.clamp(min_x, max_x);
                picker.origin.1 = picker.origin.1.clamp(min_y, max_y);
            }
        }

        // Sync recalc_mode_label into user_settings_state so the modal can display it.
        self.panel_app.user_settings_state.recalc_mode_label = match self.indicator_manager.recalc_mode {
            RecalcMode::PerTick  => "Per Tick".to_string(),
            RecalcMode::PerFrame => "Per Frame".to_string(),
            RecalcMode::PerBar   => "Per Bar".to_string(),
        };
        // Sync diagnostics flag so the checkbox reflects the current state.
        self.panel_app.user_settings_state.diagnostics_enabled = self.diagnostics_enabled;
        // Sync data_load settings into user_settings_state for the DATA & CACHE sliders.
        // Only update the cached values when the slider is not being dragged so the
        // handle does not snap back to the committed value on every frame during drag.
        if !self.panel_app.user_settings_state.is_data_slider_dragging() {
            let dl = &self.panel_app.user_manager.profile.data_load;
            self.panel_app.user_settings_state.data_bg_bars      = dl.background_bar_count;
            self.panel_app.user_settings_state.data_max_bars     = dl.max_loaded_bars;
            self.panel_app.user_settings_state.data_store_size_mb = dl.max_store_size_mb;
            self.panel_app.user_settings_state.data_cleanup_days  = dl.store_cleanup_days;
        }

        // Sync viewport dimensions.
        // In split mode, viewport sync is handled later in the split-pane
        // layout block (after panel_grid.layout() computes up-to-date rects).
        // Running it here too would read stale panel_rects from the previous
        // frame and apply an incorrect bar_shift to view_start.
        if !self.panel_app.panel_grid.is_split() {
            self.sync_viewport_from_layout();
        }

        // Deferred viewport snap: set_bars() defers snap-to-end + auto-scale
        // to here where layout dimensions are guaranteed valid.
        // In split mode this runs AFTER the split-layout block sets real
        // chart_width values (see below).
        if !self.panel_app.panel_grid.is_split() {
            let mut snapped_windows: Vec<(ChartId, f64, f64)> = Vec::new(); // (chart_id, view_start, bar_spacing)
            for (&chart_id, window) in self.panel_app.panel_grid.windows_mut().iter_mut() {
                if window.needs_auto_scale_after_bars && !window.bars.is_empty() && window.viewport.chart_width > 0.0 {
                    window.needs_auto_scale_after_bars = false;
                    // Snap to end with standard margin.
                    window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                    // No snap_cooldown needed: snap fires in prepare_frame AFTER
                    // sync_viewport_from_layout, so bar_shift cannot undo it this frame.
                    // Next frame old_width == new_width → bar_shift = 0.
                    window.calc_auto_scale();
                    // restore_scale_mode is already consumed inside set_bars() for the eager
                    // path; for the deferred path (chart_width was 0 at set_bars time),
                    // consume it here.
                    if let Some(mode) = window.restore_scale_mode.take() {
                        window.price_scale.scale_mode = mode;
                    }
                    snapped_windows.push((chart_id, window.viewport.view_start, window.viewport.bar_spacing));
                }
            }

            // Propagate viewport snap to sync-group peers so all synced windows
            // align to the same TIME position after bar load (not just user pan/zoom).
            for (chart_id, view_start, bar_spacing) in snapped_windows {
                if let Some(leaf_id) = self.panel_app.panel_grid.leaf_for_chart_id(chart_id) {
                    self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, None);
                }
            }
        }

        // Keep the alert-settings modal's alerts list always in sync.
        if self.panel_app.alert_settings_state.is_open() {
            self.panel_app.alert_settings_state.all_alerts =
                self.alert_manager.items().to_vec();
        }

        // Auto-dirty sidebar when active leaf changes (for object tree refresh).
        let current_leaf = self.panel_app.panel_grid.docking().active_leaf();
        if current_leaf != self.last_active_leaf {
            self.last_active_leaf = current_leaf;
            self.sidebar_data_dirty = true;

            // When the active chart changes, reassign all Synced panels to the new
            // chart's group and apply the group's instrument key to their states.
            if let Some(new_active_group) = self.panel_app.panel_grid
                .active_chart_id()
                .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
            {
                // Update the group's exchange/account_type from the new active window so that
                // the group carries the full instrument key (symbol already tracked by TagManager).
                if let Some(w) = self.panel_app.panel_grid.active_window() {
                    let exch = w.exchange.clone();
                    let at = w.account_type.clone();
                    if let Some(g) = self.panel_app.tag_manager.group_mut(new_active_group) {
                        g.exchange = exch;
                        g.account_type = at;
                    }
                }
                self.panel_app.tag_manager.reassign_synced_panels(new_active_group);
                self.apply_key_to_panels_in_group(new_active_group);
            }
        }

        // Populate sidebar data from chart state (guarded by dirty flag).
        if self.sidebar_state.is_right_open() && self.sidebar_data_dirty {
            // --- ObjectTree: drawing primitives + indicators ---
            self.sidebar_state.object_tree_items.clear();

            let active_cid = self.panel_app.panel_grid.active_chart_id();

            // Determine whether the active window is in a real (non-auto_created) tag group.
            let tagged_group = active_cid
                .and_then(|cid| self.panel_app.tag_manager.group_for_window(cid))
                .and_then(|gid| self.panel_app.tag_manager.group(gid))
                .filter(|g| !g.auto_created);

            // Helper: convert a PrimitiveKind into an ObjectCategory.
            let prim_category = |kind: zengeld_chart::PrimitiveKind| match kind {
                zengeld_chart::PrimitiveKind::Annotation => zengeld_chart::ObjectCategory::Text,
                zengeld_chart::PrimitiveKind::Measurement => zengeld_chart::ObjectCategory::Measurement,
                zengeld_chart::PrimitiveKind::Trading => zengeld_chart::ObjectCategory::Position,
                zengeld_chart::PrimitiveKind::Signal => zengeld_chart::ObjectCategory::Signal,
                _ => zengeld_chart::ObjectCategory::Drawing,
            };

            // Collect active window key fields before any borrows.
            let (active_window_sym, active_window_exchange, active_window_account_type) =
                self.panel_app.panel_grid.active_window()
                    .map(|w| (w.symbol.clone(), w.exchange.clone(), w.account_type.clone()))
                    .unwrap_or_default();

            if let Some(group) = tagged_group {
                // ----------------------------------------------------------------
                // TAGGED window: two sections — "Group" and (optionally) "Window"
                // ----------------------------------------------------------------

                // --- Section "Group": primitives from group.primitives ---
                if group.sync_flags.sync_drawings {
                    for p in group.primitives.iter() {
                        let data = p.data();
                        let kind = p.kind();
                        let display = p.display_name().to_string();
                        let name = if display.is_empty() { data.type_id.as_str() } else { display.as_str() };
                        // Group primitives inherit the window's exchange/account_type since
                        // PrimitiveData has no exchange/account_type fields. If multi-exchange
                        // groups are added in the future, PrimitiveData would need those fields.
                        let prim_sym = data.symbol.clone();
                        let is_active_sym = prim_sym == active_window_sym;
                        let item_state = if is_active_sym {
                            sidebar_content::types::ObjectItemState::Active
                        } else {
                            sidebar_content::types::ObjectItemState::Memory
                        };
                        let memory_kind = if is_active_sym {
                            sidebar_content::types::MemoryKind::None
                        } else {
                            sidebar_content::types::MemoryKind::GroupOtherKey
                        };
                        let item = sidebar_content::types::ObjectTreeItem::new(
                            data.id, name, prim_category(kind), &data.type_id,
                        )
                        .with_visible(data.visible)
                        .with_locked(data.locked)
                        .with_color(Some(data.color.stroke.clone()))
                        .with_section("Group")
                        .with_key(&prim_sym, &active_window_exchange, &active_window_account_type)
                        .with_item_state(item_state)
                        .with_memory_kind(memory_kind);
                        self.sidebar_state.object_tree_items.push(item);
                    }
                }

                // --- Section "Group": indicators from group.indicator_configs ---
                if group.sync_flags.sync_indicators {
                    let active_window_id = active_cid.map(|cid| cid.0);
                    for cfg in group.indicator_configs.iter() {
                        // Resolve to the active window's own instance so that widget
                        // actions (visibility, delete, settings) use the correct ID.
                        let local = active_window_id.and_then(|wid| {
                            self.indicator_manager.instances_iter()
                                .find(|i| i.window_id == Some(wid) && i.type_id == cfg.type_id)
                        });
                        let (id, name, type_id, visible, locked) = match local {
                            Some(inst) => (inst.id, inst.name.clone(), inst.type_id.clone(), inst.visible, inst.locked),
                            None => (cfg.id, cfg.name.clone(), cfg.type_id.clone(), cfg.visible, false),
                        };
                        let cfg_sym = cfg.symbol.clone();
                        let is_active_sym = cfg_sym == active_window_sym;
                        let item_state = if is_active_sym {
                            sidebar_content::types::ObjectItemState::Active
                        } else {
                            sidebar_content::types::ObjectItemState::Memory
                        };
                        let memory_kind = if is_active_sym {
                            sidebar_content::types::MemoryKind::None
                        } else {
                            sidebar_content::types::MemoryKind::GroupIndicatorOtherKey
                        };
                        let item = sidebar_content::types::ObjectTreeItem::new(
                            id, &name, zengeld_chart::ObjectCategory::Indicator, &type_id,
                        )
                        .with_visible(visible)
                        .with_locked(locked)
                        .with_section("Group")
                        .with_key(&cfg_sym, &active_window_exchange, &active_window_account_type)
                        .with_item_state(item_state)
                        .with_memory_kind(memory_kind);
                        self.sidebar_state.object_tree_items.push(item);
                    }
                }

                // --- Section "Window": window-local stashed primitives ---
                // Collect stashed primitive data first so we don't hold an active_window borrow
                // while also needing indicator_manager (which is not behind the same ref).
                // Stashed primitives are always shown regardless of sync_drawings state —
                // they represent objects that were on the window before joining the tag group.
                let stashed_prim_data: Vec<_> = self.panel_app.panel_grid.active_window()
                    .map(|w| w.stashed_primitives.iter()
                        .map(|p| {
                            let data = p.data();
                            let kind = p.kind();
                            let display = p.display_name().to_string();
                            (data.id, display, data.type_id.clone(), kind, data.visible, data.locked, data.color.stroke.clone(), data.symbol.clone())
                        })
                        .collect())
                    .unwrap_or_default();

                // Collect window-local indicator IDs before releasing the window borrow.
                let pre_tag_ids: Vec<u64> = self.panel_app.panel_grid.active_window()
                    .map(|w| w.pre_tag_indicator_ids.clone())
                    .unwrap_or_default();

                let has_window_section = !stashed_prim_data.is_empty() || !pre_tag_ids.is_empty();

                if has_window_section {
                    for (id, display, type_id, kind, visible, locked, stroke, prim_symbol) in &stashed_prim_data {
                        let name = if display.is_empty() { type_id.as_str() } else { display.as_str() };
                        let item = sidebar_content::types::ObjectTreeItem::new(
                            *id, name, prim_category(*kind), type_id,
                        )
                        .with_visible(*visible)
                        .with_locked(*locked)
                        .with_color(Some(stroke.clone()))
                        .with_section("Window")
                        .with_key(prim_symbol, &active_window_exchange, &active_window_account_type)
                        .with_item_state(sidebar_content::types::ObjectItemState::Memory)
                        .with_memory_kind(sidebar_content::types::MemoryKind::WindowStash);
                        self.sidebar_state.object_tree_items.push(item);
                    }

                    for &iid in &pre_tag_ids {
                        if let Some(inst) = self.indicator_manager.instances_iter()
                            .find(|i| i.id == iid)
                        {
                            let item = sidebar_content::types::ObjectTreeItem::new(
                                inst.id,
                                &inst.name,
                                zengeld_chart::ObjectCategory::Indicator,
                                &inst.type_id,
                            )
                            .with_visible(inst.visible)
                            .with_locked(inst.locked)
                            .with_section("Window")
                            .with_key(&active_window_sym, &active_window_exchange, &active_window_account_type)
                            .with_item_state(sidebar_content::types::ObjectItemState::Active);
                            self.sidebar_state.object_tree_items.push(item);
                        }
                    }
                }
            } else {
                // ----------------------------------------------------------------
                // UNTAGGED window (auto_created group): flat list, no section headers
                // ----------------------------------------------------------------

                // Primitives from window-local drawing_manager — all symbols, annotated by state.
                let local_prims: Vec<_> = self.panel_app.panel_grid.active_window()
                    .map(|w| w.drawing_manager.primitives().iter()
                        .map(|p| {
                            let data = p.data();
                            let kind = p.kind();
                            let display = p.display_name().to_string();
                            (data.id, display, data.type_id.clone(), kind, data.visible, data.locked, data.color.stroke.clone(), data.symbol.clone())
                        })
                        .collect())
                    .unwrap_or_default();

                for (id, display, type_id, kind, visible, locked, stroke, prim_sym) in &local_prims {
                    let name = if display.is_empty() { type_id.as_str() } else { display.as_str() };
                    let is_active_sym = *prim_sym == active_window_sym;
                    let item_state = if is_active_sym {
                        sidebar_content::types::ObjectItemState::Active
                    } else {
                        sidebar_content::types::ObjectItemState::Memory
                    };
                    let memory_kind = if is_active_sym {
                        sidebar_content::types::MemoryKind::None
                    } else {
                        sidebar_content::types::MemoryKind::WindowOtherKey
                    };
                    let item = sidebar_content::types::ObjectTreeItem::new(
                        *id, name, prim_category(*kind), type_id,
                    )
                    .with_visible(*visible)
                    .with_locked(*locked)
                    .with_color(Some(stroke.clone()))
                    .with_key(prim_sym, &active_window_exchange, &active_window_account_type)
                    .with_item_state(item_state)
                    .with_memory_kind(memory_kind);
                    self.sidebar_state.object_tree_items.push(item);
                }

                // Indicators from indicator_manager for this window — all symbols, annotated by state.
                let window_id = active_cid.map(|cid| cid.0);
                if let Some(wid) = window_id {
                    let insts: Vec<_> = self.indicator_manager.instances_iter()
                        .filter(|i| i.window_id == Some(wid))
                        .map(|i| (i.id, i.name.clone(), i.type_id.clone(), i.visible, i.locked, i.symbol.clone()))
                        .collect();
                    for (id, name, type_id, visible, locked, inst_sym) in &insts {
                        let is_active_sym = *inst_sym == active_window_sym;
                        let item_state = if is_active_sym {
                            sidebar_content::types::ObjectItemState::Active
                        } else {
                            sidebar_content::types::ObjectItemState::Memory
                        };
                        let memory_kind = if is_active_sym {
                            sidebar_content::types::MemoryKind::None
                        } else {
                            sidebar_content::types::MemoryKind::WindowOtherKey
                        };
                        let item = sidebar_content::types::ObjectTreeItem::new(
                            *id,
                            name,
                            zengeld_chart::ObjectCategory::Indicator,
                            type_id,
                        )
                        .with_visible(*visible)
                        .with_locked(*locked)
                        .with_key(inst_sym, &active_window_exchange, &active_window_account_type)
                        .with_item_state(item_state)
                        .with_memory_kind(memory_kind);
                        self.sidebar_state.object_tree_items.push(item);
                    }
                }
            }

            // Sort within sections: Active first, then Memory (stable to preserve sub-order).
            self.sidebar_state.object_tree_items.sort_by_key(|item| {
                let section_order: u8 = match item.section.as_deref() {
                    Some("Group") => 0,
                    Some("Window") => 1,
                    _ => 2,
                };
                let state_order: u8 = match item.item_state {
                    sidebar_content::types::ObjectItemState::Active => 0,
                    sidebar_content::types::ObjectItemState::Memory => 1,
                };
                (section_order, state_order)
            });

            // --- ObjectTree: compare overlay series ---
            if let Some(window) = self.panel_app.panel_grid.active_window() {
                for (i, series) in window.compare_overlay.series.iter().enumerate() {
                    let item = sidebar_content::types::ObjectTreeItem::new(
                        i as u64,
                        &series.symbol,
                        zengeld_chart::ObjectCategory::Compare,
                        "Compare",
                    )
                    .with_visible(series.visible)
                    .with_color(Some(series.color.clone()));
                    self.sidebar_state.object_tree_items.push(item);
                }
            }

            // --- Signals panel: collect per-instance SignalEvents ---
            use sidebar_content::types::{IndicatorsTabData, IndicatorSignalGroup, IndicatorSignalRow};

            // Only show signals for indicator instances that belong to the active window.
            let active_window_id_for_signals = active_cid.map(|cid| cid.0);
            let signal_groups: Vec<IndicatorSignalGroup> = self
                .indicator_manager
                .instances_iter()
                .filter(|inst| {
                    !inst.signals.is_empty()
                        && active_window_id_for_signals
                            .map(|wid| inst.window_id == Some(wid))
                            .unwrap_or(false)
                })
                .map(|inst| {
                    let mut rows: Vec<IndicatorSignalRow> = inst
                        .signals
                        .iter()
                        .map(|ev| IndicatorSignalRow {
                            bar_index: ev.bar_index as i64,
                            signal_type: format!("{:?}", ev.kind),
                            price: ev.price,
                            strength: 0.0,
                            direction: ev.direction.as_i8() as i32,
                        })
                        .collect();
                    rows.sort_by(|a, b| b.bar_index.cmp(&a.bar_index));
                    IndicatorSignalGroup {
                        instance_id: inst.id,
                        indicator_name: inst.name.clone(),
                        collapsed: self
                            .sidebar_state
                            .collapsed_signal_groups
                            .contains(&inst.id),
                        signals: rows,
                    }
                })
                .collect();

            let total_count = signal_groups.iter().map(|g| g.signals.len()).sum();
            self.sidebar_state.indicator_signals = IndicatorsTabData {
                groups: signal_groups,
                total_count,
            };

            // --- Watchlist: populate from WatchlistManager symbol list ---
            {
                use sidebar_content::types::WatchlistItem;

                self.sidebar_state.watchlist_items.clear();
                let watchlist_entries: Vec<(String, String, String)> = self
                    .sidebar_state
                    .watchlist_manager
                    .active_list()
                    .map(|list| {
                        list.all_symbols()
                            .iter()
                            .map(|ws| (ws.symbol.clone(), ws.exchange.clone(), ws.account_type.clone()))
                            .collect()
                    })
                    .unwrap_or_default();

                for (sym_name, sym_exchange, sym_account_type) in &watchlist_entries {
                    let price_data = self.panel_app.panel_grid.iter_windows()
                        .find(|(_, w)| w.symbol == *sym_name && w.exchange == *sym_exchange && w.account_type == *sym_account_type)
                        .and_then(|(_, w)| w.bars.last())
                        .map(|bar| (bar.close, bar.open, bar.high, bar.low, bar.volume));

                    if let Some((price, open, high, low, volume)) = price_data {
                        let change_pct = if open != 0.0 {
                            (price - open) / open * 100.0
                        } else {
                            0.0
                        };
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: price,
                            change_percent: change_pct,
                            high_24h: high,
                            low_24h: low,
                            volume_24h: volume,
                            account_type: sym_account_type.clone(),
                        });
                    } else if let Some(ticker) = self.mini_ticker_cache.get(&format!("{}:{}:{}", sym_name, sym_exchange, sym_account_type)) {
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: ticker.last_price,
                            change_percent: ticker.price_change_percent,
                            high_24h: ticker.high_price,
                            low_24h: ticker.low_price,
                            volume_24h: ticker.volume,
                            account_type: sym_account_type.clone(),
                        });
                    } else {
                        self.sidebar_state.watchlist_items.push(WatchlistItem {
                            symbol: sym_name.clone(),
                            exchange: sym_exchange.clone(),
                            last_price: 0.0,
                            change_percent: 0.0,
                            high_24h: 0.0,
                            low_24h: 0.0,
                            volume_24h: 0.0,
                            account_type: sym_account_type.clone(),
                        });
                    }
                }
            }

            // --- Connectors: populate from ConnectorRegistry + active pool ---
            {
                use digdigdig3::connector_manager::{ConnectorRegistry, AuthType};
                use sidebar_content::types::ConnectorStatusItem;
                use sidebar_content::types::ConnectorGroup;

                self.sidebar_state.connector_items.clear();
                let registry = self.connector_registry.get_or_insert_with(ConnectorRegistry::new);
                let active_ids = self.bridge.pool().ids();

                let metrics_map: std::collections::HashMap<String, (digdigdig3::core::types::ConnectorStats, usize)> =
                    self.bridge.collect_metrics()
                        .into_iter()
                        .map(|(eid, stats, ws)| (eid.as_str().to_string(), (stats, ws)))
                        .collect();

                {
                    use sidebar_content::MetricsSnapshot;
                    let now = std::time::Instant::now();
                    let should_sample = self.sidebar_state.metrics_last_sample
                        .is_none_or(|last| now.duration_since(last).as_secs_f64() >= 1.0);
                    if should_sample {
                        self.sidebar_state.metrics_last_sample = Some(now);
                        for (exchange_id, (stats, ws_count)) in &metrics_map {
                            self.sidebar_state.push_metrics_sample(exchange_id, MetricsSnapshot {
                                http_requests: stats.http_requests,
                                http_errors: stats.http_errors,
                                latency_ms: stats.last_latency_ms,
                                rate_used: stats.rate_used,
                                rate_max: stats.rate_max,
                                ws_count: *ws_count,
                                ws_ping_rtt_ms: stats.ws_ping_rtt_ms,
                            });
                        }
                    }
                }

                for meta in registry.list_all() {
                    let is_active = active_ids.contains(&meta.id);
                    if !is_active {
                        continue; // connector not in pool = not shown
                    }

                    let mut item = ConnectorStatusItem::new(
                        meta.id.as_str(),
                        meta.name,
                    );

                    let pool = self.bridge.pool();
                    let at = digdigdig3::AccountType::Spot;
                    let md_caps = pool.market_data_capabilities(&meta.id, at);
                    let tr_caps = pool.trading_capabilities(&meta.id, at);
                    let ac_caps = pool.account_capabilities(&meta.id, at);

                    item.enabled = *self.sidebar_state.connector_enabled
                        .get(meta.id.as_str())
                        .unwrap_or(&true);
                    item.expanded = *self.sidebar_state.connector_expanded
                        .get(meta.id.as_str())
                        .unwrap_or(&false);
                    item.rest_healthy = item.enabled;

                    let has_ws = md_caps.map_or(false, |md| md.has_ws_klines || md.has_ws_trades || md.has_ws_orderbook);
                    item.ws_connected = item.enabled && has_ws;

                    item.auth_type = match meta.authentication {
                        AuthType::ApiKey => "API Key".to_string(),
                        AuthType::OAuth2 => "OAuth2".to_string(),
                        AuthType::TOTP => "TOTP".to_string(),
                        AuthType::BasicAuth => "Basic Auth".to_string(),
                        AuthType::BearerToken => "Bearer Token".to_string(),
                        AuthType::None => "None".to_string(),
                    };
                    item.requires_api_key = meta.requires_api_key_for_data;
                    item.free_tier = meta.free_tier;
                    item.group = if md_caps.map_or(true, |md| !md.has_klines) {
                        ConnectorGroup::NonChartData
                    } else if meta.requires_api_key_for_data {
                        ConnectorGroup::RequiresApiKey
                    } else {
                        ConnectorGroup::NoApiKey
                    };

                    if let Some(md) = md_caps {
                        item.has_klines = md.has_klines;
                        item.has_trades = md.has_recent_trades;
                        item.has_orderbook = md.has_orderbook;
                        item.has_aggregated_bars = md.has_klines;
                    }
                    if let Some(md) = md_caps {
                        item.has_ws_klines = md.has_ws_klines;
                        item.has_ws_trades = md.has_ws_trades;
                        item.has_ws_orderbook = md.has_ws_orderbook;
                    }
                    if let Some(tr) = tr_caps {
                        item.has_trading = tr.has_market_order || tr.has_limit_order;
                    }
                    if let Some(ac) = ac_caps {
                        item.has_account = ac.has_balances;
                    }
                    if let Some(ac) = ac_caps {
                        item.has_positions = ac.has_positions;
                    }

                    // Derive legacy UI fields from RateLimitCapabilities
                    if let Some(pool) = meta.rate_limits.rest_pools.first() {
                        if pool.is_weight {
                            item.weight_per_minute = Some(pool.max_budget * 60 / pool.window_seconds.max(1));
                        } else {
                            let rps = pool.max_budget / pool.window_seconds.max(1);
                            item.rate_limit_per_second = if rps > 0 { Some(rps) } else { None };
                            item.rate_limit_per_minute = Some(pool.max_budget * 60 / pool.window_seconds.max(1));
                        }
                    }

                    item.base_url = meta.base_url.to_string();
                    item.ws_url = meta.websocket_url.unwrap_or("").to_string();

                    item.rest_status = "active".to_string();
                    item.ws_status = if has_ws {
                        "available".to_string()
                    } else {
                        "n/a".to_string()
                    };

                    item.kline_batch_size = md_caps
                        .and_then(|md| md.max_kline_limit)
                        .unwrap_or(0);

                    item.supported_timeframes = md_caps
                        .map(|md| md.supported_intervals.iter().map(|s| s.to_string()).collect())
                        .unwrap_or_default();

                    if let Some((stats, ws_count)) = metrics_map.get(meta.id.as_str()) {
                        item.ws_active_count = *ws_count;
                        item.http_requests_total = stats.http_requests;
                        item.http_errors_total = stats.http_errors;
                        item.last_latency_ms = stats.last_latency_ms;
                        item.rate_used = stats.rate_used;
                        item.rate_max = stats.rate_max;
                        item.rate_groups = stats.rate_groups.clone();
                        item.rate_window_seconds = meta.rate_limits.rest_pools.first().map(|p| p.window_seconds).unwrap_or(60);
                        item.ws_ping_rtt_ms = stats.ws_ping_rtt_ms;
                    }

                    item.show_metrics = *self.sidebar_state.connector_metrics_visible
                        .get(meta.id.as_str())
                        .unwrap_or(&false);

                    if let Some(history) = self.sidebar_state.metrics_history.get(meta.id.as_str()) {
                        item.metrics_history = history.iter().cloned().collect();
                    }

                    self.sidebar_state.connector_items.push(item);
                }
            }

            // --- Alerts: copy from alert manager ---
            self.sidebar_state.alert_items = self.alert_manager.items().to_vec();

            // --- ObjectTree: mark items that have bound alerts ---
            {
                let alert_items = self.alert_manager.items();
                for tree_item in &mut self.sidebar_state.object_tree_items {
                    tree_item.has_alert = alert_items.iter().any(|a| match &a.source {
                        alerts::AlertSource::Drawing { primitive_id, .. } => {
                            *primitive_id == tree_item.id
                        }
                        alerts::AlertSource::Indicator { indicator_id, .. } => {
                            *indicator_id == tree_item.id
                        }
                        alerts::AlertSource::Signal { indicator_id, .. } => {
                            *indicator_id == tree_item.id
                        }
                        _ => false,
                    });
                }
            }

            self.sidebar_data_dirty = false;
        }

        // Keep symbol / compare search results filtered by current query so
        // render_to_scene (which takes &self) can read the pre-filtered list.
        if self.modal_state.current == OpenModal::SymbolSearch
            || self.modal_state.current == OpenModal::CompareSearch
        {
            let query = self.modal_state.search_query.clone();
            self.modal_state.symbol_search_results =
                Self::build_demo_symbol_results(&query, &self.sidebar_state.watchlist_manager, &self.exchange_symbols);
        }

        // --- Split-pane layout and viewport sync ---
        //
        // When the grid is split, call panel_grid.layout() so sub-chart rects
        // are computed, then sync each leaf window's viewport dimensions so that
        // bar_to_x, visible_range, and crosshair calculations are correct.
        // Also sync group primitives into each window's drawing_manager.
        let sidebar_w = self.sidebar_state.right_width();
        let window_rect = LayoutRect::new(0.0, 0.0, width, height);
        let panel_layout_pf = ChartPanelLayout::compute(&window_rect, &self.panel_app.toolbar_config);
        let content_rect_pf = {
            let mut r = panel_layout_pf.content_rect;
            r.width = (r.width - sidebar_w).max(0.0);
            r
        };

        if self.panel_app.panel_grid.is_split() {
            let split_rect = zengeld_chart::PanelRect {
                x: 0.0,
                y: 0.0,
                width: content_rect_pf.width as f32,
                height: content_rect_pf.height as f32,
            };
            self.panel_app.panel_grid.layout(split_rect);

            let leaf_rects: Vec<_> = self.panel_app.panel_grid.panel_rects()
                .iter()
                .map(|(&leaf_id, &sub_rect)| (leaf_id, sub_rect))
                .collect();

            // Sync viewport.chart_width/chart_height for all split windows.
            // Pre-compute target dimensions using immutable borrows (build_extended_layout_for_leaf
            // needs &self), then apply with mutable borrows in a second pass.
            let leaf_dims: Vec<(zengeld_chart::LeafId, f64, f64)> = leaf_rects.iter()
                .filter_map(|&(leaf_id, sub_rect)| {
                    let leaf_layout_rect = LayoutRect {
                        x: content_rect_pf.x + sub_rect.x as f64,
                        y: content_rect_pf.y + sub_rect.y as f64,
                        width: sub_rect.width as f64,
                        height: sub_rect.height as f64,
                    };
                    // build_extended_layout_for_leaf accounts for sub-panes so that
                    // chart_height reflects only the main chart area, matching the
                    // render path and eliminating the hit-test Y offset.
                    let extended = self.build_extended_layout_for_leaf(leaf_id, &leaf_layout_rect)?;
                    Some((leaf_id, extended.main_chart.chart.width, extended.main_chart.chart.height))
                })
                .collect();

            for (leaf_id, new_chart_w, new_chart_h) in leaf_dims {
                if let Some(window) = self.panel_app.panel_grid.window_for_leaf_mut(leaf_id) {
                    let old_w = window.viewport.chart_width;
                    // Skip bar_shift when window hasn't been snapped yet (still has
                    // placeholder chart_width from Viewport::default).  The deferred
                    // snap below will compute view_start with the real chart_width.
                    // chart_width/chart_height are always updated regardless.
                    if !window.needs_auto_scale_after_bars
                        && (old_w - new_chart_w).abs() > 0.5
                        && window.viewport.bar_spacing > 0.0
                        && old_w > 0.0
                    {
                        let bar_shift = (old_w - new_chart_w) / window.viewport.bar_spacing;
                        window.viewport.view_start += bar_shift;
                    }
                    window.viewport.chart_width = new_chart_w;
                    window.viewport.chart_height = new_chart_h;
                }
            }

            // Deferred viewport snap for split mode: now chart_width is real.
            {
                let mut snapped_windows: Vec<(ChartId, f64, f64)> = Vec::new();
                for (&chart_id, window) in self.panel_app.panel_grid.windows_mut().iter_mut() {
                    if window.needs_auto_scale_after_bars && !window.bars.is_empty() && window.viewport.chart_width > 0.0 {
                        window.needs_auto_scale_after_bars = false;
                        // Snap to end with standard margin,
                        // using CURRENT bar_spacing (restored from preset).
                        window.snap_to_end(zengeld_chart::DEFAULT_SNAP_MARGIN);
                        window.calc_auto_scale();
                        if let Some(mode) = window.restore_scale_mode.take() {
                            window.price_scale.scale_mode = mode;
                        }
                        snapped_windows.push((chart_id, window.viewport.view_start, window.viewport.bar_spacing));
                    }
                }
                // (diagnostic logging removed — snap-to-end confirmed working)
                for (chart_id, view_start, bar_spacing) in snapped_windows {
                    if let Some(leaf_id) = self.panel_app.panel_grid.leaf_for_chart_id(chart_id) {
                        self.propagate_viewport_to_sync_group(leaf_id, view_start, bar_spacing, None);
                    }
                }
            }

            // Sync group primitives into split windows, filtered to each
            // window's current symbol so stale drawings don't bleed through
            // after a symbol switch.
            let group_prim_sync: Vec<(zengeld_chart::ChartId, Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>>)> = {
                let mut syncs = Vec::new();
                for &(leaf_id, _) in &leaf_rects {
                    if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
                        let window_symbol = self.panel_app.panel_grid
                            .window_for_leaf(leaf_id)
                            .map(|w| w.symbol.clone())
                            .unwrap_or_default();
                        if let Some(group_id) = self.panel_app.panel_grid
                            .window_for_leaf(leaf_id)
                            .and_then(|w| w.group_id)
                        {
                            if let Some(group) = self.panel_app.tag_manager.group(group_id) {
                                if group.sync_flags.sync_drawings && group.members.len() > 1 {
                                    let cloned: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                                        group.primitives.iter()
                                            .filter(|p| {
                                                let sym = &p.data().symbol;
                                                sym.is_empty() || sym == &window_symbol
                                            })
                                            .map(|p| p.clone_box())
                                            .collect();
                                    syncs.push((chart_id, cloned));
                                }
                            }
                        }
                    }
                }
                syncs
            };
            for (chart_id, cloned_prims) in group_prim_sync {
                if let Some(window) = self.panel_app.panel_grid.windows_mut().get_mut(&chart_id) {
                    if !window.drawing_manager.is_dragging() {
                        window.drawing_manager.sync_from_group_primitives(&cloned_prims);
                    }
                }
            }

            // Sync indicator overlay visibility per leaf.
            for &(leaf_id, _) in &leaf_rects {
                let symbol = self.panel_app.panel_grid.window_for_leaf(leaf_id)
                    .map(|w| w.symbol.clone())
                    .unwrap_or_default();
                let has_compare = self.panel_app.panel_grid.window_for_leaf(leaf_id)
                    .map(|w| !w.compare_overlay.series.is_empty())
                    .unwrap_or(false);
                let has_indicators = if let Some(chart_id) = self.panel_app.panel_grid.chart_id_for_leaf(leaf_id) {
                    !self.indicator_manager.get_instances_for_symbol_in_window(&symbol, chart_id.0).is_empty()
                } else {
                    !self.indicator_manager.get_instances_for_symbol(&symbol).is_empty()
                };
                let state = self.panel_app.indicator_overlay_state_for_leaf_mut(leaf_id);
                state.visible = has_indicators || has_compare;
            }
        } else {
            // Single pane: sync group primitives.
            if let Some(active_window) = self.panel_app.panel_grid.active_window() {
                let group_id_opt = active_window.group_id;
                let is_dragging = active_window.drawing_manager.is_dragging();
                let chart_id_opt = self.panel_app.panel_grid.active_chart_id();
                if let (Some(group_id), Some(chart_id)) = (group_id_opt, chart_id_opt) {
                    if !is_dragging {
                        // Respect the sync_drawings flag — skip forward sync if disabled.
                        let (drawings_on, is_mono) = self.panel_app.tag_manager
                            .group(group_id)
                            .map(|g| (g.sync_flags.sync_drawings, g.members.len() <= 1))
                            .unwrap_or((true, false));
                        if drawings_on && !is_mono {
                            // Capture the window's current symbol so we can filter
                            // primitives — stale drawings from the previous symbol
                            // must not be re-injected by the forward sync.
                            let window_symbol = self.panel_app.panel_grid
                                .windows()
                                .get(&chart_id)
                                .map(|w| w.symbol.clone())
                                .unwrap_or_default();
                            let cloned: Vec<Box<dyn zengeld_chart::drawing::primitives_v2::Primitive>> =
                                self.panel_app.tag_manager
                                    .group(group_id)
                                    .map(|g| g.primitives.iter()
                                        .filter(|p| {
                                            let sym = &p.data().symbol;
                                            sym.is_empty() || sym == &window_symbol
                                        })
                                        .map(|p| p.clone_box())
                                        .collect())
                                    .unwrap_or_default();
                            if let Some(window) = self.panel_app.panel_grid.windows_mut().get_mut(&chart_id) {
                                window.drawing_manager.sync_from_group_primitives(&cloned);
                            }
                        }
                    }
                }
            }

            // Single pane: sync indicator overlay visibility.
            let (symbol, has_compare) = self.panel_app.panel_grid.active_window()
                .map(|w| (w.symbol.clone(), !w.compare_overlay.series.is_empty()))
                .unwrap_or_default();
            let has_indicators = if let Some(chart_id) = self.panel_app.panel_grid.active_chart_id() {
                !self.indicator_manager.get_instances_for_symbol_in_window(&symbol, chart_id.0).is_empty()
            } else {
                !self.indicator_manager.get_instances_for_symbol(&symbol).is_empty()
            };
            self.panel_app.indicator_overlay_state.visible = has_indicators || has_compare;
        }

        // Sync sub-pane pixel geometry into window.sub_panes so that
        // PanSubPane handlers and other &mut code see up-to-date values.
        self.sync_sub_pane_geometry();

        // Snapshot agent state for the sidebar renderer (agents panel).
        // Done here in prepare_frame (&mut self) because render_to_scene takes &self.
        // Iterate all registered agent leaves and snapshot each instance.
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
    }
}
