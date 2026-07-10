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
    // Kline re-cut (2026-07-08 minimal-sufficient-source doctrine) — render
    // as standard candles, same as volume_bar/dollar_bar below.
    ("range_bar", DrawOps { draw: crate::chart_render::draw_candles }),
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
    // "dom_heatmap" / "liquidation_heatmap" REMOVED (chart-type → overlay
    // migration) — `draw_dom_heatmap_conditional`/`draw_liquidation_heatmap_conditional`
    // are now called unconditionally (they no-op when their ring-buffer ref
    // is absent) from `render_chart::render_chart`'s overlay pass, BEFORE
    // `draw_series`, so the heat layers paint UNDER the main series on ANY
    // chart type — not routed through this per-chart-type table anymore.
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

    /// Cross-concern completeness (chart-type-engine-consolidation B4, T2.1):
    /// every registry row resolves DrawOps (already pinned above), AND every
    /// `TradeAggBar`/`PriceActionTransform` row (the two kinds whose series
    /// is computed by a client-side fold machine) carries a concrete
    /// `machine` and `identity` — never `MachineKind::None`/`IdentityKey::None`.
    /// `OrderflowSlotted` and `RenderOnly` are exempt: `RenderOnly` has no
    /// fold machine by design (see `registry.rs::render_only_types_have_no_identity`),
    /// and `OrderflowSlotted` is pinned separately in that same test module.
    #[test]
    fn trade_agg_and_price_action_rows_have_machine_and_identity() {
        use mlc_core::chart_type::{ChartTypeKind, IdentityKey, MachineKind};
        for def in mlc_core::chart_type::all() {
            assert!(draw_ops_by_id(def.id).is_some(), "no DrawOps for chart_type {}", def.id);
            if matches!(def.kind, ChartTypeKind::TradeAggBar | ChartTypeKind::PriceActionTransform) {
                assert_ne!(def.machine, MachineKind::None, "{} must have a concrete machine", def.id);
                assert_ne!(def.identity, IdentityKey::None, "{} must have a concrete identity", def.id);
            }
        }
    }
}
