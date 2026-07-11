use mlc_core::chart::render_data::{ChartRect, ChartRenderState, ChartTheme, FootprintDisplayOptions, LineFamilyDisplayOptions, TpoDisplayOptions};
use mlc_core::chart::types::price_scale::PriceScale;
use mlc_core::chart::types::{Crosshair, GridOptions, Legend, TimeScale, Viewport};
use mlc_core::scale_settings::ScaleSettings;
use mlc_core::types::HaBar;
use mlc_core::chart::volume_profile::VolumeProfileData;
use mlc_core::chart::tpo_profile::TpoProfileData;
use mlc_core::TimeFormatSettings;

use super::data_bubble::DataBubble;

/// Window-side render inputs needed to build a `ChartRenderState`.
///
/// A narrow struct-of-refs instead of `&ChartContainer` — `mlc-backend`
/// cannot depend on `mlc-app` (which owns `ChartContainer`), so this
/// function takes only the handful of fields the render pass actually
/// reads.  The caller (`mlc-app`) builds this from its own `&ChartContainer`
/// at the call site (see `mlc-app::lib.rs` `render_state_for` callers).
pub struct ChartWindowRenderInputs<'a> {
    pub viewport: &'a Viewport,
    pub price_scale: &'a PriceScale,
    pub time_scale: &'a TimeScale,
    pub crosshair: &'a Crosshair,
    pub scale_settings: &'a ScaleSettings,
    /// Heikin Ashi memo cache (window-local, viewport-coupled).
    pub ha_cache: Option<&'a [HaBar]>,
    /// Volume Profile overlay, already gated by the window's `vp_show` flag.
    pub vp_overlay: Option<&'a VolumeProfileData>,
    /// TPO Profile overlay (item D), already gated by the window's
    /// `tpo_profile_show` flag — mirrors `vp_overlay`.
    pub tpo_profile_overlay: Option<&'a TpoProfileData>,
    /// OI Delta display gate (window-local view state).
    pub oi_show: bool,
    /// Funding Rate display gate (window-local view state).
    pub funding_show: bool,
    /// DOM Heatmap overlay display gate (window-local view state,
    /// chart-type → overlay migration — mirrors `oi_show`/`funding_show`).
    pub dom_heatmap_show: bool,
    /// Liquidation Heatmap overlay display gate (window-local view state,
    /// chart-type → overlay migration — mirrors `oi_show`/`funding_show`).
    pub liq_heatmap_show: bool,
    /// Footprint display toggles (ABS markers / volume text / heat
    /// intensity) — per-panel view state, mirrors `oi_show`/`funding_show`.
    pub footprint_display: FootprintDisplayOptions,
    /// Tick-bar body-opacity intensity toggle — per-panel view state,
    /// mirrors `footprint_display`.
    pub tick_intensity: bool,
    /// TPO display toggles (session-date header / POC / VA / single-prints
    /// / poor-HL / initial-balance / IB period count) — per-panel view
    /// state, mirrors `footprint_display`.
    pub tpo_display: TpoDisplayOptions,
    /// Simple line-family per-type display options (line/step-line/area/
    /// baseline widths + line_markers' marker style/size) — per-panel view
    /// state, mirrors `footprint_display`.
    pub line_family_display: LineFamilyDisplayOptions,
    /// Precomputed effective wick flag — `draw_candles`' `state.wick_enabled`
    /// gate. The caller resolves this ONCE, here, from the global "Wick"
    /// checkbox combined with the bar-family override: for range_bar /
    /// volume_bar / dollar_bar the effective flag is
    /// `instrument_wick_enabled && panel.bar_family_wicks` (those three
    /// render WITHOUT wicks by default — their open/high/low pins are
    /// chained-continuity artifacts, not genuine intrabar excursions);
    /// every other chart type keeps the plain `instrument_wick_enabled`
    /// value. Resolved at the render-state build boundary (this struct) —
    /// `chart_type` itself is NOT threaded into the renderer.
    pub wicks_effective: bool,
}

/// Whether `chart_type`'s registry row claims `bubble.bars` as its rendered
/// series (`SeriesVariant::Bars`) — i.e. a stale-generation stamp on this
/// bubble is a REAL bug, not a slot-series false positive.
///
/// Candle-family, and the trade-agg/counted-bar family (range_bar/
/// volume_bar/tick_bar/dollar_bar/tick_imbalance_bar/volume_imbalance_bar/
/// run_bar, which write their derived series directly into `bubble.bars`)
/// are `Bars`-owned. Slot-series chart types (renko/pnf/kagi/
/// three_line_break/tpo/footprint) legitimately KEEP klines in `bubble.bars`
/// while rendering their own slot (`bubble.renko_bars`/`.tpo_sessions`/…)
/// — that's normal, not stale, so they return `false` here. An
/// unregistered/unknown chart_type id also returns `false` — nothing to
/// compare against.
fn is_bars_owned_chart_type(chart_type: &str) -> bool {
    mlc_core::chart_type::by_id(chart_type)
        .map(|d| d.series_variant == mlc_core::chart_type::SeriesVariant::Bars)
        .unwrap_or(false)
}

/// Dev-only stale-array tripwire (K1 lifecycle groundwork, chart-type-
/// engine-consolidation B2). Compares `bid`'s bubble's stamped
/// [`mlc_core::chart_type::BarsGeneration`] against the chart_type the
/// bubble currently claims — a mismatch means `bubble.bars` was never
/// re-stamped for the ACTIVE chart_type (a `replace_bars`/`clear_bars` call
/// got skipped somewhere on the install/switch path), i.e. exactly the
/// stale-array class this brick exists to surface. `BarsGeneration::NONE`
/// (never stamped — bubble created but never folded/loaded) is not a
/// mismatch on its own; a genuinely fresh bubble is expected to start
/// there. Gated to `bubble.bars`-owned chart types — see
/// [`is_bars_owned_chart_type`].
///
/// Debug-only (native `debug_assertions` OR wasm dev builds, gated through
/// the existing `dlog!`/`debug_log_enabled` mechanism so release/prod stays
/// silent and zero-cost) — warns via `dlog!`, never panics: this is a
/// visibility tool, not an invariant enforcement (K1 brick, not yet built).
#[cfg(debug_assertions)]
pub fn debug_assert_generation(bubble: &DataBubble) {
    let gen = bubble.bars_generation;
    if gen == mlc_core::chart_type::BarsGeneration::NONE {
        return;
    }
    if !is_bars_owned_chart_type(bubble.chart_type) {
        return;
    }
    if gen.chart_type != bubble.chart_type {
        mlc_backend_data_bridge::dlog!(
            "[bars_generation] STALE ARRAY: bubble {:?} chart_type={} but bars stamped for generation chart_type={} (seq={}) — render snapshot may be showing the wrong series",
            bubble.id, bubble.chart_type, gen.chart_type, gen.seq
        );
    }
}

/// No-op in release builds — see the `debug_assertions` twin above.
#[cfg(not(debug_assertions))]
pub fn debug_assert_generation(_bubble: &DataBubble) {}

/// Build a `ChartRenderState` from window render inputs and the associated bubble.
///
/// Lifted out of `ChartContainer::to_render_state` so that `mlc-core` no
/// longer needs to reference `DataBubble` (which lives in `mlc-backend`)
/// — breaking the dependency cycle.  Step 4 (state lift) further narrowed
/// the `window` parameter from `&ChartContainer` (which now lives in
/// `mlc-app`) to [`ChartWindowRenderInputs`].
pub fn build_chart_render_state<'a>(
    window: ChartWindowRenderInputs<'a>,
    bubble: &'a DataBubble,
    chart_rect: ChartRect,
    theme: &'a ChartTheme,
    current_timeframe: Option<&'a str>,
    time_format_settings: Option<&'a TimeFormatSettings>,
    timeframe_interval_ms: Option<i64>,
    grid: &'a GridOptions,
    legend: &'a Legend,
) -> ChartRenderState<'a> {
    debug_assert_generation(bubble);
    ChartRenderState {
        viewport: window.viewport,
        price_scale: window.price_scale,
        time_scale: window.time_scale,
        bars: &bubble.bars,
        grid,
        crosshair: window.crosshair,
        legend,
        chart_rect,
        theme,
        time_ticks: None,
        current_timeframe,
        disable_clip: false,
        time_format_settings,
        timeframe_interval_ms,
        scale_settings: Some(window.scale_settings),
        body_enabled: true,
        border_enabled: true,
        wick_enabled: window.wicks_effective,
        use_prev_close: false,
        footprint_bars: bubble.footprint_bars.as_deref(),
        footprint_display: window.footprint_display,
        tick_intensity: window.tick_intensity,
        renko_bars: bubble.renko_bars.as_deref(),
        three_line_break: bubble.three_line_break.as_deref(),
        pnf_columns: bubble.pnf_columns.as_deref(),
        kagi_segments: bubble.kagi_segments.as_deref(),
        tpo_sessions: bubble.tpo_sessions.as_deref(),
        tpo_display: window.tpo_display,
        line_family_display: window.line_family_display,
        // ha_cache + vp_overlay + tpo_profile_overlay are window-local
        // derived caches (viewport-coupled).
        ha_cache: window.ha_cache,
        vp_overlay: window.vp_overlay,
        tpo_profile_overlay: window.tpo_profile_overlay,
        // Display gates (oi_show / funding_show) are per-window view state;
        // storage is unconditional on bubble.
        oi_delta_ref: if window.oi_show { bubble.oi_delta.as_deref() } else { None },
        funding_rate_ref: if window.funding_show { bubble.funding_rate.as_deref() } else { None },
        dom_heatmap_ref: if window.dom_heatmap_show { bubble.dom_heatmap.as_ref() } else { None },
        dom_trade_dots_ref: if window.dom_heatmap_show { bubble.dom_trade_dots.as_deref() } else { None },
        liq_heatmap_ref: if window.liq_heatmap_show { bubble.liq_heatmap.as_ref() } else { None },
        liq_dots_ref: if window.liq_heatmap_show { bubble.liq_dots.as_deref() } else { None },
        // Default false — flipped to true by the caller (`mlc-app::lib.rs`)
        // only when a non-temporal ts-shadow (renko/pnf/kagi/three_line_break)
        // is swapped into `bars` after this state is built.
        non_temporal_axis: false,
        // Default false — flipped to true by the caller (`mlc-app::lib.rs`)
        // for the counted-bar family (range/tick/volume/dollar/tick-imbalance/
        // volume-imbalance/run bars).
        hide_time_grid: false,
        // Direct 1:1 mirror of the bubble's active-series depth (hybrid-
        // data-modes-plan-2026-07-08, Step 6) — no caller override needed,
        // unlike non_temporal_axis/hide_time_grid which depend on chart_type
        // knowledge this fn doesn't have.
        history_depth: bubble.history_depth.to_display(),
        // Filled by mlc-app AFTER build (needs the indicators facade,
        // which this bubble-scoped builder deliberately has no access to).
        cvd_paint_deltas: None,
    }
}

#[cfg(test)]
mod tests {
    use super::is_bars_owned_chart_type;

    #[test]
    fn candles_is_bars_owned() {
        assert!(is_bars_owned_chart_type("candles"));
    }

    #[test]
    fn tick_bar_is_bars_owned() {
        // Trade-agg/counted-bar family writes its derived series directly
        // into `bubble.bars` — a generation mismatch here IS a real bug.
        assert!(is_bars_owned_chart_type("tick_bar"));
    }

    #[test]
    fn renko_is_not_bars_owned() {
        // Slot-series: keeps klines in `bubble.bars` while rendering
        // `bubble.renko_bars` — a chart_type/generation mismatch is expected,
        // not a bug.
        assert!(!is_bars_owned_chart_type("renko"));
    }

    #[test]
    fn tpo_is_not_bars_owned() {
        assert!(!is_bars_owned_chart_type("tpo"));
    }

    #[test]
    fn footprint_is_bars_owned() {
        // Footprint's `series_variant` is deliberately `Bars` (registry
        // doc comment: viewport bookkeeping piggybacks `bubble.bars`, and
        // `FootprintUpdate` re-stamps kline bars underneath the cluster
        // grid on every update) — it is NOT a slot-series false positive;
        // the stale-generation warning must stay real for it.
        assert!(is_bars_owned_chart_type("footprint"));
    }

    #[test]
    fn unknown_chart_type_is_not_bars_owned() {
        assert!(!is_bars_owned_chart_type("not_a_real_chart_type"));
    }
}
