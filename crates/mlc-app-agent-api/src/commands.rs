//! Drain pending Agent API commands and dispatch them to chart state.
//!
//! Lifted from `mlc-app-shell-vello/src/agent/commands.rs` and rewritten over
//! `AgentHost` so the same dispatcher serves the GPU binary and the
//! headless backend.

use std::sync::Arc;

use mlc_backend_shell_http::AgentState;
use mlc_backend_shell_http::state::AgentCommand;
use mlc_backend_indicator_engine::IndicatorParamValue;

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
                    .map(|cw| cw.current_tag_id);
                let bar_count = target_bubble_id
                    .and_then(|bid| chart.with_bubble(bid, |b| b.bars.len()))
                    .unwrap_or(0);
                let Some(cw) = chart.panel_app.panel_grid.containers_mut().values_mut()
                    .find(|cw| cw.id.0 == chart_id) else {
                    eprintln!("[AgentCommand] chart not found: {} in window {}", chart_id, window_id);
                    continue;
                };
                cw.set_from_agent(mode.as_deref(), view_start, bar_spacing, bar_count);
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

                // The panel names WHERE the indicator was ordered; the bubble it
                // is tagged to names WHO owns it. Resolve the bubble once and
                // use it for both the input bars and the ownership binding —
                // `chart_id` is a panel id and is not interchangeable with a
                // `BubbleId` (passing it straight through bound the instance to
                // a foreign bubble, or to none at all when no bubble had that
                // numeric id).
                let bid = chart.panel_app.panel_grid.containers()
                    .values().find(|cw| cw.id.0 == chart_id)
                    .map(|cw| cw.current_tag_id);
                let bars: Vec<mlc_core::Bar> = bid
                    .and_then(|bid| chart.with_bubble(bid, |b| b.bars.clone()))
                    .unwrap_or_default();

                let new_id_opt = chart.core.indicators_mut().add(bid, &type_id);
                if let Some(new_id) = new_id_opt {
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
                        chart.core.indicators_mut().set_param(new_id, k, iv);
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

                let found = chart.core.indicators().exists(indicator_id);
                let wid_opt = chart.core.indicators().bubble_id_of(indicator_id).flatten();
                if found {
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
                        chart.core.indicators_mut().set_param(indicator_id, k, iv);
                    }
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
                if w.chart.core.indicators_mut().remove(indicator_id).is_some() {
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
                    let cw_bid = chart.panel_app.panel_grid.containers().values()
                        .find(|cw| cw.id.0 == chart_id)
                        .map(|cw| cw.current_tag_id);
                    if let Some(bid) = cw_bid {
                        chart.core.drawings_mut().add_external(bid, prim);
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
                let cw_bid = chart.panel_app.panel_grid.containers().values()
                    .find(|cw| cw.id.0 == chart_id)
                    .map(|cw| cw.current_tag_id);
                if let Some(bid) = cw_bid {
                    let mut dreg = chart.core.drawings_mut();
                    if let Some(mut data) = dreg.get_data(bid, primitive_id) {
                        if let Some(pts) = points {
                            let new_pts: Vec<(i64, f64)> = pts.iter().map(|p| (p[0] as i64, p[1])).collect();
                            dreg.set_points(bid, primitive_id, &new_pts);
                        }
                        if let Some(s) = style {
                            data.color = mlc_core::drawing::primitives_v2::PrimitiveColor {
                                stroke: s.color,
                                fill: s.fill_color,
                            };
                            data.width = s.width;
                            dreg.set_data(bid, primitive_id, &data);
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
                let cw_bid = chart.panel_app.panel_grid.containers().values()
                    .find(|cw| cw.id.0 == chart_id)
                    .map(|cw| cw.current_tag_id);
                let removed = if let Some(bid) = cw_bid {
                    chart.core.drawings_mut().delete(bid, primitive_id)
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

            // ── UI test hooks (Phase 7 — admin-tier automation) ────────
            AgentCommand::UiDispatch { window_id, hit_id } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] UiDispatch: window not found: {}", window_id);
                    continue;
                };
                // The cloud overlay (News / Download) opens from the window
                // chrome, not from a panel hit id — the chrome strip is drawn
                // by the shell and its clicks never reach dispatch_panel_click
                // (`mlc-app-input-shell/src/dispatch.rs`, ChromeHit::NewsButton).
                // `overlay:news` / `overlay:download` do here what that chrome
                // click does, so a test can reach the Forum tab.
                #[cfg(feature = "cloud")]
                if let Some(kind) = hit_id.strip_prefix("overlay:") {
                    use mlc_app_layout::ui::modal_settings::CloudOverlayKind;
                    let kind = match kind {
                        "news" => Some(CloudOverlayKind::News),
                        "download" => Some(CloudOverlayKind::Download),
                        _ => None,
                    };
                    if let Some(kind) = kind {
                        w.chart.panel_app.cloud_overlay_state.toggle(kind);
                        w.chart.panel_app.cloud_login_state.close();
                    }
                    eprintln!("[AgentCommand] UiDispatch: window={}, overlay={}", window_id, hit_id);
                    continue;
                }
                // (0.0, 0.0): no real click position — see
                // `ChartApp::dispatch_panel_click`'s own doc comment for the
                // precedent (`clock_popup_state.toggle(0.0, 0.0)`).
                w.chart.dispatch_panel_click(&hit_id, 0.0, 0.0);
                eprintln!("[AgentCommand] UiDispatch: window={}, hit_id={}", window_id, hit_id);
            }

            AgentCommand::UiSetText { window_id, field, text, response_tx } => {
                if !mlc_app::text_input::known_field_ids().contains(&field.as_str()) {
                    eprintln!("[AgentCommand] UiSetText: unknown field id: {}", field);
                    let _ = response_tx.send(Err(format!("unknown field id: {}", field)));
                    continue;
                }
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] UiSetText: window not found: {}", window_id);
                    let _ = response_tx.send(Err(format!("window not found: {}", window_id)));
                    continue;
                };
                let field_wid = uzor::WidgetId::from(field.as_str());
                w.chart.input_coordinator.borrow_mut().text_fields_mut().set_text(&field_wid, &text);
                eprintln!("[AgentCommand] UiSetText: window={}, field={}", window_id, field);
                let _ = response_tx.send(Ok(()));
            }
        }
    }
}
