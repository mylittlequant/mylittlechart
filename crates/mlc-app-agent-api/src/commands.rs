//! Drain pending Agent API commands and dispatch them to chart state.
//!
//! Lifted from `mlc-app-shell-vello/src/agent/commands.rs` and rewritten over
//! `AgentHost` so the same dispatcher serves the GPU binary and the
//! headless backend.

use std::sync::Arc;

use rand::rngs::OsRng;
use rand::RngCore;

use mlc_backend_shell_http::auth::{agent_keys_paths, AgentKeyRecord};
use mlc_backend_shell_http::AgentState;
use mlc_backend_shell_http::state::{
    AgentCommand, AlertAddressDto, AlertConditionDto, AlertListEntry, TestAlertDeliveryError,
};
use mlc_backend_indicator_engine::IndicatorParamValue;

use crate::host::AgentHost;

/// Milliseconds since the Unix epoch — used only to timestamp a freshly
/// minted [`AgentKeyRecord`]. Plain `std::time::SystemTime`, not
/// `web_time`: this crate is never compiled for wasm32 (see the crate doc /
/// `mlc-app-shell-web`'s Cargo.toml — the Agent API is native/headless
/// only today).
fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// True when `chart`'s active tab is a read-only shared chart
/// (`ChartApp::is_readonly_tab`, native-share-viewer arc 2026-09-06).
///
/// Checked before every command below that would mutate the active
/// preset's own content — indicator/primitive CRUD, symbol/timeframe/
/// chart-type switches, and `UiDispatch`'s generic click path. This sits
/// ON TOP OF (not instead of) the gate `ChartApp` already enforces on its
/// own mouse-click path (`edit_indicator_recorded`/`edit_primitive_recorded`,
/// `mlc-app/src/input/mod.rs`): the commands here reach
/// `chart.core.indicators_mut()`/`drawings_mut()` directly and never pass
/// through those wrappers, so without this check a leaked bearer would be
/// the only thing standing between an agent and a "read-only" shared
/// chart's content — plan §5: "the gate lives at the `ChartApp` boundary,
/// holding even if a bearer leaks."
///
/// None of the commands guarded by this check carry a `response_tx` today
/// (all fire-and-forget `202`s), so a refusal always logs and drops; a
/// future guarded command with a synchronous response should send
/// `Err("read-only shared chart".to_string())` through it instead.
fn active_preset_is_readonly(chart: &mlc_app::ChartApp) -> bool {
    chart.is_readonly_tab(&chart.panel_app.active_preset_id)
}

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
                if active_preset_is_readonly(w.chart) {
                    eprintln!("[AgentCommand] SwitchSymbol refused: active tab is read-only (shared chart)");
                    continue;
                }
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
                if active_preset_is_readonly(w.chart) {
                    eprintln!("[AgentCommand] SwitchChartType refused: active tab is read-only (shared chart)");
                    continue;
                }
                w.chart.change_chart_type(&chart_type);
                eprintln!("[AgentCommand] SwitchChartType: window={}, type={}", window_id, chart_type);
            }

            AgentCommand::SwitchTimeframe { window_id, chart_id: _, timeframe_id } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] SwitchTimeframe: window not found: {}", window_id);
                    continue;
                };
                if active_preset_is_readonly(w.chart) {
                    eprintln!("[AgentCommand] SwitchTimeframe refused: active tab is read-only (shared chart)");
                    continue;
                }
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
                    if active_preset_is_readonly(w.chart) {
                        eprintln!("[AgentCommand] AddIndicator refused: active tab is read-only (shared chart)");
                        continue;
                    }
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
                    if active_preset_is_readonly(w.chart) {
                        eprintln!("[AgentCommand] UpdateIndicator refused: active tab is read-only (shared chart)");
                        continue;
                    }
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
                if active_preset_is_readonly(w.chart) {
                    eprintln!("[AgentCommand] RemoveIndicator refused: active tab is read-only (shared chart)");
                    continue;
                }
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
                if active_preset_is_readonly(w.chart) {
                    eprintln!("[AgentCommand] AddPrimitive refused: active tab is read-only (shared chart)");
                    continue;
                }
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
                if active_preset_is_readonly(w.chart) {
                    eprintln!("[AgentCommand] UpdatePrimitive refused: active tab is read-only (shared chart)");
                    continue;
                }
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
                if active_preset_is_readonly(w.chart) {
                    eprintln!("[AgentCommand] RemovePrimitive refused: active tab is read-only (shared chart)");
                    continue;
                }
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

            // Mint: refuse while the vault is sealed (same gate
            // `credentials.rs`'s "+ Add key" uses) — a key generated with no
            // `VaultKey` to encrypt the sidecar under would have nowhere
            // durable to live and would vanish on the next unlock's
            // `agent_keys.enc` reload. See `auth.rs`'s module doc for the
            // sidecar's storage shape.
            AgentCommand::CreateKey { label, tier, role } => {
                let Some(core) = agent_state.core() else {
                    eprintln!("[AgentCommand] CreateKey: no backend core attached, refusing '{}'", label);
                    continue;
                };
                let Some(vault_key) = core.user().vault_key() else {
                    eprintln!("[AgentCommand] CreateKey: vault is sealed, refusing to mint '{}'", label);
                    continue;
                };

                let mut raw_key = [0u8; 32];
                OsRng.fill_bytes(&mut raw_key);
                let raw_key_hex = hex::encode(raw_key);
                let fingerprint_hex = hex::encode(mlc_vault::crypto::fingerprint_sha256(&raw_key));

                let dir = mlc_core::active_profile_data_dir();
                let (_salt_path, vault_path) = agent_keys_paths(&dir);
                let mut records: Vec<AgentKeyRecord> =
                    mlc_vault::load_encrypted(&vault_key, &vault_path).unwrap_or_default();
                records.push(AgentKeyRecord {
                    fingerprint_hex,
                    label: label.clone(),
                    tier,
                    role,
                    created_at_ms: now_ms(),
                });

                if let Err(e) = mlc_vault::save_encrypted(&vault_key, &vault_path, &records) {
                    eprintln!("[AgentCommand] CreateKey: failed to save agent_keys.enc: {}", e);
                    continue;
                }
                if let Err(e) = agent_state.agent_key_store().unlock_with_key(&vault_key) {
                    eprintln!("[AgentCommand] CreateKey: minted '{}' but failed to refresh the RAM store: {}", label, e);
                    continue;
                }
                agent_state.set_last_minted_key(raw_key_hex);
                eprintln!("[AgentCommand] CreateKey: minted '{}' (tier={:?}, role={:?})", label, tier, role);
            }

            // Revoke: same load/remove/save/`unlock_with_key` dance as
            // mint, keyed by fingerprint rather than label (see the
            // `AgentCommand::DeleteKey` field doc).
            AgentCommand::DeleteKey { fingerprint_hex } => {
                let Some(core) = agent_state.core() else {
                    eprintln!("[AgentCommand] DeleteKey: no backend core attached, refusing '{}'", fingerprint_hex);
                    continue;
                };
                let Some(vault_key) = core.user().vault_key() else {
                    eprintln!("[AgentCommand] DeleteKey: vault is sealed, refusing to revoke '{}'", fingerprint_hex);
                    continue;
                };

                let dir = mlc_core::active_profile_data_dir();
                let (_salt_path, vault_path) = agent_keys_paths(&dir);
                let mut records: Vec<AgentKeyRecord> =
                    mlc_vault::load_encrypted(&vault_key, &vault_path).unwrap_or_default();
                let before = records.len();
                records.retain(|r| r.fingerprint_hex != fingerprint_hex);
                if records.len() == before {
                    eprintln!("[AgentCommand] DeleteKey: no key found with fingerprint '{}'", fingerprint_hex);
                    continue;
                }

                if let Err(e) = mlc_vault::save_encrypted(&vault_key, &vault_path, &records) {
                    eprintln!("[AgentCommand] DeleteKey: failed to save agent_keys.enc: {}", e);
                    continue;
                }
                if let Err(e) = agent_state.agent_key_store().unlock_with_key(&vault_key) {
                    eprintln!("[AgentCommand] DeleteKey: revoked '{}' but failed to refresh the RAM store: {}", fingerprint_hex, e);
                    continue;
                }
                eprintln!("[AgentCommand] DeleteKey: revoked '{}'", fingerprint_hex);
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
                // Read-only guard (native-share-viewer arc, 2026-09-06):
                // the overlay branch above already returned for chrome
                // News/Download toggles, and tab switching is its own
                // command (`AgentCommand::SwitchPreset`, never routed
                // through here) — so everything left reaching
                // `dispatch_panel_click` is a panel/toolbar click that CAN
                // edit the active preset (indicator/primitive settings,
                // drawing placement, ...), and none of it funnels through
                // `ChartApp`'s own `edit_indicator_recorded`/
                // `edit_primitive_recorded` wrappers from this entry point
                // any more reliably than the CRUD commands above do.
                if active_preset_is_readonly(w.chart) {
                    eprintln!("[AgentCommand] UiDispatch refused: active tab is read-only (shared chart), hit_id={}", hit_id);
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

            // ── Share viewer (native-share-viewer arc, 2026-09-06) ─────
            // Neither of these touches the window's CURRENT active preset
            // — `request_open_share`/`request_clone_share` only queue a
            // token for the shell to resolve into a NEW tab — so neither
            // needs the read-only guard the commands above carry.
            AgentCommand::OpenShare { window_id, token } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] OpenShare: window not found: {}", window_id);
                    continue;
                };
                w.chart.request_open_share(&token);
                eprintln!("[AgentCommand] OpenShare: window={}", window_id);
            }

            AgentCommand::CloneShare { window_id, token } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.iter_mut().find(|w| w.window_id == window_id) else {
                    eprintln!("[AgentCommand] CloneShare: window not found: {}", window_id);
                    continue;
                };
                w.chart.request_clone_share(&token);
                eprintln!("[AgentCommand] CloneShare: window={}", window_id);
            }

            // ── Alerts (Arc C3, alerts-account-wide-and-telegram-delivery-
            // 2026-09-06) — all four operate on the shared `Arc<BackendCore>`
            // (`agent_state.core()`), never a specific window: alerts are
            // account-wide, not window/chart-scoped. Only `TestAlertDelivery`
            // needs `host.windows_mut()`, to reach the UI-state mirror the
            // existing Vault-tab send-test path drains. ──────────────────
            AgentCommand::CreateAlert { address, condition, response_tx } => {
                let Some(core) = agent_state.core() else {
                    eprintln!("[AgentCommand] CreateAlert: no backend core attached");
                    let _ = response_tx.send(Err("no backend attached".to_string()));
                    continue;
                };
                let Some(account_id) = agent_state.alert_account_id() else {
                    eprintln!("[AgentCommand] CreateAlert: no backend core attached");
                    let _ = response_tx.send(Err("no backend attached".to_string()));
                    continue;
                };

                let alert_condition = match condition.direction.as_deref() {
                    Some("up") => mlc_core::alert::AlertCondition::CrossingUp,
                    Some("down") => mlc_core::alert::AlertCondition::CrossingDown,
                    _ => mlc_core::alert::AlertCondition::Crossing,
                };
                let mut spec = mlc_core::alert::AlertSpec::price_default();
                spec.source = mlc_core::alert::AlertSource::Price { symbol: address.symbol.clone() };
                spec.condition = alert_condition;
                spec.price = condition.price;
                spec.exchange = address.exchange.clone();
                spec.name = format!("{} {}", address.symbol, alert_condition.display_name());

                let id = core.alerts_mut().add_for_account(account_id, spec);

                // `AlertItem::from_spec` always stamps `AccountType::
                // default()` (Spot) for a Price-source alert regardless of
                // what `AlertSpec` carries — no create path has ever
                // threaded a real account type through (see `AlertItem::
                // targets_instrument`'s own doc). The subscription bubble
                // is therefore ensured on the SAME Spot address the stored
                // item actually carries, not on whatever `address.
                // account_type` the caller asked for — using the caller's
                // value here would silently subscribe the wrong bubble,
                // one `tick.rs`'s own evaluation walk would never resolve
                // this alert against. `account_type` is still accepted and
                // echoed on the wire for forward-compatibility with the
                // sync-item shape (plan §2/C3.4); it just is not honored by
                // creation yet.
                let instrument = mlc_core::instrument::InstrumentAddress::new(
                    address.exchange.clone(),
                    address.symbol.clone(),
                    mlc_core::instrument::AccountType::default(),
                );
                core.ensure_alert_bubble(&instrument);

                let items = core.alerts().items_all();
                let path = mlc_core::active_profile_data_dir().join("alerts.json");
                if let Err(e) = mlc_core::save_json(&path, &items, None) {
                    eprintln!("[AgentCommand] CreateAlert: failed to persist alerts.json: {}", e);
                }

                eprintln!(
                    "[AgentCommand] CreateAlert: id={} {}/{} price={}",
                    id, address.exchange, address.symbol, condition.price,
                );
                let _ = response_tx.send(Ok(id));
            }

            AgentCommand::ListAlerts { response_tx } => {
                let Some(core) = agent_state.core() else {
                    eprintln!("[AgentCommand] ListAlerts: no backend core attached");
                    let _ = response_tx.send(Err("no backend attached".to_string()));
                    continue;
                };
                let Some(account_id) = agent_state.alert_account_id() else {
                    eprintln!("[AgentCommand] ListAlerts: no backend core attached");
                    let _ = response_tx.send(Err("no backend attached".to_string()));
                    continue;
                };

                let entries: Vec<AlertListEntry> = core
                    .alerts()
                    .for_account(&account_id)
                    .iter()
                    .map(alert_item_to_list_entry)
                    .collect();
                let count = entries.len();
                let _ = response_tx.send(Ok(entries));
                eprintln!("[AgentCommand] ListAlerts: {} alert(s) for account", count);
            }

            AgentCommand::DeleteAlert { id, response_tx } => {
                let Some(core) = agent_state.core() else {
                    eprintln!("[AgentCommand] DeleteAlert: no backend core attached");
                    let _ = response_tx.send(Err("no backend attached".to_string()));
                    continue;
                };

                let removed = core.alerts_mut().remove_by_id(id);
                if removed {
                    let items = core.alerts().items_all();
                    let path = mlc_core::active_profile_data_dir().join("alerts.json");
                    if let Err(e) = mlc_core::save_json(&path, &items, None) {
                        eprintln!("[AgentCommand] DeleteAlert: failed to persist alerts.json: {}", e);
                    }
                }
                eprintln!("[AgentCommand] DeleteAlert: id={} removed={}", id, removed);
                let _ = response_tx.send(Ok(removed));
            }

            AgentCommand::TestAlertDelivery { response_tx } => {
                let mut windows = host.windows_mut();
                let Some(w) = windows.first_mut() else {
                    eprintln!("[AgentCommand] TestAlertDelivery: no window open");
                    let _ = response_tx.send(Err(TestAlertDeliveryError::NoWindow));
                    continue;
                };
                let chart: &mut mlc_app::ChartApp = &mut *w.chart;
                let alert_settings = &mut chart.panel_app.alert_settings_state;
                if !alert_settings.secrets_available {
                    eprintln!("[AgentCommand] TestAlertDelivery: vault sealed, refusing");
                    let _ = response_tx.send(Err(TestAlertDeliveryError::Sealed));
                    continue;
                }
                if alert_settings.notification_secrets.telegram.bot_token.is_empty() {
                    eprintln!("[AgentCommand] TestAlertDelivery: no bot token configured");
                    let _ = response_tx.send(Err(TestAlertDeliveryError::NoToken));
                    continue;
                }
                // Reuses the Vault tab's own send-test path
                // (`mlc-app-shell-vello/src/main.rs`'s Telegram test block,
                // which already watches this flag across every window) —
                // no Telegram call is duplicated here.
                alert_settings.tg_test_pending = true;
                eprintln!("[AgentCommand] TestAlertDelivery: queued Telegram test message");
                let _ = response_tx.send(Ok(()));
            }
        }
    }
}

/// Convert one account-wide `AlertItem` into the Agent API's wire shape —
/// field-compatible with `mylittlechart-server`'s `mlc-alert-daemon`
/// `alert_item.rs` (`{id, address{exchange,symbol,account_type}, condition{
/// kind, price, direction}, enabled, created_at}`) for the `price_cross`
/// case, plus `last_fired` for this endpoint's own operational use — extra
/// fields are tolerated by that daemon's internally-tagged `ConditionWire`
/// deserializer, so this stays forward-compatible with pushing the same
/// shape as an `alerts`-category sync item later (plan §2/C3.4).
fn alert_item_to_list_entry(alert: &mlc_core::alert::AlertItem) -> AlertListEntry {
    use mlc_core::alert::AlertCondition;

    let (kind, direction, price2, percentage): (&str, Option<&str>, Option<f64>, Option<f64>) =
        match alert.condition {
            AlertCondition::CrossingUp => ("price_cross", Some("up"), None, None),
            AlertCondition::CrossingDown => ("price_cross", Some("down"), None, None),
            AlertCondition::Crossing => ("price_cross", Some("either"), None, None),
            AlertCondition::GreaterThan => ("greater_than", None, None, None),
            AlertCondition::LessThan => ("less_than", None, None, None),
            AlertCondition::EnteringRange => ("entering_range", None, Some(alert.price2), None),
            AlertCondition::ExitingRange => ("exiting_range", None, Some(alert.price2), None),
            AlertCondition::InsideRange => ("inside_range", None, Some(alert.price2), None),
            AlertCondition::OutsideRange => ("outside_range", None, Some(alert.price2), None),
            AlertCondition::MovingUp => ("moving_up", None, None, Some(alert.percentage)),
            AlertCondition::MovingDown => ("moving_down", None, None, Some(alert.percentage)),
            AlertCondition::MovingUpOrDown => ("moving_up_or_down", None, None, Some(alert.percentage)),
        };

    AlertListEntry {
        id: alert.id,
        address: AlertAddressDto {
            exchange: alert.exchange().to_string(),
            symbol: alert.symbol().to_string(),
            account_type: alert.account_type().as_label().to_string(),
        },
        condition: AlertConditionDto {
            kind: kind.to_string(),
            price: alert.price,
            direction: direction.map(str::to_string),
            price2,
            percentage,
        },
        enabled: matches!(alert.status, mlc_core::alert::AlertStatus::Active),
        created_at: alert.created_at,
        last_fired: alert.last_triggered_at,
    }
}
