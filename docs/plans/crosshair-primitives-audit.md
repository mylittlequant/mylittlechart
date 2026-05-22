# Crosshair Pipeline Audit

Date: 2026-05-22  
Branch: zengeld-chart

---

## A. Crosshair Write/Read Inventory

### A1. `ChartPanelGrid::update_crosshair` — **ACTIVE-BASED (BUG in split mode)**

`crates/chart/src/state/panel_grid.rs:1795`

- Signature: `update_crosshair(x, y, drag_mode, drawing_active, extended) -> Option<(i64, f64, bool, Option<usize>)>`
- Internally calls `self.active_window_mut()` (line 1928, 1931) — always uses the active leaf regardless of which leaf the cursor is over.
- Returns crosshair state from `self.active_window()` (line 1937).
- **Bug**: In split mode, cursor may be over leaf B while active = leaf A. This function reads/writes leaf A's crosshair using leaf B's coordinates. Wrong coordinate system.

Callers:
- `on_drag_move` (non-freehand, non-split path): `mod.rs:3435` — calls `update_crosshair` + propagates using `active_leaf` — **double-active bug**
- `on_drag_move` (freehand branch): `mod.rs:3383` — calls `update_crosshair` with `drawing_active=true` — no split guard
- `on_mouse_move` (single-window non-drawing path): `mod.rs:4630` — calls `update_crosshair` + propagates using `active_leaf` — **this is the correct single-window path, no bug when `!is_split()`**

### A2. `ChartPanelGrid::update_crosshair_split` — **HOVERED-BASED (GOOD)**

`crates/chart/src/state/panel_grid.rs:1966`

- Signature: `update_crosshair_split(x, y, leaf_id, extended) -> Option<(i64, f64, bool, Option<usize>)>`
- Calls `self.window_for_leaf_mut(leaf_id)` (lines 1981, 1984) — correct, uses the explicit hovered leaf.
- Returns data from `self.window_for_leaf(leaf_id)` (line 1988).
- Caller: `on_mouse_move` split path `mod.rs:4502` — passes `leaf_id` from `resolve_input` (hovered leaf). **GOOD.**

### A3. `ChartWindow::update_crosshair_from_global`

`crates/chart/src/state/chart_window.rs:1143`

- Called by both `update_crosshair` (active) and `update_crosshair_split` (hovered). The function itself is neutral — it takes `&mut self` (already bound to a window). Whether it operates on the right window depends on the caller.

### A4. `ChartWindow::set_crosshair_from_timestamp`

`crates/chart/src/state/chart_window.rs:1394`

- Used by sync propagation only. Correct: converts universal timestamp to local bar index, then sets bar/price/visible.
- Direct writes: `crosshair.y` (line 1371), `crosshair.price` (line 1372), `crosshair.snapped_y` (line 1373), `crosshair.snapped_price` (line 1374, 1380).

### A5. Direct `window.crosshair.visible = false` writes

| Location | Who calls | Context |
|----------|-----------|---------|
| `panel_grid.rs:1929` | `update_crosshair` (active) | Over sub-pane separator |
| `panel_grid.rs:1982` | `update_crosshair_split` (hovered) | Over sub-pane separator |
| `mod.rs:4694` | `hide_crosshair()` | Active window only |
| `mod.rs:7934` | `hide_all_split_crosshairs()` | All leaves, no propagation |
| `sync_group.rs:34` | `hide_crosshairs_outside_sync_group()` | Non-group leaves |

---

## B. Crosshair Propagation (sync group)

### B1. `propagate_crosshair_to_sync_group` — `sync_group.rs:979`

- Takes `source_leaf` (caller decides if it's hovered or active).
- Checks `g.sync_flags.sync_crosshair` (group-level flag only — **see Bug E1**).
- Propagates to all leaves in same color-tag group (excluding source) via `set_crosshair_from_timestamp`.
- Also propagates `crosshair_price` to all order-flow panels (DOM, heatmap, L2, footprint, big trades, volume profile).

### B2. Call sites for `propagate_crosshair_to_sync_group`

| Location | Source leaf argument | Correct? |
|----------|---------------------|---------|
| `mod.rs:3439` (`on_drag_move`, non-freehand) | `docking().active_leaf()` | **BUG** — hovered leaf not used |
| `mod.rs:4505` (`on_mouse_move`, split) | `leaf_id` from `resolve_input` | **GOOD** — hovered leaf |
| `mod.rs:4635` (`on_mouse_move`, single-window) | `docking().active_leaf()` | OK — single window, active = hovered |
| `mod.rs:4698` (`hide_crosshair`) | `docking().active_leaf()` | **BUG in split** — see Section C |
| `sync_group.rs:686` (`propagate_crosshair_after_split`) | `new_leaves[0]` (source of split) | OK — seeding |

### B3. Peer receive path

Peers call `window.set_crosshair_from_timestamp(timestamp, price, visible, pane_index)` — converts timestamp to local bar index. This is correct: timestamp is coordinate-system-independent.

### B4. Mouse-leave crosshair clearing

`on_mouse_leave` (mod.rs:4678) calls `hide_crosshair()` which:
1. Clears `active_window_mut().crosshair.visible = false` — **only clears active leaf**
2. Calls `propagate_crosshair_to_sync_group(active_leaf, 0, 0.0, false, None)` — propagates hide to sync peers

**Bug B4-A**: In split mode, `on_mouse_leave` does not call `hide_all_split_crosshairs()`. It clears only the active leaf + its sync peers via propagation. Non-synced non-active leaves are NOT cleared on mouse-leave.

**Bug B4-B**: The `is_drawing` check in `on_mouse_leave` reads `active_window()` not the hovered window. If active leaf != hovered leaf and active window is idle but the hovered leaf's window is drawing, the crosshair will be incorrectly hidden.

---

## C. on_mouse_move / on_drag_move / on_mouse_leave

### C1. `on_drag_move` (`mod.rs:2491`)

**Flow:**

1. Lines 2492–2562: early-return for sidebar/panel/PTY drags — no crosshair.
2. Line 3370: `ui_drag_active` → `hide_crosshair()` (active only, no split guard).
3. Lines 3379–3388: **Freehand branch** — `extend_freehand` fires, then calls `update_crosshair(x, y, drag_mode, drawing_active=true, &extended)` — **active-based, NO is_split() guard**. In split mode with cursor on non-active leaf, this writes to active leaf using coordinates from the non-active leaf's space.
4. Lines 3419–3441: Non-freehand drag:
   - Processes `DragMove` action via `input_handler`
   - Calls `update_crosshair(x, y, drag_mode, false, &extended)` — **active-based**
   - Propagates using `docking().active_leaf()` — **active-based**
   - **No `is_split()` branch exists** — the "GOOD" `update_crosshair_split` path is ONLY in `on_mouse_move`.

**Summary for on_drag_move**:
- Single-window: correct.
- Split mode, non-freehand drag: BUG — calls `update_crosshair` (active) instead of `update_crosshair_split(hovered_leaf)`.
- Split mode, freehand drag: BUG — same issue, AND no propagation call at all after freehand update.

### C2. `on_mouse_move` (`mod.rs:4145`)

**Flow (guards, in order):**

1. Lines 4405–4412: `ui_drag_active` → `hide_all_split_crosshairs()` or `hide_crosshair()`. Correct branching.
2. Lines 4416–4423: watchlist modal open → same. Correct.
3. Lines 4425–4476: Phase 7.3 canvas gate:
   - `hovered_chart_pane = input_coordinator.hovered_widget()?.starts_with("chart:pane:")` — widget-system hovered, not `active_leaf`.
   - `is_drawing = active_window()…is_drawing()` — **BUG**: reads active window's drawing state; if hovered ≠ active and hovered window is drawing, this returns false and the gate may hide crosshair incorrectly.
   - `on_canvas` check uses `build_extended_layout()` (active window layout) via `ExtendedLayoutHitTester` — **BUG in split**: tests cursor against active leaf's sub-pane layout, not hovered leaf's layout. The cursor is outside active leaf's rect so `on_canvas = false`, crosshair is suppressed even though cursor is on a valid canvas.
   - This `on_canvas` check gates BOTH the split path and the single-window path. The split path has its own early `return` at line 4611 so it normally never hits the `on_canvas` guard at 4468 — BUT this gate runs BEFORE the `is_split()` check at line 4485, so it fires for split mode too.
4. Lines 4478–4612: **Split block** (`is_split() && !is_drawing_skip`):
   - `is_drawing_skip = active_window()…is_drawing()` — **BUG**: same issue as above.
   - `resolve_input` → gets actual hovered `leaf_id`. **GOOD**.
   - Calls `update_crosshair_split(x, y, leaf_id, &extended)` — **GOOD** (hovered-based).
   - Propagates via `propagate_crosshair_to_sync_group(leaf_id, …)` — **GOOD** (hovered leaf).
   - Separator/None → `hide_all_split_crosshairs()`. **GOOD** but no sync propagation.
   - Returns early at 4611 — good, falls through only for single-window.
5. Lines 4614–4675: Single-window path:
   - Calls `update_crosshair` (active) — **GOOD** for single window (active = only leaf).
   - Propagates via `active_leaf` — **GOOD** for single window.

### C3. `on_mouse_leave` (`mod.rs:4678`)

- `is_drawing` check uses `active_window()` — **BUG**: if hovered ≠ active in split, wrong window checked.
- If not drawing: calls `hide_crosshair()`:
  - Clears `active_window_mut().crosshair.visible = false` — only active leaf cleared.
  - Propagates to sync peers of active leaf only.
  - Non-synced, non-active leaves: **NOT cleared**. In split mode with independent leaves, those leaves retain their last crosshair position after mouse-leave.
- **Missing**: no call to `hide_all_split_crosshairs()` in split mode.

---

## D. Active vs Hovered Leaf APIs

### D1. Active leaf

- `panel_grid.docking().active_leaf() -> Option<LeafId>` — click-set active leaf.
- `panel_grid.active_window() -> Option<&ChartWindow>` — window for active leaf.
- `panel_grid.active_window_mut() -> Option<&mut ChartWindow>` — mutable.
- `panel_grid.active_chart_id() -> Option<ChartId>`.

### D2. Hovered leaf

**No canonical `hovered_leaf()` accessor exists.**

Hovered leaf is computed on-demand via `resolve_input(x, y, content_rect.x, content_rect.y)` (`panel_grid.rs:1186`). This is a full hit-test (O(n) over leaves) returning a `ChartInputTarget` enum. It is called inline at the usage site and not cached.

In `on_mouse_move` split path: `resolve_input` is called **twice** (lines 4487 and 4520) for the same `(x, y)` — once for crosshair update, once for overlay state. This is redundant.

In `on_drag_move`: `resolve_input` is **never called** — the drag path has no split awareness.

**Recommendation**: Add `fn hovered_leaf(&self, x: f64, y: f64, origin: (f64, f64)) -> Option<LeafId>` accessor that caches within a frame, or call `resolve_input` once and thread the result through.

---

## E. Sync Flags

### E1. `propagate_crosshair_to_sync_group` checks `g.sync_flags.sync_crosshair` (group-level only)

`sync_group.rs:991–995`:
```rust
let should_sync = group_id
    .and_then(|gid| self.panel_app.tag_manager.group(gid))
    .map(|g| g.sync_flags.sync_crosshair)  // ← group-level only
    .unwrap_or(true);
```

**Bug E1**: Per-member override (`MemberSyncOverride.sync_crosshair`) is **ignored**. `SyncGroup::effective_sync_crosshair(member)` exists (`tag_manager/mod.rs:154`) and is used in rendering snapshots (`lib.rs:3763, 4341`) but NOT in the propagation path. Each peer should be individually checked: if a peer has `member_override.sync_crosshair = Some(false)`, it should not receive crosshair updates.

### E2. `hide_crosshairs_outside_sync_group` (`sync_group.rs:12`)

- Checks `g.sync_flags.sync_crosshair` at line 18 — same group-level flag, not per-member.
- Correctly hides non-peer leaves.

### E3. `sync_crosshair` default

`propagate_crosshair_to_sync_group:994`: `.unwrap_or(true)` — if source_leaf has no group, sync still fires. This means ungrouped windows propagate to all color-tag peers with no flag guard. May be intentional legacy behavior.

---

## F. Bugs and Inefficiencies Summary

### Bugs (concrete, file:line)

| # | File:line | What's wrong | What should happen |
|---|-----------|-------------|-------------------|
| **F1** | `mod.rs:3383` | `on_drag_move` freehand branch calls `update_crosshair` (active) in split mode — wrong leaf gets cursor coords | Call `update_crosshair_split(hovered_leaf, …)` where hovered_leaf = `resolve_input(x,y)` |
| **F2** | `mod.rs:3435` | `on_drag_move` non-freehand calls `update_crosshair` (active) — same issue, split not handled | Same fix as F1: detect split, call `update_crosshair_split` |
| **F3** | `mod.rs:3439` | After `update_crosshair`, propagates using `docking().active_leaf()` in split mode | Propagate using hovered leaf (from `resolve_input`) |
| **F4** | `mod.rs:4686` | `on_mouse_leave` calls `hide_crosshair()` which only clears active leaf; in split mode non-synced non-active leaves retain stale crosshair | Call `hide_all_split_crosshairs()` in split mode, then propagate from each leaf's sync group (or propagate hide from a sentinel leaf) |
| **F5** | `mod.rs:4692–4694` | `hide_crosshair()` always writes to `active_window_mut()` regardless of split state | In split mode, clear hovered leaf (caller should know hovered_leaf) or clear all leaves |
| **F6** | `mod.rs:4697–4699` | `hide_crosshair()` propagates via `active_leaf` — in split mode cursor may be on non-active leaf, so active's sync group gets cleared but hovered leaf's sync group does not | Pass hovered_leaf into hide_crosshair or make it split-aware |
| **F7** | `mod.rs:4441–4443` (Phase 7.3 gate) | `is_drawing = active_window()…is_drawing()` — reads active window; if hovered ≠ active, wrong window checked | Read drawing state from hovered leaf's window |
| **F8** | `mod.rs:4452–4456` (Phase 7.3 gate) | `on_canvas` check builds extended layout for active window, hit-tests cursor against active leaf's rect — in split mode, cursor is outside active leaf's rect so `on_canvas = false`, crosshair suppressed for hovered leaf | Build extended layout for the hovered leaf's rect (same pattern as split block at 4499–4500) |
| **F9** | `mod.rs:4482–4484` (`is_drawing_skip`) | Reads `active_window()…is_drawing()` to decide whether to skip the split crosshair block; if active is drawing but hovered is not, the split-mode crosshair block is skipped and cursor falls into the single-window path | Read drawing state from hovered leaf |
| **F10** | `sync_group.rs:991–994` | `propagate_crosshair_to_sync_group` checks group-level `sync_flags.sync_crosshair` only; per-member `MemberSyncOverride` ignored | Per-peer loop should use `group.effective_sync_crosshair(SyncMemberId::Chart(peer_chart_id))` |
| **F11** | `mod.rs:3383–3386` | Freehand `on_drag_move` returns early after `update_crosshair` — no `propagate_crosshair_to_sync_group` call | Add propagation after `update_crosshair` in freehand branch (same as non-freehand branch at 3437–3440) |

### Inefficiencies

| # | Location | Issue |
|---|----------|-------|
| I1 | `mod.rs:4487` and `mod.rs:4520` | `resolve_input` called twice on same `(x,y)` per `on_mouse_move` in split mode — double O(n) leaf scan | Cache result in a local, reuse |
| I2 | `mod.rs:4435` | `input_coordinator.borrow().hovered_widget()` called, then `build_extended_layout()` also called, then split block calls `build_extended_layout_for_leaf` again — 2–3 layout builds per mouse-move in split mode | Share single extended layout where possible |
| I3 | `hide_crosshairs_outside_sync_group` + `propagate_crosshair_to_sync_group` both iterate `leaf_color_tags` — called back-to-back in `on_mouse_move` split path (4505 + 4510) | Single pass could handle both propagate-to-peers and hide-non-peers |
| I4 | `propagate_crosshair_to_sync_group:993` uses `g.sync_flags.sync_crosshair` (early bail) but the per-peer loop at 1007–1011 has no per-member gate | Would need per-member check in loop even after group-level gate |

---

## Appendix: Function Location Index

| Function | File | Line |
|----------|------|------|
| `ChartPanelGrid::update_crosshair` | `crates/chart/src/state/panel_grid.rs` | 1795 |
| `ChartPanelGrid::update_crosshair_split` | `crates/chart/src/state/panel_grid.rs` | 1966 |
| `ChartPanelGrid::resolve_input` | `crates/chart/src/state/panel_grid.rs` | 1186 |
| `ChartPanelGrid::active_window` | `crates/chart/src/state/panel_grid.rs` | 160 |
| `ChartPanelGrid::active_window_mut` | `crates/chart/src/state/panel_grid.rs` | 166 |
| `ChartPanelGrid::window_for_leaf` | `crates/chart/src/state/panel_grid.rs` | 178 |
| `ChartPanelGrid::window_for_leaf_mut` | `crates/chart/src/state/panel_grid.rs` | 184 |
| `ChartWindow::update_crosshair_from_global` | `crates/chart/src/state/chart_window.rs` | 1143 |
| `ChartWindow::set_crosshair_from_timestamp` | `crates/chart/src/state/chart_window.rs` | 1394 |
| `ChartApp::on_drag_move` | `crates/chart-app/src/input/mod.rs` | 2491 |
| `ChartApp::on_mouse_move` | `crates/chart-app/src/input/mod.rs` | 4145 |
| `ChartApp::on_mouse_leave` | `crates/chart-app/src/input/mod.rs` | 4678 |
| `ChartApp::hide_crosshair` | `crates/chart-app/src/input/mod.rs` | 4691 |
| `ChartApp::hide_all_split_crosshairs` | `crates/chart-app/src/input/mod.rs` | 7925 |
| `ChartApp::propagate_crosshair_to_sync_group` | `crates/chart-app/src/input/sync_group.rs` | 979 |
| `ChartApp::hide_crosshairs_outside_sync_group` | `crates/chart-app/src/input/sync_group.rs` | 12 |
| `ChartApp::propagate_crosshair_after_split` | `crates/chart-app/src/input/sync_group.rs` | 650 |
| `ChartApp::propagate_drawing_state_to_sync_group` | `crates/chart-app/src/input/sync_group.rs` | 1044 |
| `SyncGroup::effective_sync_crosshair` | `crates/chart/src/tag_manager/mod.rs` | 154 |
