//! Live follow and co-edit on the share token that already names the chart.
//!
//! Ops name one primitive or one indicator. Candles and the full preset are
//! not ops. While Live is on, a gesture op carries selection, an in-progress
//! drag, and an in-progress draw, and a viewport op carries pan and zoom
//! when that toggle is on. The author chart and an opened share tab attach
//! to the same token. A clone does not.

use std::collections::HashMap;

use mlc_core::drawing::PrimitiveSnapshot;
use mlc_core::preset::snapshots::{IndicatorSnapshot, OutputConfigSnapshot};

use crate::ChartApp;

#[derive(Clone, Debug)]
pub struct ShareStream {
    pub token: String,
    pub cursor: i64,
    /// Author's Live toggle. Off: cadence. On: the channel.
    pub live: bool,
    /// Pan/zoom rides the channel. Only meaningful while `live` is on.
    pub live_viewport: bool,
    pub baseline: bool,
    pub last_push_ms: f64,
    pub last_pull_ms: f64,
    /// Stream object id → last JSON applied or acknowledged.
    pub last: HashMap<String, String>,
    /// JSON captured for an in-flight push. Committed only after the post succeeds.
    pub pending: Option<HashMap<String, String>>,
    /// Local primitive id → stream object id.
    pub local_to_key: HashMap<u64, String>,
    pub key_to_local: HashMap<String, u64>,
    /// Last gesture phase sent on the edge socket.
    pub edge_phase: String,
    /// Last primitive JSON sent on the edge socket for the object under the hand.
    pub edge_point: String,
    /// Last gesture phase that was queued (`idle` / `select` / `drag` / `draw`).
    pub gesture_phase: String,
    pub gesture_sent: String,
    pub viewport_sent: String,
    /// Local id of the in-progress draw preview. Kept out of the object diff.
    pub preview_local: Option<u64>,
}

impl ShareStream {
    fn new(token: String) -> Self {
        Self {
            token,
            cursor: 0,
            live: false,
            live_viewport: false,
            baseline: false,
            last_push_ms: 0.0,
            last_pull_ms: 0.0,
            last: HashMap::new(),
            pending: None,
            local_to_key: HashMap::new(),
            key_to_local: HashMap::new(),
            edge_phase: String::new(),
            edge_point: String::new(),
            gesture_phase: String::new(),
            gesture_sent: String::new(),
            viewport_sent: String::new(),
            preview_local: None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct ShareWireOp {
    pub kind: String,
    pub object_id: String,
    pub body: String,
}

#[derive(Clone, Debug)]
pub struct ShareFollow {
    pub preset_id: String,
    pub token: String,
    pub can_send: bool,
}

#[derive(Clone, Debug)]
pub struct ShareInboundOp {
    pub seq: i64,
    pub kind: String,
    pub object_id: String,
    pub body: String,
}

impl ChartApp {
    /// The active tab's share channel, if it is a live share and not a clone.
    pub fn share_follow_target(&self) -> Option<ShareFollow> {
        let id = self.panel_app.active_preset_id.clone();
        if let Some(meta) = self.shared_chart_meta.get(&id) {
            if meta.kind != "live" {
                return None;
            }
            return Some(ShareFollow {
                preset_id: id,
                token: meta.token.clone(),
                can_send: meta.has_right("edit"),
            });
        }
        if self.is_readonly_tab(&id) {
            return None;
        }
        let token = self.panel_app.chart_share_tokens.get(&id)?.clone();
        Some(ShareFollow {
            preset_id: id,
            token,
            can_send: true,
        })
    }

    /// An edit grant arrived for the open share tab. Drawing is allowed.
    pub fn note_share_can_send(&mut self, preset_id: &str, can_send: bool) {
        let Some(meta) = self.shared_chart_meta.get_mut(preset_id) else {
            return;
        };
        if can_send {
            if !meta.rights.iter().any(|r| r == "edit") {
                meta.rights.push("edit".to_string());
            }
            self.readonly_preset_ids.remove(preset_id);
        }
    }

    /// Diff since the last send. The first call only records the baseline
    /// already stored in the preset, and returns nothing.
    pub fn share_ops_due(&mut self, now_ms: f64) -> Option<(String, Vec<ShareWireOp>)> {
        let follow = self.share_follow_target()?;
        if !follow.can_send {
            return None;
        }
        let preset_id = follow.preset_id.clone();
        let token = follow.token.clone();
        let stream = self.share_streams.entry(preset_id.clone()).or_insert_with(|| ShareStream::new(token.clone()));
        if stream.token != token {
            *stream = ShareStream::new(token.clone());
        }
        self.ensure_share_keys(&preset_id);
        let gesture = self.share_gesture(&preset_id);
        let phase_now = gesture.as_ref().map(|g| g.0.as_str()).unwrap_or("");
        let stream = self.share_streams.get(&preset_id)?;
        let phase_changed = stream.live && phase_now != stream.gesture_phase.as_str();
        let hot = stream.live && (phase_now == "drag" || phase_now == "draw");
        let gap = if !stream.live {
            8000.0
        } else if hot {
            100.0
        } else {
            400.0
        };
        if stream.pending.is_some() || (!phase_changed && now_ms - stream.last_push_ms < gap && stream.baseline) {
            return None;
        }
        let hot_key = if phase_now == "drag" { self.local_drag_key(&preset_id) } else { None };
        let current = self.share_object_json(&preset_id);
        let stream = self.share_streams.get_mut(&preset_id)?;
        stream.last_push_ms = now_ms;
        if !stream.baseline {
            stream.last = current;
            stream.baseline = true;
            stream.gesture_phase = phase_now.to_string();
            return None;
        }
        let mut ops = Vec::new();
        for (key, json) in &current {
            if hot_key.as_deref() == Some(key.as_str()) {
                continue;
            }
            if stream.last.get(key) != Some(json) {
                let kind = if key.starts_with('i') { "indicator_upsert" } else { "primitive_upsert" };
                ops.push(ShareWireOp {
                    kind: kind.to_string(),
                    object_id: key.clone(),
                    body: json.clone(),
                });
            }
        }
        for key in stream.last.keys() {
            if hot_key.as_deref() == Some(key.as_str()) {
                continue;
            }
            if !current.contains_key(key) {
                let kind = if key.starts_with('i') { "indicator_delete" } else { "primitive_delete" };
                ops.push(ShareWireOp {
                    kind: kind.to_string(),
                    object_id: key.clone(),
                    body: String::new(),
                });
            }
        }
        stream.gesture_phase = phase_now.to_string();
        if ops.is_empty() {
            return None;
        }
        let mut recorded = current;
        if let Some(hot) = &hot_key {
            if let Some(prev) = stream.last.get(hot) {
                recorded.insert(hot.clone(), prev.clone());
            } else {
                recorded.remove(hot);
            }
        }
        stream.pending = Some(recorded);
        Some((token, ops))
    }

    /// Token whose live socket should be open. Viewers included.
    pub fn edge_live_token(&self) -> Option<String> {
        let follow = self.share_follow_target()?;
        let stream = self.share_streams.get(&follow.preset_id)?;
        if stream.live { Some(follow.token) } else { None }
    }

    /// Frames for the edge socket. Empty when Live is off. The moving line
    /// is one of these frames, not a row in the control-box log.
    pub fn take_edge_frames(&mut self) -> Option<(String, Vec<String>)> {
        let follow = self.share_follow_target()?;
        let preset_id = follow.preset_id.clone();
        let stream = self.share_streams.get(&preset_id)?;
        if !stream.live || !follow.can_send {
            return None;
        }
        let token = follow.token.clone();
        let gesture = self.share_gesture(&preset_id);
        let viewport = self.share_viewport_json(&preset_id);
        let hot = self.local_drag_key(&preset_id);
        let current = self.share_object_json(&preset_id);
        let stream = self.share_streams.get_mut(&preset_id)?;
        let mut frames = Vec::new();
        if let Some((phase, json)) = gesture {
            let send_idle = phase == "idle" && !stream.gesture_phase.is_empty() && stream.edge_phase != "idle" && stream.edge_phase != phase;
            let send_move = phase != "idle" && stream.gesture_sent != json;
            if send_idle || send_move {
                frames.push(format!("{{\"t\":\"g\",\"b\":{json}}}"));
                stream.gesture_sent = json;
                stream.edge_phase = phase;
            }
        }
        if stream.live_viewport {
            if let Some(json) = viewport {
                if stream.viewport_sent != json {
                    frames.push(format!("{{\"t\":\"v\",\"b\":{json}}}"));
                    stream.viewport_sent = json;
                }
            }
        }
        if let Some(hot) = hot {
            if let Some(body) = current.get(&hot) {
                if stream.edge_point != *body {
                    frames.push(format!(
                        "{{\"t\":\"p\",\"id\":{},\"b\":{}}}",
                        serde_json::to_string(&hot).unwrap_or_else(|_| "\"\"".into()),
                        serde_json::to_string(body).unwrap_or_else(|_| "\"\"".into())
                    ));
                    stream.edge_point = body.clone();
                }
            }
        }
        if frames.is_empty() {
            None
        } else {
            Some((token, frames))
        }
    }

    pub fn apply_edge_frame(&mut self, text: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(text) else {
            return;
        };
        let kind = value.get("t").and_then(|v| v.as_str()).unwrap_or("");
        let Some(follow) = self.share_follow_target() else {
            return;
        };
        let preset_id = follow.preset_id.clone();
        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else {
            return;
        };
        match kind {
            "g" => {
                let body = value.get("b").map(|v| v.to_string()).unwrap_or_default();
                self.apply_share_gesture(&preset_id, bid, &body);
            }
            "v" => {
                let body = value.get("b").map(|v| v.to_string()).unwrap_or_default();
                self.apply_share_viewport(&body);
            }
            "p" => {
                let id = value.get("id").and_then(|v| v.as_str()).unwrap_or("");
                let body = value.get("b").and_then(|v| v.as_str()).unwrap_or("");
                if self.local_drag_key(&preset_id).as_deref() == Some(id) {
                    return;
                }
                let op = ShareInboundOp {
                    seq: 0,
                    kind: "primitive_upsert".to_string(),
                    object_id: id.to_string(),
                    body: body.to_string(),
                };
                self.apply_one_share_op(&preset_id, &op);
                if !follow.can_send {
                    if let Some(stream) = self.share_streams.get_mut(&preset_id) {
                        stream.last.insert(id.to_string(), body.to_string());
                    }
                }
            }
            _ => {}
        }
    }

    pub fn ack_share_push(&mut self, ok: bool) {
        let Some(follow) = self.share_follow_target() else {
            return;
        };
        let Some(stream) = self.share_streams.get_mut(&follow.preset_id) else {
            return;
        };
        if ok {
            if let Some(pending) = stream.pending.take() {
                stream.last = pending;
            }
        } else {
            stream.pending = None;
            stream.gesture_sent.clear();
            stream.gesture_phase.clear();
            stream.viewport_sent.clear();
        }
    }

    pub fn share_pull_due(&mut self, now_ms: f64) -> Option<(String, i64)> {
        let follow = self.share_follow_target()?;
        let stream = self.share_streams.entry(follow.preset_id.clone()).or_insert_with(|| ShareStream::new(follow.token.clone()));
        if stream.token != follow.token {
            *stream = ShareStream::new(follow.token.clone());
        }
        let gap = if stream.live { 250.0 } else { 8000.0 };
        if now_ms - stream.last_pull_ms < gap {
            return None;
        }
        stream.last_pull_ms = now_ms;
        Some((follow.token, stream.cursor))
    }

    pub fn note_share_channel(&mut self, token: &str, live: bool) {
        self.panel_app.share_settings_state.live_on = live;
        let preset = self.panel_app.active_preset_id.clone();
        if let Some(stream) = self.share_streams.get_mut(&preset) {
            if stream.token == token {
                stream.live = live;
            }
        }
    }

    pub fn set_share_live(&mut self, live: bool) {
        self.panel_app.share_settings_state.live_on = live;
        let active = self.panel_app.active_preset_id.clone();
        if let Some(stream) = self.share_streams.get_mut(&active) {
            stream.live = live;
            if !live {
                stream.live_viewport = false;
            }
        }
        if !live {
            self.panel_app.share_settings_state.live_viewport = false;
        }
        let token = self
            .shared_chart_meta(&active)
            .filter(|m| m.has_right("edit"))
            .map(|m| m.token.clone())
            .or_else(|| self.panel_app.chart_share_tokens.get(&active).cloned());
        if let Some(token) = token {
            self.pending_share_admin = Some(crate::ShareAdminCmd::Patch {
                token,
                body: format!(r#"{{"live":{live}}}"#),
            });
            self.panel_app.share_settings_state.status = "Saved for this chart".to_string();
        } else {
            self.panel_app.share_settings_state.status =
                "Saved as the default for the next Share".to_string();
        }
    }

    pub fn set_share_viewport(&mut self, on: bool) {
        if !self.panel_app.share_settings_state.live_on {
            return;
        }
        self.panel_app.share_settings_state.live_viewport = on;
        let active = self.panel_app.active_preset_id.clone();
        if let Some(stream) = self.share_streams.get_mut(&active) {
            stream.live_viewport = on;
            if !on {
                stream.viewport_sent.clear();
            }
        }
    }

    pub fn apply_share_ops(&mut self, token: &str, reset: bool, can_send: bool, live: bool, ops: &[ShareInboundOp]) {
        let Some(follow) = self.share_follow_target() else {
            return;
        };
        if follow.token != token {
            return;
        }
        if can_send {
            self.note_share_can_send(&follow.preset_id, true);
        }
        if let Some(stream) = self.share_streams.get_mut(&follow.preset_id) {
            stream.live = live;
        }
        self.panel_app.share_settings_state.live_on = live;
        if !live {
            self.panel_app.share_settings_state.live_viewport = false;
        }
        let preset_id = follow.preset_id.clone();
        if reset {
            if let Some(stream) = self.share_streams.get_mut(&preset_id) {
                stream.cursor = 0;
            }
        }
        let drag_key = self.local_drag_key(&preset_id);
        for op in ops {
            let held = drag_key.as_deref() == Some(op.object_id.as_str())
                && (op.kind == "primitive_upsert" || op.kind == "primitive_delete");
            if !held {
                self.apply_one_share_op(&preset_id, op);
            }
            if let Some(stream) = self.share_streams.get_mut(&preset_id) {
                if op.seq > stream.cursor {
                    stream.cursor = op.seq;
                }
                if !held {
                    match op.kind.as_str() {
                        "primitive_upsert" | "indicator_upsert" => {
                            stream.last.insert(op.object_id.clone(), op.body.clone());
                        }
                        "primitive_delete" | "indicator_delete" => {
                            stream.last.remove(&op.object_id);
                        }
                        _ => {}
                    }
                }
                stream.baseline = true;
            }
        }
    }

    fn ensure_share_keys(&mut self, preset_id: &str) {
        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else {
            return;
        };
        let import = self.share_import_prims.get(preset_id).cloned().unwrap_or_default();
        let mut local_to_author: HashMap<u64, u64> = HashMap::new();
        for (author, snapshot_id) in &import {
            let live = self.hydrate_prim_ids.get(snapshot_id).copied().unwrap_or(*snapshot_id);
            local_to_author.insert(live, *author);
        }
        let preview = self.share_streams.get(preset_id).and_then(|s| s.preview_local);
        let ids: Vec<u64> = self.core.drawings().primitives(bid).iter().map(|p| p.data().id).collect();
        let stream = self.share_streams.entry(preset_id.to_string()).or_insert_with(|| ShareStream::new(String::new()));
        for id in ids {
            if preview == Some(id) || stream.local_to_key.contains_key(&id) {
                continue;
            }
            let key = if let Some(author) = local_to_author.get(&id) {
                format!("p:{author}")
            } else if import.is_empty() {
                format!("p:{id}")
            } else {
                format!("n:{id}")
            };
            stream.key_to_local.insert(key.clone(), id);
            stream.local_to_key.insert(id, key);
        }
    }

    fn share_object_json(&self, preset_id: &str) -> HashMap<String, String> {
        let Some(stream) = self.share_streams.get(preset_id) else {
            return HashMap::new();
        };
        let Some(window) = self.panel_app.panel_grid.active_container() else {
            return HashMap::new();
        };
        let bid = window.current_tag_id;
        let preview = stream.preview_local;
        let mut out = HashMap::new();
        for prim in self.core.drawings().primitives(bid) {
            if preview == Some(prim.data().id) {
                continue;
            }
            let Some(key) = stream.local_to_key.get(&prim.data().id) else {
                continue;
            };
            let snap = PrimitiveSnapshot::from_primitive(prim.as_ref());
            out.insert(key.clone(), serde_json::json!({ "type_id": snap.type_id, "json": snap.json }).to_string());
        }
        let indicators = self.core.indicators();
        for inst in indicators.instances_for_bubble(bid) {
            let key = format!("i:{}", inst.id);
            let outputs = inst.outputs.iter().map(|(k, v)| {
                (k.clone(), OutputConfigSnapshot {
                    color: v.color.clone(),
                    line_width: v.line_width,
                    visible: Some(v.visible),
                })
            }).collect();
            let snap = IndicatorSnapshot {
                id: inst.id,
                type_id: inst.type_id.clone(),
                name: inst.name.clone(),
                params: inst.params.iter().map(|(k, v)| (k.clone(), serde_json::to_value(v).unwrap_or_default())).collect(),
                outputs,
                pane: inst.pane,
                order: inst.order,
                visible: inst.visible,
                locked: inst.locked,
                symbol: String::new(),
                bubble_id: inst.bubble_id,
                origin_id: inst.origin_id,
                signals_enabled: inst.signals_enabled,
                timeframe_visibility: inst.timeframe_visibility.clone(),
                values: std::collections::HashMap::new(),
            };
            if let Ok(json) = serde_json::to_string(&snap) {
                out.insert(key, json);
            }
        }
        out
    }

    fn apply_one_share_op(&mut self, preset_id: &str, op: &ShareInboundOp) {
        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else {
            return;
        };
        match op.kind.as_str() {
            "primitive_delete" => {
                self.ensure_share_keys(preset_id);
                let local = self.share_streams.get(preset_id).and_then(|s| s.key_to_local.get(&op.object_id).copied());
                if let Some(local) = local {
                    self.core.drawings_mut().delete(bid, local);
                }
                if let Some(stream) = self.share_streams.get_mut(preset_id) {
                    if let Some(local) = stream.key_to_local.remove(&op.object_id) {
                        stream.local_to_key.remove(&local);
                    }
                    stream.last.remove(&op.object_id);
                }
            }
            "primitive_upsert" => {
                self.ensure_share_keys(preset_id);
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&op.body) else {
                    return;
                };
                let type_id = value.get("type_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let json = value.get("json").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if type_id.is_empty() || json.is_empty() {
                    return;
                }
                let local = self.share_streams.get(preset_id).and_then(|s| s.key_to_local.get(&op.object_id).copied())
                    .or_else(|| {
                        let id = op.object_id.strip_prefix("p:").and_then(|s| s.parse::<u64>().ok())?;
                        let found = self.core.drawings().primitives(bid).iter().any(|p| p.data().id == id);
                        found.then_some(id)
                    });
                if let Some(local) = local {
                    self.core.drawings_mut().replace_primitive_from_json(bid, local, &type_id, &json);
                } else {
                    let registry = mlc_core::drawing::primitives_v2::PrimitiveRegistry::global().read().unwrap();
                    let Some(prim) = registry.from_json(&type_id, &json) else {
                        return;
                    };
                    drop(registry);
                    let id = self.core.drawings_mut().add_external(bid, prim);
                    if let Some(stream) = self.share_streams.get_mut(preset_id) {
                        stream.local_to_key.insert(id, op.object_id.clone());
                        stream.key_to_local.insert(op.object_id.clone(), id);
                    }
                }
            }
            "indicator_delete" => {
                let id = op.object_id.strip_prefix("i:").and_then(|s| s.parse::<u64>().ok());
                if let Some(id) = id {
                    self.core.indicators_mut().remove(id);
                }
            }
            "gesture" => self.apply_share_gesture(preset_id, bid, &op.body),
            "viewport" => self.apply_share_viewport(&op.body),
            "indicator_upsert" => {
                let Ok(snap) = serde_json::from_str::<IndicatorSnapshot>(&op.body) else {
                    return;
                };
                if !self.core.indicators().exists(snap.id) {
                    self.core.indicators_mut().restore_with_id(snap.id, Some(bid), &snap.type_id);
                }
                self.core.indicators_mut().restore_from_snapshot(&snap);
            }
            _ => {}
        }
    }

    fn local_drag_key(&self, preset_id: &str) -> Option<String> {
        let bid = self.panel_app.panel_grid.active_container()?.current_tag_id;
        let local = self.core.drawings().dragging(bid)?;
        self.share_streams.get(preset_id)?.local_to_key.get(&local).cloned()
    }

    /// `(phase, json)` for the local hand. `None` when Live is off.
    fn share_gesture(&mut self, preset_id: &str) -> Option<(String, String)> {
        let stream = self.share_streams.get(preset_id)?;
        if !stream.live {
            return None;
        }
        let window = self.panel_app.panel_grid.active_container()?;
        let bid = window.current_tag_id;
        let drawings = self.core.drawings();
        let drawing = drawings.is_drawing(bid);
        let drag_local = drawings.dragging(bid);
        let selected_local = drawings.selected(bid);
        let tool = drawings.drawing_tool_id(bid).unwrap_or_default();
        let mut points = drawings.drawing_points(bid);
        drop(drawings);
        if drawing {
            if let Some(window) = self.panel_app.panel_grid.active_container() {
                let cross = window.crosshair;
                if cross.visible {
                    let idx = cross.bar_f64.floor().max(0.0) as usize;
                    let ts = self.core.bubbles().bars(bid).get(idx).map(|bar| bar.timestamp);
                    if let Some(ts) = ts {
                        points.push((ts.saturating_mul(1000), cross.price));
                    }
                }
            }
        }
        if points.len() > 64 {
            points.drain(0..points.len() - 64);
        }
        let stream = self.share_streams.get(preset_id)?;
        let key_of = |local: Option<u64>| -> String {
            local.and_then(|id| stream.local_to_key.get(&id).cloned()).unwrap_or_default()
        };
        let phase = if drawing {
            "draw"
        } else if drag_local.is_some() {
            "drag"
        } else if selected_local.is_some() {
            "select"
        } else {
            "idle"
        };
        let body = serde_json::json!({
            "phase": phase,
            "tool": tool,
            "selected": key_of(selected_local),
            "dragging": key_of(drag_local),
            "points": points.iter().map(|(ts, price)| serde_json::json!([ts, price])).collect::<Vec<_>>(),
        });
        Some((phase.to_string(), body.to_string()))
    }

    fn share_viewport_json(&self, preset_id: &str) -> Option<String> {
        let stream = self.share_streams.get(preset_id)?;
        if !stream.live || !stream.live_viewport {
            return None;
        }
        let window = self.panel_app.panel_grid.active_container()?;
        let vp = window.viewport();
        let ps = window.price_scale();
        let price = if ps.scale_mode.is_auto_y() {
            serde_json::Value::Null
        } else {
            serde_json::json!([ps.price_min, ps.price_max])
        };
        Some(
            serde_json::json!({
                "view_start": (vp.view_start * 1000.0).round() / 1000.0,
                "bar_spacing": (vp.bar_spacing * 1000.0).round() / 1000.0,
                "price": price,
            })
            .to_string(),
        )
    }

    fn apply_share_gesture(&mut self, preset_id: &str, bid: mlc_core::ids::BubbleId, body: &str) {
        if self.core.drawings().is_dragging(bid) || self.core.drawings().is_drawing(bid) {
            return;
        }
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            return;
        };
        let phase = value.get("phase").and_then(|v| v.as_str()).unwrap_or("idle");
        let tool = value.get("tool").and_then(|v| v.as_str()).unwrap_or("");
        let selected = value.get("selected").and_then(|v| v.as_str()).unwrap_or("");
        let points: Vec<(i64, f64)> = value
            .get("points")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|p| {
                        let pair = p.as_array()?;
                        Some((pair.first()?.as_i64()?, pair.get(1)?.as_f64()?))
                    })
                    .collect()
            })
            .unwrap_or_default();
        if phase == "draw" && !tool.is_empty() && !points.is_empty() {
            self.set_share_preview(preset_id, bid, tool, &points);
        } else {
            self.clear_share_preview(preset_id, bid);
        }
        if selected.is_empty() {
            if phase == "idle" {
                self.core.drawings_mut().deselect(bid);
            }
        } else if let Some(local) = self.share_streams.get(preset_id).and_then(|s| s.key_to_local.get(selected).copied()) {
            self.core.drawings_mut().select(bid, local);
        }
    }

    fn apply_share_viewport(&mut self, body: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            return;
        };
        let view_start = value.get("view_start").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let bar_spacing = value.get("bar_spacing").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let price = value.get("price").and_then(|v| {
            let pair = v.as_array()?;
            Some((pair.first()?.as_f64()?, pair.get(1)?.as_f64()?))
        });
        let Some(window) = self.panel_app.panel_grid.active_container_mut() else {
            return;
        };
        window.apply_shared_view(view_start, bar_spacing, price);
    }

    fn clear_share_preview(&mut self, preset_id: &str, bid: mlc_core::ids::BubbleId) {
        let local = self.share_streams.get_mut(preset_id).and_then(|s| s.preview_local.take());
        if let Some(local) = local {
            self.core.drawings_mut().delete(bid, local);
        }
    }

    fn set_share_preview(&mut self, preset_id: &str, bid: mlc_core::ids::BubbleId, tool: &str, points: &[(i64, f64)]) {
        let registry = mlc_core::drawing::primitives_v2::PrimitiveRegistry::global().read().unwrap();
        let Some(prim) = registry.create(tool, points, None) else {
            return;
        };
        drop(registry);
        self.clear_share_preview(preset_id, bid);
        let id = self.core.drawings_mut().add_external(bid, prim);
        if let Some(stream) = self.share_streams.get_mut(preset_id) {
            stream.preview_local = Some(id);
        }
    }
}
