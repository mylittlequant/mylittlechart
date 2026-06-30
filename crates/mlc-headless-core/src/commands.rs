//! Drain pending Agent API commands and dispatch them to chart state.
//!
//! Lifted from `mlc-app-vello/src/agent/commands.rs` and rewritten over
//! `AgentHost` so the same dispatcher serves the GPU binary and the
//! headless backend.

use std::sync::Arc;

use mlc_agentic_server::AgentState;
use mlc_agentic_server::state::AgentCommand;
use mlc_indicators::IndicatorParamValue;

use crate::host::AgentHost;

/// Drain agent commands pushed by HTTP handlers and apply them to the chart
/// state held by `host`. Call this once per tick / frame.
pub fn drain_agent_commands<H: AgentHost>(host: &mut H, agent_state: &Arc<AgentState>) {
    let commands = agent_state.drain_commands();
    if commands.is_empty() { return; }

    for cmd in commands {
        match cmd {
            AgentCommand::SetViewport {
                window_id, chart_id, view_start, bar_spacing, mode,
            } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] window not found: {}", window_id);
                    continue;
                };
                let chart = &mut *w.chart;
                let target_bubble_id = chart.panel_app.panel_grid.containers()
                    .values().find(|cw| cw.id.0 == chart_id)
                    .map(|cw| cw.current_bubble_id);
                let bar_count = target_bubble_id
                    .and_then(|bid| chart.with_bubble(bid, |b| b.bars.len()))
                    .unwrap_or(0);
                let Some(cw) = chart.panel_app.panel_grid.containers_mut().values_mut()
                    .find(|cw| cw.id.0 == chart_id) else {
                    eprintln!("[AgentCommand] chart not found: {} in window {}", chart_id, window_id);
                    continue;
                };
                if let Some(mode_str) = &mode {
                    match mode_str.as_str() {
                        "focus" => {
                            if bar_count > 0 {
                                let visible = if cw.viewport.bar_spacing > 0.0 {
                                    (cw.viewport.chart_width / cw.viewport.bar_spacing).ceil() as usize
                                } else { 1 };
                                cw.viewport.view_start = (bar_count as f64) - (visible as f64);
                            }
                        }
                        "fit" => {
                            if bar_count > 0 && cw.viewport.chart_width > 0.0 {
                                cw.viewport.bar_spacing = cw.viewport.chart_width / bar_count as f64;
                                cw.viewport.view_start = 0.0;
                            }
                        }
                        _ => {}
                    }
                }
                if let Some(vs) = view_start { cw.viewport.view_start = vs; }
                if let Some(bs) = bar_spacing { cw.viewport.bar_spacing = bs; }
                eprintln!("[AgentCommand] SetViewport: window={}, chart={}", window_id, chart_id);
            }

            AgentCommand::SwitchSymbol {
                window_id, chart_id, symbol, exchange, timeframe, account_type,
            } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] SwitchSymbol: window not found: {}", window_id);
                    continue;
                };
                if !timeframe.is_empty() {
                    w.chart.change_timeframe(&timeframe);
                }
                w.chart.apply_symbol(
                    mlc_core::state::ChartId(chart_id),
                    &symbol, &exchange, &account_type,
                );
                eprintln!(
                    "[AgentCommand] SwitchSymbol: window={}, chart={}, symbol={}/{}/{} acct={}",
                    window_id, chart_id, exchange, symbol, timeframe, account_type,
                );
            }

            AgentCommand::SwitchChartType { window_id, chart_id: _, chart_type } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] SwitchChartType: window not found: {}", window_id);
                    continue;
                };
                w.chart.change_chart_type(&chart_type);
                eprintln!("[AgentCommand] SwitchChartType: window={}, type={}", window_id, chart_type);
            }

            AgentCommand::SwitchTimeframe { window_id, chart_id: _, timeframe_id } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] SwitchTimeframe: window not found: {}", window_id);
                    continue;
                };
                w.chart.change_timeframe(&timeframe_id);
                eprintln!("[AgentCommand] SwitchTimeframe: window={}, tf={}", window_id, timeframe_id);
            }

            AgentCommand::SwitchPreset { window_id, preset_id } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] SwitchPreset: window not found: {}", window_id);
                    continue;
                };
                w.chart.load_preset(&preset_id);
                eprintln!("[AgentCommand] SwitchPreset: window={}, preset={}", window_id, preset_id);
            }

            AgentCommand::SetInternalLayout { window_id, kind } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] SetInternalLayout: window not found: {}", window_id);
                    continue;
                };
                if w.chart.set_internal_layout(&kind) {
                    eprintln!("[AgentCommand] SetInternalLayout: window={}, kind={}", window_id, kind);
                } else {
                    eprintln!("[AgentCommand] SetInternalLayout: unknown kind '{}'", kind);
                }
            }

            // ── Indicator CRUD ──────────────────────────────────────
            AgentCommand::AddIndicator { window_id, chart_id, type_id, params, agent_id: _ } => {
                let chart_ptr: *mut mlc_app::ChartApp = {
                    let mut windows = host.windows_mut();
                    let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                        eprintln!("[AgentCommand] AddIndicator: window not found: {}", window_id);
                        continue;
                    };
                    w.chart as *mut _
                };
                // SAFETY: the host owns the ChartApp; we held a &mut to it just above
                // and the only reason we need a raw pointer is to break the lifetime tie
                // between `windows` (the Vec<WindowRefMut>) and `chart`.
                let chart: &mut mlc_app::ChartApp = unsafe { &mut *chart_ptr };

                let bars: Vec<mlc_core::Bar> = chart.panel_app.panel_grid.containers()
                    .values().find(|cw| cw.id.0 == chart_id)
                    .map(|cw| cw.current_bubble_id)
                    .and_then(|bid| chart.with_bubble(bid, |b| b.bars.clone()))
                    .unwrap_or_default();

                let new_id_opt = chart.core.indicators_mut().create_instance(&type_id);
                if let Some(new_id) = new_id_opt {
                    chart.core.indicators_mut().assign_window(new_id, Some(chart_id));
                    {
                        let mut im = chart.core.indicators_mut();
                        if let Some(inst) = im.get_instance_mut(new_id) {
                            for (k, v) in &params {
                                let iv = match v {
                                    serde_json::Value::Number(n) => {
                                        if let Some(i) = n.as_i64() {
                                            IndicatorParamValue::Int(i as i32)
                                        } else {
                                            IndicatorParamValue::Float(n.as_f64().unwrap_or(0.0))
                                        }
                                    }
                                    serde_json::Value::Bool(b) => IndicatorParamValue::Bool(*b),
                                    serde_json::Value::String(s) => IndicatorParamValue::String(s.clone()),
                                    _ => continue,
                                };
                                inst.set_param(k, iv);
                            }
                        }
                    }
                    chart.core.indicators_mut().calculate(new_id, &bars);
                    chart.sync_sub_panes_from_manager();
                    eprintln!("[AgentCommand] AddIndicator: type={}, id={}, chart={}", type_id, new_id, chart_id);
                } else {
                    eprintln!("[AgentCommand] AddIndicator: unknown type_id '{}'", type_id);
                }
            }

            AgentCommand::UpdateIndicator { window_id, chart_id: _, indicator_id, params, agent_id: _ } => {
                let chart_ptr: *mut mlc_app::ChartApp = {
                    let mut windows = host.windows_mut();
                    let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                        eprintln!("[AgentCommand] UpdateIndicator: window not found: {}", window_id);
                        continue;
                    };
                    w.chart as *mut _
                };
                // SAFETY: see AddIndicator above.
                let chart: &mut mlc_app::ChartApp = unsafe { &mut *chart_ptr };

                let (found, wid_opt) = {
                    let mut im = chart.core.indicators_mut();
                    if let Some(inst) = im.get_instance_mut(indicator_id) {
                        for (k, v) in &params {
                            let iv = match v {
                                serde_json::Value::Number(n) => {
                                    if let Some(i) = n.as_i64() {
                                        IndicatorParamValue::Int(i as i32)
                                    } else {
                                        IndicatorParamValue::Float(n.as_f64().unwrap_or(0.0))
                                    }
                                }
                                serde_json::Value::Bool(b) => IndicatorParamValue::Bool(*b),
                                serde_json::Value::String(s) => IndicatorParamValue::String(s.clone()),
                                _ => continue,
                            };
                            inst.set_param(k, iv);
                        }
                        (true, inst.bubble_id)
                    } else {
                        (false, None)
                    }
                };
                if found {
                    // Indicator ownership is by bubble — bars live on the
                    // bubble directly, no chart lookup needed.
                    let bars: Vec<mlc_core::Bar> = wid_opt
                        .and_then(|bid_raw| chart.with_bubble(
                            mlc_core::ids::BubbleId(bid_raw),
                            |b| b.bars.clone(),
                        ))
                        .unwrap_or_default();
                    chart.core.indicators_mut().calculate(indicator_id, &bars);
                    eprintln!("[AgentCommand] UpdateIndicator: id={}", indicator_id);
                } else {
                    eprintln!("[AgentCommand] UpdateIndicator: instance not found: {}", indicator_id);
                }
            }

            AgentCommand::RemoveIndicator { window_id, chart_id: _, indicator_id, agent_id: _ } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] RemoveIndicator: window not found: {}", window_id);
                    continue;
                };
                if w.chart.core.indicators_mut().remove_instance(indicator_id).is_some() {
                    w.chart.sync_sub_panes_from_manager();
                    eprintln!("[AgentCommand] RemoveIndicator: id={}", indicator_id);
                } else {
                    eprintln!("[AgentCommand] RemoveIndicator: not found: {}", indicator_id);
                }
            }

            // ── Primitive CRUD ─────────────────────────────────────
            AgentCommand::AddPrimitive { window_id, chart_id, type_id, points, style, agent_id: _ } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] AddPrimitive: window not found: {}", window_id);
                    continue;
                };
                let chart = &mut *w.chart;
                let pts: Vec<(i64, f64)> = points.iter().map(|p| (p[0] as i64, p[1])).collect();
                let color_str = style.color.clone();
                let registry = mlc_core::drawing::primitives_v2::PrimitiveRegistry::global().read().unwrap();
                if let Some(mut prim) = registry.create(&type_id, &pts, Some(&color_str)) {
                    prim.data_mut().color = mlc_core::drawing::primitives_v2::PrimitiveColor {
                        stroke: style.color,
                        fill: style.fill_color,
                    };
                    prim.data_mut().width = style.width;
                    drop(registry);
                    let cw_pid = chart.panel_app.panel_grid.containers().values()
                        .find(|cw| cw.id.0 == chart_id)
                        .map(|cw| cw.chart_panel_id);
                    if let Some(pid) = cw_pid {
                        chart.core.drawings_mut().entry_or_default(pid).add_external_primitive(prim);
                        eprintln!("[AgentCommand] AddPrimitive: type={}, chart={}", type_id, chart_id);
                    } else {
                        eprintln!("[AgentCommand] AddPrimitive: chart not found: {}", chart_id);
                    }
                } else {
                    drop(registry);
                    eprintln!("[AgentCommand] AddPrimitive: unknown type: {}", type_id);
                }
            }

            AgentCommand::UpdatePrimitive { window_id, chart_id, primitive_id, points, style, agent_id: _ } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] UpdatePrimitive: window not found: {}", window_id);
                    continue;
                };
                let chart = &mut *w.chart;
                let cw_pid = chart.panel_app.panel_grid.containers().values()
                    .find(|cw| cw.id.0 == chart_id)
                    .map(|cw| cw.chart_panel_id);
                if let Some(pid) = cw_pid {
                    let mut dreg = chart.core.drawings_mut();
                    let dm = dreg.entry_or_default(pid);
                    let prim = dm.primitives_mut()
                        .iter_mut().find(|p| p.data().id == primitive_id);
                    if let Some(prim) = prim {
                        if let Some(pts) = points {
                            let new_pts: Vec<(i64, f64)> = pts.iter().map(|p| (p[0] as i64, p[1])).collect();
                            prim.set_points(&new_pts);
                        }
                        if let Some(s) = style {
                            prim.data_mut().color = mlc_core::drawing::primitives_v2::PrimitiveColor {
                                stroke: s.color,
                                fill: s.fill_color,
                            };
                            prim.data_mut().width = s.width;
                        }
                        eprintln!("[AgentCommand] UpdatePrimitive: id={}", primitive_id);
                    } else {
                        eprintln!("[AgentCommand] UpdatePrimitive: primitive not found: {}", primitive_id);
                    }
                } else {
                    eprintln!("[AgentCommand] UpdatePrimitive: chart not found: {}", chart_id);
                }
            }

            AgentCommand::RemovePrimitive { window_id, chart_id, primitive_id, agent_id: _ } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] RemovePrimitive: window not found: {}", window_id);
                    continue;
                };
                let chart = &mut *w.chart;
                let cw_pid = chart.panel_app.panel_grid.containers().values()
                    .find(|cw| cw.id.0 == chart_id)
                    .map(|cw| cw.chart_panel_id);
                let removed = if let Some(pid) = cw_pid {
                    let mut dreg = chart.core.drawings_mut();
                    let dm = dreg.entry_or_default(pid);
                    let idx = dm.primitives()
                        .iter().position(|p| p.data().id == primitive_id);
                    if let Some(idx) = idx {
                        dm.remove(idx);
                        true
                    } else { false }
                } else { false };
                if removed {
                    eprintln!("[AgentCommand] RemovePrimitive: id={}", primitive_id);
                } else {
                    eprintln!("[AgentCommand] RemovePrimitive: not found: {} in chart {}", primitive_id, chart_id);
                }
            }

            // ── Screenshot — delegated to host ──────────────────────
            AgentCommand::RequestScreenshot { window_id, chart_id, agent_id: _, response_tx } => {
                host.request_screenshot(&window_id, chart_id, response_tx);
            }

            AgentCommand::CreateKey { label, tier } => {
                eprintln!("[AgentCommand] CreateKey ignored (auth removed): label={}, tier={}", label, tier);
            }

            AgentCommand::DeleteKey { label } => {
                eprintln!("[AgentCommand] DeleteKey ignored (auth removed): label={}", label);
            }
        }
    }
}
