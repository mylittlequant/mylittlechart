//! Live follow and co-edit on the share token that already names the chart.
//!
//! Ops name one primitive or one indicator. Candles and the full preset are
//! not ops. While Live is on, a gesture op carries selection, an in-progress
//! drag, and an in-progress draw, and a viewport op carries pan and zoom
//! when that toggle is on. The author chart and an opened share tab attach
//! to the same token. A clone does not.

use std::collections::HashMap;

use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::PrimitiveSnapshot;
use mlc_core::ids::BubbleId;
use mlc_core::preset::snapshots::{IndicatorSnapshot, OutputConfigSnapshot};

use crate::ChartApp;

/// How long a local Live click outranks what a pull reports. The PATCH that
/// stores the click is still in flight, so an older pull answers with the
/// previous value; after this window the server's answer wins again.
pub const LIVE_INTENT_HOLD_MS: f64 = 6_000.0;

/// A remote author's in-progress drawing. The render path paints it over the
/// chart; it is NOT in the drawing store, so nothing saves it, publishes it,
/// numbers it, selects it or hit-tests it.
#[derive(Clone)]
pub struct RemotePreview {
    /// The chart bubble the author was drawing on.
    pub bid: BubbleId,
    /// Always one primitive; a slice is what the render view hands out.
    pub prims: Vec<Box<dyn Primitive>>,
}

impl std::fmt::Debug for RemotePreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RemotePreview")
            .field("bid", &self.bid)
            .field("types", &self.prims.iter().map(|p| p.type_id().to_string()).collect::<Vec<_>>())
            .finish()
    }
}

#[derive(Clone, Debug)]
pub struct ShareStream {
    pub token: String,
    pub cursor: i64,
    /// This tab's Live state. The only copy: off is the cadence, on is the
    /// channel. Written by the local click and by a pull, never by another tab.
    pub live: bool,
    /// Until this instant (link clock, ms) a pull cannot change `live`.
    pub live_hold_until_ms: f64,
    /// Pan/zoom rides the channel. Only meaningful while `live` is on.
    pub live_viewport: bool,
    pub live_cursor: bool,
    pub cursor_sent: String,
    pub preview_sent: String,
    /// The full chart has been posted once. Until then the gap does not
    /// apply: a reload has only what the server stored.
    pub seeded: bool,
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
    /// The remote author's in-progress drawing, painted from here.
    pub preview: Option<RemotePreview>,
}

impl ShareStream {
    fn new(token: String) -> Self {
        Self {
            token,
            cursor: 0,
            live: false,
            live_hold_until_ms: 0.0,
            live_viewport: false,
            live_cursor: false,
            cursor_sent: String::new(),
            preview_sent: String::new(),
            seeded: false,
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
            preview: None,
        }
    }

    /// Forget what was last sent on the socket. A new connection is a new
    /// publisher in the room, so the next frame set must carry everything.
    pub(crate) fn forget_sent(&mut self) {
        self.edge_phase.clear();
        self.edge_point.clear();
        self.gesture_sent.clear();
        self.cursor_sent.clear();
        self.preview_sent.clear();
        self.viewport_sent.clear();
    }
}

#[derive(Clone, Debug)]
pub struct ShareWireOp {
    pub kind: String,
    pub object_id: String,
    pub body: String,
}

pub struct ShareCatchup {
    pub token: String,
    pub live: bool,
    pub can_send: bool,
    pub ops: Vec<ShareInboundOp>,
}

/// One HTTP push of settled objects. `preset_id` names the tab the push was
/// built for, so its acknowledgement lands there even if another tab is active
/// by the time the post returns.
pub struct SharePush {
    pub preset_id: String,
    pub token: String,
    pub ops: Vec<ShareWireOp>,
}

pub struct ShareFollow {
    pub preset_id: String,
    pub token: String,
    pub can_send: bool,
}

#[derive(Clone, Debug)]
#[derive(Default)]
struct LiveHand {
    drag: Option<(String, String)>,
    preview: Option<String>,
    cursor: Option<String>,
}

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

    /// This tab's stream, created on first use and rebuilt when the token it
    /// was made for is no longer the tab's token.
    pub(crate) fn ensure_share_stream(&mut self, preset_id: &str, token: &str) -> &mut ShareStream {
        let stream = self
            .share_streams
            .entry(preset_id.to_string())
            .or_insert_with(|| ShareStream::new(token.to_string()));
        if stream.token != token {
            *stream = ShareStream::new(token.to_string());
        }
        stream
    }

    /// The active tab's Live state. `false` when the tab has no channel.
    pub fn share_live_on(&self) -> bool {
        self.share_streams.get(&self.panel_app.active_preset_id).is_some_and(|s| s.live)
    }

    /// Whether the active tab streams its viewport.
    pub fn share_live_viewport_on(&self) -> bool {
        self.share_streams.get(&self.panel_app.active_preset_id).is_some_and(|s| s.live && s.live_viewport)
    }

    /// Whether the active tab streams its cursor.
    pub fn share_live_cursor_on(&self) -> bool {
        self.share_streams.get(&self.panel_app.active_preset_id).is_some_and(|s| s.live && s.live_cursor)
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
    pub fn share_ops_due(&mut self, now_ms: f64) -> Option<SharePush> {
        let follow = self.share_follow_target()?;
        if !follow.can_send {
            return None;
        }
        let preset_id = follow.preset_id.clone();
        let token = follow.token.clone();
        self.ensure_share_stream(&preset_id, &token);
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
        if stream.pending.is_some() || (stream.seeded && !phase_changed && now_ms - stream.last_push_ms < gap) {
            return None;
        }
        let hot_key = if phase_now == "drag" { self.local_drag_key(&preset_id) } else { None };
        let current = self.share_object_json(&preset_id);
        let stream = self.share_streams.get_mut(&preset_id)?;
        stream.last_push_ms = now_ms;
        // The first posts are the whole chart. A published preset is the
        // chart at share time; objects drawn after that exist only here
        // until they are posted, so a reload never saw them.
        let mut ops = Vec::new();
        for (key, json) in &current {
            if ops.len() >= 24 {
                break;
            }
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
        if ops.len() < 24 {
            let stale: Vec<String> = stream.last.keys().filter(|key| hot_key.as_deref() != Some(key.as_str()) && !current.contains_key(*key)).cloned().collect();
            for key in stale {
                if ops.len() >= 24 {
                    break;
                }
                let kind = if key.starts_with('i') { "indicator_delete" } else { "primitive_delete" };
                ops.push(ShareWireOp {
                    kind: kind.to_string(),
                    object_id: key,
                    body: String::new(),
                });
            }
        }
        stream.gesture_phase = phase_now.to_string();
        if ops.is_empty() {
            stream.seeded = true;
            return None;
        }
        let mut recorded = stream.last.clone();
        for op in &ops {
            if op.kind.ends_with("delete") {
                recorded.remove(&op.object_id);
            } else {
                recorded.insert(op.object_id.clone(), op.body.clone());
            }
        }
        stream.pending = Some(recorded);
        Some(SharePush { preset_id, token, ops })
    }

    /// Token whose live socket should be open: the active tab's, while that
    /// tab's own Live state is on. Viewers included.
    pub fn edge_live_token(&self) -> Option<String> {
        let follow = self.share_follow_target()?;
        self.share_streams.get(&follow.preset_id).is_some_and(|s| s.live).then_some(follow.token)
    }

    /// Drag, control-point move, in-progress drawing, and the cursor.
    /// These are the live picture. HTTP only carries the line after release.
    fn live_hand(&self, preset_id: &str) -> LiveHand {
        let Some(window) = self.panel_app.panel_grid.active_container() else {
            return LiveHand::default();
        };
        let bid = window.current_tag_id;
        let cross = window.crosshair;
        let cursor = self.share_streams.get(preset_id).is_some_and(|s| s.live_cursor).then(|| {
            // `ts` is the meaning; `bar` stays for one release so a client
            // that has not learned `ts` yet still follows the cursor.
            let cursor_ts = self.with_window_series(|series| series.ts_at_position(cross.bar_f64)).flatten();
            serde_json::json!({
                "ts": cursor_ts,
                "bar": cross.bar_f64,
                "price": cross.price,
                "visible": cross.visible,
            })
            .to_string()
        });
        let idx = cross.bar_f64.floor().max(0.0) as usize;
        let ts = self
            .core
            .bubbles()
            .bars(bid)
            .get(idx)
            .map(|bar| bar.timestamp.saturating_mul(1000))
            .unwrap_or(0);
        let drawings = self.core.drawings();
        let drag = drawings.dragging(bid).and_then(|id| {
            let prim = drawings.primitives(bid).iter().find(|p| p.data().id == id)?;
            let body = serde_json::json!({
                "type_id": prim.type_id(),
                "json": prim.to_json(),
            })
            .to_string();
            Some((format!("p:{id}"), body))
        });
        let preview = if drawings.is_drawing(bid) {
            drawings.create_preview(bid, ts, cross.price).map(|prim| {
                serde_json::json!({
                    "type_id": prim.type_id(),
                    "json": prim.to_json(),
                })
                .to_string()
            })
        } else {
            None
        };
        LiveHand { drag, preview, cursor }
    }

    /// Frames for the edge socket. Empty when Live is off. The moving line
    /// is one of these frames, not a row in the control-box log.
    pub fn take_edge_frames(&mut self) -> Option<(String, Vec<String>)> {
        // The server's welcome decides who may send. A tab that is only
        // watching never produces a frame, whatever its local rights say.
        if !self.live_link_publishing() {
            return None;
        }
        let follow = self.share_follow_target()?;
        if !follow.can_send {
            return None;
        }
        let preset_id = follow.preset_id.clone();
        if !self.share_streams.get(&preset_id).is_some_and(|s| s.live) {
            return None;
        }
        let token = follow.token.clone();
        let gesture = self.share_gesture(&preset_id);
        let viewport = self.share_viewport_json(&preset_id);
        let hand = self.live_hand(&preset_id);
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
        } else if !stream.viewport_sent.is_empty() {
            frames.push("{\"t\":\"v\",\"off\":true}".to_string());
            stream.viewport_sent.clear();
        }
        if let Some((key, body)) = hand.drag {
            if stream.edge_point != body {
                frames.push(format!(
                    "{{\"t\":\"p\",\"id\":{},\"b\":{}}}",
                    serde_json::to_string(&key).unwrap_or_else(|_| "\"\"".into()),
                    serde_json::to_string(&body).unwrap_or_else(|_| "\"\"".into())
                ));
                stream.edge_point = body;
            }
        }
        match hand.preview {
            Some(body) if stream.preview_sent != body => {
                frames.push(format!(
                    "{{\"t\":\"preview\",\"b\":{}}}",
                    serde_json::to_string(&body).unwrap_or_else(|_| "\"\"".into())
                ));
                stream.preview_sent = body;
            }
            None if !stream.preview_sent.is_empty() => {
                frames.push("{\"t\":\"preview\",\"b\":\"\"}".to_string());
                stream.preview_sent.clear();
            }
            _ => {}
        }
        if stream.live_cursor {
            if let Some(body) = hand.cursor {
                if stream.cursor_sent != body {
                    frames.push(format!(
                        "{{\"t\":\"c\",\"b\":{}}}",
                        serde_json::to_string(&body).unwrap_or_else(|_| "\"\"".into())
                    ));
                    stream.cursor_sent = body;
                }
            }
        } else if !stream.cursor_sent.is_empty() {
            frames.push("{\"t\":\"c\",\"off\":true}".to_string());
            stream.cursor_sent.clear();
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
        if kind == "welcome" {
            let publish = value.get("publish").and_then(|v| v.as_bool()).unwrap_or(false);
            self.live_link_welcome(publish);
            return;
        }
        if !self.live_link.is_open() {
            return;
        }
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
                if value.get("off").and_then(|v| v.as_bool()).unwrap_or(false) {
                    if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                        window.set_follow_remote_view(false);
                    }
                } else {
                    let body = match value.get("b") {
                        Some(v) => v.as_str().map(|s| s.to_string()).unwrap_or_else(|| v.to_string()),
                        None => String::new(),
                    };
                    self.apply_share_viewport(&body, true);
                }
            }
            "preview" => {
                let body = value.get("b").and_then(|v| v.as_str()).unwrap_or("");
                if body.is_empty() {
                    self.clear_share_preview(&preset_id);
                } else {
                    self.apply_share_preview_body(&preset_id, bid, body);
                }
            }
            "c" => {
                self.apply_share_cursor(&value);
            }
            "end" => {
                let id = self.panel_app.active_preset_id.clone();
                if self.shared_chart_meta.contains_key(&id) {
                    self.stream_ended = Some(id);
                    self.release_remote_follow();
                }
            }
            "p" => {
                self.clear_share_preview(&preset_id);
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

    /// The post built for `preset_id` finished. Acknowledges THAT tab's
    /// stream, not the active one: a tab switch between the push and its
    /// answer must not leave the original stream waiting forever.
    pub fn ack_share_push(&mut self, preset_id: &str, ok: bool) {
        let Some(stream) = self.share_streams.get_mut(preset_id) else {
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
        let stream = self.ensure_share_stream(&follow.preset_id, &follow.token);
        let gap = if stream.live { 250.0 } else { 8000.0 };
        if now_ms - stream.last_pull_ms < gap {
            return None;
        }
        stream.last_pull_ms = now_ms;
        Some((follow.token, stream.cursor))
    }

    /// A pull answered 404: the share is gone. The author's binding to it is
    /// dropped at once (and saved), and a viewer tab on it is closed with the
    /// "Shared chart unavailable" notice, so nothing asks again every few
    /// seconds.
    pub fn share_pull_gone(&mut self, token: &str) {
        self.forget_chart_share_token(token);
        self.share_viewer_link_gone(token);
        self.share_streams.retain(|_, stream| stream.token != token);
    }

    /// Turn Live on or off for the active tab. The tab's own stream is the
    /// only place the state lives; the PATCH stores it for the viewers.
    /// Does nothing on a tab that has no share to stream on.
    pub fn set_share_live(&mut self, live: bool) {
        let active = self.panel_app.active_preset_id.clone();
        let token = self
            .shared_chart_meta(&active)
            .filter(|m| m.has_right("edit"))
            .map(|m| m.token.clone())
            .or_else(|| self.panel_app.chart_share_tokens.get(&active).cloned());
        let Some(token) = token else {
            log::debug!("[Share] Live {live} ignored: this tab has no share");
            return;
        };
        let hold_until = self.live_link.clock_ms() + LIVE_INTENT_HOLD_MS;
        let stream = self.ensure_share_stream(&active, &token);
        stream.live = live;
        stream.live_hold_until_ms = hold_until;
        if !live {
            stream.live_viewport = false;
            stream.live_cursor = false;
        }
        self.pending_share_admin = Some(crate::ShareAdminCmd::Patch {
            token,
            body: format!(r#"{{"live":{live}}}"#),
        });
        self.panel_app.share_settings_state.status = "Saved for this chart".to_string();
    }

    pub fn set_share_cursor(&mut self, on: bool) {
        let active = self.panel_app.active_preset_id.clone();
        let Some(stream) = self.share_streams.get_mut(&active).filter(|s| s.live) else {
            return;
        };
        stream.live_cursor = on;
        stream.cursor_sent.clear();
        if !on {
            if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                window.set_follow_remote_cursor(false);
            }
        }
    }

    pub fn set_share_viewport(&mut self, on: bool) {
        let active = self.panel_app.active_preset_id.clone();
        let Some(stream) = self.share_streams.get_mut(&active).filter(|s| s.live) else {
            return;
        };
        stream.live_viewport = on;
        if !on {
            stream.viewport_sent.clear();
        }
    }

    pub fn queue_share_catchup(&mut self, catch: ShareCatchup) {
        if self.load_state.is_loading() {
            self.pending_share_catchup = Some(catch);
            return;
        }
        self.apply_share_ops(&catch.token, false, catch.can_send, catch.live, &catch.ops);
    }

    pub fn drain_share_catchup(&mut self) {
        if self.load_state.is_loading() {
            return;
        }
        let Some(catch) = self.pending_share_catchup.take() else {
            return;
        };
        self.apply_share_ops(&catch.token, false, catch.can_send, catch.live, &catch.ops);
    }

    pub fn apply_share_ops(&mut self, token: &str, reset: bool, can_send: bool, live: bool, ops: &[ShareInboundOp]) {
        let Some(follow) = self.share_follow_target() else {
            return;
        };
        if follow.token != token {
            return;
        }
        let preset_id = follow.preset_id.clone();
        if can_send {
            self.note_share_can_send(&preset_id, true);
        }
        let now_ms = self.live_link.clock_ms();
        let ended = {
            let stream = self.ensure_share_stream(&preset_id, token);
            let was_live = stream.live;
            // A pull that was answered before the PATCH of a local click
            // landed still carries the old value. It cannot undo the click;
            // once the server agrees, or the hold runs out, it can again.
            if live == stream.live || now_ms >= stream.live_hold_until_ms {
                stream.live = live;
                stream.live_hold_until_ms = 0.0;
            }
            if !stream.live {
                stream.live_viewport = false;
                stream.live_cursor = false;
            }
            was_live && !stream.live
        };
        if ended && self.shared_chart_meta.contains_key(&preset_id) {
            self.stream_ended = Some(preset_id.clone());
            // The author is gone. Whatever viewport and cursor lock the last
            // frames left behind ends with the stream, whether or not the
            // socket's own stop frames arrived before it closed.
            self.release_remote_follow();
        }
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
            "viewport" => self.apply_share_viewport(&op.body, false),
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
        // `view_ts` is the meaning: the moment at the left edge, which each
        // side maps onto its own loaded history. `view_start` (an index into
        // the sender's history) stays for one release for older clients.
        let view_ts = self.with_window_series(|series| series.ts_at_position(vp.view_start)).flatten();
        Some(
            serde_json::json!({
                "view_ts": view_ts,
                "view_start": (vp.view_start * 1000.0).round() / 1000.0,
                "bar_spacing": (vp.bar_spacing * 1000.0).round() / 1000.0,
                "price": price,
            })
            .to_string(),
        )
    }

    /// Run `f` on the series the active window renders (the ActiveSeries
    /// seam), without copying it. `None` when the window has no bubble yet.
    pub(crate) fn with_window_series<R>(&self, f: impl FnOnce(crate::active_series::ActiveSeries<'_>) -> R) -> Option<R> {
        let bid = self.panel_app.panel_grid.active_container()?.current_tag_id;
        let chart_type = self.core.bubbles().chart_type(bid).unwrap_or("candles");
        self.with_bubble(bid, |bubble| f(crate::active_series::resolve_active_series(chart_type, bubble)))
    }

    /// Let go of a remote author's viewport and cursor on the active window.
    pub(crate) fn release_remote_follow(&mut self) {
        if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
            window.set_follow_remote_view(false);
            window.set_follow_remote_cursor(false);
        }
    }

    /// The in-progress drawing to paint over `bid`'s chart, if the active tab's
    /// stream holds one for that bubble. The render views take this slice.
    pub(crate) fn remote_preview_overlay(&self, bid: BubbleId) -> &[Box<dyn Primitive>] {
        self.share_streams
            .get(&self.panel_app.active_preset_id)
            .and_then(|stream| stream.preview.as_ref())
            .filter(|preview| preview.bid == bid)
            .map_or(&[][..], |preview| preview.prims.as_slice())
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
            self.clear_share_preview(preset_id);
        }
        if selected.is_empty() {
            if phase == "idle" {
                self.core.drawings_mut().deselect(bid);
            }
        } else if let Some(local) = self.share_streams.get(preset_id).and_then(|s| s.key_to_local.get(selected).copied()) {
            self.core.drawings_mut().select(bid, local);
        }
    }

    pub(crate) fn apply_share_viewport(&mut self, body: &str, lock: bool) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            return;
        };
        // The timestamp wins. A frame without one is from a client that still
        // sends indices, so its `view_start` is taken as an index. A frame
        // with neither leaves the position alone (NaN is ignored downstream).
        let view_start = value
            .get("view_ts")
            .and_then(|v| v.as_i64())
            .and_then(|ts| self.with_window_series(|series| series.position_at_ts(ts)).flatten())
            .or_else(|| value.get("view_start").and_then(|v| v.as_f64()))
            .unwrap_or(f64::NAN);
        let bar_spacing = value.get("bar_spacing").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let price = value.get("price").and_then(|v| {
            let pair = v.as_array()?;
            Some((pair.first()?.as_f64()?, pair.get(1)?.as_f64()?))
        });
        let Some(window) = self.panel_app.panel_grid.active_container_mut() else {
            return;
        };
        if lock {
            window.set_follow_remote_view(true);
        }
        window.apply_shared_view(view_start, bar_spacing, price);
    }

    pub(crate) fn apply_share_cursor(&mut self, value: &serde_json::Value) {
        if value.get("off").and_then(|v| v.as_bool()).unwrap_or(false) {
            if let Some(window) = self.panel_app.panel_grid.active_container_mut() {
                window.set_follow_remote_cursor(false);
            }
            return;
        }
        let body = value.get("b").and_then(|v| v.as_str()).unwrap_or("");
        let Ok(cursor) = serde_json::from_str::<serde_json::Value>(body) else {
            return;
        };
        // Timestamp first, mapped onto THIS side's history. Without one the
        // frame is from an older client and `bar` is an index.
        let bar = cursor
            .get("ts")
            .and_then(|v| v.as_i64())
            .and_then(|ts| self.with_window_series(|series| series.position_at_ts(ts)).flatten())
            .or_else(|| cursor.get("bar").and_then(|v| v.as_f64()))
            .unwrap_or(0.0);
        let price = cursor.get("price").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let visible = cursor.get("visible").and_then(|v| v.as_bool()).unwrap_or(false);
        let Some(window) = self.panel_app.panel_grid.active_container_mut() else {
            return;
        };
        window.set_crosshair_from_bar(bar, price, visible, None);
        window.set_follow_remote_cursor(true);
    }

    fn apply_share_preview_body(&mut self, preset_id: &str, bid: BubbleId, body: &str) {
        let Ok(value) = serde_json::from_str::<serde_json::Value>(body) else {
            return;
        };
        let type_id = value.get("type_id").and_then(|v| v.as_str()).unwrap_or("");
        let json = value.get("json").and_then(|v| v.as_str()).unwrap_or("");
        if type_id.is_empty() || json.is_empty() {
            return;
        }
        let Some(prim) = mlc_core::drawing::primitives_v2::PrimitiveRegistry::global()
            .read()
            .ok()
            .and_then(|registry| registry.from_json(type_id, json))
        else {
            return;
        };
        self.set_remote_preview(preset_id, bid, prim);
    }

    fn clear_share_preview(&mut self, preset_id: &str) {
        if let Some(stream) = self.share_streams.get_mut(preset_id) {
            stream.preview = None;
        }
    }

    fn set_share_preview(&mut self, preset_id: &str, bid: BubbleId, tool: &str, points: &[(i64, f64)]) {
        let Some(prim) = mlc_core::drawing::primitives_v2::PrimitiveRegistry::global()
            .read()
            .ok()
            .and_then(|registry| registry.create(tool, points, None))
        else {
            return;
        };
        self.set_remote_preview(preset_id, bid, prim);
    }

    fn set_remote_preview(&mut self, preset_id: &str, bid: BubbleId, prim: Box<dyn Primitive>) {
        if let Some(stream) = self.share_streams.get_mut(preset_id) {
            stream.preview = Some(RemotePreview { bid, prims: vec![prim] });
        }
    }
}
