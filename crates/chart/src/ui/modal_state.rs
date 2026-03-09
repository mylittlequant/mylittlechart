//! Modal and popup state types for the chart UI.
//!
//! This module contains:
//! - `OpenModal` — which modal is currently open
//! - `ModalState` — full modal state manager (search, scroll, editing, drag)
//! - `IndicatorCategoryFilter` — sidebar filter for indicator search
//! - `ClockPopupState` — state for the timezone clock popup
//! - `IndicatorCategory` — simple indicator category enum (Trend/Momentum/…)
//! - `SearchResult` — flat symbol search result for display in the modal
//!
//! Core re-exports all of these via `pub use zengeld_chart::ui::modal_state::*`.

use crate::ui::scroll_state::ScrollState;
use crate::ui::modal_settings::TextEditingState;

// =============================================================================
// SearchResult — flat, chart-owned search result for the symbol search modal
// =============================================================================

/// A single symbol search result, flattened for display in the modal.
///
/// Core populates `ModalState::symbol_search_results` by converting from its
/// richer `state::SearchResult` type (which contains `SymbolInfo`).
#[derive(Debug, Clone, Default)]
pub struct SearchResult {
    /// Ticker symbol (e.g. "AAPL", "BTCUSD")
    pub symbol: String,
    /// Full name (e.g. "Apple Inc.")
    pub name: String,
    /// Exchange name (e.g. "NASDAQ", "BINANCE")
    pub exchange: String,
    /// Exchange identifier used as part of the composite item key.
    ///
    /// A lowercase slug such as `"binance"` or `"okx"`.  Combined with
    /// `symbol` to form `"BTC-USDT:binance"` so that two results with the same
    /// ticker from different exchanges receive distinct hover/click targets.
    ///
    /// Using `String` (not the `ExchangeId` enum) keeps the chart crate free
    /// from a hard dependency on the connectors crate.
    pub exchange_id: String,
    /// Asset type string (e.g. "Crypto", "Stock")
    pub asset_type: String,
    /// Emoji/icon for the asset category (e.g. "₿", "📈")
    pub category_icon: String,
    /// Whether this symbol is currently in the active watchlist.
    ///
    /// Set by the app layer before populating `ModalState::symbol_search_results`.
    /// Used by the renderer to show a filled or outline star icon.
    pub in_watchlist: bool,
}

// =============================================================================
// IndicatorCategory — simple categorisation of indicators
// =============================================================================

/// High-level indicator category used in the UI (indicator search modal sidebar
/// and indicator definitions in core).
///
/// **Note:** The `zengeld-terminal-indicators` crate has its own more granular
/// `IndicatorCategory` with 25+ variants. This is the coarser, 6-variant version
/// used for UI filtering and `IndicatorDefinition` metadata.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IndicatorCategory {
    Trend,
    Momentum,
    Volatility,
    Volume,
    Oscillator,
    Custom,
}

impl IndicatorCategory {
    /// Human-readable display name
    pub fn display_name(&self) -> &'static str {
        match self {
            IndicatorCategory::Trend => "Trend",
            IndicatorCategory::Momentum => "Momentum",
            IndicatorCategory::Volatility => "Volatility",
            IndicatorCategory::Volume => "Volume",
            IndicatorCategory::Oscillator => "Oscillator",
            IndicatorCategory::Custom => "Custom",
        }
    }

    /// All variants in display order
    pub fn all() -> &'static [IndicatorCategory] {
        &[
            IndicatorCategory::Trend,
            IndicatorCategory::Momentum,
            IndicatorCategory::Volatility,
            IndicatorCategory::Volume,
            IndicatorCategory::Oscillator,
            IndicatorCategory::Custom,
        ]
    }
}

// =============================================================================
// OpenModal
// =============================================================================

/// Which modal is currently open.
///
/// Only one modal can be open at a time.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum OpenModal {
    #[default]
    None,
    /// Symbol search overlay (triggered by symbol button or hotkey)
    SymbolSearch,
    /// Indicator search overlay (triggered by / or indicator button)
    IndicatorSearch,
    /// Chart settings modal
    ChartSettings,
    /// General settings modal
    GeneralSettings,
    /// Template save dialog
    SaveTemplate,
    /// Template load dialog
    LoadTemplate,
    /// Screenshot/export dialog
    Screenshot,
    /// Hotkeys reference
    HotkeysHelp,
    /// Primitive settings modal (for drawing tools)
    PrimitiveSettings,
    /// Compare symbol search (add comparison symbol)
    CompareSearch,
    /// Indicator settings modal (for indicator configuration)
    IndicatorSettings,
}

impl OpenModal {
    /// Check if any modal is open
    pub fn is_open(self) -> bool {
        self != OpenModal::None
    }

    /// Check if this is a search overlay (full-screen style)
    pub fn is_search_overlay(self) -> bool {
        matches!(self, OpenModal::SymbolSearch | OpenModal::IndicatorSearch | OpenModal::CompareSearch)
    }

}

// =============================================================================
// ModalState
// =============================================================================

/// Full modal state manager.
///
/// Tracks which modal is open, search query, scroll position, text editing
/// cursor, drag position, and indicator category filter.
#[derive(Clone, Debug, Default)]
pub struct ModalState {
    /// Currently open modal
    pub current: OpenModal,
    /// Search query for search overlays
    pub search_query: String,
    /// Active tab in tabbed modals
    pub active_tab: Option<String>,
    /// Currently hovered item id (for hover effects in search results)
    pub hovered_item_id: Option<String>,
    /// Symbol search results (populated when SymbolSearch / CompareSearch modal is open)
    pub symbol_search_results: Vec<SearchResult>,
    /// Scroll state for search results
    pub scroll: ScrollState,
    /// Modal position (for dragging)
    pub position: Option<(f64, f64)>,
    /// Is header being dragged?
    pub is_dragging: bool,
    /// Drag offset from modal top-left
    pub drag_offset: Option<(f64, f64)>,
    /// Text editing state for search input
    pub editing_text: Option<TextEditingState>,
    /// Indicator category filter (for indicator search modal)
    pub indicator_category_filter: IndicatorCategoryFilter,
    /// Whether the user is drag-selecting text in the search input.
    pub text_select_dragging: bool,
    /// When true, the indicator search modal shows the Indicator Sets view
    /// instead of the individual indicator list.
    pub show_indicator_sets: bool,
}

impl ModalState {
    /// Create new modal state (all closed)
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if any modal is open
    pub fn is_open(&self) -> bool {
        self.current.is_open()
    }

    /// Open a modal
    pub fn open(&mut self, modal: OpenModal) {
        self.current = modal;
        self.search_query.clear();
        self.active_tab = None;
        self.scroll.reset();
        // Auto-focus search input for search overlay modals so the user can type immediately
        if modal.is_search_overlay() {
            self.editing_text = Some(TextEditingState {
                field_id: "search_input".to_string(),
                text: String::new(),
                cursor: 0,
                selection_start: None,
                blink_time: 0, // updated on first render frame
            });
        } else {
            self.editing_text = None;
        }
        // Reset category filter for indicator search
        self.indicator_category_filter = IndicatorCategoryFilter::All;
        self.show_indicator_sets = false;
        // Keep position for convenience when reopening
    }

    /// Close any open modal
    pub fn close(&mut self) {
        self.current = OpenModal::None;
        self.search_query.clear();
        self.active_tab = None;
        self.hovered_item_id = None;
        self.symbol_search_results.clear();
        self.scroll.reset();
        self.position = None;
        self.is_dragging = false;
        self.drag_offset = None;
        self.editing_text = None;
        self.indicator_category_filter = IndicatorCategoryFilter::All;
        self.show_indicator_sets = false;
    }

    /// Toggle a modal (open if closed, close if same modal open)
    pub fn toggle(&mut self, modal: OpenModal) {
        if self.current == modal {
            self.close();
        } else {
            self.open(modal);
        }
    }

    /// Set search query and reset scroll
    pub fn set_query(&mut self, query: impl Into<String>) {
        let q: String = query.into();
        self.search_query = q.clone();
        self.scroll.reset();
        // Sync with editing state if present
        if let Some(ref mut edit) = self.editing_text {
            edit.cursor = q.chars().count();
            edit.text = q;
            edit.selection_start = None;
        }
    }

    /// Start editing the search input field
    pub fn start_editing(&mut self, current_time_ms: u64) {
        self.editing_text = Some(TextEditingState {
            field_id: "search_input".to_string(),
            text: self.search_query.clone(),
            cursor: self.search_query.chars().count(),
            selection_start: None,
            blink_time: current_time_ms,
        });
    }

    /// Click at position in search input (for cursor positioning)
    pub fn click_at_position(&mut self, click_x: f64, input_x: f64, char_width: f64, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            let text_x = input_x + 28.0; // After search icon
            let relative_x = (click_x - text_x).max(0.0);
            let char_index = (relative_x / char_width).round() as usize;
            edit.cursor = char_index.min(edit.text.chars().count());
            edit.selection_start = None;
            edit.reset_blink(current_time_ms);
        }
    }

    /// Insert character at cursor position
    pub fn insert_char(&mut self, c: char, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            // Delete selection first if any
            if let Some(sel_start) = edit.selection_start {
                let start = sel_start.min(edit.cursor);
                let end = sel_start.max(edit.cursor);
                let byte_start = edit.char_to_byte_pos(start);
                let byte_end = edit.char_to_byte_pos(end);
                edit.text.drain(byte_start..byte_end);
                edit.cursor = start;
                edit.selection_start = None;
            }
            let byte_pos = edit.char_to_byte_pos(edit.cursor);
            edit.text.insert(byte_pos, c);
            edit.cursor += 1;
            edit.reset_blink(current_time_ms);
            self.search_query = edit.text.clone();
            self.scroll.reset();
        }
    }

    /// Delete character before cursor (backspace)
    pub fn delete_char_before(&mut self, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            if let Some(sel_start) = edit.selection_start {
                let start = sel_start.min(edit.cursor);
                let end = sel_start.max(edit.cursor);
                let byte_start = edit.char_to_byte_pos(start);
                let byte_end = edit.char_to_byte_pos(end);
                edit.text.drain(byte_start..byte_end);
                edit.cursor = start;
                edit.selection_start = None;
            } else if edit.cursor > 0 {
                edit.cursor -= 1;
                let byte_start = edit.char_to_byte_pos(edit.cursor);
                let byte_end = edit.char_to_byte_pos(edit.cursor + 1);
                edit.text.drain(byte_start..byte_end);
            }
            edit.reset_blink(current_time_ms);
            self.search_query = edit.text.clone();
            self.scroll.reset();
        }
    }

    /// Delete character after cursor (delete key)
    pub fn delete_char_after(&mut self, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            if let Some(sel_start) = edit.selection_start {
                let start = sel_start.min(edit.cursor);
                let end = sel_start.max(edit.cursor);
                let byte_start = edit.char_to_byte_pos(start);
                let byte_end = edit.char_to_byte_pos(end);
                edit.text.drain(byte_start..byte_end);
                edit.cursor = start;
                edit.selection_start = None;
            } else if edit.cursor < edit.text.chars().count() {
                let byte_start = edit.char_to_byte_pos(edit.cursor);
                let byte_end = edit.char_to_byte_pos(edit.cursor + 1);
                edit.text.drain(byte_start..byte_end);
            }
            edit.reset_blink(current_time_ms);
            self.search_query = edit.text.clone();
            self.scroll.reset();
        }
    }

    /// Move cursor left
    pub fn move_cursor_left(&mut self, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            if edit.cursor > 0 {
                edit.cursor -= 1;
            }
            edit.selection_start = None;
            edit.reset_blink(current_time_ms);
        }
    }

    /// Move cursor right
    pub fn move_cursor_right(&mut self, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            if edit.cursor < edit.text.chars().count() {
                edit.cursor += 1;
            }
            edit.selection_start = None;
            edit.reset_blink(current_time_ms);
        }
    }

    /// Move cursor to start
    pub fn move_cursor_home(&mut self, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            edit.cursor = 0;
            edit.selection_start = None;
            edit.reset_blink(current_time_ms);
        }
    }

    /// Move cursor to end
    pub fn move_cursor_end(&mut self, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            edit.cursor = edit.text.chars().count();
            edit.selection_start = None;
            edit.reset_blink(current_time_ms);
        }
    }

    /// Select all text
    pub fn select_all(&mut self, current_time_ms: u64) {
        if let Some(ref mut edit) = self.editing_text {
            edit.selection_start = Some(0);
            edit.cursor = edit.text.chars().count();
            edit.reset_blink(current_time_ms);
        }
    }

    /// Set active tab
    pub fn set_tab(&mut self, tab: impl Into<String>) {
        self.active_tab = Some(tab.into());
    }

    /// Start dragging the modal header
    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    /// Update position during drag
    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((offset_x, offset_y)) = self.drag_offset {
            self.position = Some((mouse_x - offset_x, mouse_y - offset_y));
        }
    }

    /// End dragging
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }

    /// Check if currently dragging
    pub fn is_dragging(&self) -> bool {
        self.is_dragging
    }

    /// Set indicator category filter
    pub fn set_category_filter(&mut self, filter: IndicatorCategoryFilter) {
        self.indicator_category_filter = filter;
        self.show_indicator_sets = false;
        self.scroll.reset();
    }

    /// Toggle the Indicator Sets view.
    ///
    /// When activated, deselects the category filter so the sidebar renders
    /// the sets button as active instead.
    pub fn toggle_indicator_sets(&mut self) {
        self.show_indicator_sets = !self.show_indicator_sets;
        self.scroll.reset();
    }
}

// =============================================================================
// ClockPopupState
// =============================================================================

/// State for the clock timezone popup
#[derive(Clone, Debug, Default)]
pub struct ClockPopupState {
    /// Whether the popup is visible
    pub is_open: bool,
    /// Position (x, y) of popup anchor (bottom-right of clock button)
    pub anchor_x: f64,
    pub anchor_y: f64,
    /// Currently hovered item id (e.g. "tz:3" or "clock:use_24h")
    pub hovered_item: Option<String>,
}

impl ClockPopupState {
    pub fn open(&mut self, x: f64, y: f64) {
        self.is_open = true;
        self.anchor_x = x;
        self.anchor_y = y;
    }

    pub fn close(&mut self) {
        self.is_open = false;
    }

    pub fn toggle(&mut self, x: f64, y: f64) {
        if self.is_open {
            self.close();
        } else {
            self.open(x, y);
        }
    }
}

// =============================================================================
// IndicatorCategoryFilter
// =============================================================================

/// Category filter for the indicator search modal sidebar
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum IndicatorCategoryFilter {
    /// All indicators (no filter)
    #[default]
    All,
    /// Trend indicators (trend lines, moving averages for trend)
    Trend,
    /// Momentum indicators (RSI, Stochastic, etc.)
    Momentum,
    /// Volatility indicators (ATR, Bollinger, etc.)
    Volatility,
    /// Volume indicators
    Volume,
    /// Oscillators (MACD, CCI, etc.)
    Oscillator,
    /// Moving averages (SMA, EMA, WMA, etc.)
    Average,
    /// Other/Custom indicators
    Other,
}

impl IndicatorCategoryFilter {
    /// Get display label
    pub fn label(&self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Trend => "Trend",
            Self::Momentum => "Momentum",
            Self::Volatility => "Volatility",
            Self::Volume => "Volume",
            Self::Oscillator => "Oscillators",
            Self::Average => "Averages",
            Self::Other => "Other",
        }
    }

    /// Get icon ID
    pub fn icon_id(&self) -> &'static str {
        match self {
            Self::All => "grid",
            Self::Trend => "trend_line",
            Self::Momentum => "arrow_up",
            Self::Volatility => "fib_channel",
            Self::Volume => "histogram",
            Self::Oscillator => "sine_wave",
            Self::Average => "line_chart",
            Self::Other => "more_horizontal",
        }
    }

    /// Get all filters
    pub fn all() -> &'static [IndicatorCategoryFilter] {
        &[
            Self::All,
            Self::Trend,
            Self::Momentum,
            Self::Volatility,
            Self::Volume,
            Self::Oscillator,
            Self::Average,
            Self::Other,
        ]
    }

    /// Get filter from index
    pub fn from_index(idx: usize) -> Option<Self> {
        Self::all().get(idx).copied()
    }

    /// Get index of this filter
    pub fn index(&self) -> usize {
        Self::all().iter().position(|f| f == self).unwrap_or(0)
    }

    /// Check if an `IndicatorCategory` matches this filter.
    ///
    /// `IndicatorCategory` is the coarse 6-variant enum also defined in this
    /// module and re-exported by `zengeld-terminal-core`.
    pub fn matches(&self, category: &IndicatorCategory) -> bool {
        match self {
            Self::All => true,
            Self::Trend => matches!(category, IndicatorCategory::Trend),
            Self::Momentum => matches!(category, IndicatorCategory::Momentum),
            Self::Volatility => matches!(category, IndicatorCategory::Volatility),
            Self::Volume => matches!(category, IndicatorCategory::Volume),
            Self::Oscillator => matches!(category, IndicatorCategory::Oscillator),
            Self::Average => matches!(category, IndicatorCategory::Trend), // MAs are usually in Trend
            Self::Other => matches!(category, IndicatorCategory::Custom),
        }
    }
}

// =============================================================================
// IndicatorCatalogItem
// =============================================================================

/// Item for rendering in indicator catalog (modal or sidebar).
///
/// Moved from `zengeld-terminal-core::ui::definitions::sidebar` so that chart
/// renderers can use it without depending on core.
#[derive(Clone, Debug)]
pub struct IndicatorCatalogItem {
    /// Type ID (e.g., "sma", "rsi")
    pub type_id: String,
    /// Display name (e.g., "Simple Moving Average")
    pub name: String,
    /// Short name (e.g., "SMA")
    pub short_name: String,
    /// Category
    pub category: IndicatorCategory,
    /// Description
    pub description: String,
    /// Whether this is an overlay indicator (drawn on price chart)
    pub overlay: bool,
}

impl IndicatorCatalogItem {
    /// Create from type_id and basic info
    pub fn new(type_id: &str, name: &str, short_name: &str, category: IndicatorCategory) -> Self {
        Self {
            type_id: type_id.to_string(),
            name: name.to_string(),
            short_name: short_name.to_string(),
            category,
            description: String::new(),
            overlay: false,
        }
    }

    /// Set description
    pub fn with_description(mut self, desc: &str) -> Self {
        self.description = desc.to_string();
        self
    }

    /// Set overlay flag
    pub fn with_overlay(mut self, overlay: bool) -> Self {
        self.overlay = overlay;
        self
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_modal_state_open_close() {
        let mut state = ModalState::new();

        assert!(!state.is_open());
        assert_eq!(state.current, OpenModal::None);

        state.open(OpenModal::SymbolSearch);
        assert!(state.is_open());
        assert!(state.current.is_search_overlay());

        // Toggle same modal closes it
        state.toggle(OpenModal::SymbolSearch);
        assert!(!state.is_open());

        // Toggle different modal opens it
        state.toggle(OpenModal::ChartSettings);
        assert!(state.is_open());
        assert_eq!(state.current, OpenModal::ChartSettings);
    }

    #[test]
    fn test_indicator_category_filter_all() {
        let filters = IndicatorCategoryFilter::all();
        assert_eq!(filters.len(), 8);
        assert_eq!(filters[0], IndicatorCategoryFilter::All);
    }

    #[test]
    fn test_indicator_category_filter_matches() {
        let filter = IndicatorCategoryFilter::Momentum;
        assert!(filter.matches(&IndicatorCategory::Momentum));
        assert!(!filter.matches(&IndicatorCategory::Trend));
    }
}
