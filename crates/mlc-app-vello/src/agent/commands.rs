//! Drain pending Agent API commands and dispatch them to chart state.

use crate::App;

impl App<'_> {
    /// Drain agent commands pushed by HTTP handlers and apply them to the
    /// chart state.  Called every frame from `about_to_wait()`.
    pub(crate) fn drain_agent_commands(&mut self) {
        let agent_state = match self.agent_state {
            Some(ref s) => s.clone(),
            None => return,
        };

        let commands = agent_state.drain_commands();
        if commands.is_empty() { return; }

        for cmd in commands {
            match cmd {
                mlc_agentic_server::state::AgentCommand::SetViewport {
                    window_id, chart_id, view_start, bar_spacing, mode,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        // Resolve bar_count via immutable bubble lookup first, then take mut on the container.
                        let target_bubble_id = pw.chart.panel_app.panel_grid.containers()
                            .values().find(|cw| cw.id.0 == chart_id)
                            .map(|cw| cw.current_bubble_id);
                        let bar_count = target_bubble_id
                            .and_then(|bid| pw.chart.managers.bubble.bubble(bid))
                            .map(|b| b.bars.len())
                            .unwrap_or(0);
                        if let Some(cw) = pw.chart.panel_app.panel_grid.containers_mut().values_mut()
                            .find(|cw| cw.id.0 == chart_id)
                        {
                            if let Some(mode_str) = &mode {
                                match mode_str.as_str() {
                                    "focus" => {
                                        if bar_count > 0 {
                                            let visible = if cw.viewport.bar_spacing > 0.0 {
                                                (cw.viewport.chart_width / cw.viewport.bar_spacing).ceil() as usize
                                            } else {
                                                1
                                            };
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
                        } else {
                            eprintln!("[AgentCommand] chart not found: {} in window {}", chart_id, window_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::SwitchSymbol {
                    window_id, chart_id, symbol, exchange, timeframe, account_type,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        if !timeframe.is_empty() {
                            pw.chart.change_timeframe(&timeframe);
                        }
                        pw.chart.apply_symbol(
                            mlc_core::state::ChartId(chart_id),
                            &symbol, &exchange, &account_type,
                        );
                        eprintln!(
                            "[AgentCommand] SwitchSymbol: window={}, chart={}, symbol={}/{}/{} acct={}",
                            window_id, chart_id, exchange, symbol, timeframe, account_type,
                        );
                    } else {
                        eprintln!("[AgentCommand] SwitchSymbol: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::SwitchChartType {
                    window_id, chart_id: _, chart_type,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        pw.chart.change_chart_type(&chart_type);
                        eprintln!("[AgentCommand] SwitchChartType: window={}, type={}", window_id, chart_type);
                    } else {
                        eprintln!("[AgentCommand] SwitchChartType: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::SwitchTimeframe {
                    window_id, chart_id: _, timeframe_id,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        pw.chart.change_timeframe(&timeframe_id);
                        eprintln!("[AgentCommand] SwitchTimeframe: window={}, tf={}", window_id, timeframe_id);
                    } else {
                        eprintln!("[AgentCommand] SwitchTimeframe: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::SwitchPreset {
                    window_id, preset_id,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        pw.chart.load_preset(&preset_id);
                        eprintln!("[AgentCommand] SwitchPreset: window={}, preset={}", window_id, preset_id);
                    } else {
                        eprintln!("[AgentCommand] SwitchPreset: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::SetInternalLayout {
                    window_id, kind,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        if pw.chart.set_internal_layout(&kind) {
                            eprintln!("[AgentCommand] SetInternalLayout: window={}, kind={}", window_id, kind);
                        } else {
                            eprintln!("[AgentCommand] SetInternalLayout: unknown kind '{}'", kind);
                        }
                    } else {
                        eprintln!("[AgentCommand] SetInternalLayout: window not found: {}", window_id);
                    }
                }

                // ── Indicator CRUD ──────────────────────────────────────
                mlc_agentic_server::state::AgentCommand::AddIndicator {
                    window_id, chart_id, type_id, params, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        let bars: Vec<mlc_core::Bar> = pw.chart.panel_app.panel_grid.containers()
                            .values().find(|cw| cw.id.0 == chart_id)
                            .and_then(|cw| pw.chart.managers.bubble.bubble(cw.current_bubble_id))
                            .map(|b| b.bars.clone())
                            .unwrap_or_default();

                        if let Some(new_id) = pw.chart.indicator_manager.create_instance(&type_id) {
                            pw.chart.indicator_manager.assign_window(new_id, Some(chart_id));
                            if let Some(inst) = pw.chart.indicator_manager.get_instance_mut(new_id) {
                                // Apply custom params
                                for (k, v) in &params {
                                    use mlc_indicators::IndicatorParamValue;
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
                            pw.chart.indicator_manager.calculate(new_id, &bars);
                            pw.chart.sync_sub_panes_from_manager();
                            eprintln!("[AgentCommand] AddIndicator: type={}, id={}, chart={}", type_id, new_id, chart_id);
                        } else {
                            eprintln!("[AgentCommand] AddIndicator: unknown type_id '{}'", type_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] AddIndicator: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::UpdateIndicator {
                    window_id, chart_id: _, indicator_id, params, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        if let Some(inst) = pw.chart.indicator_manager.get_instance_mut(indicator_id) {
                            use mlc_indicators::IndicatorParamValue;
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
                            let wid = inst.window_id;
                            // Get bars from the window this indicator is bound to.
                            let bars: Vec<mlc_core::Bar> = wid
                                .and_then(|w| pw.chart.panel_app.panel_grid.containers()
                                    .values().find(|cw| cw.id.0 == w)
                                    .map(|cw| cw.current_bubble_id))
                                .and_then(|bid| pw.chart.managers.bubble.bubble(bid))
                                .map(|b| b.bars.clone())
                                .unwrap_or_default();
                            pw.chart.indicator_manager.calculate(indicator_id, &bars);
                            eprintln!("[AgentCommand] UpdateIndicator: id={}", indicator_id);
                        } else {
                            eprintln!("[AgentCommand] UpdateIndicator: instance not found: {}", indicator_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] UpdateIndicator: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::RemoveIndicator {
                    window_id, chart_id: _, indicator_id, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        if pw.chart.indicator_manager.remove_instance(indicator_id).is_some() {
                            pw.chart.sync_sub_panes_from_manager();
                            eprintln!("[AgentCommand] RemoveIndicator: id={}", indicator_id);
                        } else {
                            eprintln!("[AgentCommand] RemoveIndicator: not found: {}", indicator_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] RemoveIndicator: window not found: {}", window_id);
                    }
                }

                // ── Primitive CRUD ─────────────────────────────────────
                mlc_agentic_server::state::AgentCommand::AddPrimitive {
                    window_id, chart_id, type_id, points, style, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
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
                            let cw_pid = pw.chart.panel_app.panel_grid.containers().values()
                                .find(|cw| cw.id.0 == chart_id)
                                .map(|cw| cw.chart_panel_id);
                            if let Some(pid) = cw_pid {
                                if let Some(panel) = pw.chart.panels_store.chart_panel.get_mut(&pid) {
                                    panel.drawing_manager.add_external_primitive(prim);
                                    eprintln!("[AgentCommand] AddPrimitive: type={}, chart={}", type_id, chart_id);
                                }
                            } else {
                                eprintln!("[AgentCommand] AddPrimitive: chart not found: {}", chart_id);
                            }
                        } else {
                            drop(registry);
                            eprintln!("[AgentCommand] AddPrimitive: unknown type: {}", type_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] AddPrimitive: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::UpdatePrimitive {
                    window_id, chart_id, primitive_id, points, style, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        let cw_pid2 = pw.chart.panel_app.panel_grid.containers().values()
                            .find(|cw| cw.id.0 == chart_id)
                            .map(|cw| cw.chart_panel_id);
                        if let Some(pid) = cw_pid2 {
                            if let Some(panel) = pw.chart.panels_store.chart_panel.get_mut(&pid) {
                                let prim = panel.drawing_manager.primitives_mut()
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
                            }
                        } else {
                            eprintln!("[AgentCommand] UpdatePrimitive: chart not found: {}", chart_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] UpdatePrimitive: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::RemovePrimitive {
                    window_id, chart_id, primitive_id, agent_id: _,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        let removed = {
                            let cw_pid3 = pw.chart.panel_app.panel_grid.containers().values()
                                .find(|cw| cw.id.0 == chart_id)
                                .map(|cw| cw.chart_panel_id);
                            if let Some(pid) = cw_pid3 {
                                if let Some(panel) = pw.chart.panels_store.chart_panel.get_mut(&pid) {
                                    let idx = panel.drawing_manager.primitives()
                                        .iter().position(|p| p.data().id == primitive_id);
                                    if let Some(idx) = idx {
                                        panel.drawing_manager.remove(idx);
                                        true
                                    } else {
                                        false
                                    }
                                } else {
                                    false
                                }
                            } else {
                                false
                            }
                        };
                        if removed {
                            eprintln!("[AgentCommand] RemovePrimitive: id={}", primitive_id);
                        } else {
                            eprintln!("[AgentCommand] RemovePrimitive: not found: {} in chart {}", primitive_id, chart_id);
                        }
                    } else {
                        eprintln!("[AgentCommand] RemovePrimitive: window not found: {}", window_id);
                    }
                }

                // ── Screenshot (Phase 5) ───────────────────────────────
                mlc_agentic_server::state::AgentCommand::RequestScreenshot {
                    window_id, chart_id, agent_id: _, response_tx,
                } => {
                    if let Some(pw) = self.windows.values_mut().find(|pw| pw.window_id == window_id) {
                        pw.pending_agent_screenshots.push((chart_id, response_tx));
                        // Request a redraw so the screenshot is captured this frame.
                        pw.window.request_redraw();
                        eprintln!("[AgentCommand] RequestScreenshot queued: window={}, chart={}", window_id, chart_id);
                    } else {
                        let _ = response_tx.send(Err(format!("window not found: {}", window_id)));
                        eprintln!("[AgentCommand] RequestScreenshot: window not found: {}", window_id);
                    }
                }

                mlc_agentic_server::state::AgentCommand::CreateKey { label, tier } => {
                    eprintln!("[AgentCommand] CreateKey ignored (auth removed): label={}, tier={}", label, tier);
                }

                mlc_agentic_server::state::AgentCommand::DeleteKey { label } => {
                    eprintln!("[AgentCommand] DeleteKey ignored (auth removed): label={}", label);
                }
            }
        }
    }

}
