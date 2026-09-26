//! Live follow and co-edit on the share token that already names the chart.
//!
//! Ops name one primitive or one indicator. Candles, the viewport, and the
//! full preset are not ops. The author chart and an opened share tab attach
//! to the same token. A clone does not.

use std::collections::HashMap;

use mlc_core::drawing::PrimitiveSnapshot;
use mlc_core::preset::snapshots::{IndicatorSnapshot, OutputConfigSnapshot};

use crate::ChartApp;

#[derive(Clone, Debug)]
pub struct ShareStream {
    pub token: String,
    pub cursor: i64,
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
}

impl ShareStream {
    fn new(token: String) -> Self {
        Self {
            token,
            cursor: 0,
            baseline: false,
            last_push_ms: 0.0,
            last_pull_ms: 0.0,
            last: HashMap::new(),
            pending: None,
            local_to_key: HashMap::new(),
            key_to_local: HashMap::new(),
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
        if stream.pending.is_some() || (now_ms - stream.last_push_ms < 400.0 && stream.baseline) {
            return None;
        }
        stream.last_push_ms = now_ms;
        self.ensure_share_keys(&preset_id);
        let current = self.share_object_json(&preset_id);
        let stream = self.share_streams.get_mut(&preset_id)?;
        if !stream.baseline {
            stream.last = current;
            stream.baseline = true;
            return None;
        }
        let mut ops = Vec::new();
        for (key, json) in &current {
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
            if !current.contains_key(key) {
                let kind = if key.starts_with('i') { "indicator_delete" } else { "primitive_delete" };
                ops.push(ShareWireOp {
                    kind: kind.to_string(),
                    object_id: key.clone(),
                    body: String::new(),
                });
            }
        }
        if ops.is_empty() {
            return None;
        }
        stream.pending = Some(current);
        Some((token, ops))
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
        }
    }

    pub fn share_pull_due(&mut self, now_ms: f64) -> Option<(String, i64)> {
        let follow = self.share_follow_target()?;
        let stream = self.share_streams.entry(follow.preset_id.clone()).or_insert_with(|| ShareStream::new(follow.token.clone()));
        if stream.token != follow.token {
            *stream = ShareStream::new(follow.token.clone());
        }
        if now_ms - stream.last_pull_ms < 800.0 {
            return None;
        }
        stream.last_pull_ms = now_ms;
        Some((follow.token, stream.cursor))
    }

    pub fn apply_share_ops(&mut self, token: &str, reset: bool, can_send: bool, ops: &[ShareInboundOp]) {
        let Some(follow) = self.share_follow_target() else {
            return;
        };
        if follow.token != token {
            return;
        }
        if can_send {
            self.note_share_can_send(&follow.preset_id, true);
        }
        let preset_id = follow.preset_id.clone();
        if reset {
            if let Some(stream) = self.share_streams.get_mut(&preset_id) {
                stream.cursor = 0;
            }
        }
        let dragging = self
            .panel_app
            .panel_grid
            .active_container()
            .map(|w| self.core.drawings().is_dragging(w.current_tag_id))
            .unwrap_or(false);
        if dragging {
            return;
        }
        for op in ops {
            self.apply_one_share_op(&preset_id, op);
            if let Some(stream) = self.share_streams.get_mut(&preset_id) {
                if op.seq > stream.cursor {
                    stream.cursor = op.seq;
                }
            }
        }
        self.resync_share_last(&preset_id);
    }

    fn ensure_share_keys(&mut self, preset_id: &str) {
        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else {
            return;
        };
        let import = self.share_import_prims.get(preset_id).cloned().unwrap_or_default();
        let mut local_to_author: HashMap<u64, u64> = HashMap::new();
        for (author, local) in &import {
            local_to_author.insert(*local, *author);
        }
        let ids: Vec<u64> = self.core.drawings().primitives(bid).iter().map(|p| p.data().id).collect();
        let stream = self.share_streams.entry(preset_id.to_string()).or_insert_with(|| ShareStream::new(String::new()));
        for id in ids {
            if stream.local_to_key.contains_key(&id) {
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
        let mut out = HashMap::new();
        for prim in self.core.drawings().primitives(bid) {
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
                let Ok(value) = serde_json::from_str::<serde_json::Value>(&op.body) else {
                    return;
                };
                let type_id = value.get("type_id").and_then(|v| v.as_str()).unwrap_or("").to_string();
                let json = value.get("json").and_then(|v| v.as_str()).unwrap_or("").to_string();
                if type_id.is_empty() || json.is_empty() {
                    return;
                }
                let local = self.share_streams.get(preset_id).and_then(|s| s.key_to_local.get(&op.object_id).copied());
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

    fn resync_share_last(&mut self, preset_id: &str) {
        self.ensure_share_keys(preset_id);
        let current = self.share_object_json(preset_id);
        if let Some(stream) = self.share_streams.get_mut(preset_id) {
            stream.last = current;
            stream.baseline = true;
        }
    }
}
