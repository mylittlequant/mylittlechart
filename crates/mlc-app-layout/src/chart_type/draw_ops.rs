//! Render-plane `DrawOps` fn-pointer table for chart-type draw dispatch.
//!
//! Step 9 of the chart-type registry arc: replaces the `match chart_type`
//! block in `layout::render_chart::draw_series` with a static table lookup.
//! Draw functions stay exactly where they already lived
//! (`crate::chart_render::*`) — this module only names them.
//!
//! Data-plane ops (subscribe/clear_slot/abort) live in
//! `mlc-backend::chart_type::engine` — this table is render-plane only, and
//! deliberately cannot live in `mlc-core`/`mlc-backend` since it must name
//! `RenderContext`/`ChartRenderState` render-trait types (owner rule:
//! "рендер фн очевидно в апп").

use mlc_core::render::RenderContext;
use crate::chart_render::ChartRenderState;

/// Render-plane ops for a chart type: how to draw its series.
/// Data-plane ops (subscribe/clear_slot/abort) live in `mlc-backend::chart_type::engine`.
pub struct DrawOps {
    /// Draw the main series for this chart type into the given render context.
    ///
    /// Some chart types (dom_heatmap/liquidation_heatmap) render CONDITIONALLY —
    /// they need bubble ring-buffer state that may not be present. Those types'
    /// draw fns are no-op if the required state is absent
    /// (`draw_dom_heatmap_conditional` / `draw_liquidation_heatmap_conditional`
    /// check `state.dom_heatmap_ref` / `state.liq_heatmap_ref` internally).
    pub draw: fn(ctx: &mut dyn RenderContext, state: &ChartRenderState),
}

/// Static chart_type-id → [`DrawOps`] table, in registry order
/// (`mlc_core::chart_type::registry::all`).
static DRAWS: &[(&str, DrawOps)] = &[
    ("candles", DrawOps { draw: crate::chart_render::draw_candles }),
    ("bars", DrawOps { draw: crate::chart_render::draw_bars }),
    ("hollow_candles", DrawOps { draw: crate::chart_render::draw_hollow_candles }),
    ("heikin_ashi", DrawOps { draw: crate::chart_render::draw_heikin_ashi }),
    ("line", DrawOps { draw: crate::chart_render::draw_line_series }),
    ("step_line", DrawOps { draw: crate::chart_render::draw_step_line }),
    ("line_markers", DrawOps { draw: crate::chart_render::draw_line_with_markers }),
    ("area", DrawOps { draw: crate::chart_render::draw_area_series }),
    ("hlc_area", DrawOps { draw: crate::chart_render::draw_hlc_area }),
    ("baseline", DrawOps { draw: crate::chart_render::draw_baseline_series }),
    ("range_bar", DrawOps { draw: crate::chart_render::draw_range_bars }),
    ("volume_bar", DrawOps { draw: crate::chart_render::draw_volume_bars }),
    ("tick_bar", DrawOps { draw: crate::chart_render::draw_tick_bars }),
    // López de Prado quant bars (C9): output is OHLC BarPoint — render as
    // standard candles, same as the pre-Step-9 `draw_series` match arm.
    ("dollar_bar", DrawOps { draw: crate::chart_render::draw_candles }),
    ("tick_imbalance_bar", DrawOps { draw: crate::chart_render::draw_candles }),
    ("volume_imbalance_bar", DrawOps { draw: crate::chart_render::draw_candles }),
    ("run_bar", DrawOps { draw: crate::chart_render::draw_candles }),
    ("renko", DrawOps { draw: crate::chart_render::draw_renko }),
    ("pnf", DrawOps { draw: crate::chart_render::draw_pnf }),
    ("kagi", DrawOps { draw: crate::chart_render::draw_kagi }),
    ("three_line_break", DrawOps { draw: crate::chart_render::draw_three_line_break }),
    ("footprint", DrawOps { draw: crate::chart_render::draw_footprint }),
    ("tpo", DrawOps { draw: crate::chart_render::draw_tpo }),
    // DOM Heatmap (C12) / Liquidation Heatmap (C13): conditional on bubble
    // ring-buffer state — see `DrawOps::draw` doc comment above.
    ("dom_heatmap", DrawOps { draw: crate::chart_render::draw_dom_heatmap_conditional }),
    ("liquidation_heatmap", DrawOps { draw: crate::chart_render::draw_liquidation_heatmap_conditional }),
];

/// Look up the [`DrawOps`] for a chart_type id.
///
/// Returns `None` for unknown ids — callers fall back to `draw_candles`
/// (same default `draw_series` used before Step 9).
pub fn draw_ops_by_id(id: &str) -> Option<&'static DrawOps> {
    DRAWS.iter().find(|(k, _)| *k == id).map(|(_, v)| v)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_registry_id_has_draw_ops() {
        for def in mlc_core::chart_type::all() {
            assert!(draw_ops_by_id(def.id).is_some(), "no DrawOps for chart_type {}", def.id);
        }
    }

    #[test]
    fn draws_table_ids_unique() {
        let ids: Vec<&str> = DRAWS.iter().map(|(k, _)| *k).collect();
        let mut sorted = ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(ids.len(), sorted.len());
    }
}
