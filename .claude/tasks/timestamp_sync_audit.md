# Timestamp Sync Audit: Missing `point_timestamps` Updates After Primitive Mutations

**Date:** 2026-03-24
**Files audited:**
- `crates/chart-app/src/input.rs`
- `crates/chart-app/src/lib.rs`
- `crates/chart/src/drawing/manager.rs`

---

## Background

- `DrawingManager::sync_primitive_timestamps(prim, bars)` — syncs bar indices → timestamps for one primitive
- `DrawingManager::update_all_timestamps_from_bars(bars)` — syncs all primitives
- `DrawingManager::recalculate_all_bar_caches(bars)` — restores bar indices FROM timestamps (the reverse direction)
- After any mutation that changes bar positions, timestamps MUST be updated

The rule: whenever bar indices change on a primitive (create, drag, move, set_points, clone + translate), `update_all_timestamps_from_bars` (or `sync_primitive_timestamps`) must be called before the next save or TF switch.

---

## Section 1: Mutation Sites in `chart-app/src/input.rs`

### 1A. `on_click` — primitive creation via main chart canvas

**Location:** `input.rs:14307-14316`
**Code path:** user clicks canvas → `drawing_manager.on_click(bar, price)` → primitive completed
**Timestamp sync:** YES — immediately after `primitive_created == true`:
```rust
if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
    let bars = window.bars.clone();
    window.drawing_manager.update_all_timestamps_from_bars(&bars);  // line 14316
}
```
**Status: SYNCED**

---

### 1B. `on_click` — primitive creation via sub-pane canvas

**Location:** `input.rs:14440-14449`
**Code path:** user clicks sub-pane canvas → `drawing_manager.on_click(bar, price)` → primitive completed
**Timestamp sync:** YES — immediately after `primitive_created == true`:
```rust
if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
    let bars = window.bars.clone();
    window.drawing_manager.update_all_timestamps_from_bars(&bars);  // line 14449
}
```
**Status: SYNCED**

---

### 1C. `complete_freehand()` — freehand stroke completion on drag end

**Location:** `input.rs:3166-3215`
**Code path:** `on_drag_end()` detects freehand in progress → `window.drawing_manager.complete_freehand()` → pushes primitive into `drawing_manager.primitives`
**Timestamp sync:** NO — `complete_freehand()` creates a primitive but there is no call to `update_all_timestamps_from_bars` anywhere in this path. The code goes straight to `intercept_completed_primitive_to_group()` → `push_undo_command` → `autosave_snapshot()` without syncing timestamps.

```rust
if is_freehand_drawing {
    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
        window.drawing_manager.complete_freehand();
        // ← NO timestamp sync here!
    }
    if !self.intercept_completed_primitive_to_group() { ... }
    self.autosave_snapshot();
    return;
}
```

**Status: MISSING SYNC**
**Fix:** After `complete_freehand()`, add:
```rust
if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
    let bars = window.bars.clone();
    window.drawing_manager.update_all_timestamps_from_bars(&bars);
}
```

---

### 1D. `set_points` via coord settings — `coord_*_bar` / `coord_*_price` inputs (in prim_settings modal, tablet path ~line 5509-5548)

**Location:** `input.rs:5509-5548`
**Code path:** User edits coordinate value in primitive settings modal → `prims[idx].set_points(&pts)` directly on `drawing_manager.primitives_mut()`
**Timestamp sync:** NO — immediately after `set_points` the code returns without syncing:
```rust
prims[idx].set_points(&pts);
// nothing after this — exits the if block
```
And then at line 10016:
```rust
// Snapshot after any committed text change.
if let Some(idx) = self.panel_app.primitive_settings_state.primitive_idx {
    self.snapshot_primitive_settings_to_user_manager(idx);
}
```
No `update_all_timestamps_from_bars` call.

**Status: MISSING SYNC**
**Fix:** After `prims[idx].set_points(&pts)` for both `coord_*_bar` and `coord_*_price` branches, add a timestamp sync. Since `primitives_mut()` returns `&mut [Box<dyn Primitive>]` but the bars reference is on `window`, the easiest fix is after releasing the borrow:
```rust
// After the set_points block:
if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
    let bars = window.bars.clone();
    window.drawing_manager.update_all_timestamps_from_bars(&bars);
}
```

---

### 1E. `set_points` via coord settings — duplicate at ~line 9934-9973 (auto-commit path)

**Location:** `input.rs:9934-9973`
**Code path:** Same coord_* pattern in a second handler (appears to be the `handle_prim_settings_text_auto_committed` path vs the explicit commit).
**Timestamp sync:** NO — same issue as 1D.

**Status: MISSING SYNC**
**Fix:** Same as 1D — add `update_all_timestamps_from_bars` after both `set_points` calls.

---

### 1F. `translate_at` — clone + offset on "clone" context menu action

**Location:** `input.rs:14880-14897`
**Code path:** User clicks "clone" in context menu → `w.drawing_manager.clone_primitive(idx)` → then `w.drawing_manager.translate_at(new_idx, bar_delta, price_delta)` to offset the clone
**Timestamp sync:** NO — after `translate_at`, the code goes to capture a snapshot for undo, then `sync_drawing_back_to_group()` and `autosave_snapshot()`, but no timestamp sync:
```rust
w.drawing_manager.translate_at(new_idx, bar_delta, price_delta);
w.drawing_manager.select_by_index(new_idx);
// ... capture snapshot for undo ...
self.sync_drawing_back_to_group();
self.autosave_snapshot();
// NO update_all_timestamps_from_bars
```

**Status: MISSING SYNC**
**Fix:** After `translate_at`, before `sync_drawing_back_to_group()`:
```rust
if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
    let bars = w.bars.clone();
    w.drawing_manager.update_all_timestamps_from_bars(&bars);
}
```

---

## Section 2: Mutation Sites in `chart/src/drawing/manager.rs`

### 2A. `update_drag()` — translate during drag (DragType::Move)

**Location:** `manager.rs:1284-1291`
**Code path:** `update_drag(current_bar, current_price)` called per-frame during drag → `self.primitives[idx].translate(bar_delta, price_delta)`
**Timestamp sync:** NO at the manager level — but this is intentional. Timestamps are synced at drag END, not during each drag frame, which is correct (syncing every frame would be wasteful).
However: does `EndPrimitiveDrag` sync timestamps?

**EndPrimitiveDrag handler:** `lib.rs:5541-5584`
```rust
ChartOutputAction::EndPrimitiveDrag => {
    // ... collect move_cmd ...
    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
        window.drawing_manager.end_drag();
    }
    if let Some(cmd) = move_cmd { self.push_undo_command(cmd); }
    // ... propagate to group ...
    self.autosave_snapshot();
    // NO update_all_timestamps_from_bars!
}
```

**Status: MISSING SYNC**
`EndPrimitiveDrag` calls `end_drag()` (which only clears `self.dragging`) and pushes a `MovePrimitive` undo command, but NEVER calls `update_all_timestamps_from_bars`. After a drag completes, the primitive's bar position has changed but `point_timestamps` still holds the pre-drag values.

**Fix:** After `window.drawing_manager.end_drag()`:
```rust
if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
    let bars = window.bars.clone();
    window.drawing_manager.update_all_timestamps_from_bars(&bars);
}
```

---

### 2B. `update_drag_screen()` — control point drag via screen coords

**Location:** `manager.rs:1302-1338`
**Code path:** `update_drag_screen()` → `move_control_point_screen()` per frame
**Timestamp sync:** NO — same situation as 2A. Synced at end via `EndPrimitiveDrag`, but `EndPrimitiveDrag` doesn't sync. Same fix applies.

**Status: MISSING SYNC (same root cause as 2A)**

---

### 2C. `update_drag()` — control point drag (DragType::ControlPoint)

**Location:** `manager.rs:1293-1295`
**Code path:** `update_drag()` → `move_control_point()` per frame
**Timestamp sync:** NO — same as 2A/2B.

**Status: MISSING SYNC (same root cause as 2A)**

---

### 2D. `clone_selected()` — internal clone with +5 bar translate

**Location:** `manager.rs:1700-1715`
**Code path:** `clone_selected()` creates a clone, calls `cloned.translate(5.0, 0.0)`, then pushes to `self.primitives`
**Timestamp sync:** NO — the manager method does not have access to bars, so it cannot sync. The caller (`input.rs`) must sync. Currently the only caller is the old `clone_selected()` path (not the context menu clone), which is internal.
**Note:** The context menu clone uses `clone_primitive()` then `translate_at()` — covered in 1F above. The internal `clone_selected()` is less visible but callers should sync.

**Status: PARTIALLY COVERED** — Callers must sync, not the manager itself. Verify all call sites of `clone_selected()`.

---

### 2E. `translate_at()` — standalone translate by index

**Location:** `manager.rs:1010-1013`
Same concern as above — it's a mutation that changes bar indices. No sync inside the method (correct, bars are not available here). Callers must sync.

The only call site found in `input.rs` is the clone+offset at line 14897 — covered in 1F.

**Status: SEE 1F**

---

### 2F. `set_points_at()` — called by `Command::MovePrimitive` in `apply_command_to_active_window`

**Location:** `manager.rs:1843-1846`, called from `input.rs:6404-6406`
**Code path:** Undo/redo `MovePrimitive` command → `set_points_at()` mutates bar indices
**Timestamp sync:** NO — `apply_command_to_active_window` for `MovePrimitive` calls `set_points_at` without any timestamp sync:
```rust
Command::MovePrimitive { index, new_points, .. } => {
    if let Some(gid) = group_id {
        prim.set_points(new_points); // group path
    } else {
        w.drawing_manager.set_points_at(*index, new_points); // standalone path
    }
    // NO sync!
}
```
Then `post_apply_command_effects` for `MovePrimitive` only calls `autosave_snapshot()`, not `update_all_timestamps_from_bars`.

**Status: MISSING SYNC**
**Fix:** In `apply_command_to_active_window` after the `MovePrimitive` branch, OR in `post_apply_command_effects` for `MovePrimitive`, sync timestamps:
```rust
Command::MovePrimitive { .. } => {
    // after set_points_at or group set_points:
    if let Some(w) = self.panel_app.panel_grid.active_window_mut() {
        let bars = w.bars.clone();
        w.drawing_manager.update_all_timestamps_from_bars(&bars);
    }
    self.autosave_snapshot();
}
```
Note: for the GROUP path, the group primitives are mutated but timestamps in `group.primitives[idx].data().point_timestamps` are also not synced.

---

### 2G. `create_at()` / `insert_at()` — called by `Command::CreatePrimitive` undo/redo

**Location:** `manager.rs:1806-1835`, called from `input.rs:6332-6348`
**Code path:** Undo/redo `CreatePrimitive` restores a primitive with its saved bar indices
**Timestamp sync:** The restored primitive's `point_timestamps` come from the serialized `PrimitiveData` in the command snapshot (which was captured at creation time, when timestamps WERE synced). So timestamps should be correct in the data.

**BUT:** `recalculate_all_bar_caches` is not called on undo, so if bars shifted since the original creation (e.g., new bars loaded), the bar indices in the restored primitive may be stale.

The `BarsLoaded` handler does call `recalculate_all_bar_caches` + `update_all_timestamps_from_bars` as a belt-and-suspenders measure, which would fix this eventually.

**Status: ACCEPTABLE (deferred until BarsLoaded)** — but fragile. Consider adding sync in `post_apply_command_effects` for `CreatePrimitive`.

---

### 2H. `replace_primitive_from_json()` — called by undo of `ModifyPrimitiveFull`

**Location:** `manager.rs:1885-1902`
**Code path:** Restores full primitive state from JSON snapshot including bar positions
**Timestamp sync:** Timestamps are stored inside the JSON (via `PrimitiveData.point_timestamps`). If JSON was captured with correct timestamps, they survive round-trip. If not, timestamps are wrong.

**Status: DEPENDS ON SNAPSHOT CORRECTNESS** — if the snapshot was taken after a timestamp sync, this is fine. If the snapshot was taken from a state where timestamps were already stale, the stale values get restored.

---

## Section 3: `chart-app/src/lib.rs` — `BarsLoaded` and `LoadPreset`

### 3A. `BarsLoaded` — initial bars received

**Location:** `lib.rs:1796-1834`
**Code path:** After `set_bars()` or `update_bars()` → `recalculate_all_bar_caches` → `update_all_timestamps_from_bars`
**Status: FULLY SYNCED**
Both calls happen unconditionally for every matched window:
```rust
window.drawing_manager.recalculate_all_bar_caches(&window.bars);  // line 1830
window.drawing_manager.update_all_timestamps_from_bars(&window.bars);  // line 1834
```

---

### 3B. `LoadPreset` — full layout restore path (new presets with layout snapshot)

**Location:** `input.rs:15748-15783`
**Code path:** For each window in the preset, bars arrive from snapshot or provider → `recalculate_all_bar_caches` called

Case 1 — bars from snapshot (`!snap.bars.is_empty()`):
```rust
window.drawing_manager.recalculate_all_bar_caches(&window.bars);  // line 15759
```
`update_all_timestamps_from_bars` is NOT called.

Case 2 — bars from provider synchronously:
```rust
window.drawing_manager.recalculate_all_bar_caches(&window.bars);  // line 15767
```
`update_all_timestamps_from_bars` is NOT called.

Case 3 — bars arrive async via `BarsLoaded` later → `BarsLoaded` handler does sync both (see 3A).

**Status:** Cases 1 and 2: PARTIALLY SYNCED — `recalculate_all_bar_caches` rewrites bar indices from stored timestamps, but `update_all_timestamps_from_bars` (the reverse write) is skipped. This means after restoring a preset, the bar indices are correct but timestamps are not re-written (they weren't changed, so this is actually fine — the timestamps already exist from before). **This is OK as long as the loaded primitives already have correct timestamps in their `PrimitiveData.point_timestamps`.**

However: **if any primitive has empty `point_timestamps` (e.g., old data serialized before the timestamp system existed), `recalculate_all_bar_caches` skips it** (line 861-863):
```rust
if timestamps.is_empty() {
    // No timestamps stored yet - skip (primitive will use current bar positions)
    continue;
}
```
These primitives will silently lose anchoring on TF switch.

---

### 3C. `LoadPreset` — fallback patch path (old presets without layout snapshot)

**Location:** `input.rs:15910-15921`
**Code path:** Same three cases, same issue as 3B.

Case 1 (`!snap.bars.is_empty()`):
```rust
window.drawing_manager.recalculate_all_bar_caches(&window.bars);  // line 15912
// NO update_all_timestamps_from_bars
```

Case 2 (provider synchronous):
```rust
window.drawing_manager.recalculate_all_bar_caches(&window.bars);  // line 15918
// NO update_all_timestamps_from_bars
```

**Status:** Same as 3B — acceptable for primitives with timestamps, but migration gap for old primitives.

---

## Section 4: `to_json` / `from_json` — Timestamp Serialization

The `point_timestamps` field lives in `PrimitiveData`. Based on the code:
- `manager.rs:1904-1906`: `get_primitive_json` → `prim.to_json()` — primitives serialize via their own `to_json()`
- `manager.rs:1885-1902`: `replace_primitive_from_json` → `registry.from_json(type_id, json)` — full round-trip

**Key question:** does `PrimitiveData` (with `point_timestamps`) survive serialization?
- `apply_command_to_active_window` for `ModifyPrimitiveData` does NOT copy `point_timestamps` (line 1854-1868 of manager.rs: `set_data_at` explicitly skips `point_timestamps` and other "immutable" fields):
```rust
pub fn set_data_at(&mut self, index: usize, data: &super::primitives_v2::PrimitiveData) {
    let prim_data = prim.data_mut();
    prim_data.color = data.color.clone();
    prim_data.width = data.width;
    prim_data.style = data.style.clone();
    prim_data.visible = data.visible;
    prim_data.locked = data.locked;
    prim_data.display_name = data.display_name.clone();
    prim_data.text = data.text.clone();
    prim_data.timeframe_visibility = data.timeframe_visibility.clone();
    // Note: we don't change id or other immutable properties
    // ← point_timestamps is NOT copied!
}
```

**Status: `set_data_at` STRIPS `point_timestamps`**
When `ModifyPrimitiveData` undo/redo is applied, `set_data_at` restores the snapshot data but silently discards `point_timestamps`. After this, the primitive loses its timestamp anchoring.

---

## Section 5: `recalculate_all_bar_caches` — Skip Logic

**Location:** `manager.rs:858-885`

```rust
pub fn recalculate_all_bar_caches(&mut self, bars: &[Bar]) {
    for prim in &mut self.primitives {
        let timestamps = &prim.data().point_timestamps;
        if timestamps.is_empty() {
            // No timestamps stored yet - skip
            continue;  // ← SKIPS primitives without timestamps
        }
        let current_points = prim.points();
        if current_points.len() != timestamps.len() {
            // Mismatch - skip to avoid corruption
            continue;  // ← SKIPS primitives where count changed
        }
        // ... recalculate bar indices from timestamps ...
    }
}
```

**When it skips:**
1. `point_timestamps.is_empty()` — primitive was created before timestamp system, or after a mutation that cleared/skipped timestamps (e.g., `set_data_at`, undo of `ModifyPrimitiveData`)
2. `current_points.len() != timestamps.len()` — point count mismatch, e.g., Fib with variable levels

**Effect of skipping:** Primitive keeps its current bar indices unchanged. On TF switch, it will NOT reposition to the correct time — it stays at whatever bar index it had before.

---

## Summary Table

| # | Location | Mutation | Sync After? | Severity |
|---|----------|----------|-------------|----------|
| 1A | `input.rs:14307-14316` | `on_click` main canvas | YES | OK |
| 1B | `input.rs:14440-14449` | `on_click` sub-pane | YES | OK |
| 1C | `input.rs:3166-3215` | `complete_freehand()` | **NO** | HIGH — freehand drawings lose TF anchoring |
| 1D | `input.rs:5509-5548` | coord settings `set_points` (commit) | **NO** | MEDIUM — coordinate edits lose TF anchoring |
| 1E | `input.rs:9934-9973` | coord settings `set_points` (auto-commit) | **NO** | MEDIUM — same as 1D |
| 1F | `input.rs:14880-14897` | clone + `translate_at` | **NO** | MEDIUM — cloned primitive has wrong timestamps |
| 2A | `lib.rs:5541-5584` (`EndPrimitiveDrag`) | drag end (translate) | **NO** | HIGH — all drags lose anchoring |
| 2B | `lib.rs:5541-5584` (`EndPrimitiveDrag`) | drag end (control point) | **NO** | HIGH — same root cause as 2A |
| 2C | `manager.rs:1293-1295` | `move_control_point` per frame | NO (correct) | OK — deferred to drag end |
| 2D | `manager.rs:1700-1715` | `clone_selected` (+translate) | **NO** | MEDIUM |
| 2F | `input.rs:6396-6407` | undo/redo `MovePrimitive` | **NO** | HIGH — undo/redo of moves loses anchoring |
| 2G | `input.rs:6332-6348` | undo/redo `CreatePrimitive` | DEFERRED | OK if timestamps were saved |
| 2H | `manager.rs:1885-1902` | undo/redo `ModifyPrimitiveFull` | DEPENDS | MEDIUM — depends on snapshot quality |
| 3A | `lib.rs:1830-1834` | `BarsLoaded` | YES (both) | OK |
| 3B | `input.rs:15748-15783` | `LoadPreset` layout path | PARTIAL (recalc only) | LOW — OK for new primitives |
| 3C | `input.rs:15910-15921` | `LoadPreset` fallback path | PARTIAL (recalc only) | LOW — OK for new primitives |
| 4 | `manager.rs:1854-1868` | `set_data_at` (`ModifyPrimitiveData`) | **STRIPS point_timestamps** | HIGH — undo of config changes clears anchoring |

---

## Priority Fixes

### P0 — `EndPrimitiveDrag` (2A/2B) — affects EVERY drag operation

In `lib.rs` after `window.drawing_manager.end_drag()`:
```rust
ChartOutputAction::EndPrimitiveDrag => {
    // ... existing code ...
    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
        window.drawing_manager.end_drag();
    }
    // ADD THIS:
    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
        let bars = window.bars.clone();
        window.drawing_manager.update_all_timestamps_from_bars(&bars);
    }
    // ... rest of existing code ...
}
```

### P0 — `set_data_at` strips `point_timestamps` (section 4)

In `manager.rs:1854-1868`, `set_data_at` must NOT overwrite `point_timestamps`:
```rust
// Already correct: the comment says "we don't change id or other immutable properties"
// But point_timestamps is NOT being preserved — it's also not being set
// so if the incoming `data` has different timestamps, they are silently discarded.
// The real issue is that `point_timestamps` from the undo snapshot data
// should be RESTORED during undo of ModifyPrimitiveData.
// Fix: add to set_data_at:
prim_data.point_timestamps = data.point_timestamps.clone();
```

### P1 — `complete_freehand()` (1C)

In `input.rs` after `window.drawing_manager.complete_freehand()`:
```rust
if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
    let bars = window.bars.clone();
    window.drawing_manager.update_all_timestamps_from_bars(&bars);
}
```

### P1 — undo/redo `MovePrimitive` (2F)

In `post_apply_command_effects` in `input.rs`:
```rust
Command::CreatePrimitive { .. } | Command::DeletePrimitive { .. }
| Command::DeleteAllPrimitives { .. } | Command::RestoreAllPrimitives { .. }
| Command::MovePrimitive { .. } | Command::ModifyPrimitiveData { .. }
| Command::ModifyPrimitiveFull { .. } => {
    // ADD: sync timestamps after ANY primitive command
    if let Some(window) = self.panel_app.panel_grid.active_window_mut() {
        let bars = window.bars.clone();
        window.drawing_manager.update_all_timestamps_from_bars(&bars);
    }
    self.autosave_snapshot();
}
```

### P2 — coord settings `set_points` (1D, 1E)

Both `coord_*_bar` and `coord_*_price` branches need timestamp sync after `prims[idx].set_points(&pts)`.

### P2 — clone + translate_at (1F)

After `w.drawing_manager.translate_at(new_idx, bar_delta, price_delta)` in the "clone" context menu handler, add timestamp sync.
