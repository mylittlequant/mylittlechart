//! `DrawingManager` — stateful per-bubble backend manager for the drawing
//! system. One bubble = one editing session (`BubbleDrawingSession`).
//!
//! The drawing *data* (`primitives: Vec<Box<dyn Primitive>>`) lives on
//! `DataBubble.primitives` — this manager mutates it through `&mut
//! BubbleStore`, exactly the same shape as `AlertManager` mutating
//! `DataBubble.alerts`. Only cross-call editing-session bookkeeping (active
//! tool, drag, hover, in-progress multipoint, last-used style, selection)
//! lives on `BubbleDrawingSession`.
//!
//! All methods are `pub(crate)`: app code never calls `DrawingManager`
//! directly, it goes through the `Drawings` / `DrawingsMut` facade (see
//! `crates/mlc-backend/src/drawing/facade.rs`).
//!
//! See `docs/mlc/plans/doctrine-cutover-2026-06-27.md` §1A and
//! `docs/mlc/plans/step-3.1-drawing-manager-design.md`.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

use mlc_core::drawing::primitives_v2::config::{
    FibLevelConfig, PropertyKind, PropertyValue, SettingsTemplate, TemplateStyle,
    TimeframeVisibilityConfig,
};
use mlc_core::drawing::primitives_v2::icons::emoji::{Emoji, EmojiType};
use mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry;
use mlc_core::drawing::primitives_v2::{
    ClickBehavior, ControlPoint, ControlPointType, HitTestResult, LineStyle, Primitive,
    PrimitiveData, PrimitiveExt, PrimitiveText, SyncMode, TextAlign,
};
use mlc_core::drawing::{alloc_primitive_id, PrimitiveSnapshot};
use mlc_core::ids::BubbleId;
use mlc_core::state::selected_config::SelectedPrimitiveConfig;
use mlc_core::{Bar, PriceScale, Viewport};

use crate::bubble::BubbleStore;

use super::facade::SwapSymbolResult;

/// Apply a `TemplateStyle` snapshot to a freshly-created primitive.
fn apply_last_style_snapshot(prim: &mut Box<dyn Primitive>, style: &TemplateStyle) {
    {
        let data = prim.data_mut();
        if let Some(ref c) = style.color {
            data.color.stroke = c.clone();
        }
        if let Some(w) = style.width {
            data.width = w;
        }
        if let Some(ref s) = style.line_style {
            data.style = match s.as_str() {
                "dashed" => LineStyle::Dashed,
                "dotted" => LineStyle::Dotted,
                "large_dashed" => LineStyle::LargeDashed,
                "sparse_dotted" => LineStyle::SparseDotted,
                _ => LineStyle::Solid,
            };
        }
        if let Some(ref f) = style.fill_color {
            data.color.fill = Some(f.clone());
        }
    }
    for (prop_id, value) in &style.style_properties {
        prim.apply_style_property(prop_id, value);
    }
}

/// Compute a fingerprint for a `TemplateStyle` snapshot (preview cache key).
fn style_fingerprint(style: &TemplateStyle) -> u64 {
    let mut h = DefaultHasher::new();
    style.color.hash(&mut h);
    style.width.map(f64::to_bits).hash(&mut h);
    style.line_style.hash(&mut h);
    style.fill_color.hash(&mut h);
    style.fill_opacity.map(f64::to_bits).hash(&mut h);
    style.show_labels.hash(&mut h);
    style.show_prices.hash(&mut h);
    for (k, v) in &style.style_properties {
        k.hash(&mut h);
        format!("{v:?}").hash(&mut h);
    }
    h.finish()
}

/// Cached preview primitive to avoid per-frame rebuilds in `create_preview`.
struct PreviewCache {
    tool_id: String,
    points_len: usize,
    cursor: (u64, u64),
    style_fp: u64,
    variant: Option<String>,
    primitive: Box<dyn Primitive>,
}

/// Drawing state — tracks multi-click primitive creation.
#[derive(Clone, Debug, Default)]
pub enum DrawingState {
    /// No drawing in progress.
    #[default]
    Idle,
    /// First point placed, waiting for more points.
    Creating {
        /// Tool type_id being created.
        tool_id: String,
        /// Accumulated points as (timestamp_ms, price) pairs.
        points: Vec<(i64, f64)>,
    },
}

impl DrawingState {
    pub fn is_drawing(&self) -> bool {
        matches!(self, DrawingState::Creating { .. })
    }

    pub fn tool_id(&self) -> Option<&str> {
        match self {
            DrawingState::Creating { tool_id, .. } => Some(tool_id),
            _ => None,
        }
    }

    pub fn points(&self) -> &[(i64, f64)] {
        match self {
            DrawingState::Creating { points, .. } => points,
            _ => &[],
        }
    }

    pub fn cancel(&mut self) {
        *self = DrawingState::Idle;
    }
}

/// Type of drag operation in progress.
#[derive(Clone, Debug, PartialEq, Default)]
pub enum DragType {
    /// Dragging the whole primitive (translate).
    #[default]
    Move,
    /// Dragging a specific control point (resize/reshape).
    ControlPoint(ControlPointType),
}

/// One bubble's open drawing-editing session. Lazily created on first touch.
struct BubbleDrawingSession {
    current_tool: Option<String>,
    tool_variant: Option<String>,
    state: DrawingState,
    selected: Option<u64>,
    dragging: Option<u64>,
    drag_type: DragType,
    drag_start: Option<(i64, f64)>,
    current_pane_id: Option<u64>,
    locked: bool,
    visible: bool,
    default_color: String,
    /// Cached preview primitive from the last `create_preview` call.
    ///
    /// `Mutex` lets `create_preview(&self, …)` write the cache through a
    /// shared reference — the render path only holds `Drawings<'_>` (a
    /// read-lock facade), and preview-while-not-yet-committed is
    /// read-mostly traffic where a write-lock-per-frame-per-bubble would
    /// be a real contention risk on multi-bubble layouts.
    preview_cache: Mutex<Option<PreviewCache>>,
}

impl Default for BubbleDrawingSession {
    fn default() -> Self {
        Self {
            current_tool: None,
            tool_variant: None,
            state: DrawingState::Idle,
            selected: None,
            dragging: None,
            drag_type: DragType::Move,
            drag_start: None,
            current_pane_id: None,
            locked: false,
            visible: true,
            default_color: "#2196F3".to_string(),
            preview_cache: Mutex::new(None),
        }
    }
}

/// Stateful per-bubble drawing manager. One bubble = one editing session.
///
/// Drawing data (`primitives`) lives on `DataBubble.primitives` — this
/// manager mutates it via `&mut BubbleStore`, threading `bid` through every
/// call. App code never sees `DataBubble` or `BubbleDrawingSession`
/// directly — only through the `Drawings` / `DrawingsMut` facade.
#[derive(Default)]
pub struct DrawingManager {
    sessions: HashMap<BubbleId, BubbleDrawingSession>,
}

impl DrawingManager {
    fn session(&self, bid: BubbleId) -> Option<&BubbleDrawingSession> {
        self.sessions.get(&bid)
    }

    fn session_mut(&mut self, bid: BubbleId) -> &mut BubbleDrawingSession {
        self.sessions.entry(bid).or_default()
    }

    /// Look up the index of a primitive by stable id within a bubble's
    /// primitive list. Internal-only — never crosses the facade boundary.
    fn find_index_by_id(b: &crate::bubble::DataBubble, prim_id: u64) -> Option<usize> {
        b.primitives.iter().position(|p| p.data().id == prim_id)
    }

    // =========================================================================
    // Read — session state
    // =========================================================================

    pub(crate) fn is_drawing(&self, bid: BubbleId) -> bool {
        self.session(bid).is_some_and(|s| s.state.is_drawing())
    }

    pub(crate) fn current_tool(&self, bid: BubbleId) -> Option<&str> {
        self.session(bid).and_then(|s| s.current_tool.as_deref())
    }

    pub(crate) fn tool_variant(&self, bid: BubbleId) -> Option<&str> {
        self.session(bid).and_then(|s| s.tool_variant.as_deref())
    }

    pub(crate) fn current_pane(&self, bid: BubbleId) -> Option<u64> {
        self.session(bid).and_then(|s| s.current_pane_id)
    }

    pub(crate) fn selected(&self, bid: BubbleId) -> Option<u64> {
        self.session(bid).and_then(|s| s.selected)
    }

    pub(crate) fn dragging(&self, bid: BubbleId) -> Option<u64> {
        self.session(bid).and_then(|s| s.dragging)
    }

    pub(crate) fn is_dragging(&self, bid: BubbleId) -> bool {
        self.session(bid).is_some_and(|s| s.dragging.is_some())
    }

    pub(crate) fn drag_type(&self, bid: BubbleId) -> DragType {
        self.session(bid).map(|s| s.drag_type.clone()).unwrap_or_default()
    }

    pub(crate) fn drawing_state(&self, bid: BubbleId) -> DrawingState {
        self.session(bid).map(|s| s.state.clone()).unwrap_or_default()
    }

    pub(crate) fn drawing_points(&self, bid: BubbleId) -> Vec<(i64, f64)> {
        self.session(bid).map(|s| s.state.points().to_vec()).unwrap_or_default()
    }

    pub(crate) fn is_locked(&self, bid: BubbleId) -> bool {
        self.session(bid).is_some_and(|s| s.locked)
    }

    pub(crate) fn is_visible(&self, bid: BubbleId) -> bool {
        self.session(bid).map(|s| s.visible).unwrap_or(true)
    }

    pub(crate) fn set_locked(&mut self, bid: BubbleId, locked: bool) {
        self.session_mut(bid).locked = locked;
    }

    pub(crate) fn set_visible(&mut self, bid: BubbleId, visible: bool) {
        self.session_mut(bid).visible = visible;
    }

    pub(crate) fn toggle_lock(&mut self, bid: BubbleId) {
        let s = self.session_mut(bid);
        s.locked = !s.locked;
    }

    pub(crate) fn toggle_visible(&mut self, bid: BubbleId) {
        let s = self.session_mut(bid);
        s.visible = !s.visible;
    }

    pub(crate) fn default_color(&self, bid: BubbleId) -> String {
        self.session(bid).map(|s| s.default_color.clone()).unwrap_or_else(|| "#2196F3".to_string())
    }

    pub(crate) fn set_default_color(&mut self, bid: BubbleId, color: &str) {
        self.session_mut(bid).default_color = color.to_string();
    }

    pub(crate) fn is_freehand_tool(&self, bid: BubbleId) -> bool {
        let Some(s) = self.session(bid) else { return false };
        let Some(ref tool_id) = s.current_tool else { return false };
        let registry = PrimitiveRegistry::global().read().unwrap();
        registry.get(tool_id).map(|m| m.click_behavior == ClickBehavior::FreehandDrag).unwrap_or(false)
    }

    pub(crate) fn is_drawing_freehand(&self, bid: BubbleId) -> bool {
        if self.is_freehand_tool(bid) {
            return true;
        }
        let Some(s) = self.session(bid) else { return false };
        if let DrawingState::Creating { tool_id, .. } = &s.state {
            let registry = PrimitiveRegistry::global().read().unwrap();
            if let Some(meta) = registry.get(tool_id) {
                return meta.click_behavior == ClickBehavior::FreehandDrag;
            }
        }
        false
    }

    pub(crate) fn drawing_tool_id(&self, bid: BubbleId) -> Option<String> {
        let s = self.session(bid)?;
        if let Some(ref t) = s.current_tool {
            return Some(t.clone());
        }
        if let DrawingState::Creating { tool_id, .. } = &s.state {
            return Some(tool_id.clone());
        }
        None
    }

    // =========================================================================
    // Read — bubble primitive data
    // =========================================================================

    pub(crate) fn primitives<'a>(&self, bubbles: &'a BubbleStore, bid: BubbleId) -> &'a [Box<dyn Primitive>] {
        bubbles.bubble(bid).map(|b| b.primitives.as_slice()).unwrap_or(&[])
    }

    pub(crate) fn count(&self, bubbles: &BubbleStore, bid: BubbleId) -> usize {
        bubbles.bubble(bid).map(|b| b.primitives.len()).unwrap_or(0)
    }

    /// Indices into `primitives(bid)`, sorted by `z_order` (lowest first) —
    /// a pure rendering-order computation over the slice, not manager
    /// session state. Used by the render path to iterate primitives in
    /// paint order.
    pub(crate) fn primitives_sorted_by_z_order(&self, bubbles: &BubbleStore, bid: BubbleId) -> Vec<usize> {
        let prims = self.primitives(bubbles, bid);
        let mut indices: Vec<usize> = (0..prims.len()).collect();
        indices.sort_by_key(|&i| prims[i].data().z_order);
        indices
    }

    /// Resolve the effective color for the bubble's current tool.
    ///
    /// Priority: bubble-shared `last_used_style[tool_id]` (set by the
    /// inline toolbar / last-used style) → registry `meta.default_color` →
    /// session's `default_color`. Matches the click-primitive creation path
    /// so the live freehand preview, the click preview, and the finalized
    /// primitive all share the same color source.
    pub(crate) fn effective_color(&self, bubbles: &BubbleStore, bid: BubbleId) -> String {
        let Some(session) = self.session(bid) else { return "#2196F3".to_string() };
        let tool_id_opt: Option<String> = session.current_tool.clone().or_else(|| match &session.state {
            DrawingState::Creating { tool_id, .. } => Some(tool_id.clone()),
            _ => None,
        });
        if let Some(tool_id) = tool_id_opt {
            if let Some(style) = self.last_used_style_for(bubbles, bid, &tool_id) {
                if let Some(ref c) = style.color {
                    return c.clone();
                }
            }
            let registry = PrimitiveRegistry::global().read().unwrap();
            if let Some(meta) = registry.get(&tool_id) {
                return meta.default_color.to_string();
            }
        }
        session.default_color.clone()
    }

    pub(crate) fn get_data(&self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) -> Option<PrimitiveData> {
        let b = bubbles.bubble(bid)?;
        b.primitives.iter().find(|p| p.data().id == prim_id).map(|p| p.data().clone())
    }

    /// Which sub-pane (if any) `prim_id` belongs to.
    pub(crate) fn pane_id_of(&self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) -> Option<u64> {
        let b = bubbles.bubble(bid)?;
        b.primitives.iter().find(|p| p.data().id == prim_id).and_then(|p| p.data().pane_id)
    }

    pub(crate) fn get_points(&self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) -> Option<Vec<(i64, f64)>> {
        let b = bubbles.bubble(bid)?;
        b.primitives.iter().find(|p| p.data().id == prim_id).map(|p| p.points())
    }

    pub(crate) fn get_type_id(&self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) -> Option<String> {
        let b = bubbles.bubble(bid)?;
        b.primitives.iter().find(|p| p.data().id == prim_id).map(|p| p.type_id().to_string())
    }

    pub(crate) fn get_primitive_json(&self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) -> Option<String> {
        let b = bubbles.bubble(bid)?;
        b.primitives.iter().find(|p| p.data().id == prim_id).map(|p| p.to_json())
    }

    pub(crate) fn last_index(&self, bubbles: &BubbleStore, bid: BubbleId) -> Option<u64> {
        let b = bubbles.bubble(bid)?;
        b.primitives.last().map(|p| p.data().id)
    }

    pub(crate) fn selected_primitive<'a>(&self, bubbles: &'a BubbleStore, bid: BubbleId) -> Option<&'a Box<dyn Primitive>> {
        let prim_id = self.selected(bid)?;
        let b = bubbles.bubble(bid)?;
        b.primitives.iter().find(|p| p.data().id == prim_id)
    }

    pub(crate) fn selected_config(&self, bubbles: &BubbleStore, bid: BubbleId) -> Option<SelectedPrimitiveConfig> {
        let prim = self.selected_primitive(bubbles, bid)?;
        let data = prim.data();
        let type_id = prim.type_id();
        let supports_text = {
            let registry = PrimitiveRegistry::global().read().unwrap();
            registry.supports_text(type_id)
        };
        let text_color = if supports_text { data.text.as_ref().and_then(|t| t.color.clone()) } else { None };
        Some(SelectedPrimitiveConfig {
            name: prim.display_name().to_string(),
            color: data.color.stroke.clone(),
            width: data.width,
            style: data.style.as_str().to_string(),
            locked: data.locked,
            text_color,
            supports_text,
        })
    }

    pub(crate) fn primitive_list(&self, bubbles: &BubbleStore, bid: BubbleId) -> Vec<PrimitiveListItem> {
        let selected = self.selected(bid);
        let Some(b) = bubbles.bubble(bid) else { return Vec::new() };
        b.primitives
            .iter()
            .enumerate()
            .map(|(idx, prim)| {
                let data = prim.data();
                PrimitiveListItem {
                    index: idx,
                    id: data.id,
                    type_id: prim.type_id().to_string(),
                    display_name: prim.display_name().to_string(),
                    color: data.color.stroke.clone(),
                    visible: data.visible,
                    locked: data.locked,
                    selected: selected == Some(data.id),
                }
            })
            .collect()
    }

    pub(crate) fn hit_test(
        &self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        x: f64,
        y: f64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<u64> {
        let session = self.session(bid)?;
        if !session.visible || session.locked {
            return None;
        }
        let b = bubbles.bubble(bid)?;
        for prim in b.primitives.iter().rev() {
            if prim.data().pane_id.is_some() {
                continue;
            }
            if !matches!(prim.hit_test(x, y, bars, viewport, price_scale), HitTestResult::Miss) {
                return Some(prim.data().id);
            }
        }
        None
    }

    pub(crate) fn hit_test_in_pane(
        &self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        x: f64,
        y: f64,
        pane_id: u64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<u64> {
        let session = self.session(bid)?;
        if !session.visible || session.locked {
            return None;
        }
        let b = bubbles.bubble(bid)?;
        for prim in b.primitives.iter().rev() {
            if prim.data().pane_id != Some(pane_id) {
                continue;
            }
            if !matches!(prim.hit_test(x, y, bars, viewport, price_scale), HitTestResult::Miss) {
                return Some(prim.data().id);
            }
        }
        None
    }

    pub(crate) fn hit_test_control_point(
        &self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        x: f64,
        y: f64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<ControlPointType> {
        let session = self.session(bid)?;
        if !session.visible || session.locked {
            return None;
        }
        let prim_id = session.selected?;
        let b = bubbles.bubble(bid)?;
        let prim = b.primitives.iter().find(|p| p.data().id == prim_id)?;
        if prim.data().locked {
            return None;
        }
        match prim.hit_test(x, y, bars, viewport, price_scale) {
            HitTestResult::ControlPoint(cp) => Some(cp),
            _ => None,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn hit_test_control_point_in_pane(
        &self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        x: f64,
        y: f64,
        pane_id: u64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<ControlPointType> {
        let session = self.session(bid)?;
        if !session.visible || session.locked {
            return None;
        }
        let prim_id = session.selected?;
        let b = bubbles.bubble(bid)?;
        let prim = b.primitives.iter().find(|p| p.data().id == prim_id)?;
        if prim.data().pane_id != Some(pane_id) {
            return None;
        }
        if prim.data().locked {
            return None;
        }
        match prim.hit_test(x, y, bars, viewport, price_scale) {
            HitTestResult::ControlPoint(cp) => Some(cp),
            _ => None,
        }
    }

    pub(crate) fn selected_control_points(
        &self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Vec<ControlPoint> {
        self.selected_primitive(bubbles, bid)
            .map(|p| p.control_points(bars, viewport, price_scale))
            .unwrap_or_default()
    }

    // =========================================================================
    // Tool / pane selection
    // =========================================================================

    pub(crate) fn set_tool(&mut self, bid: BubbleId, tool_id: Option<&str>) {
        let session = self.session_mut(bid);
        session.state.cancel();
        session.tool_variant = None;
        *session.preview_cache.lock().expect("preview_cache mutex poisoned") = None;

        session.current_tool = match tool_id {
            None | Some("cursor") | Some("none") | Some("crosshair") | Some("hand") => None,
            Some(id) => {
                let (base_id, variant) = if let Some(colon_pos) = id.find(':') {
                    (&id[..colon_pos], Some(id[colon_pos + 1..].to_string()))
                } else {
                    (id, None)
                };
                let registry = PrimitiveRegistry::global().read().unwrap();
                if registry.get(base_id).is_some() {
                    session.tool_variant = variant;
                    Some(base_id.to_string())
                } else {
                    None
                }
            }
        };
    }

    pub(crate) fn set_current_pane(&mut self, bid: BubbleId, pane_id: Option<u64>) {
        self.session_mut(bid).current_pane_id = pane_id;
    }

    pub(crate) fn cancel_drawing(&mut self, bid: BubbleId) {
        let session = self.session_mut(bid);
        session.state.cancel();
        *session.preview_cache.lock().expect("preview_cache mutex poisoned") = None;
    }

    // =========================================================================
    // Selection
    // =========================================================================

    pub(crate) fn select(&mut self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) {
        if Self::find_index_by_id_on(bubbles, bid, prim_id).is_some() {
            self.session_mut(bid).selected = Some(prim_id);
        }
    }

    fn find_index_by_id_on(bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) -> Option<usize> {
        bubbles.bubble(bid).and_then(|b| Self::find_index_by_id(b, prim_id))
    }

    pub(crate) fn deselect(&mut self, bid: BubbleId) {
        if let Some(s) = self.sessions.get_mut(&bid) {
            s.selected = None;
        }
    }

    pub(crate) fn try_select_at(
        &mut self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        x: f64,
        y: f64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> bool {
        if let Some(prim_id) = self.hit_test(bubbles, bid, x, y, bars, viewport, price_scale) {
            self.session_mut(bid).selected = Some(prim_id);
            true
        } else {
            self.session_mut(bid).selected = None;
            false
        }
    }

    // =========================================================================
    // Create — click handling
    // =========================================================================

    /// Stamp identity + pane onto a freshly-created primitive and push it.
    fn finalize_new_primitive(
        b: &mut crate::bubble::DataBubble,
        session: &mut BubbleDrawingSession,
        mut prim: Box<dyn Primitive>,
        last_style: Option<&TemplateStyle>,
    ) -> u64 {
        prim.data_mut().id = alloc_primitive_id();
        prim.data_mut().pane_id = session.current_pane_id;
        if let Some(style) = last_style {
            apply_last_style_snapshot(&mut prim, style);
        }
        if let Some(variant) = session.tool_variant.clone() {
            Self::apply_variant_to_primitive(&mut prim, &variant);
        }
        prim.data_mut().symbol = b.symbol.clone();
        let id = prim.data().id;
        b.primitives.push(prim);
        session.selected = Some(id);
        id
    }

    fn apply_variant_to_primitive(prim: &mut Box<dyn Primitive>, variant: &str) {
        if prim.type_id() == "emoji" {
            if let Some(emoji_type) = EmojiType::from_id(variant) {
                let json = prim.to_json();
                if let Ok(mut emoji) = serde_json::from_str::<Emoji>(&json) {
                    emoji.emoji_type = emoji_type;
                    *prim = Box::new(emoji);
                }
            }
        }
    }

    /// Handle a click at the given data coordinates. Returns true if a
    /// primitive was created.
    pub(crate) fn on_canvas_click(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, ts_ms: i64, price: f64) -> bool {
        let tool_id = match self.current_tool(bid) {
            Some(id) => id.to_string(),
            None => return false,
        };
        let last_style = self.last_used_style_for(bubbles, bid, &tool_id);

        let registry = PrimitiveRegistry::global().read().unwrap();
        let click_behavior = match registry.click_behavior(&tool_id) {
            Some(cb) => cb,
            None => return false,
        };

        let Some(b) = bubbles.bubble_mut(bid) else { return false };
        let session = self.session_mut(bid);

        match click_behavior {
            ClickBehavior::SingleClick => {
                if let Some(prim) = registry.create(&tool_id, &[(ts_ms, price)], Some(&session.default_color)) {
                    Self::finalize_new_primitive(b, session, prim, last_style.as_ref());
                    session.current_tool = None;
                    session.tool_variant = None;
                    session.current_pane_id = None;
                    return true;
                }
                false
            }
            ClickBehavior::TwoPoint | ClickBehavior::ClickDrag => {
                match &mut session.state {
                    DrawingState::Idle => {
                        session.state = DrawingState::Creating { tool_id: tool_id.clone(), points: vec![(ts_ms, price)] };
                        false
                    }
                    DrawingState::Creating { points, tool_id: creating_tool } if creating_tool == &tool_id => {
                        points.push((ts_ms, price));
                        let points = points.clone();
                        if let Some(prim) = registry.create(&tool_id, &points, Some(&session.default_color)) {
                            Self::finalize_new_primitive(b, session, prim, last_style.as_ref());
                            session.state = DrawingState::Idle;
                            session.current_tool = None;
                            session.tool_variant = None;
                            session.current_pane_id = None;
                            return true;
                        }
                        session.state = DrawingState::Idle;
                        false
                    }
                    _ => {
                        session.state = DrawingState::Creating { tool_id: tool_id.clone(), points: vec![(ts_ms, price)] };
                        false
                    }
                }
            }
            ClickBehavior::ThreePoint => {
                Self::on_click_n_point(b, session, &registry, &tool_id, ts_ms, price, last_style.as_ref(), 3)
            }
            ClickBehavior::FourPoint => {
                Self::on_click_n_point(b, session, &registry, &tool_id, ts_ms, price, last_style.as_ref(), 4)
            }
            ClickBehavior::MultiPoint(min_points) => {
                Self::on_click_n_point(b, session, &registry, &tool_id, ts_ms, price, last_style.as_ref(), min_points as usize)
            }
            ClickBehavior::FreehandDrag => false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn on_click_n_point(
        b: &mut crate::bubble::DataBubble,
        session: &mut BubbleDrawingSession,
        registry: &PrimitiveRegistry,
        tool_id: &str,
        ts_ms: i64,
        price: f64,
        last_style: Option<&TemplateStyle>,
        min_points: usize,
    ) -> bool {
        match &mut session.state {
            DrawingState::Idle => {
                session.state = DrawingState::Creating { tool_id: tool_id.to_string(), points: vec![(ts_ms, price)] };
                false
            }
            DrawingState::Creating { points, tool_id: creating_tool } if creating_tool == tool_id => {
                points.push((ts_ms, price));
                if points.len() >= min_points {
                    let points = points.clone();
                    if let Some(prim) = registry.create(tool_id, &points, Some(&session.default_color)) {
                        Self::finalize_new_primitive(b, session, prim, last_style);
                        session.state = DrawingState::Idle;
                        session.current_tool = None;
                        session.current_pane_id = None;
                        return true;
                    }
                    session.state = DrawingState::Idle;
                }
                false
            }
            _ => {
                session.state = DrawingState::Creating { tool_id: tool_id.to_string(), points: vec![(ts_ms, price)] };
                false
            }
        }
    }

    pub(crate) fn finish_multipoint(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) -> bool {
        let session = self.session_mut(bid);
        let (tool_id, points) = match &session.state {
            DrawingState::Creating { tool_id, points } => (tool_id.clone(), points.clone()),
            _ => {
                session.state = DrawingState::Idle;
                return false;
            }
        };
        let last_style = self.last_used_style_for(bubbles, bid, &tool_id);
        let registry = PrimitiveRegistry::global().read().unwrap();
        if let Some(ClickBehavior::MultiPoint(min)) = registry.click_behavior(&tool_id) {
            if points.len() >= min as usize {
                if let Some(prim) = registry.create(&tool_id, &points, None) {
                    drop(registry);
                    let Some(b) = bubbles.bubble_mut(bid) else { return false };
                    let session = self.session_mut(bid);
                    Self::finalize_new_primitive(b, session, prim, last_style.as_ref());
                    session.state = DrawingState::Idle;
                    session.current_tool = None;
                    session.current_pane_id = None;
                    return true;
                }
            }
        }
        self.session_mut(bid).state = DrawingState::Idle;
        false
    }

    // =========================================================================
    // Freehand
    // =========================================================================

    pub(crate) fn start_freehand(&mut self, bid: BubbleId, ts_ms: i64, price: f64) -> bool {
        let session = self.session_mut(bid);
        let tool_id = match session.current_tool.clone() {
            Some(id) => id,
            None => return false,
        };
        let registry = PrimitiveRegistry::global().read().unwrap();
        let is_freehand = registry.get(&tool_id).map(|m| m.click_behavior == ClickBehavior::FreehandDrag).unwrap_or(false);
        drop(registry);
        if !is_freehand {
            return false;
        }
        session.state.cancel();
        session.state = DrawingState::Creating { tool_id, points: vec![(ts_ms, price)] };
        true
    }

    pub(crate) fn add_freehand_point(&mut self, bid: BubbleId, ts_ms: i64, price: f64, bar_interval_ms: i64) -> bool {
        const MIN_TS_FRAC: f64 = 0.15;
        const MIN_PRICE_RATIO: f64 = 0.0002;
        let min_ts_dist = (bar_interval_ms as f64 * MIN_TS_FRAC) as i64;

        let session = self.session_mut(bid);
        if let DrawingState::Creating { tool_id, points } = &mut session.state {
            let registry = PrimitiveRegistry::global().read().unwrap();
            if let Some(meta) = registry.get(tool_id) {
                if meta.click_behavior == ClickBehavior::FreehandDrag {
                    if let Some(&(last_ts, last_price)) = points.last() {
                        let ts_dist = (ts_ms - last_ts).abs();
                        let price_dist = (price - last_price).abs();
                        let avg_price = (price + last_price).abs() / 2.0;
                        let price_threshold = avg_price * MIN_PRICE_RATIO;
                        if ts_dist < min_ts_dist && price_dist < price_threshold {
                            return false;
                        }
                    }
                    points.push((ts_ms, price));
                    return true;
                }
            }
        }
        false
    }

    pub(crate) fn complete_freehand(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) -> bool {
        let session = self.session_mut(bid);
        let (tool_id_clone, points_clone, point_count) = match &session.state {
            DrawingState::Creating { tool_id, points } => (tool_id.clone(), points.clone(), points.len()),
            _ => return false,
        };

        if point_count < 2 {
            session.state = DrawingState::Idle;
            *session.preview_cache.lock().expect("preview_cache mutex poisoned") = None;
            return false;
        }

        let last_style = self.last_used_style_for(bubbles, bid, &tool_id_clone);
        let registry = PrimitiveRegistry::global().read().unwrap();
        let Some(meta) = registry.get(&tool_id_clone) else { return false };
        if meta.click_behavior != ClickBehavior::FreehandDrag {
            return false;
        }
        let color_str = last_style.as_ref().and_then(|s| s.color.clone()).unwrap_or_else(|| meta.default_color.to_string());
        let Some(prim) = registry.create(&tool_id_clone, &points_clone, Some(&color_str)) else { return false };
        drop(registry);

        let Some(b) = bubbles.bubble_mut(bid) else { return false };
        let session = self.session_mut(bid);
        Self::finalize_new_primitive(b, session, prim, last_style.as_ref());
        session.state = DrawingState::Idle;
        *session.preview_cache.lock().expect("preview_cache mutex poisoned") = None;
        session.current_pane_id = None;
        true
    }

    // =========================================================================
    // Preview
    // =========================================================================

    /// Look up the last-used style for `tool_id` from the bubble's shared
    /// style store (`DataBubble.last_used_style`).
    fn last_used_style_for(&self, bubbles: &BubbleStore, bid: BubbleId, tool_id: &str) -> Option<TemplateStyle> {
        let b = bubbles.bubble(bid)?;
        b.last_used_style.read().ok()?.get(tool_id).cloned()
    }

    /// Create a preview primitive for the bubble's current drawing state
    /// (accumulated points + cursor position). `&self` — callable from the
    /// read-only `Drawings` facade; the preview cache writes through the
    /// session's `Mutex` so render code never needs a write lock just to
    /// draw a ghost shape under the cursor.
    pub(crate) fn create_preview(
        &self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        cursor_ts_ms: i64,
        cursor_price: f64,
    ) -> Option<Box<dyn Primitive>> {
        let session = self.session(bid)?;
        let DrawingState::Creating { tool_id, points } = &session.state else {
            *session.preview_cache.lock().expect("preview_cache mutex poisoned") = None;
            return None;
        };
        if points.is_empty() {
            return None;
        }

        let style_snapshot = self.last_used_style_for(bubbles, bid, tool_id);
        let cur_fp = style_snapshot.as_ref().map(style_fingerprint).unwrap_or(0);
        let cur_key = (
            tool_id.as_str(),
            points.len(),
            cursor_ts_ms as u64,
            cursor_price.to_bits(),
            cur_fp,
            session.tool_variant.as_deref(),
        );

        {
            let cache = session.preview_cache.lock().expect("preview_cache mutex poisoned");
            if let Some(ref c) = *cache {
                if c.tool_id == cur_key.0
                    && c.points_len == cur_key.1
                    && c.cursor == (cur_key.2, cur_key.3)
                    && c.style_fp == cur_key.4
                    && c.variant.as_deref() == cur_key.5
                {
                    return Some(c.primitive.clone_box());
                }
            }
        }

        let registry = PrimitiveRegistry::global().read().unwrap();
        let meta = registry.get(tool_id)?;

        let mut preview_points = points.clone();
        preview_points.push((cursor_ts_ms, cursor_price));

        let min_points = match meta.click_behavior {
            ClickBehavior::SingleClick => 1,
            ClickBehavior::TwoPoint | ClickBehavior::ClickDrag => 2,
            ClickBehavior::ThreePoint => 3,
            ClickBehavior::FourPoint => 4,
            ClickBehavior::MultiPoint(n) => n as usize,
            ClickBehavior::FreehandDrag => return None,
        };
        while preview_points.len() < min_points {
            preview_points.push((cursor_ts_ms, cursor_price));
        }

        let seed_color_owned: String;
        let seed_color = match style_snapshot.as_ref().and_then(|s| s.color.as_deref()) {
            Some(c) => c,
            None => {
                seed_color_owned = session.default_color.clone();
                &seed_color_owned
            }
        };
        let mut prim = (meta.factory)(&preview_points, seed_color);

        if let Some(ref style) = style_snapshot {
            apply_last_style_snapshot(&mut prim, style);
        }
        if let Some(variant) = &session.tool_variant {
            Self::apply_variant_to_primitive(&mut prim, variant);
        }

        let cloned = prim.clone_box();
        *session.preview_cache.lock().expect("preview_cache mutex poisoned") = Some(PreviewCache {
            tool_id: tool_id.clone(),
            points_len: points.len(),
            cursor: (cursor_ts_ms as u64, cursor_price.to_bits()),
            style_fp: cur_fp,
            variant: session.tool_variant.clone(),
            primitive: prim,
        });

        Some(cloned)
    }

    // =========================================================================
    // Mutation — by stable prim_id
    // =========================================================================

    pub(crate) fn translate(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, dt_ms: i64, dprice: f64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.translate(dt_ms, dprice);
        }
    }

    pub(crate) fn apply_property(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        prim_id: u64,
        kind: PropertyKind,
        prop_id: &str,
        value: &PropertyValue,
    ) -> bool {
        let Some(b) = bubbles.bubble_mut(bid) else { return false };
        let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) else { return false };
        match kind {
            PropertyKind::Style => {
                prim.apply_style_property(prop_id, value);
                true
            }
            PropertyKind::Text => prim.apply_text_property(prop_id, value),
            PropertyKind::Level => prim.apply_level_property(prop_id, value),
        }
    }

    pub(crate) fn set_points(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, points: &[(i64, f64)]) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.set_points(points);
        }
    }

    pub(crate) fn set_data(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, data: &PrimitiveData) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let prim_data = prim.data_mut();
            prim_data.color = data.color.clone();
            prim_data.width = data.width;
            prim_data.style = data.style;
            prim_data.visible = data.visible;
            prim_data.locked = data.locked;
            prim_data.display_name = data.display_name.clone();
            prim_data.text = data.text.clone();
            prim_data.timeframe_visibility = data.timeframe_visibility.clone();
        }
    }

    pub(crate) fn set_level_configs(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, configs: Vec<FibLevelConfig>) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.set_level_configs(configs);
        }
    }

    pub(crate) fn set_visibility(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, visible: bool) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.set_visible(visible);
        }
    }

    pub(crate) fn toggle_visibility(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let v = !prim.is_visible();
            prim.set_visible(v);
        }
    }

    pub(crate) fn set_lock(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, locked: bool) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.set_locked(locked);
        }
    }

    pub(crate) fn toggle_lock_primitive(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let l = !prim.is_locked();
            prim.set_locked(l);
        }
    }

    pub(crate) fn set_color(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, color: &str) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().color.stroke = color.to_string();
        }
    }

    pub(crate) fn set_width(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, width: f64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().width = width.clamp(1.0, 20.0);
        }
    }

    pub(crate) fn set_style(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, style: LineStyle) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().style = style;
        }
    }

    pub(crate) fn set_sync_mode(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, mode: SyncMode) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().sync_mode = mode;
        }
    }

    pub(crate) fn set_text_content(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, content: &str) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.content = content.to_string();
            } else {
                data.text = Some(PrimitiveText::new(content));
            }
        }
    }

    pub(crate) fn set_text_font_size(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, font_size: f64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.font_size = font_size.clamp(8.0, 72.0);
            } else {
                let mut new_text = PrimitiveText::new("");
                new_text.font_size = font_size.clamp(8.0, 72.0);
                data.text = Some(new_text);
            }
        }
    }

    pub(crate) fn set_text_bold(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, bold: bool) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.bold = bold;
            } else {
                let mut new_text = PrimitiveText::new("");
                new_text.bold = bold;
                data.text = Some(new_text);
            }
        }
    }

    pub(crate) fn set_text_italic(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, italic: bool) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.italic = italic;
            } else {
                let mut new_text = PrimitiveText::new("");
                new_text.italic = italic;
                data.text = Some(new_text);
            }
        }
    }

    pub(crate) fn set_text_color(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, color: &str) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.color = Some(color.to_string());
            } else {
                let mut new_text = PrimitiveText::new("");
                new_text.color = Some(color.to_string());
                data.text = Some(new_text);
            }
        }
    }

    pub(crate) fn set_text_v_align(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, align: TextAlign) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.v_align = align;
            } else {
                let mut new_text = PrimitiveText::new("");
                new_text.v_align = align;
                data.text = Some(new_text);
            }
        }
    }

    pub(crate) fn set_text_h_align(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, align: TextAlign) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            let data = prim.data_mut();
            if let Some(ref mut text) = data.text {
                text.h_align = align;
            } else {
                let mut new_text = PrimitiveText::new("");
                new_text.h_align = align;
                data.text = Some(new_text);
            }
        }
    }

    pub(crate) fn apply_color_by_field(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, field: &str, color: &str) {
        match field {
            "stroke_color" => self.set_color(bubbles, bid, prim_id, color),
            "text_color" => self.set_text_color(bubbles, bid, prim_id, color),
            "fill_color" => {
                let Some(b) = bubbles.bubble_mut(bid) else { return };
                if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
                    prim.data_mut().color.fill = Some(color.to_string());
                }
            }
            _ => eprintln!("[DrawingManager] Unknown color field: {field}"),
        }
    }

    pub(crate) fn set_timeframe_visibility(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, config: TimeframeVisibilityConfig) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().timeframe_visibility = Some(config);
        }
    }

    pub(crate) fn set_show_on_all_timeframes(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().timeframe_visibility = Some(TimeframeVisibilityConfig::all());
        }
    }

    pub(crate) fn apply_template_style(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, template: &SettingsTemplate) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) else { return };
        let data = prim.data_mut();
        if let Some(ref color) = template.style.color {
            data.color.stroke = color.clone();
        }
        if let Some(width) = template.style.width {
            data.width = width;
        }
        if let Some(ref line_style) = template.style.line_style {
            data.style = match line_style.as_str() {
                "dashed" => LineStyle::Dashed,
                "dotted" => LineStyle::Dotted,
                "large_dashed" => LineStyle::LargeDashed,
                "sparse_dotted" => LineStyle::SparseDotted,
                _ => LineStyle::Solid,
            };
        }
        if let Some(ref tfv) = template.timeframe_visibility {
            data.timeframe_visibility = Some(tfv.clone());
        }
    }

    // =========================================================================
    // Save last-used style
    // =========================================================================

    pub(crate) fn save_last_style_from_data(&mut self, bubbles: &BubbleStore, bid: BubbleId, data: &PrimitiveData) {
        let Some(b) = bubbles.bubble(bid) else { return };
        let style = TemplateStyle {
            color: Some(data.color.stroke.clone()),
            width: Some(data.width),
            line_style: Some(data.style.as_str().to_string()),
            fill_color: data.color.fill.clone(),
            fill_opacity: None,
            show_labels: None,
            show_prices: None,
            style_properties: Vec::new(),
        };
        if let Ok(mut store) = b.last_used_style.write() {
            store.insert(data.type_id.clone(), style);
        }
    }

    pub(crate) fn save_last_style_for_primitive(&mut self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble(bid) else { return };
        let Some(prim) = b.primitives.iter().find(|p| p.data().id == prim_id) else { return };
        let data = prim.data();
        let type_id = data.type_id.clone();
        let style_props: Vec<(String, PropertyValue)> = prim
            .style_properties()
            .into_iter()
            .filter(|prop| !prop.readonly)
            .map(|prop| (prop.id, prop.value))
            .collect();
        let style = TemplateStyle {
            color: Some(data.color.stroke.clone()),
            width: Some(data.width),
            line_style: Some(data.style.as_str().to_string()),
            fill_color: data.color.fill.clone(),
            fill_opacity: None,
            show_labels: None,
            show_prices: None,
            style_properties: style_props,
        };
        if let Ok(mut store) = b.last_used_style.write() {
            store.insert(type_id, style);
        }
    }

    pub(crate) fn last_style_for(&self, bubbles: &BubbleStore, bid: BubbleId, tool_id: &str) -> Option<TemplateStyle> {
        self.last_used_style_for(bubbles, bid, tool_id)
    }

    /// Bulk-load last-used drawing styles from a persisted snapshot map.
    pub(crate) fn load_last_styles(&self, bubbles: &BubbleStore, bid: BubbleId, stored: &HashMap<String, serde_json::Value>) {
        let Some(b) = bubbles.bubble(bid) else { return };
        let Ok(mut store) = b.last_used_style.write() else { return };
        for (type_id, value) in stored {
            if store.contains_key(type_id) {
                continue;
            }
            match serde_json::from_value::<TemplateStyle>(value.clone()) {
                Ok(style) => {
                    store.insert(type_id.clone(), style);
                }
                Err(e) => {
                    eprintln!("[DrawingManager] load_last_styles: failed to deserialize style for '{type_id}': {e}");
                }
            }
        }
    }

    // =========================================================================
    // Delete
    // =========================================================================

    pub(crate) fn delete(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) -> bool {
        let Some(b) = bubbles.bubble_mut(bid) else { return false };
        let before = b.primitives.len();
        b.primitives.retain(|p| p.data().id != prim_id);
        let removed = b.primitives.len() != before;
        if removed {
            let session = self.session_mut(bid);
            if session.selected == Some(prim_id) {
                session.selected = None;
            }
            if session.dragging == Some(prim_id) {
                session.dragging = None;
                session.drag_start = None;
            }
        }
        removed
    }

    pub(crate) fn delete_selected(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) -> bool {
        let Some(prim_id) = self.selected(bid) else { return false };
        self.delete(bubbles, bid, prim_id)
    }

    pub(crate) fn delete_all(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) {
        if let Some(b) = bubbles.bubble_mut(bid) {
            b.primitives.clear();
        }
        let session = self.session_mut(bid);
        session.selected = None;
        session.dragging = None;
        session.drag_start = None;
        *session.preview_cache.lock().expect("preview_cache mutex poisoned") = None;
    }

    pub(crate) fn clone_primitive(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) -> Option<u64> {
        let Some(b) = bubbles.bubble_mut(bid) else { return None };
        let idx = Self::find_index_by_id(b, prim_id)?;
        let mut cloned = b.primitives[idx].clone();
        cloned.data_mut().id = alloc_primitive_id();
        let new_id = cloned.data().id;
        b.primitives.push(cloned);
        self.session_mut(bid).selected = Some(new_id);
        Some(new_id)
    }

    pub(crate) fn clone_selected(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) -> Option<u64> {
        let prim_id = self.selected(bid)?;
        let Some(b) = bubbles.bubble_mut(bid) else { return None };
        let idx = Self::find_index_by_id(b, prim_id)?;
        let mut cloned = b.primitives[idx].clone();
        cloned.data_mut().id = alloc_primitive_id();
        cloned.translate(300_000i64, 0.0);
        let new_id = cloned.data().id;
        b.primitives.push(cloned);
        self.session_mut(bid).selected = Some(new_id);
        Some(new_id)
    }

    // =========================================================================
    // Z-order (relative addressing, no raw idx crosses the boundary)
    // =========================================================================

    pub(crate) fn bring_to_front(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let Some(idx) = Self::find_index_by_id(b, prim_id) else { return };
        if idx != b.primitives.len() - 1 {
            let prim = b.primitives.remove(idx);
            b.primitives.push(prim);
        }
    }

    pub(crate) fn send_to_back(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let Some(idx) = Self::find_index_by_id(b, prim_id) else { return };
        if idx != 0 {
            let prim = b.primitives.remove(idx);
            b.primitives.insert(0, prim);
        }
    }

    /// Move `prim_id` to be positioned immediately before `before_prim_id`
    /// in the z-order list. `None` moves it to the back.
    pub(crate) fn reorder_before(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, before_prim_id: Option<u64>) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let Some(old_idx) = Self::find_index_by_id(b, prim_id) else { return };
        let new_idx = match before_prim_id {
            Some(target) => Self::find_index_by_id(b, target).unwrap_or(0),
            None => 0,
        };
        if old_idx == new_idx {
            return;
        }
        let prim = b.primitives.remove(old_idx);
        let insert_at = if new_idx > old_idx { new_idx - 1 } else { new_idx };
        b.primitives.insert(insert_at.min(b.primitives.len()), prim);
    }

    pub(crate) fn bring_selected_to_front(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) {
        let Some(prim_id) = self.selected(bid) else { return };
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let max_z = b.primitives.iter().map(|p| p.data().z_order).max().unwrap_or(0);
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().z_order = max_z + 1;
        }
    }

    pub(crate) fn send_selected_to_back(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) {
        let Some(prim_id) = self.selected(bid) else { return };
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let min_z = b.primitives.iter().map(|p| p.data().z_order).min().unwrap_or(0);
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().z_order = min_z - 1;
        }
    }

    pub(crate) fn move_up(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().z_order += 1;
        }
    }

    pub(crate) fn move_down(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().z_order -= 1;
        }
    }

    // =========================================================================
    // Drag and drop
    // =========================================================================

    pub(crate) fn start_drag(&mut self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64, ts_ms: i64, price: f64) -> bool {
        let Some(b) = bubbles.bubble(bid) else { return false };
        let Some(prim) = b.primitives.iter().find(|p| p.data().id == prim_id) else { return false };
        let locked = prim.data().locked;
        let session = self.session_mut(bid);
        if session.locked || locked {
            return false;
        }
        session.dragging = Some(prim_id);
        session.selected = Some(prim_id);
        session.drag_type = DragType::Move;
        session.drag_start = Some((ts_ms, price));
        true
    }

    pub(crate) fn start_control_point_drag(
        &mut self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        prim_id: u64,
        control_point: ControlPointType,
        ts_ms: i64,
        price: f64,
    ) -> bool {
        let Some(b) = bubbles.bubble(bid) else { return false };
        let Some(prim) = b.primitives.iter().find(|p| p.data().id == prim_id) else { return false };
        let locked = prim.data().locked;
        let session = self.session_mut(bid);
        if session.locked || locked {
            return false;
        }
        session.dragging = Some(prim_id);
        session.selected = Some(prim_id);
        session.drag_type = DragType::ControlPoint(control_point);
        session.drag_start = Some((ts_ms, price));
        true
    }

    pub(crate) fn update_drag(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, current_ts_ms: i64, current_price: f64) {
        let Some(session) = self.sessions.get_mut(&bid) else { return };
        let (Some(prim_id), Some((start_ts, start_price))) = (session.dragging, session.drag_start) else { return };
        let drag_type = session.drag_type.clone();
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) else { return };
        match drag_type {
            DragType::Move => {
                let ts_delta = current_ts_ms - start_ts;
                let price_delta = current_price - start_price;
                if ts_delta.abs() > 0 || price_delta.abs() > 0.0001 {
                    prim.translate(ts_delta, price_delta);
                    self.session_mut(bid).drag_start = Some((current_ts_ms, current_price));
                }
            }
            DragType::ControlPoint(point_type) => {
                prim.move_control_point(point_type, current_ts_ms, current_price);
                self.session_mut(bid).drag_start = Some((current_ts_ms, current_price));
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub(crate) fn update_drag_screen(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        screen_x: f64,
        screen_y: f64,
        current_ts_ms: i64,
        current_price: f64,
        bars: &[Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) {
        let Some(session) = self.sessions.get_mut(&bid) else { return };
        let (Some(prim_id), Some((start_ts, start_price))) = (session.dragging, session.drag_start) else { return };
        let drag_type = session.drag_type.clone();
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) else { return };
        match drag_type {
            DragType::Move => {
                let ts_delta = current_ts_ms - start_ts;
                let price_delta = current_price - start_price;
                if ts_delta.abs() > 0 || price_delta.abs() > 0.0001 {
                    prim.translate(ts_delta, price_delta);
                    self.session_mut(bid).drag_start = Some((current_ts_ms, current_price));
                }
            }
            DragType::ControlPoint(point_type) => {
                prim.move_control_point_screen(point_type, screen_x, screen_y, bars, viewport, price_scale);
                self.session_mut(bid).drag_start = Some((current_ts_ms, current_price));
            }
        }
    }

    pub(crate) fn end_drag(&mut self, bid: BubbleId) {
        if let Some(s) = self.sessions.get_mut(&bid) {
            s.dragging = None;
            s.drag_type = DragType::Move;
            s.drag_start = None;
        }
    }

    // =========================================================================
    // External insertion (preset/REST load, undo/redo restore)
    // =========================================================================

    /// Add an externally-created primitive with proper ID assignment.
    /// `pane_id` is preserved from the deserialized state (not overwritten)
    /// so drawings restored from a preset keep their pane assignment.
    pub(crate) fn add_external(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, mut prim: Box<dyn Primitive>) -> u64 {
        prim.data_mut().id = alloc_primitive_id();
        let Some(b) = bubbles.bubble_mut(bid) else { return prim.data().id };
        if prim.data().symbol.is_empty() {
            prim.data_mut().symbol = b.symbol.clone();
        }
        let id = prim.data().id;
        b.primitives.push(prim);
        self.session_mut(bid).selected = Some(id);
        id
    }

    /// Insert a primitive at a specific list position, type_id + points +
    /// full data (undo/redo recreation of a deleted primitive). The
    /// position is preserved internally so z-order-dependent operations
    /// replay identically; it is not part of the public id-keyed contract.
    pub(crate) fn insert_at(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        index: usize,
        type_id: &str,
        points: &[(i64, f64)],
        data: &PrimitiveData,
    ) -> Option<u64> {
        let registry = PrimitiveRegistry::global().read().unwrap();
        let mut prim = registry.create(type_id, points, Some(&data.color.stroke))?;
        drop(registry);
        *prim.data_mut() = data.clone();
        let id = prim.data().id;
        let Some(b) = bubbles.bubble_mut(bid) else { return None };
        if index <= b.primitives.len() {
            b.primitives.insert(index, prim);
        } else {
            b.primitives.push(prim);
        }
        Some(id)
    }

    pub(crate) fn replace_primitive_from_json(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, type_id: &str, json: &str) -> bool {
        let Some(b) = bubbles.bubble_mut(bid) else { return false };
        let Some(idx) = Self::find_index_by_id(b, prim_id) else { return false };
        let registry = PrimitiveRegistry::global().read().unwrap();
        if let Some(mut prim) = registry.from_json(type_id, json) {
            prim.data_mut().id = prim_id;
            b.primitives[idx] = prim;
            true
        } else {
            false
        }
    }

    // =========================================================================
    // Symbol swap (Audit-A N6 — bubble-scoped compound)
    // =========================================================================

    pub(crate) fn swap_symbol(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        restore_snaps: Option<&[PrimitiveSnapshot]>,
    ) -> SwapSymbolResult {
        let Some(b) = bubbles.bubble_mut(bid) else {
            return SwapSymbolResult { old_primitives: Vec::new(), restored: 0 };
        };
        let old_primitives: Vec<PrimitiveSnapshot> = b.primitives.iter().map(|p| PrimitiveSnapshot::from_primitive(p.as_ref())).collect();

        b.symbol = symbol.to_string();
        b.exchange = exchange.to_string();
        b.account_type = account_type.to_string();
        b.primitives.clear();

        let mut restored = 0usize;
        if let Some(snaps) = restore_snaps {
            if let Ok(reg) = PrimitiveRegistry::global().read() {
                for snap in snaps {
                    if let Some(mut prim) = reg.from_json(&snap.type_id, &snap.json) {
                        prim.data_mut().id = alloc_primitive_id();
                        if prim.data().symbol.is_empty() {
                            prim.data_mut().symbol = b.symbol.clone();
                        }
                        b.primitives.push(prim);
                        restored += 1;
                    }
                }
            }
        }

        let session = self.session_mut(bid);
        session.selected = None;
        session.dragging = None;
        session.drag_start = None;
        *session.preview_cache.lock().expect("preview_cache mutex poisoned") = None;

        SwapSymbolResult { old_primitives, restored }
    }

    // =========================================================================
    // Cross-bubble adoption (desync: stash/restore primitives wholesale)
    // =========================================================================

    /// Replace `bid`'s entire primitive list with `prims`. Used by the
    /// desync flow when a panel's bubble membership pointer flips from a
    /// tag bubble to a fresh private auto-bubble and the home drawings must
    /// be carried over.
    pub(crate) fn adopt(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prims: Vec<Box<dyn Primitive>>) {
        if let Some(b) = bubbles.bubble_mut(bid) {
            b.primitives = prims;
        }
        let session = self.session_mut(bid);
        session.selected = None;
        session.dragging = None;
        session.drag_start = None;
    }

    // =========================================================================
    // Legacy timestamp migration
    // =========================================================================

    fn migrate_legacy_timestamps_for_primitive(prim: &mut Box<dyn Primitive>) {
        let pts = prim.points();
        if pts.is_empty() {
            return;
        }
        let all_zero = pts.iter().all(|(ts, _)| *ts == 0);
        if !all_zero {
            return;
        }
        let legacy_ts = prim.data().point_timestamps.clone();
        if legacy_ts.is_empty() {
            return;
        }
        let migrated: Vec<(i64, f64)> = pts
            .iter()
            .enumerate()
            .map(|(i, (_, price))| (legacy_ts.get(i).copied().unwrap_or(0) * 1000, *price))
            .collect();
        prim.set_points(&migrated);
    }

    pub(crate) fn migrate_legacy_timestamps(&self, bubbles: &mut BubbleStore, bid: BubbleId) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        for prim in &mut b.primitives {
            Self::migrate_legacy_timestamps_for_primitive(prim);
        }
    }

    /// Deduplicate primitive IDs globally, across every bubble. Old presets
    /// may carry group primitives with identical IDs (from historical clone
    /// paths that didn't re-ID). Returns the count of renumbered primitives.
    pub(crate) fn dedup_primitive_ids(&self, bubbles: &mut BubbleStore) -> u32 {
        let mut total = 0u32;
        for group in bubbles.bubbles_mut() {
            let mut seen_ids = std::collections::HashSet::new();
            for p in &mut group.primitives {
                let id = p.data().id;
                if !seen_ids.insert(id) {
                    p.data_mut().id = alloc_primitive_id();
                    total += 1;
                }
            }
        }
        total
    }

    // =========================================================================
    // Test-harness-only lifecycle (not part of the public facade surface)
    // =========================================================================

    /// Drop a bubble's editing session entirely (test-harness cleanup only).
    pub(crate) fn drop_bubble(&mut self, bid: BubbleId) {
        self.sessions.remove(&bid);
    }
}

/// Item for primitive list (Object Tree).
#[derive(Clone, Debug)]
pub struct PrimitiveListItem {
    pub index: usize,
    pub id: u64,
    pub type_id: String,
    pub display_name: String,
    pub color: String,
    pub visible: bool,
    pub locked: bool,
    pub selected: bool,
}
