//! Build read-only snapshots of chart state for the Agent API.
//!
//! Lifted from `mlc-app-vello/src/agent/snapshots.rs` and rewritten over
//! `AgentHost` so the same code drives both the GPU binary and the headless
//! backend.

use std::collections::HashMap as StdHashMap;

use mlc_agentic_server::AgentState;
use mlc_agentic_server::state::{
    TerminalSnapshot, WindowSnapshot, TabSnapshot, ChartSnapshot,
    ViewportSnapshot, IndicatorSummary, PrimitiveSummary, LayoutNode,
    DerivedSeriesCounts,
    IndicatorSnapshot, IndicatorInstanceSnapshot, IndicatorOutputSnapshot,
    WatchlistSnapshot, WatchlistEntry, WatchlistItemEntry,
    ConnectorSnapshot, ConnectorEntry,
};
use mlc_indicators::IndicatorParamValue;

use crate::host::AgentHost;

fn build_layout_node(
    node: &uzor::panels::PanelNode<mlc_core::state::Panel>,
    leaf_to_chart: &StdHashMap<uzor::panels::LeafId, mlc_core::ChartId>,
) -> LayoutNode {
    match node {
        uzor::panels::PanelNode::Leaf(leaf) => {
            let chart_id = leaf_to_chart.get(&leaf.id).map(|c| c.0).unwrap_or(0);
            LayoutNode::Leaf { chart_id, leaf_id: leaf.id.0 }
        }
        uzor::panels::PanelNode::Branch(branch) => {
            let axis = match branch.layout {
                uzor::panels::WindowLayout::SplitHorizontal => "horizontal",
                uzor::panels::WindowLayout::SplitVertical   => "vertical",
                _                                           => "grid",
            };
            LayoutNode::Split {
                axis: axis.to_string(),
                proportions: branch.proportions.clone(),
                children: branch.children
                    .iter()
                    .map(|c| build_layout_node(c, leaf_to_chart))
                    .collect(),
            }
        }
    }
}

/// Build the full terminal snapshot and store it on `agent_state`.
pub fn update_terminal_snapshot<H: AgentHost>(host: &H, agent_state: &AgentState) {
    let mut snap = TerminalSnapshot::default();

    for w in host.windows() {
        let chart = w.chart;

        let presets_guard = chart.core.presets();
        let preset_name: StdHashMap<&str, &str> = presets_guard
            .iter()
            .map(|(id, p)| (id.as_str(), p.name.as_str()))
            .collect();

        let active_tab_id = chart.panel_app.active_preset_id.clone();
        let tabs: Vec<TabSnapshot> = chart.panel_app.open_tabs
            .iter()
            .map(|pid| TabSnapshot {
                name: preset_name.get(pid.as_str()).unwrap_or(&"").to_string(),
                active: *pid == active_tab_id,
                preset_id: pid.clone(),
            })
            .collect();

        let leaf_to_chart: StdHashMap<uzor::panels::LeafId, mlc_core::ChartId> =
            chart.panel_app.panel_grid
                .iter_containers()
                .map(|(lid, cw)| (lid, cw.id))
                .collect();

        let chart_ids_for_snap: Vec<(mlc_core::LeafId, mlc_core::ChartId)> =
            chart.panel_app.panel_grid
                .iter_containers()
                .map(|(lid, cw)| (lid, cw.id))
                .collect();

        let charts: Vec<ChartSnapshot> = chart_ids_for_snap.iter()
            .filter_map(|(leaf_id, chart_id)| {
                let cw = chart.panel_app.panel_grid.container_for_leaf(*leaf_id)?;
                let viewport = &cw.viewport;
                let bars_visible = if viewport.bar_spacing > 0.0 {
                    (viewport.chart_width / viewport.bar_spacing).ceil() as usize
                } else {
                    0
                };

                let symbol = chart.symbol_for_chart(*chart_id)
                    .map(String::from).unwrap_or_default();
                let exchange = chart.exchange_for_chart(*chart_id)
                    .map(String::from).unwrap_or_default();
                let tf_name = chart.timeframe_for_chart(*chart_id)
                    .map(|tf| tf.name.clone()).unwrap_or_default();

                let cw_bid_raw = cw.current_bubble_id.0;
                let indicators: Vec<IndicatorSummary> = chart.core.indicators()
                    .instances_iter()
                    .filter(|inst| inst.bubble_id == Some(cw_bid_raw))
                    .map(|inst| IndicatorSummary {
                        id: inst.id,
                        type_id: inst.type_id.clone(),
                        name: inst.name.clone(),
                    })
                    .collect();

                let pid_snap = cw.chart_panel_id;
                let primitives: Vec<PrimitiveSummary> = {
                    let dreg_snap = chart.core.drawings();
                    dreg_snap.get(pid_snap)
                        .map(|dm| dm.primitives().iter()
                            .map(|p| {
                                let d = p.data();
                                PrimitiveSummary { id: d.id, type_id: d.type_id.clone() }
                            })
                            .collect())
                        .unwrap_or_default()
                };

                let (bar_count, derived) = chart.with_bubble(cw.current_bubble_id, |b| (b.bars.len(), DerivedSeriesCounts {
                    footprint_bars:      b.footprint_bars.as_ref().map(|v| v.len()).unwrap_or(0),
                    renko_bars:          b.renko_bars.as_ref().map(|v| v.len()).unwrap_or(0),
                    three_line_break:    b.three_line_break.as_ref().map(|v| v.len()).unwrap_or(0),
                    pnf_columns:         b.pnf_columns.as_ref().map(|v| v.len()).unwrap_or(0),
                    kagi_segments:       b.kagi_segments.as_ref().map(|v| v.len()).unwrap_or(0),
                    cvd_line:            b.cvd_line.as_ref().map(|v| v.len()).unwrap_or(0),
                    tpo_sessions:        b.tpo_sessions.as_ref().map(|v| v.len()).unwrap_or(0),
                    oi_delta:            b.oi_delta.as_ref().map(|v| v.len()).unwrap_or(0),
                    funding_rate:        b.funding_rate.as_ref().map(|v| v.len()).unwrap_or(0),
                    dom_heatmap_columns: b.dom_heatmap.as_ref().map(|h| h.bucket_count()).unwrap_or(0),
                    dom_trade_dots:      b.dom_trade_dots.as_ref().map(|v| v.len()).unwrap_or(0),
                    liq_heatmap_columns: b.liq_heatmap.as_ref().map(|h| h.bucket_count()).unwrap_or(0),
                    liq_dots:            b.liq_dots.as_ref().map(|v| v.len()).unwrap_or(0),
                })).unwrap_or_default();

                Some(ChartSnapshot {
                    chart_id: cw.id.0,
                    leaf_id: leaf_id.0,
                    symbol,
                    exchange,
                    timeframe: tf_name,
                    bar_count,
                    viewport: ViewportSnapshot {
                        view_start: viewport.view_start,
                        bar_spacing: viewport.bar_spacing,
                        chart_width: viewport.chart_width,
                        chart_height: viewport.chart_height,
                        bars_visible,
                    },
                    indicator_count: indicators.len(),
                    primitive_count: primitives.len(),
                    indicators,
                    primitives,
                    derived,
                })
            })
            .collect();

        let root = chart.panel_app.panel_grid.docking().tree().root();
        let layout = LayoutNode::Split {
            axis: match root.layout {
                uzor::panels::WindowLayout::SplitHorizontal => "horizontal",
                uzor::panels::WindowLayout::SplitVertical   => "vertical",
                _                                           => "grid",
            }.to_string(),
            proportions: root.proportions.clone(),
            children: root.children
                .iter()
                .map(|c| build_layout_node(c, &leaf_to_chart))
                .collect(),
        };

        snap.windows.push(WindowSnapshot {
            window_id: w.window_id.to_string(),
            tabs,
            active_tab_id,
            charts,
            layout,
        });
    }

    if let Ok(mut s) = agent_state.terminal_snapshot.write() {
        *s = snap;
    }
}

/// Build the indicator snapshot (instances + computed output series) and
/// store it on `agent_state`.
pub fn update_indicator_snapshot<H: AgentHost>(host: &H, agent_state: &AgentState) {
    let mut snapshot = IndicatorSnapshot::default();

    for w in host.windows() {
        let chart = w.chart;
        // Indicators are keyed by bubble; resolve the symbol per bubble via the
        // bubble's own `symbol` field so the snapshot is correct under the
        // bubble-ownership model even when no chart currently points at it.
        let bubble_symbols: std::collections::HashMap<u64, String> = chart
            .core
            .bubble()
            .bubbles()
            .map(|b| (b.id.0, b.symbol.clone()))
            .collect();

        let im = chart.core.indicators();
        for inst in im.instances_iter() {
            let params: std::collections::HashMap<String, serde_json::Value> = inst
                .params
                .iter()
                .map(|(k, v)| {
                    let json_val = match v {
                        IndicatorParamValue::Int(n)    => serde_json::Value::Number(serde_json::Number::from(*n)),
                        IndicatorParamValue::Float(f)  => serde_json::Number::from_f64(*f)
                            .map(serde_json::Value::Number)
                            .unwrap_or(serde_json::Value::Null),
                        IndicatorParamValue::Bool(b)   => serde_json::Value::Bool(*b),
                        IndicatorParamValue::String(s) => serde_json::Value::String(s.clone()),
                        IndicatorParamValue::Color(c)  => serde_json::Value::String(c.clone()),
                    };
                    (k.clone(), json_val)
                })
                .collect();

            let outputs: Vec<IndicatorOutputSnapshot> = inst
                .values
                .iter()
                .map(|(name, vals)| IndicatorOutputSnapshot {
                    name: name.clone(),
                    values: vals.clone(),
                })
                .collect();

            let resolved_symbol = inst
                .bubble_id
                .and_then(|b| bubble_symbols.get(&b).cloned())
                .unwrap_or_default();
            let instance_snap = IndicatorInstanceSnapshot {
                id: inst.id,
                type_id: inst.type_id.clone(),
                type_name: inst.name.clone(),
                symbol: resolved_symbol.clone(),
                bubble_id: inst.bubble_id,
                params,
                outputs,
            };

            snapshot
                .symbols
                .entry(resolved_symbol)
                .or_default()
                .push(instance_snap);
        }
    }

    if let Ok(mut snap) = agent_state.indicator_snapshot.write() {
        *snap = snapshot;
    }
}

/// Build the watchlist snapshot from the first window's ChartApp.
///
/// In a multi-window GPU host every window has its own ChartApp but the
/// watchlist manager is global (lives on the renderer's AppState). For the
/// headless backend each ChartApp carries its own sidebar watchlist —
/// reading the first window's is the natural projection.
pub fn update_watchlist_snapshot<H: AgentHost>(host: &H, agent_state: &AgentState) {
    let windows = host.windows();
    let Some(first) = windows.first() else {
        if let Ok(mut snap) = agent_state.watchlist_snapshot.write() {
            *snap = WatchlistSnapshot::default();
        }
        return;
    };

    let wm_guard = first.chart.core.watchlist();
    let active_id = wm_guard.active_list_id;

    let watchlists: Vec<WatchlistEntry> = wm_guard.lists.iter().map(|list| {
        let items: Vec<WatchlistItemEntry> = list.all_symbols().into_iter().map(|ws| {
            WatchlistItemEntry {
                symbol: ws.symbol.clone(),
                exchange: ws.exchange.clone(),
                category: String::new(),
            }
        }).collect();

        WatchlistEntry {
            id: list.id,
            name: list.name.clone(),
            active: list.id == active_id,
            items,
        }
    }).collect();

    if let Ok(mut snap) = agent_state.watchlist_snapshot.write() {
        *snap = WatchlistSnapshot { watchlists };
    }
}

/// Build the connector status snapshot from the bridge stored on
/// [`AgentState`].
pub fn update_connector_snapshot(agent_state: &AgentState) {
    let metrics = agent_state.bridge.collect_metrics();

    let connectors: Vec<ConnectorEntry> = metrics.into_iter().map(|(eid, stats, ws_count)| {
        ConnectorEntry {
            exchange_id: eid.as_str().to_string(),
            active: stats.http_requests > 0,
            ws_active: ws_count > 0,
            symbol_count: 0,
        }
    }).collect();

    if let Ok(mut snap) = agent_state.connector_snapshot.write() {
        *snap = ConnectorSnapshot { connectors };
    }
}
