# Phase 4 — Branch-as-Group

**Date:** 2026-06-04
**Baseline:** mlc HEAD `7e6b87a` + uzor HEAD `0f39ceb` (Branch.preserve_if_empty)
**Companion docs:**
- `chart-container-cut-research-2026-06-03.md` — pre-Phase 4 chart internals inventory
- `chart-container-cut-plan-2026-06-03.md` — predecessor's 6-phase plan
- `chart-container-handoff-stalled-sep-drag.md` — what went wrong on Phase 3 attempt
- `phase3-subpane-cut-2026-06-03.md` — abandoned Phase 3 design (Wave 1-3 reverted 2026-06-04)

---

## 1. North star

Every visual group = a uzor `Branch` with sidecar metadata. Every panel
(chart, indicator, DOM, OrderEntry, etc.) = a uzor `Leaf` with a
`PanelLeaf` payload. One docking tree, one input pipeline, one render
walk.

```
DockState<PanelLeaf>
└── Branch (BranchMeta in HashMap<BranchId, BranchMeta>)
    ├── Leaf: PanelLeaf::ChartPanel(ChartPanelId)      ← OHLC canvas
    ├── Leaf: PanelLeaf::IndicatorPanel(InstanceId)    ← sub-pane indicator
    ├── Leaf: PanelLeaf::IndicatorPanel(InstanceId)
    └── Branch (nested ok)
        ├── Leaf: PanelLeaf::Dom(PanelId)
        └── Leaf: PanelLeaf::OrderEntry(PanelId)
```

### Definitions

- **Group = Branch.** Always. Even a single-leaf tag-group is a Branch
  with one child (held alive by `preserve_if_empty`).
- **ChartContainer / TradingContainer / TagGroup = BranchKind.** Stored
  in `BranchMeta.kind`. Same struct, different policy.
- **AcceptPolicy = BranchMeta.accepts.** Decides which leaf kinds can
  land in this branch on drop. ChartContainer accepts ChartPanel +
  IndicatorPanel only. TradingContainer accepts any.
- **Sync group = BranchMeta.group_id.** When two branches share a
  `group_id`, sync propagation routes between them.
- **Nested branches free of restriction.** ChartContainer inside
  TradingContainer is normal. Sub-pane reshuffles via uzor native
  reordering / split.

---

## 2. Why we are throwing away Phase 3 (Wave 1-3)

The 2026-06-03 session built `InnerLayout` extensions
(`positions`/`maximized`/`move_*`/`seed_with_positions`) and a
`SubPaneState` side-table to retire the `sub_panes: Vec<SubPane>`. While
the math compiled and tested, none of it addressed the structural
problem:

1. **Layout still hand-rolled, not uzor.** Sub-pane geometry lived in
   `InnerLayout.proportions`. uzor docking already does proportions +
   nested branches; using two layout engines for one stack guarantees
   desync.
2. **Input pipeline still dual.** Sub-pane overlay buttons (Move/Expand)
   were routed through `handle_canvas_click` rect-cache (`sub_pane_overlay_results`)
   not through the uzor widget pipeline. Buttons not registered as
   widgets cannot be clicked when the cache desyncs.
3. **Drag separator still custom.** `find_separator_at_y` +
   `drag_sub_pane_separator` exist parallel to uzor's
   `drag_separator`. Two implementations, two bug surfaces.

The owner's verdict was correct: rewiring overlay buttons through
`InnerLayout` (Wave 2) was deadweight because the problem is not "two
sources of state" but "two pipelines for the same job". Phase 4 collapses
the pipeline.

**Reverted commits (mlc, 2026-06-04):**
- `c62b8c7` — InnerLayout positions+maximized+move/reorder/seed API
- `261af88` — SubPaneState side-table + ChartContainer helpers
- `67539fa` — rewire MoveUp/Down/Expand/Restore through inner_layout +
  drag content_h fix

`SubPaneState` struct as a concept survives — it reappears in S2 as the
non-geometry state on the chart panel leaf or indicator panel leaf.

---

## 3. Types added in S1

### `PanelLeaf`

```rust
/// Payload for a leaf node in the unified docking tree.
///
/// Each variant references an entry in `TradingPanelsStore` (or
/// `chart_panel` storage once it migrates there in S2).  The leaf
/// itself carries no state; it's a typed pointer.
pub enum PanelLeaf {
    /// OHLC canvas — bars, viewport, drawings, primitives.
    ChartPanel(ChartPanelId),
    /// Indicator visualization — can live either as a chart's sub-pane
    /// (sibling of ChartPanel inside a ChartContainer Branch) or as a
    /// standalone panel in a TradingContainer Branch.  Same render
    /// path either way; data source differs only in viewport inherit.
    IndicatorPanel(InstanceId),
    /// Order book depth.
    Dom(PanelId),
    /// Level-2 tape.
    L2Tape(PanelId),
    /// Trade tape.
    TradeTape(PanelId),
    /// Big trades feed.
    BigTrades(PanelId),
    /// Footprint chart.
    Footprint(PanelId),
    /// Volume profile.
    VolumeProfile(PanelId),
    /// Liquidity heatmap.
    LiquidityHeatmap(PanelId),
    /// Order entry form.
    OrderEntry(PanelId),
    /// Position manager.
    PositionManager(PanelId),
    /// Trade log.
    TradeLog(PanelId),
    /// Risk calculator.
    RiskCalculator(PanelId),
}

impl uzor::panels::DockPanel for PanelLeaf {
    fn title(&self) -> String { … per-kind label … }
    fn type_id(&self) -> String { … e.g. "panel_chart" … }
    fn min_size(&self) -> (f32, f32) { … per-kind … }
    fn closable(&self) -> bool { true }
}
```

`InstanceId(u64)` is the indicator manager's `instance_id` — already the
de-facto stable identity for any indicator across the codebase.

`ChartPanelId(u64)` — new id type for the chart canvas (distinct from
the current `ChartId` which identified a whole `ChartWindow`).

### `BranchMeta` / `BranchKind` / `AcceptPolicy`

```rust
/// Sidecar metadata attached to each uzor Branch.  Lives in
/// `ChartApp.branch_meta: HashMap<BranchId, BranchMeta>`.
///
/// Branches without an entry are treated as `Plain` with `Any` accept
/// policy — backwards compatible with branches uzor creates implicitly
/// during splits.
pub struct BranchMeta {
    pub kind: BranchKind,
    pub accepts: AcceptPolicy,
    /// Sync group this branch belongs to.  When two branches share a
    /// `group_id`, symbol/timeframe/crosshair propagate between them.
    pub group_id: Option<SyncGroupId>,
    /// Symbol context for chart-kind branches.  None for non-chart
    /// branches.  All leaves inside this branch inherit the symbol.
    pub symbol: Option<String>,
    pub timeframe: Option<Timeframe>,
    pub exchange: Option<String>,
    pub account_type: Option<String>,
    /// Display label (used in tab strip, breadcrumb, etc.).
    pub label: Option<String>,
}

pub enum BranchKind {
    /// Chart container — exactly one ChartPanel + N IndicatorPanels.
    /// Symbol, timeframe, indicator source shared across all children.
    ChartContainer,
    /// Trading container — any panel kind allowed.  No shared symbol
    /// at the branch level (each leaf carries its own).
    TradingContainer,
    /// Tag group — visual grouping with sync semantics, but no panel
    /// kind restrictions.  Holds 1..N panels.
    TagGroup,
    /// Plain group — no semantic meaning, uzor-default.  Used for
    /// implicit branches uzor creates during split_leaf.
    Plain,
}

pub enum AcceptPolicy {
    /// Only the listed leaf kinds may be dropped into this branch.
    Only(HashSet<PanelLeafKind>),
    /// Any leaf kind allowed.
    Any,
}

pub enum PanelLeafKind {
    ChartPanel, IndicatorPanel, Dom, L2Tape, TradeTape, BigTrades,
    Footprint, VolumeProfile, LiquidityHeatmap, OrderEntry,
    PositionManager, TradeLog, RiskCalculator,
}
```

A `ChartContainer` branch has:
```rust
BranchMeta {
    kind: ChartContainer,
    accepts: Only([ChartPanel, IndicatorPanel].into()),
    group_id: Some(...),  // shared with sync peers
    symbol: Some("BTCUSDT"),
    timeframe: Some(Timeframe::Hour1),
    exchange: Some("Binance"),
    account_type: Some("S"),
    label: Some("BTCUSDT 1H"),
}
```

A `TradingContainer` branch:
```rust
BranchMeta {
    kind: TradingContainer,
    accepts: Any,
    group_id: None,  // each leaf manages its own group membership
    symbol: None,
    timeframe: None,
    exchange: None,
    account_type: None,
    label: Some("Trading Workspace"),
}
```

### Single-leaf preservation

Every Branch we create with semantic identity (i.e. anything not
`BranchKind::Plain`) MUST be marked
`tree.set_branch_preserve_if_empty(branch_id, true)` after
`split_leaf`. Otherwise uzor's `collapse_single_children_branch` will
dissolve the branch when its child count drops to 1, orphaning the
`BranchMeta` entry.

`Plain` branches keep the default `preserve_if_empty: false` because
they ARE just implicit grouping nodes that should collapse when
deflated.

---

## 4. Stages

| Stage | Goal | Approx commits | Gate |
|---|---|---|---|
| **S0** | Revert Phase 3, write this doc, commit uzor `preserve_if_empty` change. | 1 mlc revert + 1 uzor + 1 design-doc | cargo check baseline |
| **S1** | Add `PanelLeaf` / `BranchMeta` / `BranchKind` / `AcceptPolicy` / `PanelLeafKind` types. Stubs only — no consumer yet. | 1-2 | cargo check |
| **S2** | Stage 8 of old plan: migrate `ChartPanelGrid.containers: HashMap<ChartId, ChartContainer>` → `panels_store.chart_panel: HashMap<ChartPanelId, ChartPanelState>`. `ChartContainer` struct shrinks: chart-canvas fields → `ChartPanelState`; symbol/timeframe/group → `BranchMeta`; sub-pane Vec dies. ~30 callsites mechanical pass. | 5-8 | cargo check + smoke |
| **S3** | Convert outer chart docking `DockState<ChartSubPanel>` → `DockState<PanelLeaf>`. `ChartSubPanel` becomes `PanelLeaf::ChartPanel`. Add `branch_meta` HashMap on `ChartApp`. Existing single-chart layout: one ChartContainer Branch holding one ChartPanel leaf, marked `preserve_if_empty=true`. | 3-5 | cargo check + smoke (split works) |
| **S4** | Sub-pane as `PanelLeaf::IndicatorPanel` leaf inside ChartContainer branch. Indicator-add does `tree.split_leaf(chart_leaf, SplitBottom, IndicatorPanel{id})`. Sub_panes Vec dies. InnerLayout dies. ExtendedFrameLayout.sub_panes dies. Separator drag = uzor native. | 4-6 | live: sep drag works, sub-pane spawn works |
| **S5** | Render path: single `render_dock_tree(tree, area)` walks uzor, dispatches `render_chart_panel_leaf(rect)` / `render_indicator_panel_leaf(rect)` per kind. `render_full_chart_panel` monolith retired. Building blocks (`render_sub_pane_base`, `render_sub_pane_primitives`) re-used as helpers. | 4-6 | visual parity |
| **S6** | Input: every overlay button = `register_widget`. `handle_canvas_click` SubPaneButton arm dies. `find_separator_at_y` dies. `update_sub_pane_overlay_hover` dies. `HitResult::SubPaneOverlayButton/PaneSeparator` die. `pane_index: Option<usize>` → `instance_id: Option<u64>` across crosshair propagation, drag modes, output actions. | 3-5 | live: overlay buttons (Move/Expand/Hide/Delete) all work |
| **S7** | AcceptPolicy enforcement on drop. Sync group rewire: source members = BranchId not ChartId. Crosshair propagation walks branches by `group_id`. | 3-5 | sync test |
| **S8** | TradingContainer as `BranchKind::TradingContainer` + `AcceptPolicy::Any`. UI to create one. Drag any leaf in. Nested ChartContainer inside ok. | 4-8 | live: composite workspace |

Estimated sessions: S1+S2 = 1 session. S3 = 1. S4 = 1-2. S5 = 1-2. S6 =
1. S7 = 1. S8 = 1-2. Total **7-9 sessions**.

---

## 5. Migration paths

### `ChartContainer` field migration

| Old field (`ChartContainer`) | New home |
|---|---|
| `id: ChartId` | DROP — replaced by `BranchId` for the container, `ChartPanelId` for the canvas leaf |
| `title` | `BranchMeta.label` |
| `bars`, `viewport`, `price_scale`, `time_scale` | `ChartPanelState` (the canvas leaf state in `panels_store.chart_panel`) |
| `sub_panes: Vec<SubPane>` | DIES — replaced by `PanelLeaf::IndicatorPanel` leaves inside the Branch |
| `sub_pane_overlay_states / sub_pane_overlay_results` | DIES — per-leaf widget state owned by uzor InputCoordinator |
| `pane_manager` | DIES — already dead |
| `total_chart_height` | DIES — derived from uzor leaf rect |
| `symbol`, `timeframe`, `exchange`, `account_type` | `BranchMeta.symbol / .timeframe / .exchange / .account_type` |
| `is_active`, `active_pane_index` | `BranchMeta` doesn't need this; uzor's `active_leaf` is the source of truth |
| `crosshair`, `kinetic`, `drag_mode`, `last_mouse_pos`, `drag_start_*` | `ChartPanelState` (per-leaf input state) |
| `grid_options`, `crosshair_options`, `legend`, `watermark`, `tooltip` | `ChartPanelState` |
| `drawing_manager` | `ChartPanelState` |
| `signal_manager`, `trade_manager`, `marker_manager` | `BranchMeta` (instrument-level) |
| `indicator_source` | `BranchMeta` (shared across container's chart + sub-panes) |
| `price_lines`, `compare_overlay`, `command_history` | `ChartPanelState` |
| `data_provider` | `BranchMeta` |
| `show_candles / show_bars / … / chart_type` | `ChartPanelState` |
| `scale_settings` | `ChartPanelState` |
| `chart_toolbar`, `toolbar_config` | `ChartPanelState` (per chart leaf — toolbar is chart chrome) |
| `prev_close_price`, `connection_status` | `BranchMeta` |
| `group_id` | `BranchMeta.group_id` |
| `pre_tag_indicator_ids`, `stashed_primitives`, `stashed_command_history` | `BranchMeta` (sync-group bookkeeping) |
| `symbol_drawings` | `ChartPanelState` |
| `pending_symbol_load`, `needs_auto_scale_after_bars`, `scroll_fetch_*` | `ChartPanelState` |
| `restore_scale_mode`, `stashed_scale_mode` | `ChartPanelState` |
| `inner_layout`, `sub_pane_state` | DIES — replaced by uzor docking + per-leaf state |

### Snapshot / preset migration

Existing preset fields kept with `#[serde(default)]` for backwards
compat. Phase 4 reads them on load to reconstruct the new tree:

- `ChartWindowSnapshot.sub_pane_order` → tree topology: split the chart
  leaf N times, position each new IndicatorPanel leaf in the right
  Above/Below half.
- `ChartWindowSnapshot.sub_pane_height_ratios` → uzor branch proportions
  after the splits.
- `ChartWindowSnapshot.sub_pane_above_main` → `SplitTop` vs `SplitBottom`
  on each split call.
- `ChartWindowSnapshot.sub_pane_hidden` → `Leaf.hidden` flag via
  `tree.hide_leaf(leaf_id)`. **Note:** this field is saved today but
  never restored (live bug pre-existing this refactor); Phase 4 fixes
  it incidentally.

On save in Phase 4: walk the uzor tree, find ChartContainer branches,
emit one `ChartWindowSnapshot` per branch with the inner topology
encoded in the existing fields. No format change.

### Sync group migration

- `SyncMemberId::Chart(u64)` semantics shift: `u64` now equals
  `BranchId.0` (was `ChartId.0`). Wire format unchanged (`u64`).
- Crosshair propagation routes by branch: receiver looks up its branch
  via `tree.find_branch_of_leaf(...)`, reads `BranchMeta.group_id`,
  matches against source.
- `pane_index: Option<usize>` in propagation messages → `instance_id:
  Option<u64>` (or `Option<LeafId>`). Atomic rewire in S6.

### IndicatorPanel epoch artifacts

From the prior audit:
- `SubPaneId(u64)` — kept; will be reused or replaced by `LeafId` /
  `InstanceId`.
- `FreeItem::Chart(u64)` — toss in S2 (11 dead match arms).
- `FreeItem::IndicatorPanel(PanelId)` — kept; this is the standalone
  panel (already works for spawn/persist/sync, only render is stub).
  Distinct from `PanelLeaf::IndicatorPanel` (chart-internal sub-pane)
  during the migration; after S4 the two converge into one render path
  with two contexts.
- `panels_store.indicator_panel` — kept (storage backing).
- `IndicatorPanelState` — kept; gains a real render in S5.
- `PersistedFreeItemKind::IndicatorPanel` — kept (preset compat).
- `ChartContainerLeaf::IndicatorPanel` — repurposed as
  `PanelLeaf::IndicatorPanel` (same shape, unified type).

---

## 6. Critical risks

### `collapse_single_children_branch`

Mitigated by uzor commit `0f39ceb` —
`Branch.preserve_if_empty: bool` + `DockingTree::set_branch_preserve_if_empty`.
Every ChartContainer / TradingContainer / TagGroup branch we create
must be marked `preserve_if_empty=true` immediately after creation.

### `AcceptPolicy` enforcement point

uzor does NOT call back to the consumer before
`apply_panel_drop`/`dock_floating`. Options:

- **(a)** Wrap the drop interaction at the consumer layer: intercept
  drag-end, check `AcceptPolicy` of the target Branch, only call uzor's
  drop API if allowed.
- **(b)** Add an accept hook to uzor (separate PR — not in scope for
  S0).

S1-S7 use option (a). S8 may revisit if drag UX becomes complex.

### Serialized format does not carry `preserve_if_empty`

On preset restore, uzor returns all branches with `preserve_if_empty=false`.
After restore, consumer walks the tree, looks up each BranchId in its
restored sidecar `BranchMeta` map, and re-marks the flag. Easy.

### Sub-pane drawing primitives that span main + sub-pane

The earlier research confirmed none exist in production. Phase 4 stays
on this assumption; if a future primitive needs cross-leaf rendering, it
becomes a Branch-level overlay (drawn after the per-leaf walk).

---

## 7. What does NOT change

- `panels_store.dom / footprint / trade_tape / …` — already correct
  shape (HashMap keyed by PanelId).
- `SlotDockingManager`, `AgentDockingManager` — already use the
  branch-with-leaves model with the right `DockPanel` payload pattern.
- `FreeItem` — kept as the sidebar slot payload (separate from
  `PanelLeaf` until S8 merges them, possibly never).
- `InputCoordinator` — already at the right layer (`ChartApp`).
- `TagManager` API — only the member key semantics change (BranchId vs
  ChartId).
- `ChartTheme` / `ScaleTheme` / `FrameTheme` — render-time data,
  unchanged.

---

## 8. Open questions for future stages

- Tab semantics in a ChartContainer Branch: does the user see a tab
  strip for sibling IndicatorPanel leaves, or only vertical stack? S5
  decides.
- Drag-out of an IndicatorPanel from one ChartContainer to another:
  symbol context swap? Source vs destination branch policies on this.
- `FreeItem` vs `PanelLeaf` unification timing: currently sidebar slots
  use `FreeItem`, central area uses `PanelLeaf`. Same underlying panel
  types. Worth unifying in S8 (Trading container becomes the bridge).
- Custom Branch chrome (title bar with breadcrumb, tag color strip):
  not in Phase 4 scope; can be S9+.

---

## 9. Rollback strategy

| Roll back to | Effect |
|---|---|
| S1 commit | Types exist but no consumer; revert is removing 1 file + 1 enum |
| S2 commit | Big mechanical pass landed; revert is multi-commit rebase |
| S3 commit | Outer docking on PanelLeaf; revert means restoring `ChartSubPanel` payload |
| S4 commit | Sub-panes are uzor leaves; rolling back loses indicator add path |
| S5 commit | Render walk replaced monolith; rollback means restoring `render_full_chart_panel` |
| S6 commit | Input via widgets; rollback means restoring rect-cache pipeline |

**Anchor commit for shipping intermediate state:** end of S4. At that
point sub-pane separator drag works via uzor native, overlay buttons
still use rect-cache temporarily, but the structural source-of-truth
problem is solved. If S5 stalls, the codebase is in a clean shippable
intermediate state.

---

## 10. Reading order for the next agent

1. **This doc** — what we're building and why.
2. `chart-container-cut-research-2026-06-03.md` — chart internals
   inventory. Skim §1, §S1–S12.
3. `chart-container-handoff-stalled-sep-drag.md` — what went wrong on
   the prior Phase 3 attempt. Learn the failure modes.
4. uzor commit `0f39ceb` diff — understand
   `preserve_if_empty` mechanics before writing S3.
5. `crates/sidebar-content/src/free_slot.rs` + `panels_store.rs` — see
   how the existing free-slot model works (closest analogue to Phase 4
   target).

Good luck.
