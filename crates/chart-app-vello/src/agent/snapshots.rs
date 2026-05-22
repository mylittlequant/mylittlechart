//! Build read-only snapshots of chart state for the Agent API.

use crate::App;

impl App<'_> {
    /// Build and push a [`TerminalSnapshot`] from all open windows.
    ///
    /// Called at most once per second from `about_to_wait()` alongside
    /// `update_indicator_snapshot`.  Captures window/tab/chart/layout
    /// structure without computed values.
    pub(crate) fn update_terminal_snapshot(&self, agent_state: &zengeld_server::AgentState) {
        use std::collections::HashMap as StdHashMap;
        use zengeld_server::state::{
            TerminalSnapshot, WindowSnapshot, TabSnapshot, ChartSnapshot,
            ViewportSnapshot, IndicatorSummary, PrimitiveSummary, LayoutNode,
        };

        // Recursive helper: convert a PanelNode into a LayoutNode.
        fn build_layout_node(
            node: &uzor::panels::PanelNode<zengeld_chart::ChartSubPanel>,
            leaf_to_chart: &StdHashMap<uzor::panels::LeafId, zengeld_chart::ChartId>,
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

        let mut snap = TerminalSnapshot::default();

        for pw in self.windows.values() {
            // Build preset-id → name lookup.
            let preset_name: StdHashMap<&str, &str> = pw.chart.panel_app.presets
                .iter()
                .map(|(id, p)| (id.as_str(), p.name.as_str()))
                .collect();

            // Tabs
            let active_tab_id = pw.chart.panel_app.active_preset_id.clone();
            let tabs: Vec<TabSnapshot> = pw.chart.panel_app.open_tabs
                .iter()
                .map(|pid| TabSnapshot {
                    name: preset_name.get(pid.as_str()).unwrap_or(&"").to_string(),
                    active: *pid == active_tab_id,
                    preset_id: pid.clone(),
                })
                .collect();

            // Build leaf→ChartId map for layout tree.
            let leaf_to_chart: StdHashMap<uzor::panels::LeafId, zengeld_chart::ChartId> =
                pw.chart.panel_app.panel_grid
                    .iter_windows()
                    .map(|(lid, w)| (lid, w.id))
                    .collect();

            // Charts
            let charts: Vec<ChartSnapshot> = pw.chart.panel_app.panel_grid
                .iter_windows()
                .map(|(leaf_id, cw)| {
                    let viewport = &cw.viewport;
                    let bars_visible = if viewport.bar_spacing > 0.0 {
                        (viewport.chart_width / viewport.bar_spacing).ceil() as usize
                    } else {
                        0
                    };

                    let indicators: Vec<IndicatorSummary> = pw.chart.indicator_manager
                        .instances_iter()
                        .filter(|inst| inst.window_id == Some(cw.id.0))
                        .map(|inst| IndicatorSummary {
                            id: inst.id,
                            type_id: inst.type_id.clone(),
                            name: inst.name.clone(),
                        })
                        .collect();

                    let primitives: Vec<PrimitiveSummary> = cw.drawing_manager
                        .primitives()
                        .iter()
                        .map(|p| {
                            let d = p.data();
                            PrimitiveSummary { id: d.id, type_id: d.type_id.clone() }
                        })
                        .collect();

                    ChartSnapshot {
                        chart_id: cw.id.0,
                        leaf_id: leaf_id.0,
                        symbol: cw.symbol.clone(),
                        exchange: cw.exchange.clone(),
                        timeframe: cw.timeframe.name.clone(),
                        bar_count: cw.bars.len(),
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
                    }
                })
                .collect();

            // Layout tree from docking root.
            let root = pw.chart.panel_app.panel_grid.docking().tree().root();
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
                window_id: pw.window_id.clone(),
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

    /// Called at most once per second from `about_to_wait()` to avoid
    /// per-frame allocations.  Iterates all per-window indicator managers
    /// and collects instance metadata + computed output series.
    pub(crate) fn update_indicator_snapshot(&self, agent_state: &zengeld_server::AgentState) {
        use zengeld_server::state::{IndicatorSnapshot, IndicatorInstanceSnapshot, IndicatorOutputSnapshot};
        use zengeld_terminal_indicators::IndicatorParamValue;

        let mut snapshot = IndicatorSnapshot::default();

        for pw in self.windows.values() {
            // Map chart_id → symbol for this window so we can attribute
            // each indicator instance to whatever its window currently displays.
            let chart_symbols: std::collections::HashMap<u64, String> = pw
                .chart
                .panel_app
                .panel_grid
                .windows()
                .iter()
                .map(|(cid, cw)| (cid.0, cw.symbol.clone()))
                .collect();
            for inst in pw.chart.indicator_manager.instances_iter() {
                // Convert params: IndicatorParamValue → serde_json::Value
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

                // Convert computed output series (HashMap<String, Vec<f64>>)
                let outputs: Vec<IndicatorOutputSnapshot> = inst
                    .values
                    .iter()
                    .map(|(name, vals)| IndicatorOutputSnapshot {
                        name: name.clone(),
                        values: vals.clone(),
                    })
                    .collect();

                let resolved_symbol = inst
                    .window_id
                    .and_then(|w| chart_symbols.get(&w).cloned())
                    .unwrap_or_default();
                let instance_snap = IndicatorInstanceSnapshot {
                    id: inst.id,
                    type_id: inst.type_id.clone(),
                    type_name: inst.name.clone(),
                    symbol: resolved_symbol.clone(),
                    window_id: inst.window_id,
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

    /// Build and push a [`WatchlistSnapshot`] from the current AppState.
    ///
    /// Called at most once per second from `about_to_wait()`.
    pub(crate) fn update_watchlist_snapshot(&self, agent_state: &zengeld_server::AgentState) {
        use zengeld_server::state::{WatchlistSnapshot, WatchlistEntry, WatchlistItemEntry};

        let wm = &self.app_state.watchlist_manager;
        let active_id = wm.active_list_id;

        let watchlists: Vec<WatchlistEntry> = wm.lists.iter().map(|list| {
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

    /// Build and push a [`ConnectorSnapshot`] from the live-data bridge.
    ///
    /// Called at most once per second from `about_to_wait()`.
    pub(crate) fn update_connector_snapshot(&self, agent_state: &zengeld_server::AgentState) {
        use zengeld_server::state::{ConnectorSnapshot, ConnectorEntry};

        let metrics = self.bridge.collect_metrics();

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
}
