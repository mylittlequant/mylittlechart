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

use std::collections::{HashMap, HashSet};
use std::hash::{DefaultHasher, Hash, Hasher};
use std::sync::Mutex;

use mlc_core::drawing::primitives_v2::config::{
    FibLevelConfig, PropertyKind, PropertyValue, TemplateStyle,
    TimeframeVisibilityConfig,
};
use mlc_core::drawing::primitives_v2::elliott::{
    adoptable_children, adopted_parent_degree, child_kinds, child_seed, conversion_kinds, kind_id_of, kind_label,
    resample_polyline, slot_at_ordinal, slot_ordinal, slot_span, split_kind_id, successor_kinds,
    successor_seed, predecessor_kinds, predecessor_seed,
    promote_chain_shape, WaveLink, WaveSlot,
};
use mlc_core::drawing::primitives_v2::fibonacci::retracement::level_set_id_for_composition;
use mlc_core::drawing::primitives_v2::icons::emoji::{Emoji, EmojiType};
use mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry;
use mlc_core::drawing::primitives_v2::{
    ClickBehavior, ControlPoint, ControlPointType, CorrectionType, ElliottCorrection,
    ElliottDoubleCombo, ElliottImpulse, ElliottTriangle, ElliottTripleCombo, HitTestResult, ImpulseKind,
    LineStyle, Primitive, PrimitiveData, PrimitiveExt, PrimitiveText, SyncMode, TextAlign,
};
use mlc_core::drawing::{alloc_primitive_id, PrimitiveSnapshot, StyleReference};
use mlc_core::i18n::ConceptKey;
use mlc_core::ids::BubbleId;
use mlc_core::instrument::InstrumentAddress;
use mlc_core::state::ewa_engine::ewa_engine_config;
use mlc_core::state::selected_config::SelectedPrimitiveConfig;
use mlc_core::{Bar, PriceScale, Viewport};

use crate::bubble::BubbleStore;

use super::facade::SwapSymbolResult;

/// Pack a `Vec<(i64, f64)>` into a fixed-size `[(i64, f64); N]`, taking the
/// first `N` elements and zero-filling any that are missing — used to hand
/// `ElliottImpulse::new`/`ElliottCorrection::new` (which take fixed-size
/// arrays) the output of `ChildSeed::interpolate(N)` (E2). Never panics:
/// `interpolate(N)` always returns exactly `N` points, but this stays
/// correct even if that invariant ever changes, instead of a `copy_from_slice`
/// that would panic on a length mismatch.
fn points_array<const N: usize>(pts: Vec<(i64, f64)>) -> [(i64, f64); N] {
    let mut arr = [(0i64, 0.0f64); N];
    for (slot, p) in arr.iter_mut().zip(pts) {
        *slot = p;
    }
    arr
}

/// Pad `pts` out to exactly `n` points by repeating its own LAST point —
/// used by `DrawingManager::promote_wave_chain` when a chain covers fewer
/// slots than the parent type's own fixed point count (e.g. only wave 1 and
/// wave 2 of a coarser 5-wave impulse exist so far, so `pts.len() == 3` but
/// `ElliottImpulse` needs 6). Collapsing the not-yet-realized trailing
/// points onto the chain's own last boundary point keeps the parent a
/// well-formed, draggable shape rather than zero-filling them to the
/// origin — the same degenerate-but-visible convention
/// `ChildSeed::interpolate`'s own `n == 1` case documents. A no-op (a plain
/// clone) when `pts.len() >= n` already.
fn pad_points_to(pts: &[(i64, f64)], n: usize) -> Vec<(i64, f64)> {
    let mut out = pts.to_vec();
    if let Some(&last) = pts.last() {
        while out.len() < n {
            out.push(last);
        }
    }
    out
}

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
    // Empty means either "this primitive type has no ladder" or "captured
    // before this field existed" — either way, leave prim's own ladder
    // (its factory default) alone rather than clearing it.
    if !style.level_configs.is_empty() {
        prim.set_level_configs(style.level_configs.clone());
    }
    for (prop_id, value) in &style.text_properties {
        prim.apply_text_property(prop_id, value);
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
    // FibLevelConfig has no Hash impl (carries f64s) — Debug-format like the
    // PropertyValue entries above rather than adding one for a cache key.
    format!("{:?}", style.level_configs).hash(&mut h);
    for (k, v) in &style.text_properties {
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

    /// Walk `wave_follows` BACKWARD from `prim_id` to the chain's own head —
    /// the member nothing else in the chain precedes. Cycle-guarded (a
    /// malformed `follows` loop stops the walk rather than spinning
    /// forever); returns `prim_id` itself when it has no predecessor still
    /// present, or is not part of a chain at all.
    fn chain_head_id(b: &crate::bubble::DataBubble, prim_id: u64) -> u64 {
        let mut current = prim_id;
        let mut visited: HashSet<u64> = HashSet::new();
        while visited.insert(current) {
            let Some(p) = b.primitives.iter().find(|p| p.data().id == current) else { break };
            match p.wave_follows() {
                Some(prev_id) if b.primitives.iter().any(|q| q.data().id == prev_id) => current = prev_id,
                _ => break,
            }
        }
        current
    }

    /// Walk `wave_follows` FORWARD from `first_id`, collecting every chain
    /// member's own id in succession order (`first_id` itself first).
    /// Cycle-guarded the same way [`Self::chain_head_id`] is. Stops at the
    /// first id that does not resolve to a live primitive, or has no
    /// successor.
    fn chain_member_ids(b: &crate::bubble::DataBubble, first_id: u64) -> Vec<u64> {
        let mut ids = Vec::new();
        let mut current = first_id;
        let mut visited: HashSet<u64> = HashSet::new();
        while visited.insert(current) {
            if Self::find_index_by_id(b, current).is_none() {
                break;
            }
            ids.push(current);
            let next = b.primitives.iter()
                .find(|p| p.data().id != current && p.wave_follows() == Some(current))
                .map(|p| p.data().id);
            match next {
                Some(next_id) => current = next_id,
                None => break,
            }
        }
        ids
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

    /// Full serialized snapshot of one primitive — the shape undo/redo
    /// (`Command::CreatePrimitive`/`DeletePrimitive`) and clipboard paste
    /// both restore a primitive FROM, so every field the concrete type
    /// carries survives (an Elliott wave's `degree`/`link`/`follows`
    /// included — see `elliott::count`'s own module doc), never just the
    /// shared `PrimitiveData` header `get_data` alone would leave behind.
    pub(crate) fn get_snapshot(&self, bubbles: &BubbleStore, bid: BubbleId, prim_id: u64) -> Option<PrimitiveSnapshot> {
        let b = bubbles.bubble(bid)?;
        b.primitives.iter().find(|p| p.data().id == prim_id).map(|p| PrimitiveSnapshot::from_primitive(p.as_ref()))
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

    /// The id of the child primitive currently nested into `slot` of
    /// `parent_id`, if any — the wave-gesture menu's own "does this slot
    /// already hold a subdivision" read (E5, `ChartApp::try_wave_gesture` /
    /// `mlc-app-layout`'s `build_wave_menu`), asked BEFORE the primitive
    /// necessarily has a settings modal open, so it cannot reuse a modal's
    /// own cached state. Resolves through each primitive's own
    /// `wave_link()` — a plain linear scan, the same relation
    /// `restitch_wave_joints`'s own child-slot walk already reads, not a
    /// second index: a parent has at most a handful of slots, so this
    /// costs nothing and can never go stale the way a cached slot->child
    /// map could. `None` when the slot is empty, `parent_id` does not
    /// exist, or (should never happen — `create_wave_child`'s own choke
    /// point never links two children into the same slot) more than one
    /// primitive claims it, in which case the first match wins.
    pub(crate) fn wave_child_id(&self, bubbles: &BubbleStore, bid: BubbleId, parent_id: u64, slot: WaveSlot) -> Option<u64> {
        let b = bubbles.bubble(bid)?;
        b.primitives.iter()
            .find(|p| p.wave_link() == Some(WaveLink { parent_id, slot }))
            .map(|p| p.data().id)
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
        // E2 — wave-slot picker (two-step: slot, then kind). `wave_slots()`
        // is empty for every non-Elliott primitive; a `None` degree (also
        // non-Elliott) skips the loop below entirely rather than a
        // `.unwrap_or_default()` that would silently paper over an Elliott
        // primitive somehow missing its own degree. `wave_slot_kinds` is
        // built in the SAME pass, over the SAME slot list, so the two can
        // never disagree about which slots exist — a slot whose own
        // `child_kinds` comes back empty (the grammar allows nothing at
        // that position) is skipped from BOTH lists, never offered as a
        // pick target that would lead nowhere.
        let mut wave_slots: Vec<(String, String)> = Vec::new();
        let mut wave_slot_kinds: Vec<(String, Vec<(String, ConceptKey)>)> = Vec::new();
        if let Some(degree) = prim.wave_degree() {
            for &slot in prim.wave_slots() {
                let kinds = child_kinds(prim.as_ref(), slot);
                if kinds.is_empty() {
                    continue;
                }
                let slot_id = slot.id().to_string();
                wave_slots.push((slot_id.clone(), slot.label(degree).to_string()));
                let kind_pairs: Vec<(String, ConceptKey)> = kinds
                    .iter()
                    .filter_map(|&kind_id| kind_label(kind_id).map(|key| (kind_id.to_string(), key)))
                    .collect();
                wave_slot_kinds.push((slot_id, kind_pairs));
            }
        }
        // E3 — succession picker (the second inline control, "what comes
        // after this"). `display_name` is read off each candidate type's
        // OWN registered `PrimitiveMetadata` — the same source every other
        // primitive name in this codebase comes from — never a second name
        // table hand-written here.
        //
        // Family-agnostic since S2/S3 (`docs/mlc/plans/markup-engine-parity-
        // 2026-08-25.md`): a primitive outside Elliott falls through to its
        // OWN family's grammar (`markup::chain::successor_kinds`), so
        // selecting a liquidity sweep offers Structure Break and nothing
        // else. Elliott is tried first and wins where it answers — its
        // table is degree-aware and this one is not, so the fallback must
        // never shadow it.
        let successor_kind_items: Vec<(String, String)> = {
            let registry = PrimitiveRegistry::global().read().unwrap();
            let mut kinds: &[&str] = successor_kinds(prim.as_ref());
            let markup_kinds = mlc_core::drawing::primitives_v2::markup::successor_kinds(prim.type_id());
            if kinds.is_empty() {
                kinds = markup_kinds;
            }
            kinds
                .iter()
                .filter_map(|&kind_id| registry.get(kind_id).map(|meta| (kind_id.to_string(), meta.display_name.to_string())))
                .collect()
        };
        // Predecessor picker (owner ask 2026-08-14: "делать достройку
        // перед началом чейна" — a count is read backwards as often as
        // forwards). Same shape as the E3 block above, `elliott::count::
        // predecessor_kinds` read instead of `successor_kinds`.
        let predecessor_kind_items: Vec<(String, String)> = {
            let registry = PrimitiveRegistry::global().read().unwrap();
            predecessor_kinds(prim.as_ref())
                .iter()
                .filter_map(|&kind_id| registry.get(kind_id).map(|meta| (kind_id.to_string(), meta.display_name.to_string())))
                .collect()
        };
        // E4 — promotion button. Whether the selected primitive's own
        // `follows`-linked chain (found by walking backward to the chain's
        // head, then forward from there — same resolution
        // `promote_wave_chain` itself performs) currently promotes into one
        // coarser structure. Computed here, not just left to the click
        // handler, so the button's own visibility never lies about what a
        // click would do.
        let can_promote = {
            let b = bubbles.bubble(bid);
            b.is_some_and(|b| {
                let head_id = Self::chain_head_id(b, prim.data().id);
                let member_ids = Self::chain_member_ids(b, head_id);
                let chain: Vec<&dyn Primitive> = member_ids.iter()
                    .filter_map(|id| b.primitives.iter().find(|p| p.data().id == *id).map(|p| p.as_ref()))
                    .collect();
                chain.len() == member_ids.len() && promote_chain_shape(&chain).is_some()
            })
        };
        // Conversion picker (owner report 2026-08-14: the structure's own
        // global type must be changeable IN PLACE, mid-count — see
        // `elliott::count`'s own "Conversion" section). `parent`/`prev` are
        // resolved from the SAME bubble `prim` itself was read from, so
        // `conversion_kinds` sees exactly the slot/chain gates
        // `DrawingManager::convert_primitive_kind` will re-check when a
        // click actually fires — a stale entry here can only be MORE
        // restrictive than the live re-check, never less, since both read
        // the same live collection.
        let own_kind = kind_id_of(prim.as_ref());
        let parent = prim.wave_link().and_then(|link| {
            bubbles.bubble(bid).and_then(|b| b.primitives.iter().find(|p| p.data().id == link.parent_id)).map(|p| p.as_ref())
        });
        let prev = prim.wave_follows().and_then(|id| {
            bubbles.bubble(bid).and_then(|b| b.primitives.iter().find(|p| p.data().id == id)).map(|p| p.as_ref())
        });
        let conversion_kind_items: Vec<(String, ConceptKey, bool)> = conversion_kinds(prim.as_ref(), parent, prev)
            .iter()
            .filter_map(|&kind_id| {
                kind_label(kind_id).map(|key| (kind_id.to_string(), key, own_kind.as_deref() == Some(kind_id)))
            })
            .collect();
        Some(SelectedPrimitiveConfig {
            name: prim.display_name().to_string(),
            color: data.color.stroke.clone(),
            width: data.width,
            style: data.style.as_str().to_string(),
            locked: data.locked,
            text_color,
            supports_text,
            wave_slots,
            wave_slot_kinds,
            successor_kinds: successor_kind_items,
            predecessor_kinds: predecessor_kind_items,
            can_promote,
            conversion_kinds: conversion_kind_items,
            truncated_fifth: prim.truncated_fifth(),
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
        // Stamp the reference this freshly-placed primitive actually starts
        // from — see `Self::initial_style_reference`. Runs AFTER
        // `apply_last_style_snapshot` above, so it reads the primitive's
        // real seeded state (registry factory default, then the bubble's
        // last-used style if any), not the type's bare factory default.
        prim.data_mut().style_reference = Self::initial_style_reference(prim.as_ref());
        if let Some(variant) = session.tool_variant.clone() {
            Self::apply_variant_to_primitive(&mut prim, &variant);
        }
        prim.data_mut().symbol = b.address.symbol.clone();
        let id = prim.data().id;
        b.primitives.push(prim);
        session.selected = Some(id);

        // Higher-degree auto-adoption (owner ask 2026-08-14, EWA engine
        // settings' own "detect_higher_degree" switch — `crates/mlc-app-
        // layout/src/layout/modals/ewa_engine_settings.rs`): OFF by
        // default, so leaving the switch untouched changes nothing about
        // creation — the whole detector sits behind this one early return.
        if ewa_engine_config().detect_higher_degree {
            Self::adopt_higher_degree(b, id);
        }

        // A freshly placed CLAIM gets one nudge onto the bar its own
        // detector names, if that bar is within a few of where the analyst
        // clicked — see `Primitive::snap_to_detected` for the contract.
        // Creation only: never on a drag, never on a reload.
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == id) {
            prim.snap_to_detected(&b.bars);
        }
        // Same nudge for a claim whose detector reads the per-price book —
        // only when this bubble actually carries footprint, for the reason
        // `refresh_footprint_derived_caches` gives: an empty slice is
        // ambiguous with "subscribed, no bucket yet".
        if let Some(footprint) = b.footprint_bars.as_ref() {
            if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == id) {
                prim.snap_to_detected_footprint(footprint);
            }
        }

        // A freshly placed primitive has never seen the bars. Its render
        // does not care (it recomputes from `ctx.bars()` every frame), but
        // `style_properties()` has no bars of its own and reads only the
        // `#[serde(skip)]` cache — without this the settings modal opens on
        // an all-zero Hits column over a chart visibly carrying hits
        // (measured on the live stand, 2026-08-25). See
        // `Self::refresh_bar_derived` for the whole contract; this is the
        // INTERACTIVE creation path, which is not one of the
        // `refresh_wave_guards` sites and so refreshed neither cache.
        crate::bubble::bar_derived_cache::refresh_bar_derived_caches(b);
        crate::bubble::markup_guards::refresh_markup_guards(b);

        id
    }

    /// Recognise every already-drawn primitive in `b` that legally occupies
    /// one of `parent_id`'s own slots (`elliott::count::adoptable_children`)
    /// and weld/link/re-degree the new structure accordingly — the write
    /// side of higher-degree auto-adoption (owner ask 2026-08-14: "неплохо
    /// было бы детектить вверх и делать авторазметку более высокого цикла" —
    /// when the analyst draws a COARSER structure over waves he already
    /// marked, the engine recognises the existing ones as its own
    /// subdivisions instead of making him re-link them by hand). Called
    /// from [`Self::finalize_new_primitive`] only, already gated on
    /// `EwaEngineConfig::detect_higher_degree` there — this method itself
    /// does not re-check the switch.
    ///
    /// A no-op (nothing welded, nothing linked, degree untouched) when
    /// `parent_id` does not resolve or `adoptable_children` reports no
    /// match — the freshly-created primitive is left exactly as
    /// `finalize_new_primitive` built it.
    ///
    /// WELD: every matched slot's own anchor points on `parent_id` are
    /// overwritten with the adopted child's own EXACT endpoints (the
    /// analyst clicked NEAR them; adoption makes the join exact — the same
    /// invariant [`Self::restitch_wave_joints`] already maintains for a
    /// hand-made subdivision). Only the anchors of MATCHED slots move;
    /// every other point on `parent_id` stays exactly where it was drawn.
    /// LINK: every adopted child's own [`WaveLink`] is stamped onto it,
    /// naming `parent_id` and the slot it now occupies. DEGREE: `parent_id`
    /// itself is set to the adopted children's own shared degree, one step
    /// coarser (`elliott::count::adopted_parent_degree`) — never invented,
    /// always derived from what was actually adopted. Finishes with
    /// [`Self::refresh_wave_guards`]'s own bubble-scoped computation, same
    /// as every other link-mutating operation in this file.
    fn adopt_higher_degree(b: &mut crate::bubble::DataBubble, parent_id: u64) {
        let Some(parent_idx) = Self::find_index_by_id(b, parent_id) else { return };

        // Both the match and the shared-degree read happen in ONE
        // immutable borrow of `b.primitives` — `adoptions`/`parent_degree`
        // come out as owned data, so the borrow ends here and the mutation
        // below is free to proceed.
        let (adoptions, parent_degree) = {
            let parent = b.primitives[parent_idx].as_ref();
            let candidates: Vec<(u64, &dyn Primitive)> = b
                .primitives
                .iter()
                .enumerate()
                .filter(|&(i, _)| i != parent_idx)
                .map(|(_, p)| (p.data().id, p.as_ref()))
                .collect();
            let adoptions = adoptable_children(parent, &candidates);
            let parent_degree = adopted_parent_degree(&candidates, &adoptions);
            (adoptions, parent_degree)
        };
        // `adopted_parent_degree` returns `None` precisely when `adoptions`
        // is empty (its own first-entry lookup fails) — this one check
        // covers "nothing to adopt" as well as "nothing to derive a degree
        // from".
        let Some(parent_degree) = parent_degree else { return };

        let parent_type_id = b.primitives[parent_idx].type_id();
        let mut parent_points = b.primitives[parent_idx].points();
        for adoption in &adoptions {
            let Some((start_idx, end_idx)) = slot_span(parent_type_id, adoption.slot) else { continue };
            if start_idx < parent_points.len() {
                parent_points[start_idx] = adoption.start;
            }
            if end_idx < parent_points.len() {
                parent_points[end_idx] = adoption.end;
            }
        }
        b.primitives[parent_idx].set_points(&parent_points);
        b.primitives[parent_idx].set_wave_degree(parent_degree);

        for adoption in &adoptions {
            if let Some(child) = b.primitives.iter_mut().find(|p| p.data().id == adoption.child_id) {
                child.set_wave_link(Some(WaveLink { parent_id, slot: adoption.slot }));
            }
        }

        crate::bubble::wave_guards::refresh_wave_guards(b);
    }

    /// What a freshly-created primitive's `style_reference` starts as —
    /// see [`mlc_core::drawing::StyleReference`] and
    /// [`mlc_core::drawing::primitives_v2::PrimitiveData::style_reference`]'s
    /// own doc for the shared `None`-means-shown reading this feeds.
    ///
    /// A level-capable primitive's seeded composition (only the currently
    /// VISIBLE levels — an invisible entry inherited from a `last_style`
    /// snapshot is not "in" the composition any more than a deleted level
    /// on a live primitive is) is matched against `prim`'s OWN preset
    /// catalogue ([`Primitive::level_set_presets`]) via
    /// [`level_set_id_for_composition`] — the SAME canonical matching
    /// `mlc-app-layout`'s own `current_level_set`
    /// (`primitive_settings_intent.rs`) uses for the Fibonacci family, both
    /// sourced off `mlc_core::drawing::primitives_v2::fibonacci::retracement`'s
    /// preset value lists — so creation-time stamping here and the modal's
    /// own live read of "which набор is this" can never independently
    /// redefine "what a preset means" and drift apart. A composition
    /// matching none of `prim`'s presets is "custom" — there is no id to
    /// name it with, so this falls through to `None`, same as a primitive
    /// whose type offers no presets at all — today, always a primitive with
    /// no level ladder at all (`level_set_presets()` empty, so
    /// `level_set_id_for_composition` has nothing to match against and
    /// always returns `None`); every ladder-having type (Fibonacci, Gann,
    /// pitchfork, Fibonacci Time Zones) now owns its own non-empty preset
    /// catalogue.
    ///
    /// A primitive with no level ladder has nothing else nameable to stamp
    /// either — unchanged by
    /// [`mlc_core::templates::PrimitiveTemplate::system_templates`] now
    /// also minting a `"standard"` row for a non-ladder type: this
    /// function only ever matches a seeded LADDER composition against the
    /// primitive's own preset value lists, so a non-ladder type still falls
    /// through to `None` here (pre-existing behaviour, unchanged by this
    /// rename — a freshly drawn non-ladder primitive still shows "Save
    /// as…" rather than reading as already matching its own "standard"
    /// row). The only OTHER starting point `apply_last_style_snapshot` can
    /// seed it from is a raw `TemplateStyle` snapshot, which — unlike a
    /// `PrimitiveTemplate` — carries no id of its own to resolve back
    /// against later (see `TemplateStyle`'s own doc).
    fn initial_style_reference(prim: &dyn Primitive) -> Option<StyleReference> {
        let levels = prim.level_configs()?;
        let composition: Vec<f64> = levels.iter().filter(|c| c.visible).map(|c| c.level).collect();
        level_set_id_for_composition(prim.level_set_presets(), &composition)
            .map(|id| StyleReference::System(id.to_string()))
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
                Self::on_click_n_point(b, session, &registry, &tool_id, ts_ms, price, last_style.as_ref(), 3, true)
            }
            ClickBehavior::FourPoint => {
                Self::on_click_n_point(b, session, &registry, &tool_id, ts_ms, price, last_style.as_ref(), 4, true)
            }
            ClickBehavior::MultiPointFixed(n) => {
                Self::on_click_n_point(b, session, &registry, &tool_id, ts_ms, price, last_style.as_ref(), n as usize, true)
            }
            ClickBehavior::MultiPoint(min_points) => {
                Self::on_click_n_point(b, session, &registry, &tool_id, ts_ms, price, last_style.as_ref(), min_points as usize, false)
            }
            ClickBehavior::FreehandDrag => false,
        }
    }

    /// Shared click accumulator for every fixed-count-or-more click
    /// behaviour: `ThreePoint`/`FourPoint` (arity baked into the call
    /// site), `MultiPointFixed(n)` (arity IS `min_points`, `finalize_at_min
    /// = true`), and `MultiPoint(n)` (`n` is a floor, `finalize_at_min =
    /// false` — the tool keeps collecting past it and finalizes only
    /// through `finish_multipoint`, the double-click handler).
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
        finalize_at_min: bool,
    ) -> bool {
        match &mut session.state {
            DrawingState::Idle => {
                session.state = DrawingState::Creating { tool_id: tool_id.to_string(), points: vec![(ts_ms, price)] };
                false
            }
            DrawingState::Creating { points, tool_id: creating_tool } if creating_tool == tool_id => {
                points.push((ts_ms, price));
                if finalize_at_min && points.len() >= min_points {
                    let points = points.clone();
                    if let Some(prim) = registry.create(tool_id, &points, Some(&session.default_color)) {
                        Self::finalize_new_primitive(b, session, prim, last_style);
                        session.state = DrawingState::Idle;
                        session.current_tool = None;
                        // The TwoPoint/ClickDrag sibling branch above already
                        // clears this on finalize; this branch (ThreePoint/
                        // FourPoint/MultiPointFixed/MultiPoint) didn't, so a
                        // tool variant (e.g. an emoji sub-type) picked for one
                        // drawn primitive could leak into the next primitive
                        // of the same tool.
                        session.tool_variant = None;
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
                    session.tool_variant = None;
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
            ClickBehavior::MultiPoint(n) | ClickBehavior::MultiPointFixed(n) => n as usize,
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
        // Only the pivots the user has actually placed are real — everything
        // above `min_points` was just padded with the cursor above. Without
        // this the preview claims a full structure and labels the point being
        // placed with the LAST wave's label: the owner drew a fresh impulse
        // and its first click showed "(5)" (report 2026-08-14). Placed points
        // plus the one under the cursor.
        prim.set_realized_point_count(points.len() + 1);

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
        let applied = match kind {
            PropertyKind::Style => {
                prim.apply_style_property(prop_id, value);
                true
            }
            PropertyKind::Text => prim.apply_text_property(prop_id, value),
            PropertyKind::Level => prim.apply_level_property(prop_id, value),
        };
        // A property can change WHAT the primitive derives from the bars —
        // the claimed candle-pattern kind, an order block's zone mode, the
        // set of structures a survey looks for. See this module's own
        // `refresh_bar_derived` for why that cache cannot wait for the next
        // bar-array install.
        crate::bubble::bar_derived_cache::refresh_bar_derived_caches(b);
        applied
    }

    pub(crate) fn set_points(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, points: &[(i64, f64)]) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.set_points(points);
        }
        // Moving a claim re-aims it at different bars — see
        // `Self::refresh_bar_derived`.
        crate::bubble::bar_derived_cache::refresh_bar_derived_caches(b);
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
            // Every caller of this method is a read-modify-write of a live
            // `get_data` snapshot, so the reference travels with the rest of
            // the snapshot. Omitting it made `stamp_primitive_style_reference`
            // (`mlc-app`) a silent no-op: it wrote the field on its own copy
            // and this whitelist dropped it, so applying a level-set preset
            // never re-established what the primitive was made from and
            // "Save as…" stayed lit (2026-08-09).
            prim_data.style_reference = data.style_reference.clone();
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

    /// Apply a saved [`mlc_core::templates::PrimitiveTemplate`] to a live
    /// primitive — the flat style fields, the level ladder (written whenever
    /// `prim` itself supports one, per `PrimitiveTemplate`'s own
    /// target-capability gating), and the typed style/text properties, all
    /// through `PrimitiveTemplate::apply_to_primitive`, which reaches the
    /// primitive directly rather than round-tripping through a cloned
    /// `PrimitiveData`. The ONE apply path for both a system
    /// (`PrimitiveTemplate::builtin`) row and a user-saved one — they are
    /// the same type, applied the same way.
    pub(crate) fn apply_primitive_template(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        prim_id: u64,
        template: &mlc_core::templates::PrimitiveTemplate,
    ) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) else { return };
        template.apply_to_primitive(prim.as_mut());
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
            // No `dyn Primitive` available here (only a `PrimitiveData`
            // snapshot) — nothing to read the ladder or typed properties
            // from, same reasoning as `style_properties` above.
            level_configs: Vec::new(),
            text_properties: Vec::new(),
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
        let text_props: Vec<(String, PropertyValue)> = prim
            .text_properties()
            .unwrap_or_default()
            .into_iter()
            .filter(|prop| !prop.readonly)
            .map(|prop| (prop.id, prop.value))
            .collect();
        let level_configs = prim.level_configs().unwrap_or_default();
        let style = TemplateStyle {
            color: Some(data.color.stroke.clone()),
            width: Some(data.width),
            line_style: Some(data.style.as_str().to_string()),
            fill_color: data.color.fill.clone(),
            fill_opacity: None,
            show_labels: None,
            show_prices: None,
            style_properties: style_props,
            level_configs,
            text_properties: text_props,
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

    /// Delete `prim_id` and handle its two DIFFERENT kinds of Elliott
    /// reference — subdivision and succession (see `elliott::count`'s own
    /// module doc) — DIFFERENTLY, on purpose:
    ///
    /// - **Subdivision** (`Primitive::wave_link().parent_id`) cascades: every
    ///   child nested under `prim_id`, transitively, is deleted too — the
    ///   WHOLE subtree, not just the one primitive with the link cleared. A
    ///   deleted parent's own count is wrong for everything nested inside it
    ///   too, so orphaning a child into a standalone wave at the wrong
    ///   degree would leave a primitive on the chart that silently stopped
    ///   meaning what it looked like it meant; deleting the subtree is the
    ///   honest behaviour.
    /// - **Succession** (`Primitive::wave_follows()`) does NOT cascade: a
    ///   successor is a SIBLING structure, not a piece of its predecessor —
    ///   deleting an impulse must not delete the correction that came after
    ///   it, because that correction is a complete, independently-valid
    ///   structure in its own right, exactly as valid standing alone as it
    ///   was while pointing back at the impulse. What DOES need cleaning up
    ///   is the dangling reference itself: every surviving primitive whose
    ///   `wave_follows()` points into the deleted set has that reference
    ///   cleared (`Primitive::set_wave_follows(None)`) so it never looks
    ///   like it continues a structure that no longer exists.
    ///
    /// The single choke point every deletion call site (inline trash
    /// button, Delete hotkey, right-click menu, "Delete selected" dropdown)
    /// already goes through, so both rules apply uniformly with no extra
    /// plumbing.
    pub(crate) fn delete(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) -> bool {
        let Some(b) = bubbles.bubble_mut(bid) else { return false };
        let before = b.primitives.len();

        let mut to_delete: HashSet<u64> = HashSet::new();
        to_delete.insert(prim_id);
        loop {
            let mut grew = false;
            for p in b.primitives.iter() {
                let id = p.data().id;
                if to_delete.contains(&id) {
                    continue;
                }
                if let Some(link) = p.wave_link() {
                    if to_delete.contains(&link.parent_id) {
                        to_delete.insert(id);
                        grew = true;
                    }
                }
            }
            if !grew {
                break;
            }
        }

        // Succession does NOT cascade (see this method's own doc) — only
        // the dangling `follows` reference is cleared, on every surviving
        // primitive that is not itself being deleted.
        for p in b.primitives.iter_mut() {
            if to_delete.contains(&p.data().id) {
                continue;
            }
            if p.wave_follows().is_some_and(|id| to_delete.contains(&id)) {
                p.set_wave_follows(None);
            }
        }

        b.primitives.retain(|p| !to_delete.contains(&p.data().id));
        let removed = b.primitives.len() != before;
        if removed {
            let session = self.session_mut(bid);
            if session.selected.is_some_and(|id| to_delete.contains(&id)) {
                session.selected = None;
            }
            if session.dragging.is_some_and(|id| to_delete.contains(&id)) {
                session.dragging = None;
                session.drag_start = None;
            }
            // A deleted subdivision's own violations are gone with it; a
            // deleted predecessor can leave its former successor's
            // `chain_continuity` check silent (the dangling `follows` was
            // just cleared above) or a deleted child can clear its former
            // parent's `subdivision_class` check — either way, this is a
            // path that can change what `elliott::guard::violations_in`
            // sees (this method's own doc), so it refreshes the cache
            // exactly like every other mutating path in this module does.
            self.refresh_wave_guards(bubbles, bid);
            self.refresh_bar_derived(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
        }
        removed
    }

    pub(crate) fn delete_selected(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) -> bool {
        let Some(prim_id) = self.selected(bid) else { return false };
        self.delete(bubbles, bid, prim_id)
    }

    /// [`Self::delete`], plus a full snapshot of `prim_id` AND every
    /// primitive its own subdivision cascade also removes — captured BEFORE
    /// any of them are gone, so an undo command built from the return value
    /// restores the whole subtree in one step (see `Command::DeletePrimitive`'s
    /// own doc for why the cascade must travel WITH the command instead of
    /// vanishing silently). Same cascade/succession-cleanup behaviour as
    /// `Self::delete` — this is that method plus the capture, not a second
    /// deletion policy. `None` when `prim_id` does not resolve to a live
    /// primitive.
    pub(crate) fn delete_with_snapshot(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        prim_id: u64,
    ) -> Option<DeletedPrimitives> {
        let (points, snapshot, cascaded) = {
            let b = bubbles.bubble(bid)?;
            let idx = Self::find_index_by_id(b, prim_id)?;
            let points = b.primitives[idx].points();
            let snapshot = PrimitiveSnapshot::from_primitive(b.primitives[idx].as_ref());

            let mut to_delete: HashSet<u64> = HashSet::new();
            to_delete.insert(prim_id);
            loop {
                let mut grew = false;
                for p in b.primitives.iter() {
                    let id = p.data().id;
                    if to_delete.contains(&id) {
                        continue;
                    }
                    if let Some(link) = p.wave_link() {
                        if to_delete.contains(&link.parent_id) {
                            to_delete.insert(id);
                            grew = true;
                        }
                    }
                }
                if !grew {
                    break;
                }
            }
            let cascaded: Vec<PrimitiveSnapshot> = b.primitives.iter()
                .filter(|p| p.data().id != prim_id && to_delete.contains(&p.data().id))
                .map(|p| PrimitiveSnapshot::from_primitive(p.as_ref()))
                .collect();
            (points, snapshot, cascaded)
        };

        if !self.delete(bubbles, bid, prim_id) {
            return None;
        }
        Some(DeletedPrimitives { points, snapshot, cascaded })
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
        // Rendering and hit-testing walk `primitives_sorted_by_z_order()`,
        // which sorts by `data().z_order` — NOT by position in this Vec.
        // Reordering the Vec alone (above) had zero visible effect; this is
        // the write that actually moves the primitive to the top.
        let max_z = b.primitives.iter().map(|p| p.data().z_order).max().unwrap_or(0);
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().z_order = max_z + 1;
        }
    }

    pub(crate) fn send_to_back(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        let Some(idx) = Self::find_index_by_id(b, prim_id) else { return };
        if idx != 0 {
            let prim = b.primitives.remove(idx);
            b.primitives.insert(0, prim);
        }
        // See `bring_to_front` — the z_order field is what rendering and
        // hit-testing actually sort by.
        let min_z = b.primitives.iter().map(|p| p.data().z_order).min().unwrap_or(0);
        if let Some(prim) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            prim.data_mut().z_order = min_z - 1;
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
                // Same no-movement guard as `DragType::Move` above. Without
                // it, every call — including a spurious one that lands at
                // (or within noise of) the exact position the drag already
                // started from — re-applies `move_control_point`
                // unconditionally. A single stray call is a same-value
                // no-op, but a RUN of them (e.g. a zero-distance
                // press→move×N→release gesture, which still emits N
                // `UpdatePrimitiveDrag` calls) lets any per-call rounding
                // in the screen↔data round trip compound across calls
                // instead of collapsing back to nothing — a "drag" that
                // never moved the cursor must never move the geometry.
                let ts_delta = current_ts_ms - start_ts;
                let price_delta = current_price - start_price;
                if ts_delta.abs() > 0 || price_delta.abs() > 0.0001 {
                    prim.move_control_point(point_type, current_ts_ms, current_price);
                    self.session_mut(bid).drag_start = Some((current_ts_ms, current_price));
                }
            }
        }
        // Runs on every drag STEP, not at the end: a chain that visibly tears
        // while the hand moves and snaps back on release would read as a bug
        // being papered over. See `restitch_wave_joints`' own doc.
        Self::restitch_wave_joints(b, prim_id);
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
                // Same no-movement guard as `DragType::Move` above, and for
                // the same reason `update_drag`'s ControlPoint arm now has
                // it: nothing here may gate on `screen_x`/`screen_y` alone,
                // because a zero-distance gesture still calls this with the
                // SAME screen position on every step, and screen position
                // is exactly what a rotated/pixel-space override (e.g.
                // `RotatedRectangle::move_control_point_screen`) re-derives
                // its opposite-corner pin from — a run of same-position
                // calls must be a run of no-ops, not N reapplications
                // against a target that shifts slightly on every call.
                let ts_delta = current_ts_ms - start_ts;
                let price_delta = current_price - start_price;
                if ts_delta.abs() > 0 || price_delta.abs() > 0.0001 {
                    prim.move_control_point_screen(point_type, screen_x, screen_y, current_ts_ms, current_price, bars, viewport, price_scale);
                    self.session_mut(bid).drag_start = Some((current_ts_ms, current_price));
                }
            }
        }
        // Same reason as `update_drag`'s own tail — see `restitch_wave_joints`.
        Self::restitch_wave_joints(b, prim_id);
    }

    /// `bubbles` is only needed for the trailing [`Self::refresh_wave_guards`]
    /// call (a drag gesture ending is exactly the "deep pass" boundary that
    /// method's own doc describes) — every other field this method touches
    /// lives on `self.sessions`.
    pub(crate) fn end_drag(&mut self, bubbles: &mut BubbleStore, bid: BubbleId) {
        if let Some(s) = self.sessions.get_mut(&bid) {
            s.dragging = None;
            s.drag_type = DragType::Move;
            s.drag_start = None;
        }
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
    }

    // =========================================================================
    // Elliott context-violations refresh (E2/guard follow-up, 2026-08-13) —
    // the DEEP pass `elliott::guard::violations_in` needs but a lone
    // primitive's own `render` cannot run (`&self` only, no path to its
    // siblings — see that function's own module doc).
    // =========================================================================

    /// Recompute every Elliott primitive's own SIBLING-dependent violations
    /// for the whole bubble and cache them
    /// (`Primitive::set_context_violations`) — two passes on purpose: the
    /// first, immutable, computes `(index, Vec<Violation>)` for every
    /// primitive whose `wave_degree()` is `Some` (i.e. every Elliott
    /// primitive), passing the WHOLE bubble slice as `others` to
    /// `elliott::guard::violations_in`; the second, mutable, writes each
    /// result back. A mutable borrow of one primitive while the immutable
    /// slice is still borrowed for `others` is not something one pass can
    /// do — `violations_in` needs to see every sibling at once.
    ///
    /// Every Elliott primitive is written, even one that now produces an
    /// empty vec: a fixed count that goes silent (its own violation cured)
    /// must have its stale cache CLEARED, not left holding the last
    /// non-empty result forever.
    ///
    /// A no-op when `bid` does not resolve to a live bubble. Called after
    /// every mutation that can change what the rules see — wave child/
    /// successor creation, chain promotion (and its demotion counterpart,
    /// covered transitively: `Command::DemoteWaveChain`'s own apply arm in
    /// `mlc-app` ends with `Self::delete`, which already refreshes below),
    /// primitive deletion, drag end, and external insertion (preset/undo
    /// restore, `Self::insert_at`) — see each of those methods' own call to
    /// this one. Never called per frame and never from `Self::update_drag`:
    /// the SELF-only checks (`elliott::guard::violations`) already
    /// recompute live every frame from a primitive's own `render`; this is
    /// the expensive whole-bubble pass, and it runs once per gesture, not
    /// once per pixel of drag.
    ///
    /// Thin `bid`-resolving wrapper — the actual bubble-scoped computation
    /// lives in `crate::bubble::wave_guards::refresh_wave_guards` (moved
    /// there 2026-08-14, second report: `BubbleStore::restore_from_snapshots`
    /// — a path that never touches `DrawingManager` at all — needed the
    /// SAME computation and could only get it if it lived one level lower
    /// than `bid` + `&mut BubbleStore`). Every call site here keeps working
    /// unchanged; this is the shape THIS crate's live-editing methods need.
    pub(crate) fn refresh_wave_guards(&self, bubbles: &mut BubbleStore, bid: BubbleId) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        crate::bubble::wave_guards::refresh_wave_guards(b);
    }

    /// Recompute every primitive's own bars-derived `#[serde(skip)]` cache
    /// in `bid`'s bubble — the sibling of [`Self::refresh_wave_guards`],
    /// called from the same mutation points for the same reason.
    ///
    /// `BubbleStore`'s own sweep fires only on a batch bar-array install
    /// (`replace_bars`/`set_bars`/`clear_bars`/restore). That covers "the
    /// bars changed under a primitive" and nothing else, so a primitive
    /// ADDED or EDITED after the bars loaded kept an empty cache until the
    /// next symbol or timeframe switch. `render` never noticed — it
    /// recomputes from `ctx.bars()` every frame — but `style_properties()`
    /// has no bars of its own and reads only the cache, so the settings
    /// modal reported `Total Hits 0` and an all-zero Hits column over a
    /// chart visibly carrying dozens of hits (measured on the live binary,
    /// 2026-08-25). The pass costs one detector run per primitive, which
    /// is exactly what one frame of `render` already pays.
    pub(crate) fn refresh_bar_derived(&self, bubbles: &mut BubbleStore, bid: BubbleId) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        crate::bubble::bar_derived_cache::refresh_bar_derived_caches(b);
        // Immediately after, never separately — see
        // `markup_guards`'s own ordering contract: that pass APPENDS onto the
        // list this one just rebuilt.
        crate::bubble::markup_guards::refresh_markup_guards(b);
    }

    /// Clear any [`WaveLink`]/`follows` reference in `bid`'s own bubble that
    /// no longer resolves within that same collection — see
    /// `crate::bubble::wave_guards::clear_dangling_wave_links`'s own doc for
    /// the full rationale. A no-op when `bid` does not resolve to a live
    /// bubble. Returns the number of primitives whose `link` and/or
    /// `follows` was cleared.
    ///
    /// Thin `bid`-resolving wrapper, same reasoning as
    /// [`Self::refresh_wave_guards`] above.
    pub(crate) fn clear_dangling_wave_links(&self, bubbles: &mut BubbleStore, bid: BubbleId) -> u32 {
        let Some(b) = bubbles.bubble_mut(bid) else { return 0 };
        crate::bubble::wave_guards::clear_dangling_wave_links(b)
    }

    // =========================================================================
    // Wave joints — a chain stays a chain while it is dragged
    // =========================================================================

    /// Re-stitch every joint `moved_id`'s own drag just pulled apart.
    ///
    /// Owner report 2026-08-13, with a screenshot of a chain stretched into
    /// disconnected pieces: "цепочка это цепочка нельзя растягивать её по
    /// кускам так чтобы она разорвала связь". A `follows` chain is one
    /// structure's subdivision written as separate primitives — the point
    /// where two members meet is ONE pivot, not two that happen to coincide,
    /// so a drag that moves one side must carry the other. `elliott::guard`'s
    /// own `chain_continuity` rule stays: it reports a gap this function
    /// cannot close (a pasted or hand-edited file whose members were never
    /// dragged), not one it just created.
    ///
    /// The hand wins. Whatever `moved_id` now says its first/last point is,
    /// its predecessor's last point and its successors' first points are set
    /// to match — the neighbour DEFORMS at the shared pivot, it is not
    /// translated whole, because moving a member must not silently move
    /// structures the user never touched.
    ///
    /// Subdivisions are welded in BOTH directions, which is the second half
    /// of the same report (2026-08-13, screenshot of stray impulses beside
    /// the count): "я спавню подволну внутри волны, и она вместо того чтобы
    /// ЗАМЕНЯТЬ собой отрезок, просто накладывается сверху, и я могу его
    /// выдрать... он должен именно что становиться частью". A subdivision IS
    /// the wave it subdivides, so its two endpoints are the parent's own
    /// pivots and not copies of them:
    ///
    /// - the parent moves → every child is re-anchored onto the slot span it
    ///   occupies (`elliott::count::child_seed`, the function that placed
    ///   it), keeping the shape it was drawn with by proportional rescale
    ///   rather than being rebuilt as a straight line;
    /// - the child moves → its first/last are written straight into the
    ///   parent's own point array at that slot's span
    ///   (`elliott::count::slot_span`), so the child cannot be torn off the
    ///   count. The parent's pivot is shared with the NEIGHBOURING wave, so
    ///   moving it moves that wave's end too — that is not a side effect, it
    ///   is what a pivot is — and the parent's other subdivisions and its own
    ///   chain joints are re-stitched right after.
    ///
    /// Exactly one level of propagation runs in each direction from the
    /// primitive the hand moved: child → parent → (parent's chain neighbours
    /// and parent's other children). It is deliberately not a fixpoint —
    /// every write here is a straight assignment, so a second pass would
    /// change nothing, and a bounded walk cannot loop on a malformed
    /// link/follows cycle in a hand-edited file.
    fn restitch_wave_joints(b: &mut crate::bubble::DataBubble, moved_id: u64) {
        let Some(moved_idx) = Self::find_index_by_id(b, moved_id) else { return };
        let (first, last, link, is_wave) = {
            let p = b.primitives[moved_idx].as_ref();
            let pts = p.points();
            (pts.first().copied(), pts.last().copied(), p.wave_link(), p.wave_degree().is_some())
        };
        if !is_wave {
            return;
        }
        let (Some(first), Some(last)) = (first, last) else { return };

        Self::restitch_neighbours(b, moved_id, None);

        // Weld upward: the wave this structure subdivides now ends where this
        // structure ends.
        let Some(link) = link else { return };
        let Some(parent_idx) = Self::find_index_by_id(b, link.parent_id) else { return };
        if parent_idx == moved_idx {
            return;
        }
        let span = {
            let parent = b.primitives[parent_idx].as_ref();
            slot_span(parent.type_id(), link.slot)
        };
        let Some((start_idx, end_idx)) = span else { return };
        let mut parent_points = b.primitives[parent_idx].points();
        if end_idx >= parent_points.len() {
            return;
        }
        if parent_points[start_idx] == first && parent_points[end_idx] == last {
            return;
        }
        parent_points[start_idx] = first;
        parent_points[end_idx] = last;
        b.primitives[parent_idx].set_points(&parent_points);
        // The parent's own joints moved with it — its chain neighbours and
        // its OTHER subdivisions follow. `moved_id` is excluded: it is the
        // one the hand is holding, and re-anchoring it here would fight the
        // drag it just caused.
        Self::restitch_neighbours(b, link.parent_id, Some(moved_id));
    }

    /// The joints AROUND `id`: its chain predecessor's last point and its
    /// successors' first points snap to `id`'s own ends, and every
    /// subdivision of `id` — except `skip_child`, see
    /// [`Self::restitch_wave_joints`]' own doc — is re-anchored onto the wave
    /// it subdivides.
    fn restitch_neighbours(
        b: &mut crate::bubble::DataBubble,
        id: u64,
        skip_child: Option<u64>,
    ) {
        let Some(idx) = Self::find_index_by_id(b, id) else { return };
        let (first, last, follows) = {
            let p = b.primitives[idx].as_ref();
            let pts = p.points();
            (pts.first().copied(), pts.last().copied(), p.wave_follows())
        };
        let (Some(first), Some(last)) = (first, last) else { return };

        // The predecessor ends where this structure begins.
        if let Some(prev_id) = follows {
            if let Some(i) = Self::find_index_by_id(b, prev_id) {
                if i != idx {
                    Self::set_endpoint(b.primitives[i].as_mut(), false, first);
                }
            }
        }

        // Every successor begins where this structure ends.
        let successor_idxs: Vec<usize> = b
            .primitives
            .iter()
            .enumerate()
            .filter(|(i, p)| *i != idx && p.wave_follows() == Some(id))
            .map(|(i, _)| i)
            .collect();
        for i in successor_idxs {
            Self::set_endpoint(b.primitives[i].as_mut(), true, last);
        }

        // Every subdivision still spans the wave it subdivides.
        let child_slots: Vec<(usize, WaveSlot)> = b
            .primitives
            .iter()
            .enumerate()
            .filter_map(|(i, p)| {
                if i == idx || Some(p.data().id) == skip_child {
                    return None;
                }
                p.wave_link().filter(|l| l.parent_id == id).map(|l| (i, l.slot))
            })
            .collect();
        for (i, slot) in child_slots {
            let seed = child_seed(b.primitives[idx].as_ref(), slot);
            if let Some(seed) = seed {
                Self::rescale_between(b.primitives[i].as_mut(), seed.start, seed.end);
            }
        }
    }

    /// Move one end of `prim`'s own point array to `to` — `first` selects
    /// which end. Goes through `points()`/`set_points` (the trait's own
    /// accessors) rather than any concrete type's field, matching how every
    /// other cross-primitive operation in this file reads a wave.
    fn set_endpoint(prim: &mut dyn Primitive, first: bool, to: (i64, f64)) {
        let mut pts = prim.points();
        if pts.is_empty() {
            return;
        }
        let idx = if first { 0 } else { pts.len() - 1 };
        if pts[idx] == to {
            return;
        }
        pts[idx] = to;
        prim.set_points(&pts);
    }

    /// Map `prim`'s own points from the span they currently occupy onto
    /// `start..end`, preserving each point's relative position — so a
    /// subdivision whose parent wave moved keeps the shape the user drew
    /// instead of collapsing back to the evenly-interpolated seed.
    ///
    /// Each axis is handled on its own: an axis whose old span is degenerate
    /// (every point on the same timestamp, or a perfectly flat wave) carries
    /// no relative position to preserve, so those points are distributed
    /// evenly by index rather than dividing by zero.
    fn rescale_between(prim: &mut dyn Primitive, start: (i64, f64), end: (i64, f64)) {
        let pts = prim.points();
        if pts.len() < 2 {
            return;
        }
        let old_first = pts[0];
        let old_last = pts[pts.len() - 1];
        let old_ts_span = (old_last.0 - old_first.0) as f64;
        let old_price_span = old_last.1 - old_first.1;
        let n = pts.len();
        let rescaled: Vec<(i64, f64)> = pts
            .iter()
            .enumerate()
            .map(|(i, &(ts, price))| {
                let even = i as f64 / (n - 1) as f64;
                let t_ts = if old_ts_span.abs() > f64::EPSILON {
                    (ts - old_first.0) as f64 / old_ts_span
                } else {
                    even
                };
                let t_price = if old_price_span.abs() > f64::EPSILON {
                    (price - old_first.1) / old_price_span
                } else {
                    even
                };
                (
                    start.0 + ((end.0 - start.0) as f64 * t_ts).round() as i64,
                    start.1 + (end.1 - start.1) * t_price,
                )
            })
            .collect();
        prim.set_points(&rescaled);
    }

    // =========================================================================
    // External insertion (preset/REST load, undo/redo restore)
    // =========================================================================

    /// Add an externally-created primitive with proper ID assignment.
    /// `pane_id` is preserved from the deserialized state (not overwritten)
    /// so drawings restored from a preset keep their pane assignment.
    pub(crate) fn add_external(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, mut prim: Box<dyn Primitive>) -> u64 {
        // A restored primitive KEEPS its own id when nothing else is using
        // it. This used to allocate unconditionally, and that silently broke
        // every Elliott relation on reload: `WaveLink::parent_id` and
        // `follows` name ids, so re-stamping every primitive on the way in
        // left every subdivision and every chain pointing at ids that no
        // longer existed — the subdivided legs came back, the context rules
        // went quiet, and two degrees' labels sat on top of each other again
        // (found 2026-08-14 while verifying the label-collision fix, which
        // could not work until this did).
        //
        // Uniqueness is still guaranteed: an id already present in ANY
        // bubble, or a zero id from a primitive built without one, falls
        // back to a fresh allocation. `persistence::hydration` seeds the
        // global counter above every restored id right after the restore, so
        // later allocations cannot collide with what was just loaded.
        let incoming = prim.data().id;
        let taken = incoming == 0 || {
            let mut used = false;
            bubbles.each_bubble(|bv| {
                if !used && bv.primitive_ids().any(|id| id == incoming) {
                    used = true;
                }
            });
            used
        };
        if taken {
            prim.data_mut().id = alloc_primitive_id();
        }
        let Some(b) = bubbles.bubble_mut(bid) else { return prim.data().id };
        if prim.data().symbol.is_empty() {
            prim.data_mut().symbol = b.address.symbol.clone();
        }
        let id = prim.data().id;
        b.primitives.push(prim);
        self.session_mut(bid).selected = Some(id);
        // The COLD-LOAD path runs through here (`persistence::hydration`
        // restores every saved drawing with `add_external`), and a restored
        // Elliott primitive arrives with its `link`/`follows` already set —
        // so without this, a reload showed a count with none of its
        // render caches filled: subdivided legs drawn again under their
        // subdivisions, no context violations, and every label back on top
        // of its neighbour's. Refreshing per insert is O(n) passes over a
        // bulk restore; measured against a real preset that is tens of
        // primitives, not thousands, and correctness after a reload is not
        // negotiable for a cache the render reads every frame.
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
        id
    }

    /// Create a new Elliott child primitive nested into `slot` of the
    /// parent primitive `parent_id`, built as `kind_id` (E2 — the inline
    /// config toolbar's two-step wave-slot/wave-kind picker: slot first,
    /// then which structure goes in it). Returns `None` when `parent_id`
    /// is gone, `kind_id` is not one of `elliott::count::
    /// child_kinds(parent, slot)` — the grammar itself is enforced HERE,
    /// not just suggested by the picker's own row list, so a stale or
    /// forged action id can never create an illegal subdivision, the same
    /// fail-closed shape `create_wave_successor` below already uses for
    /// `successor_kinds` — or when `elliott::count::child_seed` itself
    /// would refuse: `slot` does not exist on the parent's own type, the
    /// parent is not (yet) an Elliott wave, or the parent's stored points
    /// do not yet reach the slot's span. `child_seed` is still the ONLY
    /// source of the child's degree and anchor points — this method never
    /// invents either.
    ///
    /// Child TYPE is resolved from `kind_id` via `elliott::count::
    /// split_kind_id` — NOT guessed from `WaveSlot::is_motive` (the old
    /// heuristic this replaces: a motive slot always seeded an impulse
    /// child and everything else a correction, which cannot express that
    /// wave 2 may be many structures but never a triangle, that wave 4 may
    /// be a triangle, or that a zigzag's own wave A is a five and a flat's
    /// own wave A is a three — see `elliott::count`'s own "Grammar"
    /// section doc). `"elliott_correction"` additionally carries the kind
    /// id's own subtype (zigzag/flat/expanded/running) onto the new
    /// primitive's `correction_type`. The intermediate points between the
    /// seed's `start`/`end` are interpolated evenly
    /// (`ChildSeed::interpolate`) so the child renders as a visible,
    /// immediately-draggable wave rather than a degenerate line collapsed
    /// onto its own start point.
    ///
    /// Goes through the SAME `add_external` path every other
    /// externally-constructed primitive uses — proper id allocation,
    /// symbol fill-in, and select-on-create (so the caller can hand the
    /// user straight into dragging the new child's points) — never a
    /// direct `bubbles.bubble_mut(..).primitives.push(..)`.
    pub(crate) fn create_wave_child(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        parent_id: u64,
        slot: WaveSlot,
        kind_id: &str,
    ) -> Option<u64> {
        let (seed, color) = {
            let b = bubbles.bubble(bid)?;
            let idx = Self::find_index_by_id(b, parent_id)?;
            let parent = b.primitives[idx].as_ref();
            // Fail-closed re-check — a stale or forged action id must not
            // be able to create an illegal subdivision (see this method's
            // own doc).
            if !child_kinds(parent, slot).iter().any(|&k| k == kind_id) {
                return None;
            }
            let seed = child_seed(parent, slot)?;
            (seed, parent.data().color.stroke.clone())
        };

        let link = WaveLink { parent_id, slot };
        let (type_id, subtype) = split_kind_id(kind_id);
        let child: Box<dyn Primitive> = match type_id {
            "elliott_impulse" => {
                let mut p = ElliottImpulse::new(points_array::<6>(seed.interpolate(6)), &color);
                p.degree = seed.degree;
                p.link = Some(link);
                p.impulse_kind = match subtype {
                    Some("leading_diagonal") => ImpulseKind::LeadingDiagonal,
                    Some("ending_diagonal") => ImpulseKind::EndingDiagonal,
                    _ => ImpulseKind::Plain,
                };
                Box::new(p)
            }
            "elliott_correction" => {
                let mut p = ElliottCorrection::new(points_array::<4>(seed.interpolate(4)), &color);
                p.degree = seed.degree;
                p.link = Some(link);
                p.correction_type = match subtype.unwrap_or("zigzag") {
                    "flat" => CorrectionType::Flat,
                    "expanded" => CorrectionType::Expanded,
                    "running" => CorrectionType::Running,
                    _ => CorrectionType::Zigzag,
                };
                Box::new(p)
            }
            "elliott_triangle" => {
                let mut p = ElliottTriangle::new(points_array::<6>(seed.interpolate(6)), &color);
                p.degree = seed.degree;
                p.link = Some(link);
                Box::new(p)
            }
            "elliott_double_combo" => {
                let mut p = ElliottDoubleCombo::new(points_array::<7>(seed.interpolate(7)), &color);
                p.degree = seed.degree;
                p.link = Some(link);
                Box::new(p)
            }
            "elliott_triple_combo" => {
                let mut p = ElliottTripleCombo::new(seed.interpolate(9), &color);
                p.degree = seed.degree;
                p.link = Some(link);
                Box::new(p)
            }
            _ => return None,
        };

        let new_id = self.add_external(bubbles, bid, child);
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
        Some(new_id)
    }

    /// Create a new Elliott successor primitive continuing `prev_id` at
    /// `prev_id`'s OWN degree (E3 — the inline config toolbar's SECOND wave
    /// picker, "what comes after this"). Returns `None` when `prev_id` is
    /// gone, is not an Elliott wave (`elliott::count::successor_seed`
    /// itself returns `None`), or `type_id` is not one of
    /// `elliott::count::successor_kinds(prev)` — the alternation grammar is
    /// enforced HERE, not just suggested by the picker's own row list, so a
    /// stale or forged action id can never create a motive-after-motive or
    /// corrective-after-corrective pair.
    ///
    /// Unlike `create_wave_child`, the new primitive's `follows` is
    /// stamped, never `link` — see `elliott::count`'s own module doc for
    /// why succession and subdivision are different relations with
    /// different fields. `successor_seed` is the ONLY source of the
    /// successor's degree and starting anchor; this method never invents
    /// either, only resolves the per-type point count needed to interpolate
    /// the seed's own default extent (`SuccessorSeed::interpolate`).
    ///
    /// Goes through the SAME `add_external` path every other
    /// externally-constructed primitive uses, same as `create_wave_child`.
    /// Arm the drawing tool for `type_id` as the next claim after
    /// `prev_id`, or refuse.
    ///
    /// The non-Elliott half of the succession picker. It ARMS a tool rather
    /// than creating a primitive, and that is the whole difference between
    /// the two families' write paths: an Elliott successor has seeded
    /// geometry (`successor_seed` hands it the pivot the previous structure
    /// ended on), while a smart-money claim is an assertion about ONE bar
    /// that only the analyst can point at. There is nothing to seed, so the
    /// picker's job ends at "this is what may come next, go place it" — and
    /// the edge itself needs no help, because
    /// `bubble::markup_guards` derives it from the grammar the moment the
    /// claim lands.
    ///
    /// Fail-closed, and re-checked HERE rather than trusted from the picker
    /// that offered it: the same discipline `create_wave_successor` applies
    /// to `successor_kinds`. A stale or forged action id naming a type the
    /// grammar does not allow after `prev_id` arms nothing and returns
    /// `false`.
    pub(crate) fn arm_markup_successor(
        &mut self,
        bubbles: &BubbleStore,
        bid: BubbleId,
        prev_id: u64,
        type_id: &str,
    ) -> bool {
        let allowed = bubbles
            .bubble(bid)
            .and_then(|b| Self::find_index_by_id(b, prev_id).map(|idx| b.primitives[idx].type_id()))
            .is_some_and(|prev_type| {
                mlc_core::drawing::primitives_v2::markup::successor_kinds(prev_type)
                    .iter()
                    .any(|&kind_id| kind_id == type_id)
            });
        if !allowed {
            return false;
        }
        self.set_tool(bid, Some(type_id));
        true
    }

    pub(crate) fn create_wave_successor(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        prev_id: u64,
        type_id: &str,
    ) -> Option<u64> {
        let (seed, color) = {
            let b = bubbles.bubble(bid)?;
            let idx = Self::find_index_by_id(b, prev_id)?;
            let prev = b.primitives[idx].as_ref();
            // `.contains(&type_id)` would not type-check: the slice holds
            // `&'static str` and `type_id: &str` is not `'static` — a plain
            // value comparison sidesteps that (str equality does not care
            // whether the two references share a lifetime).
            if !successor_kinds(prev).iter().any(|&kind_id| kind_id == type_id) {
                return None;
            }
            let seed = successor_seed(prev)?;
            (seed, prev.data().color.stroke.clone())
        };

        let follows = Some(prev_id);
        let successor: Box<dyn Primitive> = match type_id {
            "elliott_impulse" => {
                let mut p = ElliottImpulse::new(points_array::<6>(seed.interpolate(6)), &color);
                p.degree = seed.degree;
                p.follows = follows;
                Box::new(p)
            }
            "elliott_correction" => {
                let mut p = ElliottCorrection::new(points_array::<4>(seed.interpolate(4)), &color);
                p.degree = seed.degree;
                p.follows = follows;
                Box::new(p)
            }
            "elliott_triangle" => {
                let mut p = ElliottTriangle::new(points_array::<6>(seed.interpolate(6)), &color);
                p.degree = seed.degree;
                p.follows = follows;
                Box::new(p)
            }
            "elliott_double_combo" => {
                let mut p = ElliottDoubleCombo::new(points_array::<7>(seed.interpolate(7)), &color);
                p.degree = seed.degree;
                p.follows = follows;
                Box::new(p)
            }
            "elliott_triple_combo" => {
                let mut p = ElliottTripleCombo::new(seed.interpolate(9), &color);
                p.degree = seed.degree;
                p.follows = follows;
                Box::new(p)
            }
            _ => return None,
        };

        let new_id = self.add_external(bubbles, bid, successor);
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
        Some(new_id)
    }

    /// Create a new Elliott predecessor primitive that `next_id` continues
    /// FROM (owner ask 2026-08-14: "щас мы чейн можем только с конца
    /// строить, а хорошо бы при даблклике или ПКМ на 0 точку делать
    /// достройку перед началом чейна" — a count is read backwards as often
    /// as forwards, so a chain must be buildable from either end).
    /// [`Self::create_wave_successor`] read backwards: returns `None` when
    /// `next_id` is gone, is not an Elliott wave (`elliott::count::
    /// predecessor_seed` itself returns `None`), or `type_id` is not one
    /// of `elliott::count::predecessor_kinds(next)` — the alternation
    /// grammar is enforced HERE, not just suggested by the picker's own
    /// row list, the same fail-closed shape `create_wave_successor` uses
    /// against `successor_kinds`.
    ///
    /// Unlike `create_wave_successor` (which only ever touches the NEW
    /// primitive), this ALSO re-points an EXISTING primitive's own
    /// `follows` — `next_id`'s own `follows` becomes `Some(new_id)` —
    /// because `follows` always names the PREDECESSOR, and `next_id`'s
    /// predecessor is now the structure this call just created. The
    /// caller is responsible for reading `next_id`'s own PRIOR `follows`
    /// (if it wants to undo the re-point later — see
    /// `Command::CreateWavePredecessor`'s own doc) BEFORE calling this;
    /// this method only ever reports the NEW state, never the old one.
    ///
    /// `predecessor_seed` is the ONLY source of the predecessor's degree
    /// and far anchor. Its LAST point is welded EXACTLY onto `next_id`'s
    /// own FIRST point by `Self::restitch_wave_joints` below, after the
    /// `follows` stamp — the same weld a drag step already performs for
    /// every other wave joint — rather than assumed to already match from
    /// interpolation alone.
    ///
    /// Goes through the SAME `add_external` path every other
    /// externally-constructed primitive uses, same as `create_wave_child`/
    /// `create_wave_successor`. Returns the new predecessor's id together
    /// with its own full snapshot — the caller (`create_wave_predecessor_
    /// and_record`, `mlc-app`) needs both to build `Command::
    /// CreateWavePredecessor` without a second query.
    pub(crate) fn create_wave_predecessor(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        next_id: u64,
        type_id: &str,
    ) -> Option<(u64, PrimitiveSnapshot)> {
        let (seed, color) = {
            let b = bubbles.bubble(bid)?;
            let idx = Self::find_index_by_id(b, next_id)?;
            let next = b.primitives[idx].as_ref();
            // `.contains(&type_id)` would not type-check — same reason
            // `create_wave_successor` sidesteps it with a value compare.
            if !predecessor_kinds(next).iter().any(|&kind_id| kind_id == type_id) {
                return None;
            }
            let seed = predecessor_seed(next)?;
            (seed, next.data().color.stroke.clone())
        };

        let predecessor: Box<dyn Primitive> = match type_id {
            "elliott_impulse" => {
                let mut p = ElliottImpulse::new(points_array::<6>(seed.interpolate(6)), &color);
                p.degree = seed.degree;
                Box::new(p)
            }
            "elliott_correction" => {
                let mut p = ElliottCorrection::new(points_array::<4>(seed.interpolate(4)), &color);
                p.degree = seed.degree;
                Box::new(p)
            }
            "elliott_triangle" => {
                let mut p = ElliottTriangle::new(points_array::<6>(seed.interpolate(6)), &color);
                p.degree = seed.degree;
                Box::new(p)
            }
            "elliott_double_combo" => {
                let mut p = ElliottDoubleCombo::new(points_array::<7>(seed.interpolate(7)), &color);
                p.degree = seed.degree;
                Box::new(p)
            }
            "elliott_triple_combo" => {
                let mut p = ElliottTripleCombo::new(seed.interpolate(9), &color);
                p.degree = seed.degree;
                Box::new(p)
            }
            _ => return None,
        };

        let new_id = self.add_external(bubbles, bid, predecessor);

        if let Some(b) = bubbles.bubble_mut(bid) {
            if let Some(next) = b.primitives.iter_mut().find(|p| p.data().id == next_id) {
                next.set_wave_follows(Some(new_id));
            }
            Self::restitch_wave_joints(b, next_id);
        }
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);

        let snapshot = self.get_snapshot(bubbles, bid, new_id)?;
        Some((new_id, snapshot))
    }

    /// Set (or clear) `prim_id`'s own [`WaveLink`] directly — the write
    /// path chain promotion/demotion needs (`Self::promote_wave_chain` and
    /// its undo-apply counterpart in `Command::DemoteWaveChain`), never
    /// exposed for anything else: every OTHER caller that links a
    /// primitive to a parent does so at construction time
    /// (`create_wave_child`), not by mutating a live one.
    ///
    /// Refreshes the guard cache (`Self::refresh_wave_guards`) on every
    /// call — `Command::PromoteWaveChain`'s own apply arm calls this once
    /// per chain member in a loop, so the LAST call in that loop is what
    /// leaves the cache correct; the redundant intermediate recomputes are
    /// cheap (this is a rare user action or an undo/redo click, never a
    /// per-frame path).
    pub(crate) fn set_wave_link(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, link: Option<WaveLink>) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(p) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            p.set_wave_link(link);
        }
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
    }

    /// Every `(child_id, slot)` pair currently [`WaveLink`]-ed under
    /// `parent_id` in `bid`'s own bubble — a plain read, derived from
    /// whatever is already persisted on each primitive's own `link` field,
    /// never a stashed pending value on `self`.
    ///
    /// This is how the `mlc-app` layer learns whether a just-created
    /// primitive's own construction adopted anything (`Self::
    /// adopt_higher_degree` stamps the link directly onto each adopted
    /// child, at creation time, before this can ever be called) — the
    /// caller resolves the new primitive's own id the same way it already
    /// does for `Command::CreatePrimitive` (`Self::last_index`, right after
    /// the click/finish call returns), then asks this whether that id
    /// adopted anything, and records `Command::PromoteWaveChain` instead of
    /// `Command::CreatePrimitive` when it did (see that command's own doc).
    pub(crate) fn wave_children_of(&self, bubbles: &BubbleStore, bid: BubbleId, parent_id: u64) -> Vec<(u64, WaveSlot)> {
        let Some(b) = bubbles.bubble(bid) else { return Vec::new() };
        b.primitives
            .iter()
            .filter_map(|p| p.wave_link().filter(|l| l.parent_id == parent_id).map(|l| (p.data().id, l.slot)))
            .collect()
    }

    /// Set (or clear) `prim_id`'s own `follows` directly — the write path
    /// a wave-predecessor undo/redo needs (`Command::CreateWavePredecessor`/
    /// `Command::DeleteWavePredecessor`'s own apply arms in `mlc-app`), the
    /// same shape [`Self::set_wave_link`] already offers for the other
    /// relation. `Self::create_wave_predecessor` itself does NOT go
    /// through this — it stamps `follows` directly while it still holds
    /// the fresh `&mut DataBubble` from `add_external`, so it can also run
    /// `Self::restitch_wave_joints` in the same borrow.
    pub(crate) fn set_wave_follows(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, follows: Option<u64>) {
        let Some(b) = bubbles.bubble_mut(bid) else { return };
        if let Some(p) = b.primitives.iter_mut().find(|p| p.data().id == prim_id) {
            p.set_wave_follows(follows);
        }
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
    }

    /// Collapse the `follows`-linked same-degree chain `prim_id` belongs to
    /// into ONE structure at the degree above (E4 — see `elliott::count::
    /// promote_chain_shape`'s own doc for the mapping rule this applies).
    /// `prim_id` may be ANY member of the chain, not just its head — this
    /// resolves the true head first (`Self::chain_head_id`) exactly the way
    /// `selected_config`'s own `can_promote` check does, so the button's
    /// availability and the click's own effect can never disagree.
    ///
    /// Returns `None` for every reason `promote_chain_shape` itself would
    /// refuse (mixed degree, a member already linked under another parent,
    /// a chain longer than the parent type's own slot count, broken
    /// alternation) or when `prim_id` does not resolve to a live primitive.
    ///
    /// Builds the parent through the SAME `add_external` path every other
    /// externally-constructed primitive uses, then stamps `link` on every
    /// chain member so the new parent's own delete cascade (`Self::delete`)
    /// now owns them exactly like any other subdivision — promotion turns a
    /// same-degree succession chain into a coarser structure's OWN
    /// subdivision, the same [`WaveLink`] relation E2 wires by hand. Where
    /// the chain is shorter than the parent type's own slot count (e.g.
    /// only wave 1 and wave 2 of a coarser impulse exist so far), the
    /// parent's own trailing not-yet-realized points collapse onto the
    /// chain's own last boundary point — draggable into place afterward,
    /// same degenerate-but-visible convention `ChildSeed::interpolate`'s
    /// `n == 1` case already uses elsewhere in this module.
    ///
    /// Returns the new parent's id and every `(member_id, slot)` pair it
    /// was stamped with, in chain order — the caller builds the undo
    /// command from these directly, without a second query.
    pub(crate) fn promote_wave_chain(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        prim_id: u64,
    ) -> Option<(u64, Vec<(u64, WaveSlot)>)> {
        let (shape, member_ids, color) = {
            let b = bubbles.bubble(bid)?;
            let head_id = Self::chain_head_id(b, prim_id);
            let member_ids = Self::chain_member_ids(b, head_id);
            let chain: Vec<&dyn Primitive> = member_ids.iter()
                .filter_map(|id| b.primitives.iter().find(|p| p.data().id == *id).map(|p| p.as_ref()))
                .collect();
            if chain.len() != member_ids.len() {
                return None;
            }
            let shape = promote_chain_shape(&chain)?;
            let color = chain[0].data().color.stroke.clone();
            (shape, member_ids, color)
        };

        let needed_points: usize = if shape.type_id == "elliott_impulse" { 6 } else { 4 };
        // `shape.points.len()` is the chain's own REAL point count (chain
        // length + 1 boundary points — see `PromotedShape::points`' own
        // doc) BEFORE `pad_points_to` repeats the last one to fill out the
        // parent's fixed count. Stamping that onto the new parent's own
        // `realized_points` is what stops the padded trailing points from
        // rendering as a stray line/label — see each Elliott type's own
        // `realized_points` field doc.
        let realized = shape.points.len();
        let padded = pad_points_to(&shape.points, needed_points);

        let parent: Box<dyn Primitive> = match shape.type_id {
            "elliott_impulse" => {
                let mut p = ElliottImpulse::new(points_array::<6>(padded), &color);
                p.degree = shape.degree;
                p.realized_points = Some(realized);
                Box::new(p)
            }
            "elliott_correction" => {
                let mut p = ElliottCorrection::new(points_array::<4>(padded), &color);
                p.degree = shape.degree;
                p.realized_points = Some(realized);
                Box::new(p)
            }
            _ => return None,
        };

        let parent_id = self.add_external(bubbles, bid, parent);

        let members: Vec<(u64, WaveSlot)> = member_ids.into_iter().zip(shape.slots.iter().copied()).collect();
        let Some(b) = bubbles.bubble_mut(bid) else { return Some((parent_id, members)) };
        for &(member_id, slot) in &members {
            if let Some(p) = b.primitives.iter_mut().find(|p| p.data().id == member_id) {
                p.set_wave_link(Some(WaveLink { parent_id, slot }));
            }
        }

        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
        Some((parent_id, members))
    }

    /// Change WHAT the Elliott structure `prim_id` fundamentally IS —
    /// convert it to `kind_id` IN PLACE, mid-count (owner report
    /// 2026-08-14: "я создал АБЦ как зигзаг, но в процессе понял, что хочу
    /// сделать его плоскостью ... допустим я выбрал зигзаг, а в процессе
    /// понял, что это скорее двойной зигзаг ... Они должны быть просто в
    /// этом же дропдауне выше, как глобальный статус отрезка"). Markup is a
    /// process; unlike [`Self::create_wave_child`]/[`Self::create_wave_successor`]
    /// (which build a NEW primitive), this rewrites an EXISTING one without
    /// disturbing its id, its z-order position, or the relations it sits in.
    ///
    /// Returns `None` — never mutates anything — when: `prim_id` is not a
    /// live Elliott primitive; `kind_id` is not one of
    /// `elliott::count::conversion_kinds(prim, parent, prev)` — the SAME
    /// fail-closed re-check `create_wave_child`/`create_wave_successor`
    /// already run for their own grammars, for the same reason: a stale or
    /// forged action id must not be able to produce an illegal structure;
    /// or `kind_id` names the kind `prim_id` already is (a no-op is not an
    /// undo entry — `conversion_kinds` always lists the current kind, so
    /// this is the one candidate this method itself refuses to act on).
    ///
    /// The new primitive's points are `elliott::count::resample_polyline`
    /// applied to the OLD point array at the new type's own pivot count
    /// (never a fresh interpolated seed the way a brand-new child/successor
    /// gets one) — the whole point of an in-place conversion is keeping the
    /// shape the user already drew. Every field of the old `PrimitiveData`
    /// carries over onto the new primitive UNCHANGED — colour, width,
    /// style, text, locked, visible, pane, symbol, style reference,
    /// timeframe visibility, and the rest — except `type_id`/`display_name`,
    /// which take the NEW type's own registered identity (read off a
    /// throwaway `Type::new(..)` before it is overwritten): a conversion
    /// changes what the structure IS, not where it sits, what it looks
    /// like, or what it belongs to, but the type/name fields are precisely
    /// the ones that MUST change, or the menu would keep calling a triangle
    /// an "Elliott Correction Wave". `degree`/`link`/`follows` carry over
    /// unchanged too — a conversion never moves a structure to a different
    /// degree, un-nests it from its parent, or detaches it from its chain.
    ///
    /// The new primitive replaces the old one AT ITS OWN INDEX (never
    /// remove-and-push) — z-order is part of what the user arranged.
    ///
    /// Every subdivision nested inside `prim_id` is remapped by ORDINAL,
    /// not by name (`elliott::count::{slot_ordinal, slot_at_ordinal}` — see
    /// their own doc for why): a child in the first wave stays in the
    /// first wave regardless of what the parent becomes. A child whose
    /// ordinal has no match on the new type loses its own `link` and
    /// becomes standalone — this method never deletes a child.
    /// `restitch_wave_joints` re-anchors every surviving child onto its
    /// (possibly differently-spanned) slot afterward, same as every other
    /// wave-topology-changing method in this file.
    ///
    /// Returns `(before, after, children)` for the caller to build a
    /// [`mlc_core::state::Command::ConvertPrimitiveKind`] undo entry from
    /// directly, without a second query — `before`/`after` are full
    /// snapshots (`PrimitiveSnapshot::from_primitive`, taken immediately
    /// before/after the replace), `children` is `(child_id, slot_before,
    /// slot_after)` per subdivision, `slot_after` being `None` for one that
    /// lost its link.
    pub(crate) fn convert_primitive_kind(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        prim_id: u64,
        kind_id: &str,
    ) -> Option<(PrimitiveSnapshot, PrimitiveSnapshot, Vec<(u64, Option<WaveSlot>, Option<WaveSlot>)>)> {
        // Phase 1: read-only resolution + the fail-closed grammar re-check.
        let (before, old_points, old_data, old_degree, old_link, old_follows, old_type_id, child_before) = {
            let b = bubbles.bubble(bid)?;
            let idx = Self::find_index_by_id(b, prim_id)?;
            let prim = b.primitives[idx].as_ref();
            let own_kind = kind_id_of(prim)?;
            if kind_id == own_kind {
                return None;
            }
            let parent = prim.wave_link().and_then(|link| {
                b.primitives.iter().find(|p| p.data().id == link.parent_id).map(|p| p.as_ref())
            });
            let prev = prim.wave_follows().and_then(|id| {
                b.primitives.iter().find(|p| p.data().id == id).map(|p| p.as_ref())
            });
            if !conversion_kinds(prim, parent, prev).iter().any(|&k| k == kind_id) {
                return None;
            }
            let degree = prim.wave_degree()?;
            let child_before: Vec<(u64, WaveSlot)> = b.primitives.iter()
                .filter_map(|p| p.wave_link().filter(|l| l.parent_id == prim_id).map(|l| (p.data().id, l.slot)))
                .collect();
            (
                PrimitiveSnapshot::from_primitive(prim),
                prim.points(),
                prim.data().clone(),
                degree,
                prim.wave_link(),
                prim.wave_follows(),
                prim.type_id(),
                child_before,
            )
        };

        // Phase 2: build the new primitive from the resampled shape.
        let (new_type, subtype) = split_kind_id(kind_id);
        let needed_points: usize = match new_type {
            "elliott_impulse" | "elliott_triangle" => 6,
            "elliott_correction" => 4,
            "elliott_double_combo" => 7,
            "elliott_triple_combo" => 9,
            _ => return None,
        };
        let resampled = resample_polyline(&old_points, needed_points);
        let stroke = old_data.color.stroke.clone();
        let mut new_prim: Box<dyn Primitive> = match new_type {
            "elliott_impulse" => {
                let mut p = ElliottImpulse::new(points_array::<6>(resampled), &stroke);
                p.degree = old_degree;
                p.link = old_link;
                p.follows = old_follows;
                p.impulse_kind = match subtype {
                    Some("leading_diagonal") => ImpulseKind::LeadingDiagonal,
                    Some("ending_diagonal") => ImpulseKind::EndingDiagonal,
                    _ => ImpulseKind::Plain,
                };
                Box::new(p)
            }
            "elliott_correction" => {
                let mut p = ElliottCorrection::new(points_array::<4>(resampled), &stroke);
                p.degree = old_degree;
                p.link = old_link;
                p.follows = old_follows;
                p.correction_type = match subtype.unwrap_or("zigzag") {
                    "flat" => CorrectionType::Flat,
                    "expanded" => CorrectionType::Expanded,
                    "running" => CorrectionType::Running,
                    _ => CorrectionType::Zigzag,
                };
                Box::new(p)
            }
            "elliott_triangle" => {
                let mut p = ElliottTriangle::new(points_array::<6>(resampled), &stroke);
                p.degree = old_degree;
                p.link = old_link;
                p.follows = old_follows;
                Box::new(p)
            }
            "elliott_double_combo" => {
                let mut p = ElliottDoubleCombo::new(points_array::<7>(resampled), &stroke);
                p.degree = old_degree;
                p.link = old_link;
                p.follows = old_follows;
                Box::new(p)
            }
            "elliott_triple_combo" => {
                let mut p = ElliottTripleCombo::new(resampled, &stroke);
                p.degree = old_degree;
                p.link = old_link;
                p.follows = old_follows;
                Box::new(p)
            }
            _ => return None,
        };

        // Identity: the NEW type's own registered `type_id`/`display_name`
        // (read off the fresh instance before its `data` is overwritten),
        // every OTHER field of `PrimitiveData` carried over from the OLD
        // primitive wholesale — see this method's own doc for why the two
        // fields are the deliberate exception.
        let new_type_id = new_prim.data().type_id.clone();
        let new_display_name = new_prim.data().display_name.clone();
        *new_prim.data_mut() = old_data;
        new_prim.data_mut().type_id = new_type_id;
        new_prim.data_mut().display_name = new_display_name;

        // Phase 3: replace at its own index, remap children by ordinal, restitch.
        let b = bubbles.bubble_mut(bid)?;
        let idx = Self::find_index_by_id(b, prim_id)?;
        b.primitives[idx] = new_prim;
        let after = PrimitiveSnapshot::from_primitive(b.primitives[idx].as_ref());

        let children: Vec<(u64, Option<WaveSlot>, Option<WaveSlot>)> = child_before
            .into_iter()
            .map(|(child_id, slot_before)| {
                let slot_after = slot_ordinal(old_type_id, slot_before).and_then(|ord| slot_at_ordinal(new_type, ord));
                if let Some(cidx) = Self::find_index_by_id(b, child_id) {
                    let link = slot_after.map(|slot| WaveLink { parent_id: prim_id, slot });
                    b.primitives[cidx].set_wave_link(link);
                }
                (child_id, Some(slot_before), slot_after)
            })
            .collect();

        Self::restitch_wave_joints(b, prim_id);
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);

        Some((before, after, children))
    }

    /// Insert a primitive at a specific list position, restored from its
    /// OWN serialized snapshot (undo/redo recreation of a deleted or
    /// newly-created primitive). Restoring through
    /// `PrimitiveRegistry::from_json` recreates every field the concrete
    /// type carries — including the ones outside `PrimitiveData` entirely,
    /// like an Elliott wave's `degree`/`link`/`follows` (`elliott::count`'s
    /// own module doc) — never just the shared header a fresh
    /// `registry.create` + `PrimitiveData` overwrite would leave behind.
    /// The position is preserved internally so z-order-dependent operations
    /// replay identically; it is not part of the public id-keyed contract.
    /// `None` when `type_id` has no registered reader for `json`, or `bid`
    /// does not resolve to a live bubble.
    ///
    /// This is the one external-insertion path preset load and undo/redo
    /// restore both go through (`Command::CreatePrimitive`/
    /// `DeletePrimitive` undo, `Command::RestoreAllPrimitives`,
    /// `Command::PromoteWaveChain`'s own parent recreation) — refreshes the
    /// guard cache (`Self::refresh_wave_guards`) after every insert for
    /// exactly that reason: a restored Elliott wave can be someone's
    /// subdivision or predecessor again, and that is a path that can
    /// change what `elliott::guard::violations_in` sees.
    pub(crate) fn insert_at(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        index: usize,
        type_id: &str,
        json: &str,
    ) -> Option<u64> {
        let registry = PrimitiveRegistry::global().read().unwrap();
        let prim = registry.from_json(type_id, json)?;
        drop(registry);
        let id = prim.data().id;
        let Some(b) = bubbles.bubble_mut(bid) else { return None };
        if index <= b.primitives.len() {
            b.primitives.insert(index, prim);
        } else {
            b.primitives.push(prim);
        }
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
        Some(id)
    }

    /// Reconstruct a primitive from a CLIPBOARD snapshot (Ctrl+C/Ctrl+V),
    /// then restamp exactly the fields a paste always overwrites — new id,
    /// cleared origin/window, the caller-supplied pane, and the
    /// caller's already position-shifted points — before inserting it at
    /// `index`. Restoring through the same `PrimitiveRegistry::from_json`
    /// path [`Self::insert_at`] uses, so a pasted Elliott wave keeps its
    /// degree/link/follows instead of resetting to a bare type default the
    /// way a `PrimitiveData`-only reconstruction would.
    ///
    /// Unlike [`Self::insert_at`], the reconstructed primitive's OWN
    /// `PrimitiveData` (color, width, style, text, locked, visible,
    /// timeframe visibility, style reference, …) is wholesale replaced by
    /// `data` — a clone of the copied primitive's data with the pasted
    /// fields already updated by the caller — matching this method's
    /// pre-snapshot behaviour for every field `PrimitiveData` itself owns.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn insert_pasted_at(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        index: usize,
        type_id: &str,
        json: &str,
        points: &[(i64, f64)],
        data: &PrimitiveData,
    ) -> Option<u64> {
        let registry = PrimitiveRegistry::global().read().unwrap();
        let mut prim = registry.from_json(type_id, json)
            .or_else(|| registry.create(type_id, points, Some(&data.color.stroke)))?;
        drop(registry);
        prim.set_points(points);
        *prim.data_mut() = data.clone();
        let id = prim.data().id;
        let Some(b) = bubbles.bubble_mut(bid) else { return None };
        if index <= b.primitives.len() {
            b.primitives.insert(index, prim);
        } else {
            b.primitives.push(prim);
        }
        // Same reason [`Self::insert_at`] refreshes: a pasted Elliott
        // primitive is restored through `from_json` and therefore arrives
        // carrying its own `link`/`follows`, so what the guard sees changed
        // the moment it landed in the collection.
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
        Some(id)
    }

    pub(crate) fn replace_primitive_from_json(&mut self, bubbles: &mut BubbleStore, bid: BubbleId, prim_id: u64, type_id: &str, json: &str) -> bool {
        let replaced = {
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
        };
        // A replace is how an undone point/settings change lands, so the
        // replaced primitive's geometry — and with it every rule read off a
        // parent or a predecessor — can differ from what the cache holds.
        if replaced {
            self.refresh_wave_guards(bubbles, bid);
            self.refresh_bar_derived(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);
        }
        replaced
    }

    // =========================================================================
    // Symbol swap (Audit-A N6 — bubble-scoped compound)
    // =========================================================================

    pub(crate) fn swap_symbol(
        &mut self,
        bubbles: &mut BubbleStore,
        bid: BubbleId,
        address: &InstrumentAddress,
        restore_snaps: Option<&[PrimitiveSnapshot]>,
    ) -> SwapSymbolResult {
        let Some(b) = bubbles.bubble_mut(bid) else {
            return SwapSymbolResult { old_primitives: Vec::new(), restored: 0 };
        };
        let old_primitives: Vec<PrimitiveSnapshot> = b.primitives.iter().map(|p| PrimitiveSnapshot::from_primitive(p.as_ref())).collect();

        // KEY-INTEGRITY (series-leak fix 2026-07-17): this manager must NOT
        // rebind the data bubble's instrument address. It used to write
        // `b.address = address.clone()` here — and because every ApplySymbol
        // caller runs this compound BEFORE `set_bid_instrument_full`, the
        // proper setter then saw `identity_changed == false` and skipped
        // `reset_raw_layers()` on EVERY symbol switch: the old instrument's
        // kline layers/display/trade-built runtime survived and the new
        // symbol's series merged on top (two series on one chart). Instrument
        // identity is owned by `BubbleStore::set_*` alone; drawings only need
        // the new address for stamping restored primitives — taken from the
        // `address` PARAM below.
        b.primitives.clear();

        let mut restored = 0usize;
        if let Some(snaps) = restore_snaps {
            if let Ok(reg) = PrimitiveRegistry::global().read() {
                // Same rule as `add_external`, and for the same reason: a
                // restored primitive KEEPS its own id unless something in
                // this bubble already claimed it. `WaveLink::parent_id` and
                // `follows` name ids, so re-stamping on the way in silently
                // detached every subdivision and every chain — the bubble is
                // cleared just above, so the only ids that can collide are
                // ones this very loop already restored (a legacy preset that
                // saved duplicates).
                let mut claimed: HashSet<u64> = HashSet::new();
                for snap in snaps {
                    if let Some(mut prim) = reg.from_json(&snap.type_id, &snap.json) {
                        let incoming = prim.data().id;
                        if incoming == 0 || !claimed.insert(incoming) {
                            let fresh = alloc_primitive_id();
                            prim.data_mut().id = fresh;
                            claimed.insert(fresh);
                        }
                        if prim.data().symbol.is_empty() {
                            prim.data_mut().symbol = address.symbol.clone();
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

        // The bubble's whole primitive collection was just cleared and
        // possibly refilled from snapshots, so every cached context
        // violation in it describes primitives that are gone.
        self.refresh_wave_guards(bubbles, bid);
        self.refresh_bar_derived(bubbles, bid);

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
    ///
    /// Deliberately does NOT remap `wave_link`/`follows` the way
    /// `mlc_core::drawing::reassign_primitive_ids_remapped` does for a
    /// legitimate copy-with-restamp batch (2026-08-14 report): a collision
    /// here means two primitives already shared one id BEFORE this ran, so
    /// there is no way to tell which of the two a third primitive's
    /// reference to that id actually meant — remapping would be a guess,
    /// not a fix. The caller (`hydration_impl.rs`'s cold-restore sweep)
    /// runs `DrawingManager::clear_dangling_wave_links` right after this,
    /// which drops any reference this renumbering left unresolved instead
    /// of leaving it dangling.
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

    // =========================================================================
    // Scoped park/unpark — bubble-id-subset extraction (shared-BackendCore
    // arc, §5.3). Replaces whole-manager `swap()`: under a single
    // process-wide `BackendCore`, wholesale-swapping would drop every OTHER
    // window's drawing sessions too.
    // =========================================================================

    /// Remove and return only the editing sessions for `bubble_ids` as a
    /// fresh, small `DrawingManager`.
    ///
    /// Used exclusively by preset tab park (`mlc-app/src/persistence/
    /// profile.rs::park_active_preset`).
    pub(crate) fn extract_for_bubbles(&mut self, bubble_ids: &[BubbleId]) -> DrawingManager {
        let mut out = DrawingManager::default();
        for &bid in bubble_ids {
            if let Some(session) = self.sessions.remove(&bid) {
                out.sessions.insert(bid, session);
            }
        }
        out
    }

    /// Re-insert every editing session from `other`.
    ///
    /// Used exclusively by preset tab unpark (`mlc-app/src/persistence/
    /// profile.rs::unpark_preset`).
    pub(crate) fn merge_for_bubbles(&mut self, other: DrawingManager) {
        for (bid, session) in other.sessions {
            self.sessions.insert(bid, session);
        }
    }
}

#[cfg(test)]
mod park_unpark_tests {
    //! Round-trip coverage for `extract_for_bubbles`/`merge_for_bubbles` —
    //! the shared-BackendCore-arc scoped-park replacement for whole-manager
    //! `swap()` (`docs/mlc/plans/shared-backend-core-2026-07-03.md` §5.3).

    use super::*;

    #[test]
    fn extract_then_merge_round_trips_to_identical_manager() {
        let mut mgr = DrawingManager::default();
        let a = BubbleId(1);
        let b = BubbleId(2);
        let c = BubbleId(3);
        // `set_locked` implicitly creates a session (session_mut is
        // entry-or-default) — use distinct locked flags to identify which
        // session traveled where.
        mgr.set_locked(a, true);
        mgr.set_locked(b, true);
        mgr.set_locked(c, false);

        let extracted = mgr.extract_for_bubbles(&[a, b]);
        assert!(!mgr.is_locked(a), "extracted session must be gone from source (is_locked defaults to false when absent)");
        assert!(!mgr.is_locked(b));
        assert!(!mgr.is_locked(c), "non-extracted session must be untouched (still false)");
        assert!(extracted.is_locked(a));
        assert!(extracted.is_locked(b));

        mgr.merge_for_bubbles(extracted);
        assert!(mgr.is_locked(a));
        assert!(mgr.is_locked(b));
        assert!(!mgr.is_locked(c));
    }

    #[test]
    fn extract_other_windows_sessions_are_untouched() {
        let mut mgr = DrawingManager::default();
        let window_a_bid = BubbleId(1);
        let window_b_bid = BubbleId(2);
        mgr.set_locked(window_a_bid, true);
        mgr.set_locked(window_b_bid, true);

        let parked_b = mgr.extract_for_bubbles(&[window_b_bid]);

        assert!(mgr.is_locked(window_a_bid), "window A's session must survive parking window B's preset");
        assert!(!parked_b.is_locked(window_a_bid), "extraction must not leak window A's session into window B's payload");
    }
}

#[cfg(test)]
mod wave_link_restore_tests {
    //! Cold-restore proof for the 2026-08-14 reports ("an Elliott parent
    //! and its subdivision, created in one session, LOSE their relation
    //! across a page reload"). Manual repro the report names: draw an
    //! impulse, subdivide wave 2 as a zigzag, reload.
    //!
    //! First attempt (commit `d36e6ae5`) put the sweep in `mlc-app`'s
    //! `apply_preset_cold`, one level above `BubbleStore::
    //! restore_from_snapshots`. The owner measured it still broken on a
    //! plain wasm page reload. Second attempt (this one) moves the sweep
    //! INTO `restore_from_snapshots` itself — see
    //! `crate::bubble::wave_guards`'s own module doc for why that is the
    //! convergence point every restore path is required to go through.
    //! These tests exercise `BubbleStore::restore_from_snapshots` ALONE,
    //! with no separate sweep call, over a snapshot that made a full JSON
    //! round trip — i.e. the exact bytes a page reload reads back. They
    //! prove the DATA the renderer reads is correct after a bare restore;
    //! the visual half (does the renderer actually skip the parent's own
    //! leg once `subdivided_slots` is filled) needs a running app and a
    //! screenshot, neither available to this agent — UNVERIFIED here.

    use super::*;
    use crate::bubble::{BubbleStore, BubbleView};
    use crate::tag::TagManager;
    use mlc_core::ids::SyncMemberId;
    use mlc_core::preset::snapshots::DataBubbleSnapshot;
    use mlc_core::tag::Tag;

    /// Build one bubble holding an Elliott impulse and, nested into its own
    /// wave-2 slot, a zigzag correction. Returns `(bubbles, tags, bid,
    /// parent_id, child_id)`.
    fn build_impulse_with_zigzag_subdivision() -> (BubbleStore, TagManager, BubbleId, u64, u64) {
        let mut bubbles = BubbleStore::new();
        let bid = bubbles.create_bubble_auto("BTCUSDT".to_string(), mlc_core::state::Timeframe::h1());
        let mut tags = TagManager::new();
        let mut tag = Tag::new_auto(bid, [0.0, 0.0, 0.0, 0.0]);
        tag.members.insert(SyncMemberId::Chart(1));
        tags.insert(tag);

        let mut mgr = DrawingManager::default();
        let parent = ElliottImpulse::new(
            [(0, 0.0), (1_000, 10.0), (2_000, 5.0), (3_000, 15.0), (4_000, 10.0), (5_000, 20.0)],
            "#ffffff",
        );
        let parent_id = mgr.add_external(&mut bubbles, bid, Box::new(parent));

        let mut child = ElliottCorrection::new(
            [(1_000, 10.0), (1_250, 7.0), (1_500, 9.0), (2_000, 5.0)],
            "#ffffff",
        );
        child.correction_type = CorrectionType::Zigzag;
        child.link = Some(WaveLink { parent_id, slot: WaveSlot::Impulse2 });
        let child_id = mgr.add_external(&mut bubbles, bid, Box::new(child));

        (bubbles, tags, bid, parent_id, child_id)
    }

    /// Snapshot `bid`, joined with its paired tag, and round-trip it
    /// through JSON — the exact bytes a page reload reads back.
    fn snapshot_round_trip(bubbles: &BubbleStore, tags: &TagManager, bid: BubbleId) -> Vec<DataBubbleSnapshot> {
        let bubble_ref = bubbles.bubble(bid).expect("bubble exists");
        let view = BubbleView::new(bubble_ref);
        let tag_ref = tags.get(bid).expect("paired tag exists");
        let synced = tags.synced_panels_snapshot();
        let snap = view.to_snapshot(tag_ref, &synced);

        let json = serde_json::to_string(&vec![snap]).expect("snapshot must serialize");
        serde_json::from_str(&json).expect("snapshot must deserialize")
    }

    #[test]
    fn wave_link_survives_snapshot_round_trip() {
        let (bubbles, tags, bid, parent_id, child_id) = build_impulse_with_zigzag_subdivision();
        let snaps = snapshot_round_trip(&bubbles, &tags, bid);

        let mut restored_bubbles = BubbleStore::new();
        let restored_ids = restored_bubbles.restore_from_snapshots(&snaps);
        assert_eq!(restored_ids, vec![bid], "restore_from_snapshots must keep the bubble's own persisted id");

        let restored_link = {
            let b = restored_bubbles.bubble(bid).expect("bubble restored");
            let child = b.primitives.iter().find(|p| p.data().id == child_id)
                .expect("child primitive restored under its own persisted id");
            child.wave_link()
        };
        assert_eq!(
            restored_link,
            Some(WaveLink { parent_id, slot: WaveSlot::Impulse2 }),
            "child's wave_link().parent_id must still resolve to the parent after a snapshot round trip \
             — restore_from_snapshots must not re-stamp a persisted primitive id",
        );
    }

    #[test]
    fn guard_cache_is_filled_by_restore_from_snapshots_alone() {
        let (bubbles, tags, bid, parent_id, _child_id) = build_impulse_with_zigzag_subdivision();
        let snaps = snapshot_round_trip(&bubbles, &tags, bid);

        // No `DrawingManager` anywhere in this test — `restore_from_snapshots`
        // must fill the guard cache on its own, since it is the ONLY
        // production entry point that turns a `DataBubbleSnapshot` list back
        // into live bubbles (see `crate::bubble::wave_guards`'s own module
        // doc). Before this fix the cache stayed at its freshly-deserialized
        // empty default here — exactly the symptom measured in the running
        // app (the parent's own straight leg drawn again under its own
        // subdivision).
        let mut restored_bubbles = BubbleStore::new();
        restored_bubbles.restore_from_snapshots(&snaps);

        let b = restored_bubbles.bubble(bid).expect("bubble restored");
        let parent = b.primitives.iter().find(|p| p.data().id == parent_id).expect("parent restored");
        assert_eq!(
            parent.subdivided_slots(),
            &[WaveSlot::Impulse2],
            "restore_from_snapshots alone must fill subdivided_slots — no separate sweep call needed",
        );
    }

    #[test]
    fn dangling_link_in_a_snapshot_is_cleared_by_restore_from_snapshots_alone() {
        // A file saved before the id-preservation doctrine existed (or
        // hand-edited) can carry a `link` naming an id nothing in the
        // bubble owns any more. Build that shape directly into a
        // `DataBubbleSnapshot` — a healthy save never produces one — and
        // prove `restore_from_snapshots` itself drops it, not a caller-side
        // step.
        let mut bubbles = BubbleStore::new();
        let bid = bubbles.create_bubble_auto("BTCUSDT".to_string(), mlc_core::state::Timeframe::h1());
        let mut tags = TagManager::new();
        let mut tag = Tag::new_auto(bid, [0.0, 0.0, 0.0, 0.0]);
        tag.members.insert(SyncMemberId::Chart(1));
        tags.insert(tag);

        let mut mgr = DrawingManager::default();
        let mut orphan_child = ElliottCorrection::new(
            [(1_000, 10.0), (1_250, 7.0), (1_500, 9.0), (2_000, 5.0)],
            "#ffffff",
        );
        orphan_child.correction_type = CorrectionType::Zigzag;
        orphan_child.link = Some(WaveLink { parent_id: 999_999, slot: WaveSlot::Impulse2 });
        let child_id = mgr.add_external(&mut bubbles, bid, Box::new(orphan_child));

        let snaps = snapshot_round_trip(&bubbles, &tags, bid);

        let mut restored_bubbles = BubbleStore::new();
        restored_bubbles.restore_from_snapshots(&snaps);

        let b = restored_bubbles.bubble(bid).expect("bubble restored");
        let child = b.primitives.iter().find(|p| p.data().id == child_id).expect("child restored");
        assert_eq!(
            child.wave_link(), None,
            "restore_from_snapshots alone must clear a link naming an id no bubble contains, not leave it dangling",
        );
    }
}

#[cfg(test)]
mod auto_adoption_gate_tests {
    //! Proves higher-degree auto-adoption (owner ask 2026-08-14,
    //! `elliott::count::adoptable_children` +
    //! `DrawingManager::adopt_higher_degree`) sits ENTIRELY behind
    //! `EwaEngineConfig::detect_higher_degree`, which defaults to `false` —
    //! drawing a brand new top-level structure exactly over an
    //! already-drawn one must NOT adopt it while the switch is off, even
    //! when the geometry is an exact match (the exact scenario the switch,
    //! once on, is meant to recognise).

    use super::*;
    use crate::bubble::BubbleStore;
    use mlc_core::drawing::primitives_v2::config::WaveDegree;
    use mlc_core::state::ewa_engine::{EwaEngineConfig, set_ewa_engine_config};

    /// Both halves of the gate live in ONE test function on purpose: the
    /// switch is process-global mutable state (`EWA_ENGINE_CONFIG`,
    /// `mlc-core`'s own `OnceLock<RwLock<..>>`), and `cargo test` runs
    /// different `#[test]` functions on different threads by default — two
    /// separate tests each flipping the SAME global would race each other
    /// (measured: splitting this into two functions made the "on" half
    /// intermittently read `false` because the "off" half's own reset ran
    /// concurrently). One function, one thread, deterministic — the same
    /// reason `ewa_engine.rs`'s own `set_then_read_round_trips` test resets
    /// the global at its OWN end rather than relying on test isolation.
    #[test]
    fn adoption_gated_by_detect_higher_degree() {
        let mut bubbles = BubbleStore::new();
        let bid = bubbles.create_bubble_auto("BTCUSDT".to_string(), mlc_core::state::Timeframe::h1());
        let mut mgr = DrawingManager::default();

        // A standalone, already-drawn impulse CONFINED to the new
        // structure's own wave-1 span below (0..1_000) — `adoptable_children`
        // matches a candidate's own overall first/last point, not any
        // interior one, so a would-be wave-1 subdivision must itself start
        // and end there. Its own first/last are 0.05 off the new
        // structure's own wave-1 anchors — inside the 2% click-slop
        // tolerance, and exactly what the "on" half below must weld onto.
        let mut existing = ElliottImpulse::new(
            [(0, 100.05), (200, 103.0), (400, 101.0), (600, 106.0), (800, 104.0), (1_000, 109.95)],
            "#2196F3",
        );
        existing.degree = WaveDegree::Minute;
        let existing_id = mgr.add_external(&mut bubbles, bid, Box::new(existing));

        let click_points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (6_000, 90.0), (7_000, 150.0), (8_000, 100.0), (9_000, 160.0),
        ];
        let click = |mgr: &mut DrawingManager, bubbles: &mut BubbleStore| {
            // Six clicks: `ClickBehavior::MultiPointFixed(6)` finalizes on
            // the 6th (see `on_click_n_point`'s own doc).
            mgr.set_tool(bid, Some("elliott_impulse"));
            for &(ts, price) in &click_points {
                mgr.on_canvas_click(bubbles, bid, ts, price);
            }
        };

        // --- OFF (the default): drawing the exact same geometry must not
        // adopt anything.
        set_ewa_engine_config(EwaEngineConfig::default());
        assert!(!ewa_engine_config().detect_higher_degree, "this half's own precondition");
        click(&mut mgr, &mut bubbles);
        let off_round_id = {
            let b = bubbles.bubble(bid).expect("bubble exists");
            let existing_after = b.primitives.iter().find(|p| p.data().id == existing_id).expect("existing primitive survives");
            assert_eq!(existing_after.wave_link(), None, "detect_higher_degree defaults to false — must not auto-adopt");
            let new_prim = b.primitives.iter().find(|p| p.data().id != existing_id).expect("new impulse was created");
            assert_eq!(new_prim.points(), click_points.to_vec(), "switch off — no weld, points stay exactly as clicked");
            new_prim.data().id
        };
        // This off-round primitive's own points are an EXACT (zero-slop)
        // match for the same slot the on-round below re-targets — left
        // standing, it would out-compete `existing` for adoption (its own
        // default `Intermediate` degree is COARSER than `existing`'s own
        // `Minute`, and `adoptable_children` keeps only the coarsest match
        // per slot). Removed so the on-round below has exactly one
        // candidate to adopt, unambiguously.
        mgr.delete(&mut bubbles, bid, off_round_id);

        // --- ON: the SAME geometry, drawn again, now adopts the existing
        // impulse into its own wave-1 slot.
        let mut cfg = EwaEngineConfig::default();
        cfg.detect_higher_degree = true;
        set_ewa_engine_config(cfg);
        click(&mut mgr, &mut bubbles);
        let new_id = mgr
            .last_index(&bubbles, bid)
            .filter(|&id| id != existing_id)
            .expect("second click sequence created a second new impulse");

        let b = bubbles.bubble(bid).expect("bubble exists");
        let existing_after = b.primitives.iter().find(|p| p.data().id == existing_id).expect("existing primitive survives");
        assert_eq!(existing_after.wave_link(), Some(WaveLink { parent_id: new_id, slot: WaveSlot::Impulse1 }));

        let new_prim = b.primitives.iter().find(|p| p.data().id == new_id).expect("new impulse exists");
        // Welded onto the CHILD's own exact endpoints, not the clicked ones.
        assert_eq!(new_prim.points()[0], (0, 100.05));
        assert_eq!(new_prim.points()[1], (1_000, 109.95));
        // Every OTHER point stays exactly as clicked.
        assert_eq!(new_prim.points()[2..], click_points[2..]);
        assert_eq!(new_prim.wave_degree(), Some(WaveDegree::Minute.coarser()));

        set_ewa_engine_config(EwaEngineConfig::default());
    }
}

#[cfg(test)]
mod impulse_kind_conversion_tests {
    //! Point 4 of the diagonal arc: `conversion_kinds` OFFERS the two
    //! diagonal kinds (`elliott::count`'s own tests prove that half) —
    //! this proves `convert_primitive_kind` itself, once a click actually
    //! fires, keeps the structure's own points and id, the SAME contract
    //! every other in-place Elliott conversion already gets (see that
    //! method's own doc).

    use super::*;
    use crate::bubble::BubbleStore;

    #[test]
    fn converting_impulse_to_ending_diagonal_keeps_points_and_id() {
        let mut bubbles = BubbleStore::new();
        let bid = bubbles.create_bubble_auto("BTCUSDT".to_string(), mlc_core::state::Timeframe::h1());
        let mut mgr = DrawingManager::default();

        let points: [(i64, f64); 6] = [
            (0, 100.0), (1_000, 110.0), (2_000, 105.0), (3_000, 130.0), (4_000, 120.0), (5_000, 140.0),
        ];
        let prim_id = mgr.add_external(&mut bubbles, bid, Box::new(ElliottImpulse::new(points, "#2196F3")));

        let (_before, _after, children) = mgr
            .convert_primitive_kind(&mut bubbles, bid, prim_id, "elliott_impulse:ending_diagonal")
            .expect("a plain impulse must be convertible to an ending diagonal — conversion_kinds offers it unconditionally");
        assert!(children.is_empty(), "no subdivisions were nested, none should be reported");

        let b = bubbles.bubble(bid).expect("bubble exists");
        let converted = b.primitives.iter().find(|p| p.data().id == prim_id)
            .expect("the converted primitive keeps its own id — replaced at its own index, never removed and re-pushed");
        assert_eq!(converted.type_id(), "elliott_impulse", "still an ElliottImpulse structurally, only the kind field changed");
        assert_eq!(
            converted.points(), points.to_vec(),
            "6 pivots -> 6 pivots, so resample_polyline's own n == points.len() short-circuit keeps every point exactly",
        );
        assert_eq!(
            kind_id_of(converted.as_ref()).as_deref(), Some("elliott_impulse:ending_diagonal"),
            "the new primitive's own impulse_kind actually reads back as the requested diagonal, not silently plain",
        );
    }
}

/// Return value of [`DrawingManager::delete_with_snapshot`] — everything an
/// undo command needs to restore a cascading delete in one step.
#[derive(Clone, Debug)]
pub struct DeletedPrimitives {
    /// The deleted primary primitive's own points — kept separately (not
    /// re-parsed from `snapshot.json`) so `Command::primitive_identity` can
    /// hand back a borrowed slice for cold-load id matching.
    pub points: Vec<(i64, f64)>,
    /// The deleted primary primitive's own full serialized state.
    pub snapshot: PrimitiveSnapshot,
    /// Every OTHER primitive the same delete's own subdivision cascade also
    /// removed (e.g. Elliott children nested under the primary primitive) —
    /// empty when nothing cascaded.
    pub cascaded: Vec<PrimitiveSnapshot>,
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

#[cfg(test)]
mod markup_successor_tests {
    //! The non-Elliott half of the succession picker
    //! (`docs/mlc/plans/markup-engine-parity-2026-08-25.md` S4): what may
    //! come next in a family's own narrative, and the fail-closed re-check
    //! that stands behind the offer.

    use super::*;
    use crate::bubble::BubbleStore;

    fn bubble_with(type_id: &str) -> (DrawingManager, BubbleStore, BubbleId, u64) {
        let mgr = DrawingManager::default();
        let mut bubbles = BubbleStore::new();
        let bid = bubbles.create_bubble("BTCUSDT".to_string(), mlc_core::Timeframe::m1());
        let b = bubbles.bubble_mut(bid).expect("just-created bubble");
        let registry = PrimitiveRegistry::global().read().unwrap();
        let prim = registry
            .create(type_id, &[(1_000_000, 100.0)], Some("#3ba3ff"))
            .unwrap_or_else(|| panic!("registry has no {type_id}"));
        let id = prim.data().id;
        b.primitives.push(prim);
        drop(registry);
        (mgr, bubbles, bid, id)
    }

    #[test]
    fn the_shift_is_what_may_follow_a_sweep() {
        let (mut mgr, bubbles, bid, id) = bubble_with("liquidity_sweep");
        assert!(mgr.arm_markup_successor(&bubbles, bid, id, "structure_break"));
        assert_eq!(mgr.current_tool(bid), Some("structure_break"));
    }

    /// The offer is re-checked here, not trusted: a type the grammar does
    /// not allow after this claim arms nothing at all.
    #[test]
    fn a_type_the_grammar_refuses_arms_nothing() {
        let (mut mgr, bubbles, bid, id) = bubble_with("liquidity_sweep");
        assert!(!mgr.arm_markup_successor(&bubbles, bid, id, "order_block"));
        assert_eq!(mgr.current_tool(bid), None, "a refused offer must leave the tool untouched");
    }

    /// Elliott keeps its own write path — this one must decline so the
    /// caller falls through to `create_wave_successor`.
    #[test]
    fn an_elliott_selection_falls_through_to_elliotts_own_path() {
        let (mut mgr, bubbles, bid, id) = bubble_with("elliott_impulse");
        assert!(!mgr.arm_markup_successor(&bubbles, bid, id, "elliott_correction"));
        assert_eq!(mgr.current_tool(bid), None);
    }

    #[test]
    fn an_id_naming_nothing_arms_nothing() {
        let (mut mgr, bubbles, bid, _) = bubble_with("liquidity_sweep");
        assert!(!mgr.arm_markup_successor(&bubbles, bid, 999_999, "structure_break"));
        assert_eq!(mgr.current_tool(bid), None);
    }
}
