//! Chart render DATA types (themes, configs, render-state bags). Pure data consumed by backend + painters; the painters themselves live in mlc-app-layout (S1).

use crate::chart::types::{Viewport, PriceScale, TimeScale, GridOptions, Crosshair, Legend, TimeTick};
use crate::chart::annotations::LineStyle;
use crate::Bar;

/// Chart area rectangle
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartRect {
    pub x: f64,
    pub y: f64,
    pub width: f64,
    pub height: f64,
}

impl ChartRect {
    pub fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Get bounds for clamping operations
    #[inline]
    pub fn bounds(&self) -> ChartBounds {
        ChartBounds {
            left: self.x,
            right: self.x + self.width,
            top: self.y,
            bottom: self.y + self.height,
        }
    }
}

/// Bounds for clamping render coordinates to chart area
///
/// Used to prevent rendering outside the chart boundaries
/// (e.g., over price scale or time scale areas).
#[derive(Clone, Copy, Debug, Default)]
pub struct ChartBounds {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
}

impl ChartBounds {
    /// Create bounds from a ChartRect
    #[inline]
    pub fn from_rect(rect: &ChartRect) -> Self {
        rect.bounds()
    }

    /// Check if X coordinate is within bounds
    #[inline]
    pub fn x_in_bounds(&self, x: f64) -> bool {
        x >= self.left && x <= self.right
    }

    /// Check if Y coordinate is within bounds
    #[inline]
    pub fn y_in_bounds(&self, y: f64) -> bool {
        y >= self.top && y <= self.bottom
    }

    /// Clamp X coordinate to bounds
    #[inline]
    pub fn clamp_x(&self, x: f64) -> f64 {
        x.clamp(self.left, self.right.max(self.left))
    }

    /// Clamp Y coordinate to bounds
    #[inline]
    pub fn clamp_y(&self, y: f64) -> f64 {
        y.clamp(self.top, self.bottom.max(self.top))
    }

    /// Check if a vertical line segment (high_y to low_y) is completely outside bounds
    /// Note: In screen coordinates, high_y < low_y (top of screen is 0)
    #[inline]
    pub fn is_y_range_outside(&self, high_y: f64, low_y: f64) -> bool {
        high_y >= self.bottom || low_y <= self.top
    }

    /// Clamp a rectangle to bounds, returning (x, width) tuple
    /// Returns None if rectangle is completely outside bounds
    #[inline]
    pub fn clamp_rect_x(&self, x: f64, width: f64) -> Option<(f64, f64)> {
        let left = x.max(self.left);
        let right = (x + width).min(self.right);
        let w = right - left;
        if w > 0.0 { Some((left, w)) } else { None }
    }

    /// Clamp a rectangle to bounds, returning (y, height) tuple
    /// Returns None if rectangle is completely outside bounds
    #[inline]
    pub fn clamp_rect_y(&self, y: f64, height: f64) -> Option<(f64, f64)> {
        let top = y.max(self.top);
        let bottom = (y + height).min(self.bottom);
        let h = bottom - top;
        if h > 0.0 { Some((top, h)) } else { None }
    }
}

/// Theme colors for chart rendering
#[derive(Clone, Debug)]
pub struct ChartTheme {
    /// Background color
    pub background: String,
    /// Grid line color
    pub grid_line: String,
    /// Text color
    pub text: String,
    /// Up candle body
    pub candle_up: String,
    /// Down candle body
    pub candle_down: String,
    /// Up candle wick
    pub wick_up: String,
    /// Down candle wick
    pub wick_down: String,
    /// Up candle border (optional, for hollow/outlined candles)
    pub candle_up_border: Option<String>,
    /// Down candle border (optional, for hollow/outlined candles)
    pub candle_down_border: Option<String>,
    /// Legend value color for bullish
    pub legend_value_up: String,
    /// Legend value color for bearish
    pub legend_value_down: String,
    /// Crosshair color
    pub crosshair: String,
    /// Scale background
    pub scale_bg: String,
    /// Scale border
    pub scale_border: String,
    /// Sub-pane background (with style opacity applied)
    pub sub_pane_bg: String,
    /// Footprint cluster buy-side color (volume text + heat fill).
    pub footprint_buy: String,
    /// Footprint cluster sell-side color (volume text + heat fill).
    pub footprint_sell: String,
    /// Renko brick fill (up).
    pub renko_up: String,
    /// Renko brick fill (down).
    pub renko_down: String,
    /// Point & Figure X-column glyph color.
    pub pnf_x: String,
    /// Point & Figure O-column glyph color.
    pub pnf_o: String,
    /// Three Line Break up-line fill.
    pub tlb_up: String,
    /// Three Line Break down-line fill.
    pub tlb_down: String,
    /// Kagi yang (thick) line color.
    pub kagi_yang: String,
    /// Kagi yin (thin) line color.
    pub kagi_yin: String,
    /// Hollow candles bullish (outline) color.
    pub hollow_up: String,
    /// Hollow candles bearish (fill) color.
    pub hollow_down: String,
    /// "Line" chart type's own line color (owner-approved settings matrix
    /// wave 3, 2026-07-11).
    pub line: String,
    /// Step line (staircase) line color.
    pub step_line: String,
    /// Line-with-markers marker color.
    pub line_markers: String,
    /// Area chart line color.
    pub area_line: String,
    /// Area chart top fill color.
    pub area_top: String,
    /// Area chart bottom fill color.
    pub area_bottom: String,
    /// Baseline chart top (above-baseline) line color.
    pub baseline_top_line: String,
    /// Baseline chart top (above-baseline) fill color.
    pub baseline_top_fill: String,
    /// Baseline chart bottom (below-baseline) line color.
    pub baseline_bottom_line: String,
    /// Baseline chart bottom (below-baseline) fill color.
    pub baseline_bottom_fill: String,
    /// HLC/OHLC bars up color.
    pub bar_up: String,
    /// HLC/OHLC bars down color.
    pub bar_down: String,
    /// Candle draw form (skin texture layer): Solid / ThinGlow / Phosphor.
    pub candle_style: crate::theme::CandleStyle,
    /// Soft glow underlay on candle bodies (skin texture layer).
    pub candle_glow: bool,
    /// DOM-heat gradient, bid side: base colour and the "hot" endpoint a
    /// deep level leans toward. Same token pair the DOM ladder's intensity
    /// bars use, so the on-chart heat and the panel read as one skin
    /// instead of two palettes.
    pub heat_bid: String,
    pub heat_bid_hot: String,
    /// Ask side of the same pair.
    pub heat_ask: String,
    pub heat_ask_hot: String,
    /// Volume-bar colours straight from the skin. The volume overlay used
    /// to synthesise its own by pasting a 50%-alpha suffix onto the candle
    /// colour, which is why it read as washed-out next to everything else
    /// the same skin paints.
    pub volume_up: String,
    pub volume_down: String,
}

impl Default for ChartTheme {
    fn default() -> Self {
        Self {
            background: "#1e222d".to_string(),
            grid_line: "#2a2e39".to_string(),
            text: "#d1d4dc".to_string(),
            candle_up: "#26a69a".to_string(),
            candle_down: "#ef5350".to_string(),
            wick_up: "#26a69a".to_string(),
            wick_down: "#ef5350".to_string(),
            candle_up_border: None,
            candle_down_border: None,
            legend_value_up: "#26a69a".to_string(),
            legend_value_down: "#ef5350".to_string(),
            crosshair: "#9598a1".to_string(),
            scale_bg: "#1e222d".to_string(),
            scale_border: "#2a2e39".to_string(),
            sub_pane_bg: "#1e222d".to_string(),
            footprint_buy: "#26a69a".to_string(),
            footprint_sell: "#ef5350".to_string(),
            renko_up: "#26a69a".to_string(),
            renko_down: "#ef5350".to_string(),
            pnf_x: "#26a69a".to_string(),
            pnf_o: "#ef5350".to_string(),
            tlb_up: "#26a69a".to_string(),
            tlb_down: "#ef5350".to_string(),
            kagi_yang: "#26a69a".to_string(),
            kagi_yin: "#ef5350".to_string(),
            hollow_up: "#26a69a".to_string(),
            hollow_down: "#ef5350".to_string(),
            line: "#26a69a".to_string(),
            step_line: "#26a69a".to_string(),
            line_markers: "#26a69a".to_string(),
            area_line: "#26a69a".to_string(),
            area_top: "rgba(38, 166, 154, 0.28)".to_string(),
            area_bottom: "rgba(38, 166, 154, 0.05)".to_string(),
            baseline_top_line: "#26a69a".to_string(),
            baseline_top_fill: "rgba(38, 166, 154, 0.28)".to_string(),
            baseline_bottom_line: "#ef5350".to_string(),
            baseline_bottom_fill: "rgba(239, 83, 80, 0.28)".to_string(),
            bar_up: "#26a69a".to_string(),
            bar_down: "#ef5350".to_string(),
            candle_style: crate::theme::CandleStyle::Solid,
            candle_glow: false,
            heat_bid: "#14b8a6".to_string(),
            heat_bid_hot: "#30ff76".to_string(),
            heat_ask: "#dc352b".to_string(),
            heat_ask_hot: "#ffd311".to_string(),
            volume_up: "#26a69a".to_string(),
            volume_down: "#ef5350".to_string(),
        }
    }
}

/// Footprint cluster display toggles + heat-intensity setting.
///
/// Per-panel (`ChartPanelState::footprint_display`), threaded into
/// [`ChartRenderState`] the same way `grid`/`legend` are — see
/// `mlc-backend::bubble::facade::render_state_for` and its 4 call sites in
/// `mlc-app::lib.rs`. `Copy` (all scalar fields) so it can be passed by
/// value into `ChartRenderState` without adding a lifetime.
///
/// `Serialize`/`Deserialize` for `ChartWindowSnapshot` persistence — see
/// `mlc-core::preset::snapshots::ChartWindowSnapshot::footprint_display`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct FootprintDisplayOptions {
    /// Draw the "ABS" absorption-bar label + stacked-imbalance triangle
    /// markers. Default `true` (matches pre-toggle behavior).
    pub show_abs_markers: bool,
    /// Draw the per-cell buy/sell volume numbers. Default `true` (matches
    /// pre-toggle behavior). The existing legibility gate (bar width /
    /// font-size threshold) still applies underneath this toggle — turning
    /// this off suppresses numbers even when the legibility gate would
    /// otherwise allow them; the heat-fill cells stay visible either way.
    pub show_volume_text: bool,
    /// Alpha multiplier for the zoomed-out heat-fill cells (dominant-side
    /// color, alpha by volume share). Default `Normal` (matches pre-toggle
    /// behavior — the multiplier is a no-op at 1.0).
    pub heat_intensity: FootprintHeatIntensity,
    /// Row-grouping width in instrument ticks (canon-fix wave 2, item 5) —
    /// `0` = raw ungrouped rows (one row per tick, matches pre-toggle
    /// behavior), `>0` bins adjacent price levels into `ticks × tick_size`
    /// -wide rows before rendering (see `FoldParams::group_step`,
    /// `TradeToFootprintDerived`'s `group_step` param in dig3). Converted to
    /// a price step at the subscribe seam the same way renko/pnf convert
    /// their own tick-based params (`build_subscribe_params`). Default `0`.
    #[serde(default)]
    pub group_ticks: u32,
    /// Stacked-imbalance cell ratio threshold (owner-approved settings
    /// matrix, wave 1, item 8) — a cell's dominant-side volume must be at
    /// least this multiple of the opposite side's volume to qualify as an
    /// imbalance for the ABS-marker / stacked-triangle detectors. Replaces
    /// the previously-hardcoded `CELL_IMBALANCE_RATIO` constant in
    /// `chart_render/footprint.rs`. Default `1.5` (the constant's old
    /// value — pixel-identical to the pre-toggle render). Distinct from the
    /// STACKED ratio (3.0), which stays a separate named const — this
    /// toggle only ever touches the per-cell ratio.
    #[serde(default = "default_imbalance_threshold")]
    pub imbalance_threshold: f64,
    /// Per-cell content/coloring mode (chart-type-render-doctrine
    /// 2026-07-09 §8 "Cell layout modes"). Default `BidAsk` — the original
    /// two-number split layout, pixel-identical to pre-mode behavior.
    /// Orthogonal to every other field on this struct: imbalance
    /// highlighting, stacked-imbalance zones, POC/VA fills, and absorption
    /// markers stay active in every mode — this field only changes what
    /// the per-row foreground text/graphic and background heat show.
    #[serde(default)]
    pub cell_mode: FootprintCellMode,
}

/// `#[serde(default = ...)]` sentinel for
/// `FootprintDisplayOptions::imbalance_threshold` — old presets (pre this
/// field) deserialize onto the pre-toggle hardcoded `CELL_IMBALANCE_RATIO`
/// value.
fn default_imbalance_threshold() -> f64 {
    1.5
}

impl Default for FootprintDisplayOptions {
    fn default() -> Self {
        Self {
            show_abs_markers: true,
            show_volume_text: true,
            heat_intensity: FootprintHeatIntensity::Normal,
            group_ticks: 0,
            imbalance_threshold: default_imbalance_threshold(),
            cell_mode: FootprintCellMode::default(),
        }
    }
}

/// Footprint per-cell content/coloring mode (chart-type-render-doctrine
/// 2026-07-09 §8 — canonical cross-platform cell layout taxonomy).
///
/// UI labels (exact strings from the doctrine, used verbatim in the
/// CLUSTERS SETTINGS "Cell mode" dropdown):
/// - `BidAsk` → "Bid×Ask"
/// - `Delta` → "Delta"
/// - `Volume` → "Volume"
/// - `VolumeDeltaColor` → "Volume (Delta Color)"
/// - `Profile` → "Profile"
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FootprintCellMode {
    /// Two numbers per row, bid left / ask right of the bar's vertical
    /// center — the original layout, pixel-identical default.
    #[default]
    BidAsk,
    /// Single signed number per row (`ask − bid`), text colored by sign
    /// using the footprint block's existing up/down digit colors; cell
    /// background tinted by delta sign with magnitude-scaled alpha.
    Delta,
    /// Single unsigned total per row (`bid + ask`), neutral text color;
    /// background heat by volume magnitude (reuses the existing dominant-
    /// side heat-fill machinery).
    Volume,
    /// Text shows the volume total (like `Volume`), but the cell
    /// background is colored by delta sign/magnitude instead of volume
    /// magnitude — decouples displayed number from displayed color
    /// (ATAS/Bookmap convention).
    VolumeDeltaColor,
    /// No text — a horizontal mini-histogram per row: two segments (bid
    /// part left in down-color, ask part right in up-color), bar length
    /// proportional to the row's volume share of the bar's max row
    /// volume, left-anchored in the cell.
    Profile,
}

impl FootprintCellMode {
    /// Exact UI label from the doctrine (§8 "Cell layout modes").
    pub fn label(self) -> &'static str {
        match self {
            Self::BidAsk => "Bid×Ask",
            Self::Delta => "Delta",
            Self::Volume => "Volume",
            Self::VolumeDeltaColor => "Volume (Delta Color)",
            Self::Profile => "Profile",
        }
    }

    /// Stable id used for persistence + dropdown option value.
    pub fn as_id(self) -> &'static str {
        match self {
            Self::BidAsk => "bid_ask",
            Self::Delta => "delta",
            Self::Volume => "volume",
            Self::VolumeDeltaColor => "volume_delta_color",
            Self::Profile => "profile",
        }
    }

    /// Parse back from the persisted/dropdown id. Unknown values fall back
    /// to `BidAsk` (additive-field back-compat sentinel).
    pub fn from_id(id: &str) -> Self {
        match id {
            "delta" => Self::Delta,
            "volume" => Self::Volume,
            "volume_delta_color" => Self::VolumeDeltaColor,
            "profile" => Self::Profile,
            _ => Self::BidAsk,
        }
    }

    /// Parse from the exact UI label (the modal's dropdown option value IS
    /// the label for this control, unlike `heat_intensity`'s
    /// single-word-lowercase ids). Unknown values fall back to `BidAsk`.
    pub fn from_label(label: &str) -> Self {
        match label {
            "Delta" => Self::Delta,
            "Volume" => Self::Volume,
            "Volume (Delta Color)" => Self::VolumeDeltaColor,
            "Profile" => Self::Profile,
            _ => Self::BidAsk,
        }
    }
}

/// Footprint heat-fill alpha multiplier preset.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum FootprintHeatIntensity {
    Low,
    #[default]
    Normal,
    High,
}

impl FootprintHeatIntensity {
    /// Multiplier applied to the computed heat-fill alpha. `Normal` is a
    /// no-op (1.0) so the default render is pixel-identical to the
    /// pre-toggle hardcoded formula.
    pub fn alpha_multiplier(self) -> f64 {
        match self {
            Self::Low => 0.5,
            Self::Normal => 1.0,
            Self::High => 1.6,
        }
    }

    /// Stable id used for persistence + dropdown option value.
    pub fn as_id(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Normal => "normal",
            Self::High => "high",
        }
    }

    /// Parse back from the persisted/dropdown id. Unknown values fall back
    /// to `Normal` (additive-field back-compat sentinel).
    pub fn from_id(id: &str) -> Self {
        match id {
            "low" => Self::Low,
            "high" => Self::High,
            _ => Self::Normal,
        }
    }
}

/// TPO (Market Profile) display toggles (owner-approved settings matrix,
/// wave 1). Per-panel (`ChartPanelState::tpo_display`), threaded into
/// [`ChartRenderState`] the same way `footprint_display`/`tick_intensity`
/// are — see `mlc-backend::bubble::render::build_chart_render_state` and
/// its 4 call sites in `mlc-app::lib.rs`. `Copy` (all scalar fields) so it
/// can be passed by value into `ChartRenderState` without adding a
/// lifetime. `Serialize`/`Deserialize` for `ChartWindowSnapshot`
/// persistence, mirroring `FootprintDisplayOptions`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TpoDisplayOptions {
    /// Draw the session date-header strip above each block (e.g. "Jun
    /// 17"). Default `false` (owner order: the header duplicates the time
    /// axis). Gates both the date-header draw call AND the
    /// `DATE_HEADER_PX` reserved vertical space in `chart_render/tpo.rs` —
    /// when off, the header is not drawn and the space is reclaimed by the
    /// profile body.
    #[serde(default)]
    pub show_session_dates: bool,
    /// Draw the POC (Point of Control) row tint + horizontal line. Default
    /// `true` (matches pre-toggle behavior).
    #[serde(default = "default_true")]
    pub show_poc: bool,
    /// Draw the Value Area fill band (VAL to VAH). Default `true` (matches
    /// pre-toggle behavior).
    #[serde(default = "default_true")]
    pub show_value_area: bool,
    /// Draw single-print rows (dashed border, exactly 1 letter touched the
    /// price level). Default `true` (matches pre-toggle behavior).
    #[serde(default = "default_true")]
    pub show_single_prints: bool,
    /// Draw Poor High / Poor Low triangle markers. Default `true` (matches
    /// pre-toggle behavior).
    #[serde(default = "default_true")]
    pub show_poor_hl: bool,
    /// Draw the Initial Balance dashed bracket. Default `true` (matches
    /// pre-toggle behavior).
    #[serde(default = "default_true")]
    pub show_initial_balance: bool,
    /// Number of chronological TPO periods (letters, starting from 'A')
    /// that define the Initial Balance range. Default `2` (canon Market
    /// Profile convention — periods 'A' and 'B'). Dropdown options are
    /// 2/3/4.
    #[serde(default = "default_ib_periods")]
    pub ib_periods: u8,
}

/// `#[serde(default = ...)]` sentinel shared by every `TpoDisplayOptions`
/// bool field that defaults "on" (matches the pre-toggle render).
fn default_true() -> bool {
    true
}

/// `#[serde(default = ...)]` sentinel for `TpoDisplayOptions::ib_periods`.
fn default_ib_periods() -> u8 {
    2
}

impl Default for TpoDisplayOptions {
    fn default() -> Self {
        Self {
            show_session_dates: false,
            show_poc: true,
            show_value_area: true,
            show_single_prints: true,
            show_poor_hl: true,
            show_initial_balance: true,
            ib_periods: default_ib_periods(),
        }
    }
}

/// Marker glyph shape for the `"line_markers"` chart type (owner-approved
/// settings matrix wave 3, 2026-07-11). Distinct from
/// `RenderMarkerShape` (drawing-primitive markers, unrelated feature).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, serde::Serialize, serde::Deserialize)]
pub enum LineMarkerStyle {
    #[default]
    Circle,
    Square,
    Triangle,
    Diamond,
}

impl LineMarkerStyle {
    /// Stable id used for persistence + dropdown option value.
    pub fn as_id(self) -> &'static str {
        match self {
            Self::Circle => "circle",
            Self::Square => "square",
            Self::Triangle => "triangle",
            Self::Diamond => "diamond",
        }
    }

    /// Parse back from the persisted/dropdown id. Unknown values fall back
    /// to `Circle` (additive-field back-compat sentinel, matches the
    /// `FootprintHeatIntensity::from_id` precedent).
    pub fn from_id(id: &str) -> Self {
        match id {
            "square" => Self::Square,
            "triangle" => Self::Triangle,
            "diamond" => Self::Diamond,
            _ => Self::Circle,
        }
    }
}

/// Simple line-family per-type display options: line/step-line/area/
/// baseline widths + `line_markers`' marker style/size (owner-approved
/// settings matrix wave 3, 2026-07-11). Colors live in [`ChartTheme`]
/// (mirrors how wave 2 put renko/pnf/tlb/kagi/hollow colors in the theme);
/// these are the non-color per-type options, stored the same way
/// [`FootprintDisplayOptions`]/[`TpoDisplayOptions`] store their non-color
/// per-type toggles. Per-panel (`ChartPanelState::line_family_display`),
/// threaded into [`ChartRenderState`] the same way `footprint_display`/
/// `tpo_display` are. `Copy` (all scalar fields) so it can be passed by
/// value without adding a lifetime. `Serialize`/`Deserialize` for
/// `ChartWindowSnapshot` persistence, mirroring `TpoDisplayOptions`.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct LineFamilyDisplayOptions {
    /// Line width (px) for the `"line"` chart type. Default `2.0` (matches
    /// the pre-toggle hardcoded `ctx.set_stroke_width(2.0)`).
    #[serde(default = "default_line_family_width")]
    pub line_width: f64,
    /// Line width (px) for the `"step_line"` chart type. Default `2.0`.
    #[serde(default = "default_line_family_width")]
    pub step_line_width: f64,
    /// Line width (px) for the `"area"` chart type's top line. Default
    /// `2.0`.
    #[serde(default = "default_line_family_width")]
    pub area_line_width: f64,
    /// Line width (px) for the `"baseline"` chart type's top/bottom lines.
    /// Default `2.0`.
    #[serde(default = "default_line_family_width")]
    pub baseline_line_width: f64,
    /// Marker glyph shape for the `"line_markers"` chart type. Default
    /// `Circle` (matches the pre-toggle hardcoded `ctx.arc(...)` circle).
    #[serde(default)]
    pub marker_style: LineMarkerStyle,
    /// Marker radius (px) for the `"line_markers"` chart type. Default
    /// `3.0` (matches the pre-toggle hardcoded `marker_radius`).
    #[serde(default = "default_marker_size")]
    pub marker_size: f64,
}

/// `#[serde(default = ...)]` sentinel shared by every `LineFamilyDisplayOptions`
/// line-width field.
fn default_line_family_width() -> f64 {
    2.0
}

/// `#[serde(default = ...)]` sentinel for `LineFamilyDisplayOptions::marker_size`.
fn default_marker_size() -> f64 {
    3.0
}

impl Default for LineFamilyDisplayOptions {
    fn default() -> Self {
        Self {
            line_width: default_line_family_width(),
            step_line_width: default_line_family_width(),
            area_line_width: default_line_family_width(),
            baseline_line_width: default_line_family_width(),
            marker_style: LineMarkerStyle::default(),
            marker_size: default_marker_size(),
        }
    }
}

/// Render-facing mirror of `mlc_backend::bubble::HistoryDepth` — mlc-core
/// cannot depend on mlc-backend (which owns the bubble-side source of
/// truth), so `mlc-backend::bubble::render::build_chart_render_state`
/// translates 1:1 into this shape when it populates `ChartRenderState`.
///
/// Drives the degraded-history badge (hybrid-data-modes-plan-2026-07-08,
/// Step 6) — `Capped`/`LiveOnly` render a small pane-corner chip,
/// `Full`/`Unknown` render nothing.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HistoryDepthDisplay {
    /// No seed fetch reported yet for the active series (still loading, or
    /// this chart_type has no seed concept at all — dom/liquidation
    /// heatmap). No badge.
    #[default]
    Unknown,
    /// Deep/paginated history, no ceiling hit. No badge.
    Full,
    /// A soft/hard venue ceiling was hit — `achieved` is the last known
    /// depth in points, shown in the badge text.
    Capped { achieved: usize },
    /// No REST pagination exists at all for this venue/chart_type — the
    /// active series is backed only by what has accumulated live since
    /// subscribe.
    LiveOnly,
}

/// How the DOM Heatmap is INKED — every value comes from the overlay's
/// `IndicatorInstance`, none of it a constant, because the layer is tuned by
/// eye against the reference and the tuning has to be reachable.
///
/// Grid geometry (`row_ticks`, `band_pct`) is NOT here: it decides what the
/// buffer stores, so it lives at the ingest seam instead.
#[derive(Clone, Copy, Debug)]
pub struct DomHeatmapInk {
    /// Gain on the on-screen depth normaliser.
    pub sensitivity: f64,
    /// Percentile of the visible depths used AS that normaliser, instead of
    /// the plain maximum. With the maximum, a single wall sets the scale and
    /// every ordinary level sinks under the alpha cut — they only surfaced
    /// as price approached and their size grew, which read as the layer
    /// loading in late. 100 restores max-normalisation.
    pub norm_pct: f64,
    /// Exponent of the depth→alpha curve. `<1` lifts weak cells, `>1` sinks
    /// them into the background so only walls stand out.
    pub contrast: f64,
    /// Fill alpha at full depth.
    pub opacity: f64,
    /// A cell at this multiple of the visible mean is drawn as a solid wall
    /// with dark digits.
    pub wall_x: f64,
    /// Per-cell size numbers on/off.
    pub show_values: bool,
    /// Scale on the auto-fitted digit size.
    pub value_size: f64,
    /// Aspect ratio (height ÷ width) past which a cell stops printing its
    /// number. A square or landscape cell carries digits fine; a portrait
    /// one crowds them between its own edges. Default 70/30.
    pub value_aspect: f64,
}

impl Default for DomHeatmapInk {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            norm_pct: 90.0,
            contrast: 1.7,
            opacity: 0.38,
            wall_x: 5.0,
            show_values: true,
            value_size: 1.0,
            value_aspect: 70.0 / 30.0,
        }
    }
}

/// How the realized-liquidation layer is INKED — same rule as
/// [`DomHeatmapInk`]: every value comes from the overlay's
/// `IndicatorInstance`, none of it a constant.
#[derive(Clone, Copy, Debug)]
pub struct LiquidationInk {
    /// Gain on the normaliser for level intensity.
    pub sensitivity: f64,
    /// Price grouping for levels, in PERCENT of price. A fraction of price
    /// rather than a tick count, so the grouping means the same thing at
    /// 60k as at 0.6.
    pub row_pct: f64,
    /// Notional a price row must collect before it is drawn as a level.
    pub min_usd: f64,
    /// Level bars on/off.
    pub show_levels: bool,
    /// Individual prints on/off.
    pub show_dots: bool,
    /// Scale on the print mark — line weight and accent radius together.
    pub dot_scale: f64,
    /// Half-reach of a print's price line, in BARS, at the largest print in
    /// view. Bars rather than pixels so the mark keeps its meaning across
    /// zoom levels; smaller prints reach proportionally less.
    pub span_bars: f64,
    /// Fill alpha of the level bars at full intensity.
    pub opacity: f64,
}

impl Default for LiquidationInk {
    fn default() -> Self {
        Self {
            sensitivity: 1.0,
            row_pct: 0.02,
            min_usd: 25_000.0,
            show_levels: true,
            show_dots: true,
            dot_scale: 1.0,
            span_bars: 6.0,
            opacity: 0.55,
        }
    }
}

/// How the projected-liquidation layer reads.
#[derive(Clone, Copy, Debug)]
pub struct LiquidationProjectionInk {
    /// Gain on the band normaliser.
    pub sensitivity: f64,
    /// Band height as a PERCENT of price — must match the grouping the
    /// model used, or the bands overlap or leave gaps.
    pub row_pct: f64,
    /// Fill alpha at full intensity.
    pub opacity: f64,
    /// Keep bands price already traded through, dimmed. Off leaves only
    /// standing liquidity, which is the trading question; on shows where
    /// the pressure has already been spent.
    pub show_consumed: bool,
}

impl Default for LiquidationProjectionInk {
    fn default() -> Self {
        Self { sensitivity: 1.0, row_pct: 0.05, opacity: 0.5, show_consumed: true }
    }
}

/// State needed for chart rendering
///
/// This struct aggregates all the data needed to render a chart frame.
/// Platforms create this from their own state and pass it to render functions.
#[derive(Debug)]
pub struct ChartRenderState<'a> {
    /// Viewport for coordinate conversion
    pub viewport: &'a Viewport,
    /// Price scale for Y-axis
    pub price_scale: &'a PriceScale,
    /// Time scale for X-axis
    pub time_scale: &'a TimeScale,
    /// Bar data
    pub bars: &'a [Bar],
    /// Grid options
    pub grid: &'a GridOptions,
    /// Crosshair state
    pub crosshair: &'a Crosshair,
    /// Legend state
    pub legend: &'a Legend,
    /// Chart area dimensions
    pub chart_rect: ChartRect,
    /// Theme colors
    pub theme: &'a ChartTheme,
    /// Pre-computed time ticks (shared between grid and time scale)
    /// If None, will be computed on-demand (for backwards compatibility)
    pub time_ticks: Option<&'a [TimeTick]>,
    /// Current timeframe label (e.g., "1H", "15m", "1D") for primitive visibility filtering
    /// If None, all primitives are visible regardless of timeframe settings
    pub current_timeframe: Option<&'a str>,
    /// Disable clipping to chart_rect bounds (for Glass styles where chart renders under toolbars)
    /// When true, chart content can render beyond chart_rect boundaries
    pub disable_clip: bool,
    /// Time format settings for rendering time labels
    /// If None, defaults will be used
    pub time_format_settings: Option<&'a crate::TimeFormatSettings>,
    /// Timeframe interval in milliseconds for countdown calculation
    pub timeframe_interval_ms: Option<i64>,
    /// Scale settings for countdown and other scale features
    pub scale_settings: Option<&'a crate::ScaleSettings>,
    /// Whether candle bodies are visible
    pub body_enabled: bool,
    /// Whether candle borders are visible
    pub border_enabled: bool,
    /// Whether candle wicks are visible
    pub wick_enabled: bool,
    /// Use previous bar's close to determine candle color (instead of open vs close)
    pub use_prev_close: bool,
    /// Footprint bar data for cluster rendering. When `Some(_)` and the chart
    /// type is `footprint`, the cluster renderer paints Bid × Ask per price
    /// level, POC highlight, and value-area background.
    /// When `None` (default), footprint dispatch falls back to draw_candles.
    pub footprint_bars: Option<&'a [crate::types::FootprintBar]>,
    /// Footprint display toggles (ABS markers / volume text / heat
    /// intensity) — per-panel config, mirrors `ChartPanelState::footprint_display`.
    /// Unused for every other chart type.
    pub footprint_display: FootprintDisplayOptions,
    /// Tick-bar body-opacity intensity toggle — per-panel config, mirrors
    /// `ChartPanelState::tick_intensity`. When `false` (default),
    /// `draw_tick_bars` renders plain solid bodies exactly like
    /// `draw_candles`; when `true` it restores the volume-modulated alpha
    /// formula. Unused for every other chart type.
    pub tick_intensity: bool,
    /// Renko brick data. When `Some(_)` and chart_type == "renko" the
    /// renko renderer draws filled rect bricks; otherwise renko dispatch
    /// renders nothing.
    pub renko_bars: Option<&'a [crate::types::RenkoBrick]>,
    /// Three Line Break data. When `Some(_)` and chart_type ==
    /// "three_line_break" the TLB renderer draws filled rect lines.
    pub three_line_break: Option<&'a [crate::types::ThreeLineBreakLine]>,
    /// Point-and-Figure column data.
    pub pnf_columns: Option<&'a [crate::types::PnfColumn]>,
    /// Kagi segment data.
    pub kagi_segments: Option<&'a [crate::types::KagiSegment]>,
    /// TPO Market Profile session snapshots — typically a small ring
    /// (current session + N closed sessions).
    pub tpo_sessions: Option<&'a [crate::types::TpoSession]>,
    /// TPO display toggles (session-date header / POC / VA / single-prints
    /// / poor-HL / initial-balance / IB period count) — per-panel config,
    /// mirrors `ChartPanelState::tpo_display`. Unused for every other
    /// chart type.
    pub tpo_display: TpoDisplayOptions,
    /// Simple line-family per-type display options (line/step-line/area/
    /// baseline widths + line_markers' marker style/size) — per-panel
    /// config, mirrors `ChartPanelState::line_family_display`. Unused for
    /// every other chart type.
    pub line_family_display: LineFamilyDisplayOptions,
    /// Pre-computed Heikin Ashi cache. When `Some(_)` and chart_type
    /// == "heikin_ashi" the HA renderer reads directly from this slice
    /// instead of running the recursive HA formula from index 0 each
    /// frame. Window maintains the cache on `ChartContainer.ha_cache`.
    pub ha_cache: Option<&'a [crate::types::HaBar]>,
    /// Pre-computed Volume Profile overlay. When `Some(_)` and
    /// `vp_show == true` the VP renderer draws the right-side histogram.
    /// Maintained by `ChartContainer::ensure_vp` in the prepare prepass.
    pub vp_overlay: Option<&'a crate::chart::volume_profile::VolumeProfileData>,
    /// Pre-computed TPO Profile overlay (item D — block-histogram
    /// time-at-price profile, distinct from the letter-based
    /// `tpo_sessions` TPO Market Profile chart type). When `Some(_)` and
    /// `tpo_profile_show == true` the renderer draws a right-anchored
    /// block histogram, styled like Volume Profile but with no letters.
    /// Maintained by `ChartContainer::ensure_tpo_profile_from_bars` in the
    /// prepare prepass.
    pub tpo_profile_overlay: Option<&'a crate::chart::tpo_profile::TpoProfileData>,
    /// Open Interest Δ bars for the OI Delta sub-pane renderer (C15).
    /// `None` when the toggle is off or no data has arrived yet.
    pub oi_delta_ref: Option<&'a [crate::types::OiDeltaBar]>,
    /// Funding Rate cells for the strip renderer (C15).
    /// `None` when the toggle is off or no data has arrived yet.
    pub funding_rate_ref: Option<&'a [crate::types::FundingRateCell]>,
    /// DOM Heatmap background overlay buffer (C12; chart-type → overlay
    /// migration). `None` when the `dom_heatmap_show` window flag is off or
    /// no data has arrived yet — mirrors `oi_delta_ref`/`funding_rate_ref`.
    pub dom_heatmap_ref: Option<&'a crate::chart::dom_heatmap::DomHeatmapBuffer>,
    /// DOM Heatmap trade-circle overlay (C12). Same gate as `dom_heatmap_ref`.
    pub dom_trade_dots_ref: Option<&'a [crate::chart::dom_heatmap::DomTradeDot]>,
    /// Realized-liquidation tape (prints + the levels they fold into).
    /// `None` when the overlay instance is absent or nothing has landed.
    pub liq_tape_ref: Option<&'a crate::chart::liquidations::LiquidationTape>,
    /// How the liquidation layer reads — every knob a parameter on the
    /// `overlay_liquidation_heatmap` instance, never a constant.
    pub liq_ink: LiquidationInk,
    /// Projected liquidation levels (`overlay_liquidation_projection`), or
    /// `None` when that overlay is off / has no open-interest series yet.
    pub liq_projection_ref: Option<&'a [crate::chart::liquidation_projection::ProjectedLevel]>,
    /// How the projected layer reads.
    pub liq_projection_ink: LiquidationProjectionInk,
    /// DOM Heatmap ink settings, read off the `overlay_dom_heatmap`
    /// `IndicatorInstance` (catalog defaults when no instance exists).
    /// Unused (but always populated) when `dom_heatmap_ref` is `None`.
    pub dom_params: DomHeatmapInk,
    /// True when the chart type is non-temporal (renko / pnf / kagi /
    /// three_line_break). These types render on a derived series whose
    /// `bars` shadow carries each element's real open timestamp but no
    /// fixed calendar cadence — the X-axis must resolve calendar boundaries
    /// only to real elements and never extrapolate synthetic future/past
    /// timestamps. Forwarded to `TimeScale::generate_ticks`, which routes
    /// `true` through the auto calendar grid (round boundaries snapped to
    /// the nearest real element, falling back to the raw per-element picker
    /// only when the calendar grid finds no usable step).
    pub non_temporal_axis: bool,
    /// True for the counted-bar family (range/tick/volume/dollar/tick-imbalance/
    /// volume-imbalance/run bars). Historically hid the vertical time grid
    /// (non-uniform per-bar spacing had no calendar cadence to draw a grid
    /// from); `TimeScale::generate_ticks` now routes this family through the
    /// same auto calendar grid as kline charts, so `mlc-app-layout::grid`
    /// no longer reads this field to gate the grid — kept as chart-type
    /// registry metadata (`ChartTypeDef::hides_time_grid`,
    /// `active_series::chart_type_hides_time_grid`) for any future consumer
    /// that needs the "is this a counted-bar type" classification.
    pub hide_time_grid: bool,
    /// History depth backing the bubble's active series (hybrid-data-modes-
    /// plan-2026-07-08, Step 6) — mirrors `mlc_backend::bubble::
    /// HistoryDepth`, populated 1:1 by `build_chart_render_state`. Drives
    /// the degraded-history badge; `Full`/`Unknown` render nothing.
    pub history_depth: HistoryDepthDisplay,
    /// CVD Display=Candles overlay body-painter (owner spec 2026-07-11):
    /// per-bar delta (diff of the CVD indicator's cumulative per-bar
    /// output), index-aligned to `bars`. When `Some`, the candle-family
    /// draw fns recolor BODY + BORDER by the delta's sign (wick stays
    /// direction-colored); `NaN` entries fall back to direction colors.
    /// Populated by mlc-app only when the active chart type is
    /// `ChartTypeDef::delta_paintable` and a visible CVD instance has
    /// Display=Candles; `None` everywhere else — zero change to the
    /// default render.
    pub cvd_paint_deltas: Option<Vec<f64>>,
    /// Whether the venue actually publishes a bulk daily trade-archive for
    /// this instrument's account type (`TradeArchiveCapabilities::
    /// for_account(..).is_some()`) — resolved by `build_chart_render_state`
    /// via the data-bridge's connector hub. Gates the data-gap band's
    /// second "zip MM-DD..MM-DD pending" label line
    /// (`chart_render::grid::draw_data_gaps`): that line promises an
    /// archive day-file is on its way, which is only honest when the venue
    /// actually has one. Default `false` (no promise) — matches the
    /// "exchange not connected yet" / no-archive case.
    pub trade_archive_available: bool,
    /// True when the displayed kline series is TRADE-BUILT (owner report
    /// 2026-07-16) — folded client-side from the shared `TradeStore`
    /// because the active timeframe has no native venue source and no
    /// native divisor to aggregate from (`DataBubble::
    /// active_trade_built_kline` is `Some` for the rendered bubble). Its
    /// history is owned by the zip/trade plane exactly like the
    /// incremental-prepend fold family, so the data-gap band
    /// (`chart_render::grid::draw_data_gaps`) applies to it too — a hole in
    /// a trade-built series is a real missing-trades hole, not glued kline
    /// continuity. Default `false` (kline-fed types glue gaps and show no
    /// band, matching pre-existing behavior).
    pub bars_trade_built: bool,
}

impl<'a> ChartRenderState<'a> {
    /// True when there is ANY series data to paint — either kline `bars`, or
    /// a populated own-slot for a chart type that renders from a derived
    /// series (footprint / renko / PnF / Kagi / three-line-break / TPO).
    /// The kline-loading skeleton gate must NOT blank these chart types just
    /// because klines haven't landed yet — their primary data lives in the
    /// own-slot, not in `bars`. CVD is not in this list — it renders through
    /// the normal indicator sub-pane system, not `ChartRenderState`.
    ///
    /// DOM heatmap / liquidation heatmap are ALSO not in this list
    /// (chart-type → overlay migration) — they are background overlays on
    /// top of whatever real chart_type is active, never the sole series on
    /// the pane, so their presence must not suppress the loading skeleton
    /// for the underlying chart type.
    pub fn has_any_series_data(&self) -> bool {
        !self.bars.is_empty()
            // `Some(empty)` = footprint subscribed, seed in flight
            // (`DataBubble::mark_footprint_pending`) — render the pane
            // chrome + in-pane spinner, not the loading skeleton.
            || self.footprint_bars.is_some()
            || self.renko_bars.is_some_and(|s| !s.is_empty())
            || self.pnf_columns.is_some_and(|s| !s.is_empty())
            || self.kagi_segments.is_some_and(|s| !s.is_empty())
            || self.three_line_break.is_some_and(|s| !s.is_empty())
            || self.tpo_sessions.is_some_and(|s| !s.is_empty())
    }
}

/// Configuration for scale rendering
#[derive(Clone, Debug)]
pub struct ScaleConfig {
    /// Price scale width in pixels
    pub price_scale_width: f64,
    /// Time scale height in pixels
    pub time_scale_height: f64,
    /// Font size for scale labels
    pub font_size: f64,
    /// Crosshair label font size
    pub crosshair_font_size: f64,
}

impl Default for ScaleConfig {
    fn default() -> Self {
        Self {
            price_scale_width: crate::PRICE_SCALE_WIDTH,
            time_scale_height: crate::TIME_SCALE_HEIGHT,
            font_size: 11.0,
            crosshair_font_size: 11.0,
        }
    }
}

/// Extended theme for scales (includes scale-specific colors)
#[derive(Clone, Debug)]
pub struct ScaleTheme {
    /// Scale background color
    pub scale_bg: String,
    /// Scale border color
    pub scale_border: String,
    /// Scale text color
    pub scale_text: String,
    /// Scale text color (medium weight)
    pub scale_text_medium: String,
    /// Scale text color (muted)
    pub scale_text_muted: String,
    /// Crosshair label background
    pub crosshair_label_bg: String,
    /// Crosshair label background (styled with opacity)
    pub crosshair_label_bg_styled: String,
    /// Crosshair label text
    pub crosshair_label_text: String,
}

impl Default for ScaleTheme {
    fn default() -> Self {
        Self {
            scale_bg: "#1e222d".to_string(),
            scale_border: "#2a2e39".to_string(),
            scale_text: "#d1d4dc".to_string(),
            scale_text_medium: "#9598a1".to_string(),
            scale_text_muted: "#6a6d78".to_string(),
            crosshair_label_bg: "#363a45".to_string(),
            crosshair_label_bg_styled: "#363a45".to_string(),
            crosshair_label_text: "#d1d4dc".to_string(),
        }
    }
}

/// Configuration for crosshair rendering
#[derive(Clone, Debug)]
pub struct CrosshairConfig {
    /// Whether vertical line is visible
    pub vert_visible: bool,
    /// Vertical line width
    pub vert_width: f64,
    /// Vertical line style
    pub vert_style: LineStyle,
    /// Whether horizontal line is visible
    pub horz_visible: bool,
    /// Horizontal line width
    pub horz_width: f64,
    /// Horizontal line style
    pub horz_style: LineStyle,
}

impl Default for CrosshairConfig {
    fn default() -> Self {
        Self {
            vert_visible: true,
            vert_width: 1.0,
            vert_style: LineStyle::Dashed,
            horz_visible: true,
            horz_width: 1.0,
            horz_style: LineStyle::Dashed,
        }
    }
}

/// Legend data for rendering
#[derive(Clone, Debug)]
pub struct LegendData {
    /// Symbol name
    pub symbol: String,
    /// Timeframe label
    pub timeframe: String,
    /// Current bar data
    pub bar: Bar,
    /// Change from open
    pub change: f64,
    /// Percent change
    pub percent: f64,
    /// Is bullish
    pub is_bullish: bool,
}

/// Tooltip content lines
#[derive(Clone, Debug)]
pub struct TooltipLines {
    pub lines: Vec<String>,
}

impl TooltipLines {
    pub fn from_bar(bar: &Bar) -> Self {
        Self {
            lines: vec![
                format!("O: {}", Self::fmt_price(bar.open)),
                format!("H: {}", Self::fmt_price(bar.high)),
                format!("L: {}", Self::fmt_price(bar.low)),
                format!("C: {}", Self::fmt_price(bar.close)),
                format!("V: {:.0}", bar.volume),
            ],
        }
    }

    /// Format price with appropriate precision for tooltip display
    pub fn fmt_price(price: f64) -> String {
        if price == 0.0 {
            "0".to_string()
        } else if price.abs() < 1e-8 {
            // Scientific notation for extremely small prices
            let exp = price.abs().log10().floor() as i32;
            let mantissa = price / 10f64.powi(exp);
            format!("{:.2}e{}", mantissa, exp)
        } else if price.abs() < 0.01 {
            format!("{:.8}", price)
        } else if price.abs() < 1.0 {
            format!("{:.6}", price)
        } else if price.abs() < 100.0 {
            format!("{:.4}", price)
        } else if price.abs() < 10000.0 {
            format!("{:.2}", price)
        } else {
            format!("{:.2}", price)
        }
    }
}

/// Render-side price line configuration (distinct from `chart::annotations::price_line::PriceLine`)
#[derive(Clone, Debug)]
pub struct RenderPriceLine {
    pub price: f64,
    pub color: String,
    pub line_width: f64,
    pub line_style: LineStyle,
    pub title: String,
    pub visible: bool,
}

/// Render-side marker shape (distinct from `chart::annotations::markers::MarkerShape`)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RenderMarkerShape {
    Circle,
    Square,
    ArrowUp,
    ArrowDown,
}

/// Render-side chart marker (distinct from `chart::annotations::markers::Marker`)
#[derive(Clone, Debug)]
pub struct RenderMarker {
    pub bar_idx: usize,
    pub price: f64,
    pub shape: RenderMarkerShape,
    pub color: String,
    pub size: f64,
    pub text: Option<String>,
}

/// Sub-pane geometry and state
#[derive(Clone, Debug)]
pub struct PaneGeom {
    /// Pane rectangle (in screen coordinates)
    pub rect: ChartRect,
    /// Minimum value for Y-axis
    pub value_min: f64,
    /// Maximum value for Y-axis
    pub value_max: f64,
    /// Pane title
    pub title: String,
}

/// Histogram rendering style
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum HistogramStyle {
    /// Draw bars from bottom of pane
    #[default]
    FromBottom,
    /// Draw bars from zero line (for MACD-style)
    Centered,
}

/// Theme for pane rendering
#[derive(Clone, Debug)]
pub struct PaneTheme {
    pub background: String,
    pub separator: String,
    pub grid_line: String,
    pub text: String,
    pub scale_bg: String,
    pub scale_border: String,
    pub up_color: String,
    pub down_color: String,
}

impl Default for PaneTheme {
    fn default() -> Self {
        Self {
            background: "#131722".to_string(),
            separator: "#2a2e39".to_string(),
            grid_line: "#2a2e3980".to_string(),
            text: "#d1d4dc".to_string(),
            scale_bg: "#1e222d".to_string(),
            scale_border: "#2a2e39".to_string(),
            up_color: "#26a69a".to_string(),
            down_color: "#ef5350".to_string(),
        }
    }
}

/// Options for grid rendering
#[derive(Clone, Debug)]
pub struct GridRenderOptions {
    /// Line color
    pub color: String,
    /// Line width
    pub width: f64,
    /// Line style
    pub style: LineStyle,
    /// Whether grid is visible
    pub visible: bool,
}

impl Default for GridRenderOptions {
    fn default() -> Self {
        Self {
            color: "#2a2e39".to_string(),
            width: 1.0,
            style: LineStyle::Solid,
            visible: true,
        }
    }
}

/// Line rendering style
#[derive(Clone, Copy, Debug, Default)]
pub struct LineRenderStyle {
    /// Dash length (0 for solid)
    pub dash: f64,
    /// Gap length
    pub gap: f64,
}

impl LineRenderStyle {
    pub fn solid() -> Self {
        Self { dash: 0.0, gap: 0.0 }
    }

    pub fn dashed() -> Self {
        Self { dash: 8.0, gap: 4.0 }
    }

    pub fn dotted() -> Self {
        Self { dash: 2.0, gap: 2.0 }
    }

    pub fn large_dashed() -> Self {
        Self { dash: 12.0, gap: 6.0 }
    }

    pub fn sparse_dotted() -> Self {
        Self { dash: 2.0, gap: 8.0 }
    }

    pub fn from_line_style(style: LineStyle) -> Self {
        match style {
            LineStyle::Solid => Self::solid(),
            LineStyle::Dashed => Self::dashed(),
            LineStyle::Dotted => Self::dotted(),
            LineStyle::LargeDashed => Self::large_dashed(),
            LineStyle::SparseDotted => Self::sparse_dotted(),
        }
    }

    pub fn is_solid(&self) -> bool {
        self.dash <= 0.0
    }
}
