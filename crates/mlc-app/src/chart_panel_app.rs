//! `ChartPanelApp` — the chart panel autonomous application.
//!
//! Owns toolbar state, modal states, and the chart data via
//! `ChartPanelGrid`.  Profile + settings snapshots live on `BackendCore`'s
//! `UserProfileManager` (step 3.6c) — `load_user_state` reads them from disk
//! once at startup and returns them for the caller to write into `core`.
//! Moved here from `mlc-app-layout::panel_app` because it is squarely
//! app-level state.  The pure rendering helpers (`ChartPanelLayout`,
//! `compute_chart_internal_layout`, `chart_toolbar_render_toolbars`,
//! `render_toolbars_with_theme`) stay in `mlc-app-layout::panel_app`.

use mlc_core::engine::render::RenderContext;

use crate::panel_grid::ChartPanelGrid;
use crate::chart_container::ChartContainer;
use mlc_core::state::Timeframe;
use mlc_core::modal::ChartOpenModal;
use mlc_core::state::selected_config::SelectedPrimitiveConfig;
use mlc_backend::template::TemplateManager;
use mlc_core::indicator_source::IndicatorSource;
use mlc_core::preset::preset::ChartPreset;

use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::chart_render::{ScaleTheme, ChartTheme};
use mlc_app_layout::ui::modal_settings::{
    PrimitiveSettingsState, IndicatorSettingsState, ChartSettingsState,
    IndicatorOverlayState, OverlaySettingsState, TagsTabsState,
    PresetNameInputState, ChartBrowserState, AlertSettingsState,
    CompareSettingsState,
};
use mlc_app_layout::ui::context_menu::ContextMenuState;
use mlc_app_layout::layout::render_frame::{ChartModalRenderResult, ChartModalLayout};
use mlc_app_layout::layout::modals::{
    render_primitive_settings_modal,
    render_primitive_color_picker_popup,
    render_indicator_settings_modal,
    render_indicator_color_picker_popup,
    render_chart_settings_color_picker_popup,
    render_settings_modal, ChartSettingsData,
    render_panel_color_tag_picker_popup,
    render_tags_tabs_modal,
};
use mlc_app_layout::panel_app::{
    ChartPanelLayout,
    ChartToolbarState, ToolbarConfig, ChartToolbarRenderResult,
    chart_toolbar_render_toolbars,
};

use crate::user_manager::load_user_state_from_disk;

pub struct ChartPanelApp {
    pub toolbar_state: ChartToolbarState,
    title: String,
    /// Per-toolbar visibility and definition configuration for this panel.
    pub toolbar_config: ToolbarConfig,
    /// Which chart-local modal is currently open.
    pub active_modal: ChartOpenModal,
    /// Chart data and split layout manager.
    ///
    /// Holds all ChartContainer instances (one per split leaf) and the
    /// DockingManager that computes each sub-chart's rectangle.
    pub panel_grid: ChartPanelGrid,
    /// Chart's local theme manager — subordinate to terminal's ThemeManager.
    ///
    /// The terminal calls `apply_terminal_theme` to command this manager.
    /// All chart rendering (splits, toolbars, scales, candles) derives
    /// colors from this manager.
    pub theme_manager: mlc_core::theme::ThemeManager,

    // Modal states — owned per chart panel so each chart has independent modal UI.
    /// Primitive (drawing) settings modal state.
    pub primitive_settings_state: PrimitiveSettingsState,
    /// Chart settings modal state.
    pub chart_settings_state: ChartSettingsState,
    /// Indicator settings modal state.
    pub indicator_settings_state: IndicatorSettingsState,
    /// Indicator overlay state (used in single-window mode).
    pub indicator_overlay_state: IndicatorOverlayState,
    /// Per-leaf indicator overlay states (used in split mode).
    pub indicator_overlay_states: std::collections::HashMap<uzor::panels::LeafId, IndicatorOverlayState>,
    /// Context menu state (right-click menu).
    pub context_menu_state: ContextMenuState,
    /// Overlay (leaf) settings modal state.
    pub overlay_settings_state: OverlaySettingsState,
    /// Tags & Tabs modal state (unified panel-tree + sync-group manager).
    pub tags_tabs_state: TagsTabsState,
    /// Alert settings modal state.
    pub alert_settings_state: AlertSettingsState,
    /// Compare settings modal state.
    pub compare_settings_state: CompareSettingsState,
    /// User settings modal state.
    pub user_settings_state: mlc_app_layout::ui::modal_settings::UserSettingsState,
    /// Cloud sign-in / account overlay state (cloud builds only).
    #[cfg(feature = "cloud")]
    pub cloud_login_state: mlc_app_layout::ui::modal_settings::CloudLoginState,
    /// EWA Engine Settings modal state (owner ask 2026-08-14).
    pub ewa_engine_settings_state: mlc_app_layout::ui::modal_settings::EwaEngineSettingsState,
    /// Price Action Engine Settings modal state — the non-Elliott sibling
    /// of the field above, one modal for the three markup engines
    /// (`docs/mlc/plans/markup-engine-parity-2026-08-25.md` S6).
    pub markup_engine_settings_state: mlc_app_layout::ui::modal_settings::MarkupEngineSettingsState,
    /// Clock popup state (timezone selector + 24h toggle).
    pub clock_popup_state: mlc_app_layout::ui::ClockPopupState,

    // Panel color tag picker — opened when the user clicks the colored square
    // on an overlay tab header.
    /// Color picker state for the panel color tag picker.
    pub panel_color_picker: mlc_app_layout::ui::color_picker_state::ColorPickerState,
    /// Which leaf's color tag is being edited.
    pub panel_color_picker_leaf: Option<uzor::panels::LeafId>,
    /// Per-leaf color tags (RGBA, 0.0–1.0 each channel).
    pub leaf_color_tags: std::collections::HashMap<uzor::panels::LeafId, [f32; 4]>,

    // Sync color grid popup — lightweight preset grid that replaces the full
    // L1/L2 picker for panel color-tag assignment.
    /// State for the sync color grid popup.
    pub sync_color_grid: mlc_app_layout::ui::sync_color_grid::SyncColorGridState,
    /// Draw result from the last rendered sync color grid frame (used for hit testing).
    pub sync_color_grid_draw: mlc_app_layout::ui::sync_color_grid::SyncColorGridDrawResult,

    /// ID of the currently active preset. Defaults to `"__default__"`.
    /// Updated when a preset is loaded or saved-as.
    pub active_preset_id: String,
    /// Ordered list of preset IDs that are currently open as tabs.
    /// A preset can exist in core.presets() without being open.
    pub open_tabs: Vec<String>,
    /// Preset name input modal state (Save As / Rename).
    pub preset_name_input: PresetNameInputState,
    /// Chart browser modal state (Open Chart).
    pub chart_browser: ChartBrowserState,
    /// Whether autosave is enabled for presets.
    pub autosave_enabled: bool,
}

impl ChartPanelApp {
    pub fn new(title: &str) -> Self {
        let initial_window = ChartContainer::new(title, Timeframe::h1());
        Self {
            toolbar_state: ChartToolbarState::new(),
            title: title.to_string(),
            toolbar_config: ToolbarConfig::standalone(),
            active_modal: ChartOpenModal::None,
            panel_grid: ChartPanelGrid::new(initial_window),
            theme_manager: mlc_core::theme::ThemeManager::new(),
            primitive_settings_state: PrimitiveSettingsState::new(),
            chart_settings_state: ChartSettingsState::new(),
            indicator_settings_state: IndicatorSettingsState::new(),
            indicator_overlay_state: IndicatorOverlayState::default(),
            indicator_overlay_states: std::collections::HashMap::new(),
            context_menu_state: ContextMenuState::new(),
            overlay_settings_state: OverlaySettingsState::new(),
            tags_tabs_state: TagsTabsState::default(),
            alert_settings_state: AlertSettingsState::new(),
            compare_settings_state: CompareSettingsState::new(),
            user_settings_state: mlc_app_layout::ui::modal_settings::UserSettingsState::default(),
            #[cfg(feature = "cloud")]
            cloud_login_state: mlc_app_layout::ui::modal_settings::CloudLoginState::default(),
            ewa_engine_settings_state: mlc_app_layout::ui::modal_settings::EwaEngineSettingsState::default(),
            markup_engine_settings_state: mlc_app_layout::ui::modal_settings::MarkupEngineSettingsState::default(),
            clock_popup_state: mlc_app_layout::ui::ClockPopupState::default(),
            panel_color_picker: mlc_app_layout::ui::color_picker_state::ColorPickerState::new(),
            panel_color_picker_leaf: None,
            leaf_color_tags: std::collections::HashMap::new(),
            sync_color_grid: mlc_app_layout::ui::sync_color_grid::SyncColorGridState::new(),
            sync_color_grid_draw: mlc_app_layout::ui::sync_color_grid::SyncColorGridDrawResult::default(),
            active_preset_id: "__default__".to_string(),
            open_tabs: Vec::new(),
            preset_name_input: PresetNameInputState::default(),
            chart_browser: ChartBrowserState::default(),
            autosave_enabled: true,
        }
    }

    /// Load all user state from disk.
    ///
    /// Call once at startup after construction. Populates `active_preset_id`
    /// and `open_tabs` on self. Returns the loaded presets, template manager,
    /// profile, settings snapshots, and vault key so the caller can write
    /// them into `BackendCore` (presets/templates/user profile no longer
    /// live on `ChartPanelApp` — step 3.6c).
    ///
    /// All errors are logged but non-fatal: missing files produce defaults.
    pub fn load_user_state(
        &mut self,
    ) -> (
        std::collections::HashMap<String, mlc_core::preset::preset::ChartPreset>,
        TemplateManager,
        mlc_core::UserProfile,
        mlc_core::user_profile::SettingsSnapshots,
        Option<mlc_vault::vault::VaultKey>,
    ) {
        let (presets, template_manager, mut profile, snapshots, vault_key) =
            load_user_state_from_disk(None);

        if !profile.ui.active_preset_id.is_empty() {
            self.active_preset_id = profile.ui.active_preset_id.clone();
        }

        // Restore open_tabs and filter out any stale IDs (presets no longer on disk).
        self.open_tabs = profile.ui.open_tabs.clone();
        self.open_tabs.retain(|id| presets.contains_key(id));

        // Migration: old data has no open_tabs — open only the active preset.
        if self.open_tabs.is_empty() && !presets.is_empty() {
            if !self.active_preset_id.is_empty()
                && presets.contains_key(&self.active_preset_id)
            {
                self.open_tabs = vec![self.active_preset_id.clone()];
            } else {
                let mut all: Vec<_> = presets.values().collect();
                all.sort_by_key(|p| std::cmp::Reverse(p.created_at));
                if let Some(newest) = all.first() {
                    self.open_tabs = vec![newest.id.clone()];
                }
            }
        }

        // Record this launch.
        profile.record_launch(env!("CARGO_PKG_VERSION"));

        // Save profile immediately to persist the updated launch record.
        if let Err(e) = mlc_core::user_profile::storage::save_profile(&profile, vault_key.as_ref()) {
            eprintln!("[ChartPanelApp] failed to save profile: {}", e);
        }

        eprintln!(
            "[ChartPanelApp] user state loaded: {} presets, {} prim-templates, {} ind-templates",
            presets.len(),
            template_manager.primitive_templates.len(),
            template_manager.indicator_templates.len(),
        );

        (presets, template_manager, profile, snapshots, vault_key)
    }

    /// Return a mutable reference to the per-leaf indicator overlay state.
    ///
    /// Creates a default state entry if one does not exist yet for the given leaf.
    pub fn indicator_overlay_state_for_leaf_mut(
        &mut self,
        leaf_id: uzor::panels::LeafId,
    ) -> &mut IndicatorOverlayState {
        self.indicator_overlay_states
            .entry(leaf_id)
            .or_default()
    }

    /// Return a shared reference to the per-leaf indicator overlay state.
    ///
    /// Returns a reference to a default state if one does not exist for the leaf.
    /// The state is lazily inserted on first mutable access; if it has not been
    /// created yet this will fall back to the single-window state as a sentinel.
    pub fn indicator_overlay_state_for_leaf(
        &self,
        leaf_id: uzor::panels::LeafId,
    ) -> Option<&IndicatorOverlayState> {
        self.indicator_overlay_states.get(&leaf_id)
    }

    /// Called by terminal when global theme changes.
    ///
    /// Updates this chart's internal ThemeManager to match the terminal preset,
    /// marking it dirty so the next render picks up the new colors.
    pub fn apply_terminal_theme(&mut self, preset_name: &str) {
        self.theme_manager.set_preset(preset_name);
    }

    /// Build a `ChartTheme` render struct from the current theme_manager state.
    ///
    /// Used by `render_chart_content` and other internal rendering paths.
    pub fn chart_theme_for_render(&self) -> ChartTheme {
        let rt = self.theme_manager.current();
        ChartTheme {
            background: rt.chart.background.clone(),
            grid_line: rt.chart.grid_line.clone(),
            text: rt.colors.text_primary.clone(),
            candle_up: rt.series.candle_up_body.clone(),
            candle_down: rt.series.candle_down_body.clone(),
            wick_up: rt.series.candle_up_wick.clone(),
            wick_down: rt.series.candle_down_wick.clone(),
            candle_up_border: rt.series.candle_up_border.clone(),
            candle_down_border: rt.series.candle_down_border.clone(),
            legend_value_up: rt.chart.legend_value_up.clone(),
            legend_value_down: rt.chart.legend_value_down.clone(),
            crosshair: rt.chart.crosshair_line.clone(),
            scale_bg: rt.chart.scale_bg.clone(),
            scale_border: rt.chart.scale_border.clone(),
            sub_pane_bg: rt.chart.background.clone(),
            footprint_buy: rt.series.footprint_buy.clone(),
            footprint_sell: rt.series.footprint_sell.clone(),
            renko_up: rt.series.renko_up.clone(),
            renko_down: rt.series.renko_down.clone(),
            pnf_x: rt.series.pnf_x.clone(),
            pnf_o: rt.series.pnf_o.clone(),
            tlb_up: rt.series.tlb_up.clone(),
            tlb_down: rt.series.tlb_down.clone(),
            kagi_yang: rt.series.kagi_yang.clone(),
            kagi_yin: rt.series.kagi_yin.clone(),
            hollow_up: rt.series.hollow_up.clone(),
            hollow_down: rt.series.hollow_down.clone(),
            line: rt.series.line_color.clone(),
            step_line: rt.series.step_line_color.clone(),
            line_markers: rt.series.line_markers_color.clone(),
            area_line: rt.series.area_line.clone(),
            area_top: rt.series.area_top.clone(),
            area_bottom: rt.series.area_bottom.clone(),
            baseline_top_line: rt.series.baseline_top_line.clone(),
            baseline_top_fill: rt.series.baseline_top_fill.clone(),
            baseline_bottom_line: rt.series.baseline_bottom_line.clone(),
            baseline_bottom_fill: rt.series.baseline_bottom_fill.clone(),
            bar_up: rt.series.bar_up.clone(),
            bar_down: rt.series.bar_down.clone(),
            candle_style: rt.panel_style.candle_style,
            candle_glow: rt.panel_style.glow,
            heat_bid: rt.series.heat_bid.clone(),
            heat_bid_hot: rt.series.heat_bid_hot.clone(),
            heat_ask: rt.series.heat_ask.clone(),
            heat_ask_hot: rt.series.heat_ask_hot.clone(),
            volume_up: rt.series.volume_up.clone(),
            volume_down: rt.series.volume_down.clone(),
        }
    }

    /// Build a `ScaleTheme` render struct from the current theme_manager state.
    ///
    /// Maps RuntimeTheme color fields to every ScaleTheme slot so that price
    /// and time scales, the scale corner, and crosshair labels all respect the
    /// active theme preset.
    pub fn scale_theme_for_render(&self) -> ScaleTheme {
        let rt = self.theme_manager.current();
        ScaleTheme {
            scale_bg:                   rt.colors.toolbar_bg.clone(),
            scale_border:               rt.chart.scale_border.clone(),
            scale_text:                 rt.chart.scale_text.clone(),
            scale_text_medium:          rt.chart.scale_text_muted.clone(),
            scale_text_muted:           rt.chart.scale_text_muted.clone(),
            crosshair_label_bg:         rt.chart.crosshair_label_bg.clone(),
            crosshair_label_bg_styled:  rt.crosshair_label_bg_styled(),
            crosshair_label_text:       rt.chart.crosshair_label_text.clone(),
        }
    }

    /// Build a `FrameTheme` render struct from the current theme_manager state.
    ///
    /// Maps RuntimeTheme color fields to every FrameTheme slot so that chart
    /// frame borders and modal backgrounds all respect the active theme preset.
    pub fn frame_theme_for_render(&self) -> FrameTheme {
        let rt = self.theme_manager.current();
        FrameTheme {
            toolbar_bg:           rt.colors.toolbar_bg.clone(),
            toolbar_border:       rt.colors.ui_border.clone(),
            chart_border:         rt.chart.chart_border.clone(),
            frame_border:         rt.chart.frame_border.clone(),
            show_scale_separators: true,
        }
    }

    /// Build a `DropdownTheme` from the current theme_manager state.
    ///
    /// Maps `RuntimeTheme` color fields to every `DropdownTheme` slot so that
    /// dropdown menus, submenus and context menus all respect the active theme.
    pub fn dropdown_theme_for_render(&self) -> mlc_app_layout::ui::dropdown::DropdownTheme {
        let rt = self.theme_manager.current();
        mlc_app_layout::ui::dropdown::DropdownTheme {
            background:            rt.colors.dropdown_bg.clone(),
            border:                rt.colors.border.clone(),
            shadow:                "rgba(0,0,0,0.5)".to_string(),
            item_text:             rt.colors.text_primary.clone(),
            item_text_hover:       rt.colors.text_primary.clone(),
            item_text_disabled:    rt.colors.text_muted.clone(),
            item_bg_hover:         rt.colors.button_bg_hover.clone(),
            item_danger:           rt.colors.danger.clone(),
            item_danger_bg_hover:  "rgba(0,0,0,0.15)".to_string(), // always semi-transparent overlay
            header_text:           rt.colors.text_primary.clone(),
            header_border:         rt.colors.border.clone(),
            separator:             rt.colors.divider.clone(),
            shortcut_text:         rt.colors.text_muted.clone(),
            accent:                rt.colors.accent.clone(),
        }
    }

    /// Build a `ToolbarTheme` (toolbar_render) from the current theme_manager state.
    ///
    /// Used by `render_toolbars_with_theme` so chart toolbars match the active theme.
    pub fn toolbar_theme_for_render(&self) -> mlc_app_layout::ui::toolbar_theme::ToolbarTheme {
        let rt = self.theme_manager.current();
        mlc_app_layout::ui::toolbar_theme::ToolbarTheme {
            background: rt.colors.toolbar_bg.clone(),
            dropdown_bg: rt.colors.dropdown_bg.clone(),
            separator: rt.colors.toolbar_divider.clone(),
            item_bg_hover: rt.colors.button_bg_hover.clone(),
            item_bg_active: rt.colors.button_bg_active.clone(),
            button_bg:       rt.colors.button_bg.clone(),
            button_bg_hover: rt.colors.button_bg_hover.clone(),
            item_text: rt.colors.text_primary.clone(),
            item_text_muted: rt.colors.text_secondary.clone(),
            item_text_hidden: mlc_core::apply_opacity(&rt.colors.text_primary, 0.5),
            item_text_hover: rt.colors.text_primary.clone(),
            item_text_active: rt.colors.text_active.clone(),
            accent: rt.colors.accent.clone(),
            accent_hover: rt.colors.accent_hover.clone(),
            success: rt.colors.success.clone(),
            danger: rt.colors.danger.clone(),
            warning: rt.colors.warning.clone(),
            sidebar_style: rt.style_params.toolbar_sidebar_style,
            terminal_bg: rt.colors.toolbar_bg.clone(),
            pane_header_focused: rt.colors.accent.clone(),
            pane_header_hover: rt.colors.button_bg_hover.clone(),
            pane_header_idle: rt.colors.toolbar_bg.clone(),
            bubble_user_bg: rt.colors.bubble_user_bg.clone(),
            bubble_user_border: rt.colors.accent.clone(),
            bubble_user_text: rt.colors.text_primary.clone(),
            bubble_tool_bg: rt.colors.toolbar_bg.clone(),
            bubble_tool_text: rt.colors.text_secondary.clone(),
            selection: rt.colors.text_selection.clone(),
            danger_hover_bg: mlc_core::apply_opacity(&rt.colors.danger, 0.25),
        }
    }

    /// Render chart toolbars using this panel's ThemeManager for colors.
    ///
    /// Delegates to `toolbar_state.render_toolbars` with the theme built from
    /// the chart's own ThemeManager (which is commanded by the terminal).
    /// Uses `self.toolbar_config` to determine which toolbars to render.
    pub fn render_toolbars_with_theme<'a>(
        &self,
        ctx: &mut dyn RenderContext,
        layout: &ChartPanelLayout,
        selected_primitive: Option<&SelectedPrimitiveConfig>,
        clock_time: Option<&str>,
        split_without_group: bool,
        active_symbol: Option<&str>,
        active_timeframe: Option<&str>,
        sidebar_w: f64,
        active_sync_flags: Option<&mlc_core::SyncFlags>,
        active_is_mono_group: bool,
        presets: impl Iterator<Item = &'a mlc_core::preset::preset::ChartPreset>,
        allowed_timeframes: Option<&std::collections::HashSet<&'static str>>,
        active_chart_type: Option<&str>,
        active_chart_type_param: Option<f64>,
        chart_type_data_modes: Option<&std::collections::HashMap<&'static str, mlc_core::chart_type::DataMode>>,
        timeframe_native_map: Option<&std::collections::HashMap<String, bool>>,
        footprint_live_only: bool,
        granularity_favorites: &mlc_core::user_profile::GranularityFavorites,
        granularity_edit_render: Option<&mlc_core::chart_type::GranularityEditRender>,
        pin_granularity_favorites: bool,
        granularity_drag_render: Option<&mlc_core::chart_type::GranularityDragRender>,
        split_symbol_compare: bool,
        symbol_quick_search_render: Option<&mlc_core::state::modal_state::SymbolQuickSearchRender>,
        symbol_quick_search: bool,
        active_exchange: Option<&str>,
        active_account_type: Option<&str>,
        suppress_chart_controls: bool,
    ) -> ChartToolbarRenderResult {
        let toolbar_theme = self.toolbar_theme_for_render();
        let dropdown_theme = self.dropdown_theme_for_render();
        let is_expanded = self.panel_grid.is_expanded();
        let preset_summaries: Vec<mlc_core::PresetSummary> = presets
            .map(preset_to_summary)
            .collect();
        chart_toolbar_render_toolbars(&self.toolbar_state, ctx, layout, &self.toolbar_config, selected_primitive, Some(&toolbar_theme), Some(&dropdown_theme), clock_time, &preset_summaries, &self.active_preset_id, self.autosave_enabled, active_sync_flags, is_expanded, split_without_group, active_is_mono_group, active_symbol, active_timeframe, sidebar_w, allowed_timeframes, active_chart_type, active_chart_type_param, chart_type_data_modes, timeframe_native_map, footprint_live_only, granularity_favorites, granularity_edit_render, pin_granularity_favorites, granularity_drag_render, split_symbol_compare, symbol_quick_search_render, symbol_quick_search, active_exchange, active_account_type, suppress_chart_controls, false)
    }

    /// Render the trading container's EMBEDDED header toolbar (owner spec
    /// 2026-07-25) — the same composer as the main top toolbar, driven by a
    /// SEPARATE `ChartToolbarState` and the `trading_header_toolbar()` def,
    /// with container-derived actives. Chart-only knobs are neutral: no
    /// presets, no sync flags, no chart type, favorites/quicksearch shared
    /// with the main toolbar so both systems behave identically.
    #[allow(clippy::too_many_arguments)]
    pub fn render_trading_header_toolbar(
        &self,
        tb: &ChartToolbarState,
        ctx: &mut dyn RenderContext,
        header_rect: &mlc_app_layout::layout::LayoutRect,
        active_symbol: &str,
        active_timeframe: &str,
        active_exchange: &str,
        active_account_type: &str,
        granularity_favorites: &mlc_core::user_profile::GranularityFavorites,
        granularity_edit_render: Option<&mlc_core::chart_type::GranularityEditRender>,
        pin_granularity_favorites: bool,
        granularity_drag_render: Option<&mlc_core::chart_type::GranularityDragRender>,
        symbol_quick_search_render: Option<&mlc_core::state::modal_state::SymbolQuickSearchRender>,
        symbol_quick_search: bool,
        timeframe_native_map: Option<&std::collections::HashMap<String, bool>>,
        center_label: &str,
        chart_mode: bool,
        free_mode: bool,
        active_chart_type: Option<&str>,
    ) -> ChartToolbarRenderResult {
        let toolbar_theme = self.toolbar_theme_for_render();
        let dropdown_theme = self.dropdown_theme_for_render();
        let cfg = ToolbarConfig::custom()
            .with_top(Some(mlc_core::toolbar::trading_header_toolbar(center_label, chart_mode, free_mode)));
        let layout = ChartPanelLayout::compute(header_rect, &cfg);
        chart_toolbar_render_toolbars(
            tb, ctx, &layout, &cfg,
            None,                      // selected_primitive
            Some(&toolbar_theme),
            Some(&dropdown_theme),
            None,                      // clock_time
            &[],                       // presets
            "",                        // active_preset_id
            false,                     // autosave_enabled
            None,                      // sync_flags
            false,                     // is_expanded
            false,                     // split_without_group
            false,                     // is_mono_group
            Some(active_symbol),
            Some(active_timeframe),
            0.0,                       // sidebar_w (modal math n/a here)
            None,                      // allowed_timeframes (all)
            active_chart_type,         // embedded chart's type — drives the
                                       // chart_type_selector button icon
            None,                      // active_chart_type_param
            None,                      // chart_type_data_modes
            timeframe_native_map,
            false,                     // footprint_live_only
            granularity_favorites,
            granularity_edit_render,
            pin_granularity_favorites,
            granularity_drag_render,
            false,                     // split_symbol_compare
            symbol_quick_search_render,
            symbol_quick_search,
            Some(active_exchange),
            Some(active_account_type),
            false,                     // suppress_chart_controls
            true,                      // quicksearch_minimal (results only)
        )
    }

    /// Render a data-section sidebar header toolbar (global-docking Stage B,
    /// owner spec 2026-07-30) — the Trading-Container-style header for the
    /// DOM / Trade Tape / L2 / Big Trades sections: symbol quick-search
    /// system + color tag (+ DOM thematic chips). Same composer, same
    /// `tch:` instance/state as the TC header — only one right section is
    /// visible at a time, so the instance is shared.
    #[allow(clippy::too_many_arguments)]
    pub fn render_section_header_toolbar(
        &self,
        tb: &ChartToolbarState,
        ctx: &mut dyn RenderContext,
        header_rect: &mlc_app_layout::layout::LayoutRect,
        active_symbol: &str,
        active_exchange: &str,
        active_account_type: &str,
        symbol_quick_search_render: Option<&mlc_core::state::modal_state::SymbolQuickSearchRender>,
        symbol_quick_search: bool,
        top_def: uzor::panel_api::PanelToolbarDef,
    ) -> ChartToolbarRenderResult {
        let toolbar_theme = self.toolbar_theme_for_render();
        let dropdown_theme = self.dropdown_theme_for_render();
        let cfg = ToolbarConfig::custom().with_top(Some(top_def));
        let layout = ChartPanelLayout::compute(header_rect, &cfg);
        let neutral_favs = mlc_core::user_profile::GranularityFavorites::default();
        chart_toolbar_render_toolbars(
            tb, ctx, &layout, &cfg,
            None,                      // selected_primitive
            Some(&toolbar_theme),
            Some(&dropdown_theme),
            None,                      // clock_time
            &[],                       // presets
            "",                        // active_preset_id
            false,                     // autosave_enabled
            None,                      // sync_flags
            false,                     // is_expanded
            false,                     // split_without_group
            false,                     // is_mono_group
            Some(active_symbol),
            None,                      // active_timeframe (no TF system here)
            0.0,                       // sidebar_w
            None,                      // allowed_timeframes
            None,                      // active_chart_type
            None,                      // active_chart_type_param
            None,                      // chart_type_data_modes
            None,                      // timeframe_native_map
            false,                     // footprint_live_only
            &neutral_favs,
            None,                      // granularity_edit_render
            false,                     // pin_granularity_favorites
            None,                      // granularity_drag_render
            false,                     // split_symbol_compare
            symbol_quick_search_render,
            symbol_quick_search,
            Some(active_exchange),
            Some(active_account_type),
            false,                     // suppress_chart_controls
            true,                      // quicksearch_minimal (results only)
        )
    }

    /// Render all chart-owned modals in a single call.
    ///
    /// This method encapsulates the rendering of:
    /// - Primitive settings modal (if open)
    /// - Primitive color picker popup (if open)
    /// - Indicator settings modal (if open, requires `indicator_source`)
    /// - Indicator color picker popup (if open)
    /// - Chart settings color picker popup (if open)
    ///
    /// The context menu, panel tree manager, and panel color tag picker stay in
    /// core because they are not chart-specific.
    ///
    /// # Parameters
    /// - `ctx` — render context
    /// - `modal_layout` — positional data for modal placement (computed from frame layout by core)
    /// - `drawing_manager` — active window's drawing manager (for primitive settings)
    /// - `indicator_source` — indicator data source (for indicator settings modal)
    /// - `chart_settings_data` — chart settings data for the chart settings modal (if open)
    /// - `theme_manager` — theme manager (for appearance tab)
    /// - `frame_theme` — frame theme for modal backgrounds
    /// - `toolbar_theme` — toolbar theme for color pickers
    /// - `toolbar_state` — toolbar state (passed through to primitive settings)
    /// - `current_time_ms` — current timestamp for cursor blink
    /// - `input_coordinator` — uzor input coordinator
    pub fn render_modals<'a>(
        &self,
        ctx: &mut dyn mlc_core::render::RenderContext,
        modal_layout: &ChartModalLayout,
        drawing_manager: Option<&dyn mlc_core::drawing::DrawingRenderView>,
        indicator_source: Option<&dyn IndicatorSource>,
        chart_settings_data: Option<&ChartSettingsData>,
        theme_manager: Option<&mlc_core::theme::ThemeManager>,
        frame_theme: &mlc_app_layout::layout::FrameTheme,
        toolbar_theme: &mlc_app_layout::ui::toolbar_theme::ToolbarTheme,
        toolbar_state: &mlc_app_layout::layout::toolbar_state::ToolbarState,
        current_time_ms: u64,
        input_coordinator: &mut uzor::input::InputCoordinator,
        groups: &[mlc_core::TagView],
        selected_group: Option<&mlc_core::TagView>,
        template_manager: &mlc_backend::template::Templates<'_>,
        presets: impl Iterator<Item = &'a mlc_core::preset::preset::ChartPreset>,
    ) -> ChartModalRenderResult {
        let mut result = ChartModalRenderResult::default();

        // Primitive settings modal
        if self.primitive_settings_state.is_open() {
            if let Some(dm) = drawing_manager {
                result.primitive_settings = Some(render_primitive_settings_modal(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    modal_layout.prim_modal_y,
                    &self.primitive_settings_state,
                    dm.primitives(),
                    frame_theme,
                    toolbar_theme,
                    toolbar_state,
                    current_time_ms,
                    input_coordinator,
                    template_manager.primitive_templates(),
                    template_manager.primitive_template_order(),
                ));
            }
        }

        // Compare settings modal
        if self.compare_settings_state.is_open() {
            result.compare_settings = Some(mlc_app_layout::layout::modals::compare_settings::render_compare_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.compare_settings_state,
                frame_theme,
                toolbar_theme,
                input_coordinator,
                template_manager.compare_templates(),
            ));
        }

        // Alert settings modal
        if self.alert_settings_state.is_open() {
            result.alert_settings = Some(mlc_app_layout::layout::modals::alert_settings::render_alert_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.alert_settings_state,
                frame_theme,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Indicator settings modal
        if self.indicator_settings_state.is_open() {
            if let Some(indicator_id) = self.indicator_settings_state.indicator_id {
                if let Some(source) = indicator_source {
                    if let Some(settings_data) = source.get_settings_data(indicator_id) {
                        result.indicator_settings = Some(render_indicator_settings_modal(
                            ctx,
                            modal_layout.ind_screen_w,
                            modal_layout.ind_screen_h,
                            modal_layout.chart_x,
                            modal_layout.chart_y,
                            &self.indicator_settings_state,
                            &settings_data.name,
                            &settings_data.params,
                            &settings_data.outputs,
                            settings_data.display_info.as_ref(),
                            settings_data.signals_enabled,
                            &settings_data.signal_display,
                            settings_data.timeframe_visibility.as_ref(),
                            current_time_ms,
                            frame_theme,
                            toolbar_theme,
                            input_coordinator,
                            template_manager.indicator_templates(),
                        ));
                    }
                }
            }
        }

        // Chart settings modal
        if self.chart_settings_state.is_open {
            if let (Some(data), Some(tm)) = (chart_settings_data, theme_manager) {
                result.chart_settings = Some(render_settings_modal(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    modal_layout.chart_x,
                    modal_layout.chart_y,
                    &self.chart_settings_state,
                    data,
                    tm,
                    frame_theme,
                    toolbar_theme,
                    current_time_ms,
                    input_coordinator,
                    template_manager.chart_templates(),
                ));
            }
        }

        // Tags & Tabs modal
        if self.tags_tabs_state.is_open {
            result.tags_tabs = Some(render_tags_tabs_modal(
                ctx,
                &self.tags_tabs_state,
                &self.overlay_settings_state,
                frame_theme,
                toolbar_theme,
                self.panel_grid.docking(),
                self.panel_grid.is_expanded(),
                groups,
                selected_group,
                &self.leaf_color_tags,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.theme_manager,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                input_coordinator,
            ));
        }

        // Preset name input modal — skip CreateIndicatorSet mode here;
        // it will be rendered AFTER the search overlay in lib.rs so it
        // appears visually on top.
        if self.preset_name_input.is_open
            && self.preset_name_input.mode != mlc_app_layout::ui::modal_settings::PresetNameInputMode::CreateIndicatorSet
        {
            use mlc_app_layout::layout::modals::preset_name_input::render_preset_name_input;
            result.preset_name_input = Some(render_preset_name_input(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.preset_name_input,
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // Template name overlay modal — primitive settings
        if self.primitive_settings_state.save_template_mode {
            use mlc_app_layout::layout::modals::template_name_modal::render_template_name_modal;
            result.prim_template_name = Some(render_template_name_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                "prim_tmpl_name",
                "prim_tmpl",
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // Template name overlay modal — indicator settings
        if self.indicator_settings_state.save_template_mode {
            use mlc_app_layout::layout::modals::template_name_modal::render_template_name_modal;
            result.ind_template_name = Some(render_template_name_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                "ind_tmpl_name",
                "ind_tmpl",
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // Template name overlay modal — compare settings
        if self.compare_settings_state.save_template_mode {
            use mlc_app_layout::layout::modals::template_name_modal::render_template_name_modal;
            result.cmp_template_name = Some(render_template_name_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                "cmp_tmpl_name",
                "cmp_tmpl",
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // Template name overlay modal — chart settings
        if self.chart_settings_state.save_template_mode {
            use mlc_app_layout::layout::modals::template_name_modal::render_template_name_modal;
            result.chart_template_name = Some(render_template_name_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                "chart_tmpl_name",
                "chart_tmpl",
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // Chart browser modal
        if self.chart_browser.is_open {
            use mlc_app_layout::layout::modals::chart_browser::render_chart_browser;
            let preset_summaries: Vec<mlc_core::PresetSummary> = presets
                .map(preset_to_summary)
                .collect();
            result.chart_browser = Some(render_chart_browser(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.chart_browser,
                &preset_summaries,
                &self.active_preset_id,
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // User settings modal
        if self.user_settings_state.is_open {
            use mlc_app_layout::layout::modals::user_settings::render_user_settings_modal;
            result.user_settings = Some(render_user_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.user_settings_state,
                frame_theme,
                toolbar_theme,
                current_time_ms,
                input_coordinator,
            ));
        }

        // Cloud sign-in / account overlay (cloud builds only).
        #[cfg(feature = "cloud")]
        if self.cloud_login_state.is_open {
            mlc_app_layout::layout::modals::render_cloud_login_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.cloud_login_state,
                toolbar_theme,
                input_coordinator,
            );
        }

        // Price Action Engine Settings modal
        if self.markup_engine_settings_state.is_open {
            use mlc_app_layout::layout::modals::markup_engine_settings::render_markup_engine_settings_modal;
            result.markup_engine_settings = Some(render_markup_engine_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.markup_engine_settings_state,
                frame_theme,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // EWA Engine Settings modal
        if self.ewa_engine_settings_state.is_open {
            use mlc_app_layout::layout::modals::ewa_engine_settings::render_ewa_engine_settings_modal;
            result.ewa_engine_settings = Some(render_ewa_engine_settings_modal(
                ctx,
                modal_layout.prim_screen_w,
                modal_layout.prim_screen_h,
                &self.ewa_engine_settings_state,
                frame_theme,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Skeleton overlay — opaque background fill that hides chart content
        // while the vault unlock / welcome wizard is shown.  Drawn BEFORE the
        // modal so the modal appears on top of the solid background.
        //
        // Fill only the chart content area (between the toolbars). The
        // VelloGpu path composites toolbars on top of the chart scene after
        // this overlay, so the old "fill the whole window" code worked there
        // by accident; the VelloCpu path renders everything into a single
        // pixel buffer in order (toolbars first, then chart + skeleton
        // overlay), so a full-window fill blanked out the left/top/bottom
        // toolbars that had already been drawn. Restricting the overlay to
        // the content rect keeps the toolbars visible on every backend.
        if self.user_settings_state.show_welcome_wizard
            || self.user_settings_state.show_profile_manager
        {
            ctx.set_fill_color(&toolbar_theme.button_bg);
            ctx.fill_rect(
                modal_layout.chart_x,
                modal_layout.chart_y,
                modal_layout.content_w,
                modal_layout.content_h,
            );
        }

        // Welcome Wizard — rendered on top of everything when active.
        // This is shown on first launch (no profile.json existed at startup).
        // It is non-closeable: the user must pick a mode to proceed.
        if self.user_settings_state.show_welcome_wizard {
            use mlc_app_layout::layout::modals::welcome_wizard::render_welcome_wizard;
            let text_color = &toolbar_theme.item_text.clone();
            if result.user_settings.is_none() {
                result.user_settings = Some(Default::default());
            }
            if let Some(ref mut ws_result) = result.user_settings {
                render_welcome_wizard(
                    ctx,
                    modal_layout.prim_screen_w,
                    modal_layout.prim_screen_h,
                    &self.user_settings_state,
                    text_color,
                    toolbar_theme,
                    frame_theme,
                    current_time_ms,
                    input_coordinator,
                    ws_result,
                );
            }
        }

        // Profile Manager overlay — unified modal for profile selection, vault unlock,
        // passphrase creation, and new profile creation.
        // Replaces the old vault_unlock overlay and vault_profile_picker.
        //
        // In skeleton mode the content area is passed so the manager can render
        // as a full-area login screen rather than a small centered modal.
        if self.user_settings_state.show_profile_manager {
            use mlc_app_layout::layout::modals::profile_manager::render_profile_manager;
            let text_color = &toolbar_theme.item_text.clone();
            if result.user_settings.is_none() {
                result.user_settings = Some(Default::default());
            }
            if let Some(ref mut ws_result) = result.user_settings {
                render_profile_manager(
                    ctx,
                    modal_layout.chart_x,
                    modal_layout.chart_y,
                    modal_layout.content_w,
                    modal_layout.content_h,
                    &self.user_settings_state,
                    text_color,
                    toolbar_theme,
                    frame_theme,
                    current_time_ms,
                    input_coordinator,
                    ws_result,
                );
            }
        }

        result
    }

    /// Renders color picker popups and the sync color grid popup.
    ///
    /// Must be called AFTER the sidebar is rendered so that these popups
    /// are drawn on top of the sidebar (correct z-order).
    /// The returned [`ChartModalRenderResult`] only populates `color_picker`
    /// and `sync_color_grid`; all other fields are `None`.
    pub fn render_color_picker_popups(
        &self,
        ctx: &mut dyn mlc_core::render::RenderContext,
        modal_layout: &ChartModalLayout,
        toolbar_theme: &mlc_app_layout::ui::toolbar_theme::ToolbarTheme,
        input_coordinator: &mut uzor::input::InputCoordinator,
        // Pre-resolved color of sync_color_grid.target_panel's non-auto group
        // (caller does the bubble_store lookup so render stays bubble-free).
        target_panel_group_color: Option<[f32; 4]>,
    ) -> ChartModalRenderResult {
        let mut result = ChartModalRenderResult::default();

        // Primitive color picker popup (above primitive settings modal)
        if self.primitive_settings_state.is_color_picker_open() {
            result.color_picker = Some(render_primitive_color_picker_popup(
                ctx,
                &self.primitive_settings_state,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Compare settings color picker popup (rendered above the compare settings modal)
        if self.compare_settings_state.is_open() && self.compare_settings_state.is_color_picker_open() {
            result.color_picker = Some(mlc_app_layout::layout::modals::compare_color_picker::render_compare_color_picker_popup(
                ctx,
                &self.compare_settings_state,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Indicator color picker popup (above indicator settings modal)
        if self.indicator_settings_state.is_color_picker_open() {
            result.color_picker = Some(render_indicator_color_picker_popup(
                ctx,
                &self.indicator_settings_state,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Chart settings color picker popup
        if self.chart_settings_state.is_color_picker_open() {
            result.color_picker = Some(render_chart_settings_color_picker_popup(
                ctx,
                &self.chart_settings_state,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Panel color tag picker popup (legacy L1/L2 — kept for non-panel use)
        if self.panel_color_picker.is_open() {
            result.color_picker = Some(render_panel_color_tag_picker_popup(
                ctx,
                &self.panel_color_picker,
                toolbar_theme,
                input_coordinator,
            ));
        }

        // Sync color grid popup — lightweight preset grid for panel color tags
        if self.sync_color_grid.is_open() {
            use mlc_app_layout::ui::sync_color_grid::{draw_sync_color_grid, SyncColorGridDrawResult};
            use uzor::{Rect, input::Sense};
            use mlc_app_layout::ui::z_order::ZLayer;

            let current_color = if let Some(lid) = self.sync_color_grid.target_leaf {
                self.leaf_color_tags.get(&lid).copied()
            } else if self.sync_color_grid.target_panel.is_some() {
                target_panel_group_color
            } else {
                None
            };

            let draw_result: SyncColorGridDrawResult =
                draw_sync_color_grid(ctx, &self.sync_color_grid, current_color, toolbar_theme);

            // Register input zones on the ColorPicker layer (topmost interactive).
            let layer_id = ZLayer::ColorPicker.push_named(input_coordinator, "sync_color_grid");

            // Full-screen backdrop to catch clicks outside the popup
            input_coordinator.register_on_layer(
                "sync_color_grid:backdrop",
                Rect { x: 0.0, y: 0.0, width: modal_layout.prim_screen_w, height: modal_layout.prim_screen_h },
                Sense::CLICK,
                &layer_id,
            );

            let [px, py, pw, ph] = draw_result.popup_rect;
            // Background / "absorb click inside popup" rect
            input_coordinator.register_on_layer(
                "sync_color_grid:bg",
                Rect { x: px, y: py, width: pw, height: ph },
                Sense::CLICK,
                &layer_id,
            );

            // Individual swatch rects
            for &(idx, [sx, sy, sw, sh]) in &draw_result.swatch_rects {
                input_coordinator.register_on_layer(
                    format!("sync_color_grid:swatch:{}", idx),
                    Rect { x: sx, y: sy, width: sw, height: sh },
                    Sense::CLICK,
                    &layer_id,
                );
            }

            // "+" add-custom button
            if let Some([ax, ay, aw, ah]) = draw_result.add_button_rect {
                input_coordinator.register_on_layer(
                    "sync_color_grid:add",
                    Rect { x: ax, y: ay, width: aw, height: ah },
                    Sense::CLICK,
                    &layer_id,
                );
            }

            // Remove row
            let [rx, ry, rw, rh] = draw_result.remove_rect;
            input_coordinator.register_on_layer(
                "sync_color_grid:remove",
                Rect { x: rx, y: ry, width: rw, height: rh },
                Sense::CLICK,
                &layer_id,
            );

            result.sync_color_grid = Some(draw_result);
        }

        result
    }

    pub fn set_title(&mut self, title: &str) {
        self.title = title.to_string();
    }

    /// Open a chart-local modal.
    pub fn open_modal(&mut self, modal: ChartOpenModal) {
        self.active_modal = modal;
    }

    /// Close any chart-local modal.
    pub fn close_modal(&mut self) {
        self.active_modal = ChartOpenModal::None;
    }

    /// Toggle a chart-local modal.
    pub fn toggle_modal(&mut self, modal: ChartOpenModal) {
        if self.active_modal == modal {
            self.close_modal();
        } else {
            self.open_modal(modal);
        }
    }

    /// Check if any chart-local modal is open.
    pub fn is_modal_open(&self) -> bool {
        self.active_modal.is_open()
    }

    /// Open the Tags & Tabs modal (defaults to TABS sidebar).
    pub fn open_tags_tabs(&mut self) {
        self.tags_tabs_state.open();
        // Pre-select active leaf so MAP buttons appear immediately
        if let Some(leaf_id) = self.panel_grid.docking().active_leaf() {
            self.overlay_settings_state.selected_node_id = Some(leaf_id.0);
        }
    }

    /// Open the Tags & Tabs modal without pre-selecting any node.
    ///
    /// Used when the gesture that opened it did not originate from a
    /// main-grid chart leaf — e.g. the Trading tab's header gear
    /// (workspace-panel arc Stage 3). `selected_node_id` is compared
    /// against main-grid `LeafId`s only (no panel-identity concept exists
    /// in the tags&tabs modal), so pre-selecting the stale last-active
    /// chart leaf would mis-highlight an unrelated node. Honest minimal
    /// behavior: open with nothing pre-selected.
    pub fn open_tags_tabs_without_preselect(&mut self) {
        self.tags_tabs_state.open();
    }

    /// Open the Tags & Tabs modal for a specific leaf (TABS sidebar, leaf highlighted).
    pub fn open_tags_tabs_for_leaf(&mut self, leaf_id: uzor::panels::LeafId) {
        self.tags_tabs_state.open();
        // Store the target leaf in the embedded overlay_settings state for highlighting
        self.overlay_settings_state.target_leaf_id = Some(leaf_id);
        // Also pre-select so MAP buttons appear immediately
        self.overlay_settings_state.selected_node_id = Some(leaf_id.0);
    }

    /// Close the Tags & Tabs modal.
    pub fn close_tags_tabs(&mut self) {
        self.tags_tabs_state.close();
    }

    /// Open the panel color tag picker for the given leaf.
    ///
    /// `anchor_rect` is `[x, y, w, h]` of the color tag square in screen
    /// coordinates (from `LeafTabHitZones.color_tag_rect`).
    pub fn open_panel_color_tag_picker(
        &mut self,
        leaf_id: uzor::panels::LeafId,
        anchor_rect: [f64; 4],
        window_w: f64,
        window_h: f64,
        current_color: Option<&str>,
    ) {
        self.panel_color_picker_leaf = Some(leaf_id);
        self.panel_color_picker.open_l1_smart(
            anchor_rect[0],
            anchor_rect[1],
            anchor_rect[2],
            anchor_rect[3],
            window_w,
            window_h,
            current_color,
        );
    }

    /// Close the panel color tag picker.
    pub fn close_panel_color_tag_picker(&mut self) {
        self.panel_color_picker.close();
        self.panel_color_picker_leaf = None;
    }

    /// Close panel color tag picker one level (L2→L1 or L1→Closed).
    pub fn close_panel_color_tag_picker_one_level(&mut self) {
        use mlc_app_layout::ui::color_picker_state::ColorPickerLevel;
        match self.panel_color_picker.level {
            ColorPickerLevel::L2 => {
                self.panel_color_picker.back_to_l1();
            }
            _ => {
                self.panel_color_picker.close();
                self.panel_color_picker_leaf = None;
            }
        }
    }


}

/// Project a full `ChartPreset` into the layout-friendly `PresetSummary`.
fn preset_to_summary(preset: &ChartPreset) -> mlc_core::PresetSummary {
    let first_window = preset
        .windows
        .first()
        .map(|w| (w.symbol.clone(), w.timeframe.name.clone()));
    mlc_core::PresetSummary {
        id: preset.id.clone(),
        name: preset.name.clone(),
        created_at: preset.created_at,
        first_window,
    }
}
