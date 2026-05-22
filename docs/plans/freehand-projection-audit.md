# Freehand-Drag Pipeline Audit — F-COLOR + F-PROJ

HEAD `3a5db75` · read-only · 2026-05-23

---

## Section A — Freehand preview render path

**Two call sites** in `render_chart.rs` draw the in-progress stroke each frame:

| Site | Guard | Lines |
|------|-------|-------|
| Sub-pane path | `dm.is_drawing() && dm.current_pane() == Some(instance_id)` | [render_chart.rs:1894–1944](crates/chart/src/layout/render_chart.rs#L1894) |
| Main chart path | `dm.is_drawing() && dm.current_pane().is_none()` | [render_chart.rs:2439–2501](crates/chart/src/layout/render_chart.rs#L2439) |

Both paths are structurally identical:

```
if dm.is_freehand_tool() {
    let effective_color = dm.effective_color();   // ← BUG F-COLOR
    ...draw stroke using effective_color...
} else {
    dm.create_preview(...)   // click-based path (fixed by d2b29a1)
}
```

**Points buffer**: `dm.drawing_points()` returns `DrawingState::Creating { points, .. }` slice.
Populated by `add_freehand_point` → `extend_freehand` during drag-move. The DM holds exactly one `Vec<(f64,f64)>` inside `DrawingState::Creating`.

**Color source — the bug**:

`dm.effective_color()` at [`manager.rs:384`](crates/chart/src/drawing/manager.rs#L384):

```rust
pub fn effective_color(&self) -> String {
    if let Some(tool_id) = &self.current_tool {
        let registry = PrimitiveRegistry::global().read().unwrap();
        if let Some(meta) = registry.get(tool_id) {
            return meta.default_color.to_string();  // ← registry constant, NOT style_store
        }
    }
    self.default_color.clone()
}
```

Returns `meta.default_color` — a compile-time registry constant (e.g. `"#2196F3"` blue).
Does **not** read `self.style_store`, which holds the user-picked toolbar color.

---

## Section B — Freehand finalize path

`complete_freehand()` at [`manager.rs:621`](crates/chart/src/drawing/manager.rs#L621).

Color seed inside `complete_freehand` at `manager.rs:638–640`:

```rust
let color_str: String = self.style_store.read().ok()
    .and_then(|s| s.get(&tool_id_clone).and_then(|st| st.color.clone()))
    .unwrap_or_else(|| self.default_color.clone());
```

**Correctly reads `style_store`** — commit `d2b29a1` fix is present and correct.
The finalized primitive gets the right color; only the live preview does not.

**Finalize → peer propagation** (standalone path):
[`mod.rs:4119–4133`](crates/chart-app/src/input/mod.rs#L4119)
```
complete_freehand()
  → intercept_completed_primitive_to_group()  [grouped path]
  → propagate_new_primitive_to_sync_group()   [standalone path]
```
Both paths correctly publish the finished primitive to peers.

---

## Section C — Sync-group projection for freehand (F-PROJ)

### Is `propagate_drawing_state_to_sync_group` called during freehand drag-move?

**No.** The drag-move handler at [`mod.rs:3400`](crates/chart-app/src/input/mod.rs#L3400):

```rust
if self.panel_app.panel_grid.extend_freehand(x, y, &extended) {
    // ... crosshair update / split-mode handling ...
    return;   // ← exits without calling propagate_drawing_state_to_sync_group
}
```

`propagate_drawing_state_to_sync_group` is called only from:
- `modals.rs:1366, 1389, 1403, 1518, 1532` — click-primitive completion/point-add events
- `mod.rs:5896` — tool deselect (clears peer state)

Not from the freehand drag-move path.

### What `propagate_drawing_state_to_sync_group` actually propagates

At [`sync_group.rs:1185–1198`](crates/chart-app/src/input/sync_group.rs#L1185):

```rust
let (tool_id, points) = match w.drawing_manager.drawing_state() {
    DrawingState::Creating { tool_id, points } => (Some(tool_id.clone()), points.clone()),
    DrawingState::Idle => (None, Vec::new()),
};
// → peer_window.drawing_manager.set_synced_drawing_state(tool_id, points)
```

It extracts `(tool_id, points)` from the source DM's `DrawingState::Creating` and calls `set_synced_drawing_state` on peers. This **would work** for freehand IF it were called — the peer DM enters `DrawingState::Creating` with the propagated points.

### Does the peer rendering path handle synced freehand state?

The peer render guard at `render_chart.rs:1899`:

```rust
if dm.is_freehand_tool() { ... }
```

`is_freehand_tool()` at [`manager.rs:607`](crates/chart/src/drawing/manager.rs#L607) checks `self.current_tool`. On peers, `current_tool` is **not set** — `set_synced_drawing_state` only sets `self.state`, not `self.current_tool`. So `is_freehand_tool()` returns `false` on the peer, and the freehand render branch is **never entered** even if the peer has `DrawingState::Creating` with the synced points.

The peer falls into `create_preview()` which returns `None` for `FreehandDrag` (as documented in `primitives-pipeline-audit.md`).

**Double gap**:
1. `propagate_drawing_state_to_sync_group` never called on freehand drag-move.
2. Even if called, peer render path won't fire: `is_freehand_tool()` needs `current_tool` set, but sync only sets `state`.

---

## Section D — Concrete fix recommendations

### F-COLOR fix

**File**: [`crates/chart/src/drawing/manager.rs:384–392`](crates/chart/src/drawing/manager.rs#L384)

**Change**: `effective_color()` must read `style_store[tool_id].color` first, fall back to `meta.default_color`, then `self.default_color`. Mirror the pattern already used in `complete_freehand` at `manager.rs:638`.

```rust
// BEFORE (reads registry constant):
return meta.default_color.to_string();

// AFTER (read style_store first, same as complete_freehand):
let stored = self.style_store.read().ok()
    .and_then(|s| s.get(tool_id).and_then(|st| st.color.clone()));
if let Some(c) = stored {
    return c;
}
return meta.default_color.to_string();
```

Two call sites in `render_chart.rs` (`1903`, `2454`) both call `dm.effective_color()` — both get the fix for free with no render changes.

---

### F-PROJ fix — design (a) recommended

**Design choice**: **(a) add `propagate_drawing_state_to_sync_group` call on every freehand point added, AND fix the peer render guard.**

Two surgical changes:

**Change 1** — call propagation on every freehand extend:
[`crates/chart-app/src/input/mod.rs:3400–3428`](crates/chart-app/src/input/mod.rs#L3400)

After `extend_freehand` returns `true`, before `return`:
```rust
// add inside the `if extend_freehand` block, after crosshair update:
if let Some(active_leaf) = self.panel_app.panel_grid.docking().active_leaf() {
    self.propagate_drawing_state_to_sync_group(active_leaf);
}
```

**Change 2** — fix peer render guard to not require `current_tool`:
[`crates/chart/src/layout/render_chart.rs:1899`](crates/chart/src/layout/render_chart.rs#L1899) and `2450`

Replace `dm.is_freehand_tool()` with a check that works on both source and peer DMs:
```rust
// Instead of:
if dm.is_freehand_tool() {
// Use:
if dm.is_drawing_freehand_state() {
```

Add `is_drawing_freehand_state()` to `DrawingManager` — checks `DrawingState::Creating { tool_id, .. }` and looks up `tool_id` in registry for `FreehandDrag`. This works on peers because the peer's `state` carries the tool_id even when `current_tool` is `None`.

Alternatively, extend `set_synced_drawing_state` to also set `current_tool` on the peer (simpler, one-line fix, but has side-effects on peer toolbar state). Not recommended.

**Design (b)** (incremental primitive publish every N points) is cruder and requires a special ephemeral primitive type. Reject.

**Design (c)** (special "live stroke" primitive) is equivalent to (a) but adds a new primitive variant with no benefit. Reject.

---

## Summary table

| Bug | Root cause file:line | Fix file:line |
|-----|---------------------|---------------|
| F-COLOR | `manager.rs:388` — `effective_color()` returns `meta.default_color` (registry constant) instead of `style_store[tool_id].color` | `manager.rs:384–392` — add `style_store` read before registry fallback |
| F-PROJ (gap 1) | `mod.rs:3400–3428` — `extend_freehand` block exits without `propagate_drawing_state_to_sync_group` | `mod.rs:3427` — insert propagate call before `return` |
| F-PROJ (gap 2) | `render_chart.rs:1899,2450` — guard `is_freehand_tool()` checks `current_tool` (not set on peers) | `manager.rs` — add `is_drawing_freehand_state()` that reads tool_id from `DrawingState::Creating` |
