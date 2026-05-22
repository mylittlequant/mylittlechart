# Primitives Pipeline Audit

**Scope**: primitive creation, preview, sync-group projection, inline toolbar style.  
**Basis**: source read 2026-05-22, branch `zengeld-chart`.

---

## A. Primitive Creation Path

### A1. Style applied at creation (all ClickBehavior variants)

Every creation path ends with `apply_last_style_snapshot(prim, last_style)`.

| Entry point | File:line | `default_color` used as seed | `last_used_style` applied |
|---|---|---|---|
| `on_click` — SingleClick | `manager.rs:659` | `&self.default_color` | `last_style` cloned at `:648` |
| `on_click` — TwoPoint 2nd click | `manager.rs:693` | `&self.default_color` | same `last_style` |
| `on_click` — ThreePoint 3rd click | `manager.rs:737` | `&self.default_color` | same |
| `on_click` — FourPoint | `manager.rs:773` | `&self.default_color` | same |
| `on_click` — MultiPoint auto-finish | `manager.rs:811` | `&self.default_color` | same |
| `finish_multipoint` | `manager.rs:859` | `&self.default_color` | via `apply_last_style_to_prim` |
| `complete_freehand` | `manager.rs:542` | `meta.default_color` (tool metadata!) | via `apply_last_style_to_prim` `:549` |

**Bug A1**: `complete_freehand` seeds factory with `meta.default_color` (tool-registry constant), not `self.default_color` / `last_used_style.color`. If `last_used_style` has a color entry, `apply_last_style_to_prim` overwrites it correctly at `:549`. But if `last_used_style` is empty (first stroke after cold start), the primitive gets the registry metadata color, not `self.default_color`. All other paths use `self.default_color` as seed. Inconsistency.

### A2. Where `last_used_style` is populated

1. `save_last_style_from_data(data)` — `manager.rs:2146` — saves basic fields only (no `style_properties`)
2. `save_last_style_at_index(index)` — `manager.rs:2166` — full capture including extended style_properties. Called from `snapshot_primitive_settings_to_user_manager` → `lib.rs:6133`
3. `load_last_styles(map)` — `manager.rs:2208` — restore from persisted JSON on startup (does NOT overwrite already-set entries)

`snapshot_primitive_settings_to_user_manager` is called from:
- `color_picker.rs:621` — after color change
- `modals.rs:152, 162, 257, 274` — after inline toolbar style/width changes
- `settings.rs` — ~15 call sites after settings panel changes

**Important**: `save_last_style_at_index` only touches the **active window's** `DrawingManager`. Peer windows' `last_used_style` are never updated.

### A3. Where `default_color` is set/changed

- Initialized to `"#2196F3"` — `manager.rs:203`
- `set_default_color(&str)` — `manager.rs:282` — public setter
- **No call site in chart-app sets `default_color` in response to toolbar color selection.** The toolbar writes to `last_used_style` via `snapshot_primitive_settings_to_user_manager`, not to `default_color`. So `default_color` is essentially a cold-start fallback that is never updated at runtime.

---

## B. Drawing-State Propagation (In-Progress Preview)

### B1. `propagate_drawing_state_to_sync_group` — all call sites

| File:line | Context |
|---|---|
| `modals.rs:1353` | Main-pane click finalized primitive (standalone path) — clears peer preview |
| `modals.rs:1376` | Same, grouped path |
| `modals.rs:1389,1390` | Main-pane click added a point (not yet complete) — propagates |
| `modals.rs:1482` | Sub-pane click finalized |
| `modals.rs:1505` | Sub-pane click finalized (grouped) |
| `modals.rs:1518,1519` | Sub-pane click added a point |
| `mod.rs:5725` | Freehand complete — clears peer preview |

**All 7 call sites pass `active_leaf`** (from `self.panel_app.panel_grid.docking().active_leaf()`), not hovered leaf. This is correct for click-based tools (the click routes through `set_active_leaf` before arriving here), but see Section E for caveat.

### B2. What propagation carries

`propagate_drawing_state_to_sync_group` at `sync_group.rs:1044` extracts:
```
(tool_id: Option<String>, points: Vec<(f64, f64)>)
```
No style information is extracted or passed. Calls `set_synced_drawing_state(tool_id, points)` on each peer at `sync_group.rs:1087`.

`set_synced_drawing_state` at `manager.rs:436` only writes `DrawingState::Creating { tool_id, points }`. It does **not** touch `last_used_style` or `default_color` on the peer.

### B3. Peer preview color — where it diverges

`create_preview` at `manager.rs:578` builds the seed color at `:613`:
```rust
let seed_color = self.last_used_style
    .get(tool_id)
    .and_then(|s| s.color.as_deref())
    .unwrap_or(&self.default_color);  // ← peer's OWN default_color
```
Then applies peer's own `last_used_style` at `:622`.

**Root cause of color desync (Bug B1)**: peer DM has its own `last_used_style` HashMap (empty or from a previous session) and its own `default_color` (`#2196F3` unless never set). Neither is updated when the source window changes color. If source drew a red line, peer preview shows blue.

**Exact divergence line**: `manager.rs:613–616` — peer reads from its own `last_used_style` / `default_color`, which are never synchronized.

---

## C. Finalized Primitive Propagation

### C1. `propagate_new_primitive_to_sync_group` (legacy / standalone path)

`sync_group.rs:902`. Called from:
- `modals.rs:1351, 1480` — main pane and sub-pane click completion (standalone only)
- `mod.rs:4064` — freehand complete (standalone)

Calls `clone_primitive_for_sync(prim_id, peer_chart_id.0)` at `:959`. This clones the fully-styled primitive (color is already applied at source creation time). **Color is correct here** — the clone carries source's color.

Guard at `:930–937`: skipped when `group_id.is_some()` — grouped windows go through TagManager path.

### C2. TagManager / grouped path

`intercept_completed_primitive_to_group` at `sync_group.rs:1101`. Pops the last primitive from the active window's DM and pushes into `group.primitives`. Primitive already has final style from creation.  
`sync_from_group_primitives` at `manager.rs:907` — called each frame in `prepare_frame` — distributes `group.primitives` to all member DMs via `clone_box`. **Color correct here too.**

### C3. `update_synced_primitive_points` — `lib.rs:5821`

Updates points only. No style fields touched. Color of existing peer clone is unchanged. Not a color bug, but if a primitive were somehow out of sync in color before this call the desync would persist.

### C4. Where finalized color CAN differ from preview color

During the drawing-in-progress phase, peer preview uses wrong color (Bug B1).  
Once finalized, the fully-styled primitive is cloned — correct color reaches peers.  
**Result**: visual flicker during drawing — preview wrong color → primitive finalized → jumps to correct color. User sees the preview in wrong color while drawing.

---

## D. Inline Toolbar — Style Change Application

### D1. Color change path

User clicks inline color swatch → `color_picker.rs` hit-test → `apply_primitive_color(color)` at `color_picker.rs:580`.

Inside `apply_primitive_color`:
1. Reads `primitive_settings_state.primitive_idx` — the selected primitive index in the **active** window (`color_picker.rs:585`)
2. Calls `active_window_mut()` and mutates `data.color.stroke` (`color_picker.rs:592–614`)
3. Calls `set_data_at(idx, &data)` — writes back to active DM
4. Calls `sync_drawing_back_to_group()` (`color_picker.rs:617`) → writes active DM back to `group.primitives` → per-frame sync distributes to all peer DMs. **Peer primitive color updated correctly.**
5. Calls `snapshot_primitive_settings_to_user_manager(idx)` (`color_picker.rs:621`) → calls `save_last_style_at_index(idx)` on **active window only** (`lib.rs:6133`).

**Bug D1**: `save_last_style_at_index` updates `last_used_style` only in the active window's DM (`lib.rs:6133`). Peer DMs' `last_used_style` remain stale. Next time the peer is used as source (user draws in split peer window), the new color is not inherited.

### D2. Width / line-style change path (`modals.rs:156–163`)

```
inline:width → increase_selected_width() → sync_drawing_back_to_group() → snapshot_primitive_settings_to_user_manager(idx)
```
Same issue: `save_last_style_at_index` updates source DM only.

### D3. What commit `75ce206` fixed / did NOT fix

The commit synchronized `primitive_settings_state.primitive_idx` so that inline toolbar changes operate on the correct index. This fixed the wrong-primitive problem (toolbar was applying style to idx=0 when selected was idx=5, etc.).

It did **NOT** fix:
- Peer DMs' `last_used_style` not being updated (Bug D1)
- Preview color desync during in-progress drawing (Bug B1)

### D4. "Default for next draw" path — setting color before drawing

There is no explicit "pre-draw color picker" separate from the inline toolbar. Setting color via inline toolbar on an existing primitive triggers `snapshot_primitive_settings_to_user_manager` → `save_last_style_at_index` → updates source DM's `last_used_style`. Peer DMs unchanged → next draw in peer window uses stale color.

There is **no API call path** that propagates `last_used_style` changes to peer DMs. This is the central architectural gap.

---

## E. Hovered-vs-Active for Primitive Creation

### E1. Click-based tools

Flow in `on_drag_start` / `on_click` (`mod.rs:2370–2401`):
1. `resolve_input(x, y)` returns `ChartInputTarget::Chart { leaf_id }` — this is the **physically hovered** leaf
2. `set_active_leaf(leaf_id)` at `mod.rs:2388` — **active is updated to hovered BEFORE drawing click**
3. Then `handle_canvas_click` calls `active_window_mut()` — which is now the hovered leaf's window

**Correct**: primitive lands in the hovered leaf. Active and hovered are the same at click time.

### E2. Freehand tools

`handle_drag_start` at `mod.rs:2436` calls `panel_grid.handle_drag_start(x, y, &extended)`. Inside that, `start_freehand` is called on the active window's DM. Same routing: `set_active_leaf` runs at `:2388` before `handle_drag_start`. Freehand starts in hovered leaf. **Correct.**

### E3. Sub-pane drawing

`modals.rs:1440–1450`: drawing click on sub-pane calls `active_window_mut().drawing_manager.on_click(bar, price)`. Same active=hovered guarantee (set_active_leaf fires in on_drag_start before modals dispatch). **Correct.**

### E4. Crosshair vs. primitive-creation source of truth

Both use `active_window()` after `set_active_leaf` routing. They share the same leaf source. No known divergence here.

---

## F. Bugs and Inefficiencies

### F1. Bug — Preview color desync (CRITICAL)

**File:line**: `manager.rs:613–616`  
**What**: `create_preview` on a peer DM uses peer's own `last_used_style` / `default_color` for preview color. Both are never synchronized from source.  
**Expected**: peer preview matches source preview color exactly.  
**Fix**: pass a `TemplateStyle` snapshot through `set_synced_drawing_state` and cache it in DM alongside the state. `create_preview` uses that snapshot when available.

### F2. Bug — `last_used_style` not propagated to peers after style edit

**File:line**: `lib.rs:6133`, `color_picker.rs:621`, `modals.rs:152,162,257,274` and all other `snapshot_primitive_settings_to_user_manager` call sites  
**What**: `save_last_style_at_index` updates only the active window's DM. All peer DMs retain stale `last_used_style`.  
**Expected**: after user changes style on primitive in window A, next draw in window B (a group peer) should inherit same style.  
**Fix**: after `save_last_style_at_index`, iterate group member DMs and call `dm.last_used_style.insert(type_id, style.clone())` on each. Or: hoist `last_used_style` out of `DrawingManager` into a group-level / app-level `Arc<Mutex<HashMap<String, TemplateStyle>>>`.

### F3. Bug — `complete_freehand` ignores `default_color` for cold start

**File:line**: `manager.rs:542`  
**What**: seeds factory with `meta.default_color` (registry constant), not `self.default_color`. Other `on_click` paths use `self.default_color`. If no `last_used_style` entry exists, freehand creates with registry constant, click-tools create with `self.default_color`.  
**Fix**: use `self.last_used_style.get(tool_id).and_then(|s| s.color.as_deref()).unwrap_or(&self.default_color)` as seed, same as `create_preview` does at `:613`.

### F4. Bug — `propagate_drawing_state_to_sync_group` marked DEPRECATED but still sole mechanism

**File:line**: `sync_group.rs:1041–1043` comment  
**What**: doc says "DEPRECATED, will be removed once all windows use TagManager groups" but it is still the only mechanism for in-progress preview propagation to peers — called from 7 sites. The TagManager path has no equivalent for preview state.  
**Risk**: function will be removed prematurely, breaking peer preview.

### F5. Bug — `propagate_drawing_state_to_sync_group` uses `active_leaf`, but does NOT use group-gating on the propagation fan-out

**File:line**: `sync_group.rs:1062–1065`  
**What**: peers are found by `leaf_color_tags` matching, not by TagManager group membership. A window in a different group but same color tag would receive spurious preview state. This is inconsistent with `propagate_new_primitive_to_sync_group` which already has a `group_id.is_some()` guard.

### F6. Inefficiency — `create_preview` called every frame unconditionally

**File:line**: `render_chart.rs:1948, 2505`  
**What**: `dm.create_preview(cursor_bar, cursor_price)` creates a new `Box<dyn Primitive>` heap allocation every frame (60fps) during in-progress drawing, including on peer DMs. No change detection.  
**Fix**: cache preview primitive in DM; invalidate only when `state.points`, cursor position, or style changes.

### F7. Inefficiency — `PrimitiveRegistry::global().read().unwrap()` inside `create_preview`

**File:line**: `manager.rs:587`  
**What**: global `RwLock` acquired and released every frame per DM instance during preview rendering. With N split windows all rendering preview, N lock acquisitions per frame.  
**Fix**: the registry is read-only after init; pass `&PrimitiveRegistry` as a parameter or pre-cache the `meta` pointer.

### F8. Inefficiency — `sync_drawing_back_to_group` on every style change clones entire primitives list

**File:line**: `sync_group.rs:1233–1238`  
**What**: called ~30+ times from various style-change handlers. Each call clones all primitives in the window via `clone_box` into `group.primitives`. With 100+ primitives this is a full deep clone per keystroke/drag.  
**Fix**: diff-based or ID-based targeted update for single-primitive edits.

### F9. Inefficiency — `propagate_drawing_state_to_sync_group` triggered on every mouse-move point added

**File:line**: `modals.rs:1389, 1518` — called after every `on_click` point addition  
These are click-type additions, not mouse-move. But for freehand, `add_freehand_point` is NOT followed by propagation at all. So freehand peer preview is static (only the initial `start_freehand` creates the state; subsequent points are not propagated). This is a **feature gap**: peer preview doesn't animate for freehand strokes.

### F10. Architectural smell — per-window `last_used_style` HashMap

**File**: `manager.rs:156`  
The `last_used_style` is conceptually app-global (or at least group-global): "when I draw the next trend_line, use red, 2px". Storing it inside each `DrawingManager` forces all synchronization to be manual. Every path that changes style must explicitly fan out to all peer DMs — which is currently not done anywhere.  
**Fix**: hoist `last_used_style: Arc<Mutex<HashMap<String, TemplateStyle>>>` to either `ChartApp` level (truly global) or `SyncGroup` level. All DMs share the same reference; no propagation needed.

---

## Summary Table

| ID | Severity | File:line | Description |
|---|---|---|---|
| B1 | HIGH | `manager.rs:613–616` | Peer preview color always wrong (uses peer's own `last_used_style`) |
| D1 | HIGH | `lib.rs:6133` | `save_last_style_at_index` updates source DM only; peers stale |
| A1 | MED | `manager.rs:542` | `complete_freehand` uses `meta.default_color` not `self.default_color` as factory seed |
| F5 | MED | `sync_group.rs:1062` | Preview propagation fan-out ignores TagManager group boundary |
| F4 | LOW | `sync_group.rs:1041` | Deprecated fn is still sole preview-sync mechanism; removal risk |
| F9 | LOW | `mod.rs:5725` | Freehand peer preview static (points not propagated during stroke) |
| F6 | PERF | `render_chart.rs:1948,2505` | Preview primitive heap-allocated every frame |
| F7 | PERF | `manager.rs:587` | Global `RwLock` acquired every frame per DM |
| F8 | PERF | `sync_group.rs:1233` | Full primitives clone on every style change |
| F10 | ARCH | `manager.rs:156` | `last_used_style` per-DM; should be group/app-level shared |
