//! Frame render result types.
//!
//! These types capture the output of frame rendering (hit zones, rects, etc.)
//! and are defined here so that both `zengeld-chart` and `zengeld-terminal-core`
//! can use them without a circular dependency.
//!
//! ## What lives here
//!
//! - Result structs returned by rendering functions (hit zones, button rects, etc.)
//! - Pure-data types with no dependency on core-only state managers
//!
//! ## What stays in core
//!
//! - `ToolbarContent` (references `RenderToolbarSection` from core)
//! - `ChartWindowRenderData` (references `IndicatorManager`, `SubPane`, `CompareOverlay`)
//! - `FrameRenderResult` (references `DropdownResult`, `PanelHeaderHitZones`, `ColorPickerRenderResult`)
//! - `ColorPickerRenderResult` (references `ColorPickerLevel`, `ColorPickerL1Result`, `ColorPickerL2Result`)
//! - `RenderThemes`, `build_render_themes` (depend on `ThemeManager`)

// Use the same Rect type that core's WidgetRect aliases to, so core can use
// these result types without type mismatches.
use uzor::types::Rect as WidgetRect;
use super::rects::LayoutRect;
use super::render_chart::ScaleCornerHitZones;
use crate::panel_app::ChartToolbarRenderResult;
use crate::layout::modals::overlay_settings::OverlaySettingsResult;
use crate::layout::modals::tags_tabs_modal::TagsTabsResult;

// =============================================================================
// Context Menu
// =============================================================================

/// Result of context menu rendering with hit zones
#[derive(Clone, Debug)]
pub struct ContextMenuResult {
    /// Menu bounding rect
    pub menu_rect: WidgetRect,
    /// Item rects for hit testing: (action_id, rect)
    pub item_rects: Vec<(String, WidgetRect)>,
    /// Hovered item id
    pub hovered_item_id: Option<String>,
}

// =============================================================================
// Inline Config
// =============================================================================

/// Result of inline config rendering with hit zones
#[derive(Clone, Debug, Default)]
pub struct InlineConfigResult {
    /// Total bounding rect
    pub rect: WidgetRect,
    /// Button rects for hit testing: (action_id, rect)
    pub item_rects: Vec<(String, WidgetRect)>,
}

// =============================================================================
// Slider Track
// =============================================================================

// Re-export SliderTrackInfo from the widget slider module (canonical definition)
pub use crate::ui::widgets::slider::SliderTrackInfo;

// =============================================================================
// Primitive Settings Modal
// =============================================================================

/// Result of primitive settings modal rendering with hit zones
#[derive(Clone, Debug, Default)]
pub struct PrimitiveSettingsResult {
    /// Modal bounding rect
    pub modal_rect: WidgetRect,
    /// Header rect (for dragging)
    pub header_rect: WidgetRect,
    /// Close button rect
    pub close_btn_rect: WidgetRect,
    /// Tab rects for hit testing: (tab_id, rect)
    pub tab_rects: Vec<(String, WidgetRect)>,
    /// Content area rect
    pub content_rect: WidgetRect,
    /// Content item rects for hit testing: (item_id, rect)
    pub content_items: Vec<(String, WidgetRect)>,
    /// Slider track info for drag handling
    pub slider_tracks: Vec<SliderTrackInfo>,
    /// Character X positions for the text_content input field (for click-to-cursor and drag-to-select).
    /// Populated only while the text_content field is visible (Text tab open).
    pub text_content_char_x_positions: Vec<f64>,
    /// Bounding rect of the text_content input field.
    pub text_content_input_rect: Option<WidgetRect>,
    /// Character X positions for whichever text input is currently active.
    /// Populated for any actively-edited text field (generalises text_content_char_x_positions).
    pub active_input_char_positions: Vec<f64>,
    /// Bounding rect of the currently active text input field.
    pub active_input_rect: Option<WidgetRect>,
}

// =============================================================================
// Panel Tree Manager Modal
// =============================================================================

/// Result of panel tree manager modal rendering with hit zones
#[derive(Clone, Debug, Default)]
pub struct PanelTreeManagerResult {
    /// Modal bounding rect
    pub modal_rect: WidgetRect,
    /// Header rect (for dragging)
    pub header_rect: WidgetRect,
    /// Close button rect
    pub close_btn_rect: WidgetRect,
    /// Tab rects for hit testing: (tab_id, rect)
    pub tab_rects: Vec<(String, WidgetRect)>,
    /// Content area rect
    pub content_rect: WidgetRect,
    /// Content item rects for hit testing: (item_id, rect)
    pub content_items: Vec<(String, WidgetRect)>,
}

// =============================================================================
// Modal Search
// =============================================================================

/// Result of modal search (symbol/indicator) rendering with hit zones
#[derive(Clone, Debug, Default)]
pub struct ModalSearchResult {
    /// Modal bounding rect
    pub modal_rect: WidgetRect,
    /// Header rect (for dragging)
    pub header_rect: Option<WidgetRect>,
    /// Close button rect
    pub close_btn_rect: WidgetRect,
    /// Search input rect
    pub input_rect: WidgetRect,
    /// Result item rects for hit testing: (type_id, rect)
    pub item_rects: Vec<(String, WidgetRect)>,
    /// Star icon rects for symbol search results: (symbol, rect).
    /// Stored separately so they can be registered after item rects, giving
    /// them higher input priority on the overlapping row hit zone.
    pub star_rects: Vec<(String, WidgetRect)>,
    /// Currently hovered item type_id
    pub hovered_item_id: Option<String>,
    /// Total content height (for scrollbar calculation)
    pub total_content_height: f64,
    /// Scrollbar handle rect (if scrollbar visible)
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect (for drag-to-scroll ratio calculation)
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Viewport height (for scroll calculations)
    pub viewport_height: f64,
    /// Results area rect (for scroll hit testing)
    pub results_rect: Option<WidgetRect>,
    /// Category filter rects (for indicator search sidebar): (filter_index, rect)
    pub category_rects: Vec<(usize, WidgetRect)>,
    /// Sidebar rect (for indicator search)
    pub sidebar_rect: Option<WidgetRect>,
    /// X positions of character boundaries in the search input field.
    /// Used for click-to-cursor positioning without needing RenderContext.
    pub search_char_positions: Vec<f64>,
}

// =============================================================================
// Right Sidebar
// =============================================================================

/// Result of right sidebar rendering with hit zones
#[derive(Clone, Debug, Default)]
pub struct RightSidebarResult {
    /// Sidebar bounding rect
    pub sidebar_rect: WidgetRect,
    /// Item rects for hit testing: (item_id, rect)
    /// For indicators: item_id is type_id
    /// For object tree: item_id is object id as string
    pub item_rects: Vec<(String, WidgetRect)>,
    /// Delete button rects for hit testing: (item_id, rect)
    /// For object tree items that can be deleted
    pub delete_button_rects: Vec<(String, WidgetRect)>,
    /// Settings button rects for hit testing: (item_id, rect)
    /// For indicator items in object tree (opens settings modal)
    pub settings_button_rects: Vec<(String, WidgetRect)>,
    /// Currently hovered item id
    pub hovered_item_id: Option<String>,
    /// Content area rect (for scroll handling)
    pub content_rect: WidgetRect,
    /// Total content height (for scrollbar calculation)
    pub content_height: f64,
    /// Scrollbar handle rect (for drag detection)
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect (for drag calculations)
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Whether the alert create button was clicked (UZOR integration)
    pub alert_create_clicked: bool,
}

// =============================================================================
// Chart Settings Modal
// =============================================================================

/// Result of chart settings modal rendering with hit zones
#[derive(Clone, Debug, Default)]
pub struct ChartSettingsModalResult {
    /// Modal bounding rect
    pub modal_rect: WidgetRect,
    /// Header rect (for dragging)
    pub header_rect: WidgetRect,
    /// Close button rect
    pub close_btn_rect: WidgetRect,
    /// Tab rects for hit testing: (tab_id, rect)
    pub tab_rects: Vec<(String, WidgetRect)>,
    /// Content area rect
    pub content_rect: WidgetRect,
    /// Content item rects for hit testing: (item_id, rect)
    pub content_items: Vec<(String, WidgetRect)>,
    /// Footer button rects: (button_id, rect)
    pub footer_buttons: Vec<(String, WidgetRect)>,
    /// Slider track info for drag handling
    pub slider_tracks: Vec<SliderTrackInfo>,
    /// Scrollbar handle rect (for Scales & Lines tab)
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect (for click-to-scroll)
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Total content height (for scroll calculations)
    pub total_content_height: f64,
    /// Viewport height (for scroll calculations)
    pub viewport_height: f64,
    /// Character X positions for the currently active text input field.
    pub active_input_char_positions: Vec<f64>,
    /// Bounding rect of the currently active text input field.
    pub active_input_rect: Option<WidgetRect>,
}

// =============================================================================
// Indicator Settings Modal
// =============================================================================

/// Result of indicator settings modal rendering with hit zones
#[derive(Clone, Debug, Default)]
pub struct IndicatorSettingsModalResult {
    /// Modal bounding rect
    pub modal_rect: WidgetRect,
    /// Header rect (for dragging)
    pub header_rect: WidgetRect,
    /// Close button rect
    pub close_btn_rect: WidgetRect,
    /// Tab rects for hit testing: (tab_id, rect)
    pub tab_rects: Vec<(String, WidgetRect)>,
    /// Content area rect
    pub content_rect: WidgetRect,
    /// Content item rects for hit testing: (item_id, rect)
    pub content_items: Vec<(String, WidgetRect)>,
    /// Footer button rects: (button_id, rect)
    pub footer_buttons: Vec<(String, WidgetRect)>,
    /// Scrollbar handle rect (for Info tab)
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect (for click-to-scroll)
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Total content height (for scroll calculations)
    pub total_content_height: f64,
    /// Viewport height (for scroll calculations)
    pub viewport_height: f64,
    /// Signals enabled state (for Signals tab)
    pub signals_enabled: bool,
    /// Signals toggle rect (for Signals tab hit testing)
    pub signals_toggle_rect: Option<WidgetRect>,
    /// Slider tracks for Visibility tab (dual-handle sliders)
    pub slider_tracks: Vec<SliderTrackInfo>,
    /// Character X positions for the currently active text input field.
    pub active_input_char_positions: Vec<f64>,
    /// Bounding rect of the currently active text input field.
    pub active_input_rect: Option<WidgetRect>,
}

// =============================================================================
// Indicator Overlay
// =============================================================================

/// Hit zones for a single indicator row in overlay dropdown
#[derive(Clone, Debug, Default)]
pub struct IndicatorRowResult {
    /// Indicator instance ID (for indicators) or compare series index (for compare entries)
    pub instance_id: u64,
    /// Whether this row represents a compare series entry (not a regular indicator)
    pub is_compare: bool,
    /// Full row rect
    pub row_rect: WidgetRect,
    /// Visibility toggle button rect
    pub visibility_btn: WidgetRect,
    /// Alert button rect
    pub alert_btn: WidgetRect,
    /// Settings button rect
    pub settings_btn: WidgetRect,
    /// Delete button rect
    pub delete_btn: WidgetRect,
}

/// Result of indicator overlay rendering with hit zones
#[derive(Clone, Debug, Default)]
pub struct IndicatorOverlayResult {
    /// Main button rect (toggle dropdown) - only shown when closed
    pub button_rect: WidgetRect,
    /// Close button rect (chevron at bottom) - only shown when open
    pub close_button_rect: Option<WidgetRect>,
    /// Dropdown rect (if open)
    pub dropdown_rect: Option<WidgetRect>,
    /// Indicator row results for hit testing
    pub indicator_rows: Vec<IndicatorRowResult>,
}

// =============================================================================
// Color Picker Render Result
// =============================================================================

/// Result of color picker popup rendering for hit testing
#[derive(Clone, Debug)]
pub struct ColorPickerRenderResult {
    /// Which level is displayed (L1 or L2)
    pub level: crate::ui::widgets::ColorPickerLevel,
    /// L1 result (if L1 is showing)
    pub l1_result: Option<crate::ui::widgets::ColorPickerL1Result>,
    /// L2 result (if L2 is showing)
    pub l2_result: Option<crate::ui::widgets::ColorPickerL2Result>,
}

// =============================================================================
// Single Chart Panel Result
// =============================================================================

/// Result of rendering a single chart panel
#[derive(Debug)]
pub struct SingleChartPanelResult {
    /// Scale corner hit zones for the window
    pub corner_zones: ScaleCornerHitZones,
    /// Layout rect for the window (for hit testing)
    pub window_rect: LayoutRect,
    /// Indicator overlay result (if overlay was visible)
    pub indicator_overlay: Option<IndicatorOverlayResult>,
    /// Local toolbar hit zones for this window
    pub toolbar_result: ChartToolbarRenderResult,
}

// =============================================================================
// Chart Modal Layout
// =============================================================================

/// Positional data needed to position chart modals correctly.
///
/// Computed by core from the frame layout and passed to `ChartPanelApp::render_modals()`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartModalLayout {
    /// Total screen width (right edge of top toolbar). Used to clamp primitive settings modal.
    pub prim_screen_w: f64,
    /// Total screen height (bottom edge of left toolbar). Used to clamp primitive settings modal.
    pub prim_screen_h: f64,
    /// Default Y position for the primitive settings modal.
    pub prim_modal_y: f64,
    /// Width of the chart content area. Used for indicator settings modal centering.
    pub ind_screen_w: f64,
    /// Height of the chart content area. Used for indicator settings modal clamping.
    pub ind_screen_h: f64,
    /// Left edge of the chart content area (right of left toolbar).
    pub chart_x: f64,
    /// Top edge of the chart content area (below top toolbar).
    pub chart_y: f64,
    /// Width of the full content area between toolbars (for full-area overlays like skeleton login).
    pub content_w: f64,
    /// Height of the full content area between toolbars (for full-area overlays like skeleton login).
    pub content_h: f64,
}

// =============================================================================
// Alert Settings Modal
// =============================================================================

/// Result of alert settings modal rendering with hit zones.
#[derive(Clone, Debug, Default)]
pub struct AlertSettingsResult {
    /// Modal bounding rect.
    pub modal_rect: WidgetRect,
    /// Header rect (for dragging).
    pub header_rect: WidgetRect,
    /// Close button rect.
    pub close_btn_rect: WidgetRect,
    /// Content area rect.
    pub content_rect: WidgetRect,
    /// Content item rects for hit testing: (item_id, rect).
    pub content_items: Vec<(String, WidgetRect)>,
    /// Scrollbar handle rect for the AlertsList tab.
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect for the AlertsList tab.
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Viewport rect for the AlertsList tab list area.
    pub list_viewport_rect: Option<WidgetRect>,
    /// Total content height of the AlertsList tab.
    pub list_total_content_height: f64,
}

// =============================================================================
// User Settings Modal
// =============================================================================

/// Result of user settings modal rendering with hit zones.
#[derive(Clone, Debug, Default)]
pub struct UserSettingsResult {
    /// Modal bounding rect.
    pub modal_rect: WidgetRect,
    /// Header rect (for dragging).
    pub header_rect: WidgetRect,
    /// Close button rect.
    pub close_btn_rect: WidgetRect,
    /// Sidebar tab rects: (tab_id, rect).
    pub tab_rects: Vec<(String, WidgetRect)>,
    /// Content item rects for hit testing: (item_id, rect).
    pub content_items: Vec<(String, WidgetRect)>,
    /// Scrollable viewport rect for the profile list (ProfileList page only).
    pub profile_list_viewport_rect: WidgetRect,
    /// Total content height of the profile list (profiles + cloud section).
    pub profile_list_total_content_h: f64,
    /// Scrollbar handle rect for profile list (for drag detection).
    pub profile_list_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect for profile list (for drag calculation).
    pub profile_list_track_rect: Option<WidgetRect>,
    /// Per-field character X positions for click-to-cursor in profile manager inputs.
    /// Each entry is (field_key, char_x_positions) where field_key matches the
    /// widget IDs used in dispatch_panel_click (e.g. "e2e_passphrase_input").
    pub input_char_positions: Vec<(String, Vec<f64>)>,
    /// Viewport rect of the active tab's scrollable area (for wheel routing).
    pub scroll_viewport_rect: Option<WidgetRect>,
    /// Total content height of the active tab's scrollable area.
    pub scroll_content_height: f64,
    /// Scrollbar handle rect (for drag detection).
    pub scrollbar_handle_rect: Option<WidgetRect>,
    /// Scrollbar track rect (for drag calculations).
    pub scrollbar_track_rect: Option<WidgetRect>,
    /// Viewport height of the active tab's scrollable area.
    pub scroll_viewport_height: f64,
}

// =============================================================================
// Chart Modal Render Result
// =============================================================================

/// Result of rendering the chart-owned modals via ChartPanelApp::render_modals().
///
/// Collects all hit-zone data from primitive settings, indicator settings, and
/// color pickers so core can forward them to the FrameRenderResult.
#[derive(Default)]
pub struct ChartModalRenderResult {
    /// Primitive settings modal result (if open)
    pub primitive_settings: Option<PrimitiveSettingsResult>,
    /// Indicator settings modal result (if open)
    pub indicator_settings: Option<IndicatorSettingsModalResult>,
    /// Chart settings modal result (if open)
    pub chart_settings: Option<ChartSettingsModalResult>,
    /// Overlay settings modal result (if open)
    pub overlay_settings: Option<OverlaySettingsResult>,
    /// Tags & Tabs modal result (if open)
    pub tags_tabs: Option<TagsTabsResult>,
    /// Color picker result (last one wins — primitive, indicator, or chart settings)
    pub color_picker: Option<ColorPickerRenderResult>,
    /// Sync color grid draw result (if the popup was rendered this frame).
    pub sync_color_grid: Option<crate::ui::sync_color_grid::SyncColorGridDrawResult>,
    /// Preset name input modal result (if open).
    pub preset_name_input: Option<crate::layout::modals::preset_name_input::PresetNameInputResult>,
    /// Chart browser modal result (if open).
    pub chart_browser: Option<crate::layout::modals::chart_browser::ChartBrowserResult>,
    /// Alert settings modal result (if open).
    pub alert_settings: Option<AlertSettingsResult>,
    /// Compare settings modal result (if open).
    pub compare_settings: Option<crate::layout::modals::compare_settings::CompareSettingsResult>,
    /// Template name overlay modal result for primitive settings (if open).
    pub prim_template_name: Option<crate::layout::modals::template_name_modal::TemplateNameModalResult>,
    /// Template name overlay modal result for indicator settings (if open).
    pub ind_template_name: Option<crate::layout::modals::template_name_modal::TemplateNameModalResult>,
    /// Template name overlay modal result for compare settings (if open).
    pub cmp_template_name: Option<crate::layout::modals::template_name_modal::TemplateNameModalResult>,
    /// Template name overlay modal result for chart settings (if open).
    pub chart_template_name: Option<crate::layout::modals::template_name_modal::TemplateNameModalResult>,
    /// User settings modal result (if open).
    pub user_settings: Option<UserSettingsResult>,
}

// =============================================================================
// Multi Chart Render Result
// =============================================================================

/// Result of rendering multiple chart windows
#[derive(Clone, Debug, Default)]
pub struct MultiChartRenderResult {
    /// Scale corner hit zones for each window (indexed by window position)
    pub window_corners: Vec<ScaleCornerHitZones>,
    /// Rects for each window (for hit testing which window was clicked).
    /// These are the **content** rects (after carving out toolbar areas) and are
    /// used for chart-area layout computations (scale corners, canvas clicks).
    pub window_rects: Vec<LayoutRect>,
    /// Full window rects (including toolbar areas) for each window.
    /// Used by the ScopedRegion widget system so that toolbar-button hit zones
    /// (which live outside the content area) are within the region boundary.
    pub full_window_rects: Vec<LayoutRect>,
    /// Indicator overlay results for each window (only active window has full result)
    pub indicator_overlays: Vec<Option<IndicatorOverlayResult>>,
    /// Local toolbar hit zones for each window (indexed by window position)
    pub toolbar_results: Vec<ChartToolbarRenderResult>,
}
