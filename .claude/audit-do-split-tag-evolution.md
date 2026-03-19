# Audit: do_split / perform_desync Tag Evolution

**Date**: 2026-03-19
**Repo**: `mylittlechart/`
**Files examined**: `crates/chart-app/src/input.rs`, `crates/chart/src/tag_manager/mod.rs`, `crates/chart-app/src/lib.rs`

---

## Tag System Spec (Correct Behaviour)

| Scenario | Expected result |
|---|---|
| Window at startup | Gets invisible `auto_created` group (no color in UI) |
| Split UNTAGGED (auto) window | Both windows get **new visible color tag** (promoted to real group, shared) |
| Split TAGGED window | Both windows stay in **same existing tag group** |
| UNTAG from visible tag | Window disconnects, gets **new invisible auto group** |
| UNTAG from visible tag (had own prims before joining) | New auto group + **stashed_primitives restored** |
| Split child (new pane born from split) | No stash — window never had its own content |
| Primitives sync | ONLY within same `group_id`, forwarded by pre-render `sync_from_group_primitives` each frame |

---

## Commit History for do_split / perform_desync

```
cbf55c8  (pre-0.5.95)  — original logic, NO auto_created concept
c80fde2  (0.5.95-99)   — introduced auto_created groups, desync creates new auto group
01745c9  (0.5.103)     — REGRESSION: split auto → gave each leaf its own auto group (isolated),
                          instead of promoting to shared visible tag
79bcbbf  (0.5.104)     — FIXED: split auto → promotes to shared visible color tag (current HEAD)
```

---

## State 1: Before v0.5.95 (commit `cbf55c8~1`)

### do_split logic

```
1. Snapshot: pre_split_color (from leaf_color_tags), pre_split_group_id, pre_split_chart_id
2. Call split_active(kind) → new_leaves[]
3. If pre_split_group_id exists → disconnect old chart, reuse that group for new leaves
   Else → call next_unused_color() + create_group() with that color
4. Insert group's color into leaf_color_tags for all new_leaves
5. Compute is_new_group by checking members.is_empty() && primitives.is_empty()
6. Connect all new leaf charts to group in TagManager
7. Set window.group_id for all new leaves
8. If is_new_group:
   a. Snapshot source window's pre-tag indicator IDs → window.pre_tag_indicator_ids
   b. Clone source window's own primitives → window.stashed_primitives
   c. Stash source command history → window.stashed_command_history
9. Clear primitives from non-source (split children) windows
10. If !is_new_group → sync_group_indicators_to_new_members()
11. If is_new_group → seed group.primitives + group.indicator_configs from source window
12. Create indicator instances for non-source leaves from tag
13. sync_sub_panes_from_manager()
```

**Key property**: There was no `auto_created` concept. Every window that had no `group_id` would get a new real visible-color group on split. The check `if let Some(existing_group) = pre_split_group_id` meant that if the window was already in a group (tagged), split reused it. If not, it created a new one.

### perform_desync logic (before v0.5.95)

```
1. Remove from leaf_color_tags
2. Snapshot had_group_id, pre_tag_ids, has_stash
3. Disconnect chart from TagManager group
4. Clear drawing_manager primitives
5. If has_stash → restore stashed_primitives
6. Set window.group_id = None  (TERMINAL — window ends up with no group!)
7. Clear pre_tag_indicator_ids, stashed_primitives
8. Restore stashed_command_history
9. Remove tag indicators, unhide pre-tag indicators
10. sync_sub_panes_from_manager()
```

**Bug in pre-0.5.95**: `window.group_id` was set to `None` permanently. No new auto group was created. This was the "standalone path is dead" violation — windows existed without any group.

---

## State 2: At v0.5.95 (commit `c80fde2`)

### What changed

**c80fde2** introduced the `auto_created` field on `SyncGroup` and three related changes:

1. **New window startup**: when a new chart is created, `create_group_auto()` is immediately called and the chart is connected to it. Every window always has a group.

2. **perform_desync**: instead of setting `group_id = None`, a new `create_group_auto()` group is created for the desynced window. The restored primitives from `drawing_manager` are moved into the new auto group's `group.primitives`.

3. **Preset restore**: when re-populating `leaf_color_tags` after load, `auto_created` groups are skipped (`if group.auto_created { continue; }`).

### do_split at v0.5.95 — CRITICAL FLAW

The `do_split` logic at `c80fde2` does **not** check `auto_created`. When splitting a window that has an auto group:

```rust
let group_id = if let Some(existing_group) = pre_split_group_id {
    // Source leaf was already in a group — reuse it.
    if let Some(old_cid) = pre_split_chart_id {
        let _ = self.panel_app.tag_manager.disconnect(old_cid);
    }
    existing_group  // <-- reuses the AUTO group for BOTH new leaves!
} else {
    ...
```

This means splitting an untagged (auto) window causes both new leaves to share the same `auto_created` group — so their primitives sync. But `auto_created` groups are invisible in the UI (no color swatch shown), so the user has no way to know they are synced. This was the **primitive leaking** bug.

---

## State 3: At v0.5.103 (commit `01745c9`) — REGRESSION

### What changed

`01745c9` added `pre_split_is_auto` check:

```rust
let pre_split_is_auto = pre_split_group_id
    .and_then(|gid| self.panel_app.tag_manager.group(gid))
    .map(|g| g.auto_created)
    .unwrap_or(false);
```

But the fix was **wrong**: it gave each split leaf its **own separate auto group**:

```rust
if pre_split_is_auto {
    // Source leaf — reconnect to its original auto group.
    // Non-source leaves — each gets a brand-new independent auto group.
    for &leaf_id in new_leaves.iter().skip(1) {
        let new_auto_gid = self.panel_app.tag_manager.create_group_auto(...);
        ...
    }
    // Early-return: skip the shared-group logic below entirely.
    self.sync_sub_panes_from_manager();
    return;
}
```

This means **split UNTAGGED → both windows still look untagged** (no color swatch), and they are isolated from each other. They don't sync at all, which contradicts the spec: "Split UNTAGGED → both windows get first free color tag (shared group)".

Also, this version had the `pre_split_group_id` disconnect happen **before** the branch, but the early return skipped stash/seeding logic entirely, causing the source window to keep primitives in `drawing_manager` but not move them to the group. This creates a desync: the new auto group's `group.primitives` is empty, but `drawing_manager` has content. On the next frame, `sync_from_group_primitives` would **clear** the source window's drawings (because the auto group has no primitives seeded into it).

---

## State 4: At v0.5.104 (commit `79bcbbf`) — FIXED (current HEAD)

### What changed

`79bcbbf` reverted the per-leaf isolation and instead **promotes auto → visible tag**:

```rust
if pre_split_is_auto {
    // Was untagged (auto group) — split promotes to a real color tag.
    let color = self.panel_app.tag_manager.next_unused_color();
    let gid = self.panel_app.tag_manager.create_group(color, symbol, timeframe);
    // auto_created: false  (visible in UI)
    gid
}
```

Both split windows are placed into this new visible group. The flow then continues through the full shared-group path (stash, seed, `leaf_color_tags`, indicators).

This is correct per spec: **Split UNTAGGED → both windows get first free color tag**.

---

## The sync_from_group_primitives Primitive Leak

`crates/chart-app/src/lib.rs` lines ~2999–3060 run **every frame** (pre-render):

```rust
// For each leaf with a group_id:
if let Some(group) = tag_manager.group(group_id) {
    let cloned = group.primitives.iter().map(|p| p.clone_box()).collect();
    // ...
    window.drawing_manager.sync_from_group_primitives(&cloned);
}
```

`sync_from_group_primitives` (in `crates/chart/src/drawing/manager.rs:792`) fully replaces `drawing_manager.primitives` with the group's primitive list every frame.

### When does this cause leaking?

1. **Auto group has no primitives seeded** (v0.5.103 bug): After split, the source window's auto group is empty. On next frame, `sync_from_group_primitives([])` wipes all the window's drawings.

2. **Two different group_ids sharing primitives**: This cannot happen directly. Primitives only leak if two windows somehow share the same `group_id` when they shouldn't. The v0.5.95 bug (reusing auto group for both leaves without visible tag) caused exactly this: both windows shared an auto group invisibly.

3. **Drawing during drag suppressed**: The sync correctly skips update during drag (`if !window.drawing_manager.is_dragging()`), so in-progress drawings are safe.

### Correct flow: drawing a primitive on a tagged window

```
User draws → ChartApp records CreatePrimitive into group.primitives (not dm)
           → Next frame: sync_from_group_primitives clones group.primitives into dm
           → All group members get the same update (each has same group_id → same group.primitives clone)
```

No cross-group leaking is possible in this flow as long as every window has a unique group that it does not share with unintended peers.

---

## Summary: Where Each Bug Was Introduced

| Commit | Bug |
|---|---|
| `cbf55c8~1` (pre-0.5.95) | `perform_desync` set `group_id = None`, windows had no group |
| `c80fde2` (0.5.95) | `do_split` reused auto group for both leaves → invisible shared sync (primitive leak in UI) |
| `01745c9` (0.5.103) | `do_split` gave each leaf its own isolated auto group → spec violated (split should create visible tag); also source window primitives wiped on next frame because auto group had empty `group.primitives` |
| `79bcbbf` (0.5.104) | **Correct**: auto → visible tag promotion; both leaves share new group; stash/seed logic runs |

---

## Correct do_split Spec

```
BEFORE SPLIT:
  snapshot: active_leaf, pre_split_color, pre_split_group_id, pre_split_chart_id, pre_split_is_auto

CALL split_active(kind) → new_leaves[]

DETERMINE target group_id:
  Case A: pre_split_group_id exists AND NOT auto_created
    → disconnect old chart from group (leaf is destroyed)
    → group_id = existing tagged group (all new leaves share it)
    → is_new_group = false

  Case B: pre_split_group_id exists AND auto_created
    → disconnect old chart from auto group
    → create new VISIBLE group (create_group, auto_created:false) with next_unused_color()
    → group_id = new visible group
    → is_new_group = true  (no members, no state yet)

  Case C: no pre_split_group_id
    → create new VISIBLE group with pre_split_color or next_unused_color()
    → group_id = new group
    → is_new_group = true

ASSIGN group:
  → leaf_color_tags[leaf] = group.color for ALL new_leaves
  → tag_manager.connect(chart_id, group_id) for ALL new_leaves
  → window.group_id = Some(group_id) for ALL new_leaves

IF is_new_group:
  source = new_leaves[0]
  → snapshot source indicator IDs → window.pre_tag_indicator_ids
  → clone source own primitives → window.stashed_primitives
  → stash source command_history → window.stashed_command_history
  → seed group.primitives = clone of source primitives (own only, origin_id==None)
  → seed group.indicator_configs from source indicators
  → create indicator instances for non-source leaves via sync_group_indicators_to_new_members

IF !is_new_group:
  → clear primitives from ALL new_leaves (group sync will fill each frame)
  → sync_group_indicators_to_new_members for ALL new_leaves

ALWAYS:
  → sync_sub_panes_from_manager()
```

---

## Correct perform_desync Spec

```
1. Remove from leaf_color_tags (no color swatch in UI)
2. Snapshot: had_group_id, pre_tag_ids, has_stash
3. tag_manager.disconnect(chart_id)
4. Clear drawing_manager.primitives (group sync will no longer run for this window)
5. If has_stash → restore stashed_primitives into drawing_manager
6. Set window.group_id = None (temporary)
7. Clear pre_tag_indicator_ids, stashed_primitives
8. Restore stashed_command_history
9. Remove tag indicators (those NOT in pre_tag_ids)
10. Unhide pre_tag_ids indicators
11. Create new auto group: create_group_auto(next_unused_color(), symbol, tf)
12. tag_manager.connect(chart_id, new_auto_group_id)
13. window.group_id = Some(new_auto_group_id)
14. Move drawing_manager primitives into new_auto_group.primitives
    (so pre-render sync doesn't wipe them on next frame)
15. sync_sub_panes_from_manager()
```

Step 14 is critical: after desync, the window has restored primitives in `drawing_manager`. If they are not seeded into the new auto group, the next frame's `sync_from_group_primitives([])` wipes them.

---

## File References

- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart-app/src/input.rs` — `do_split` (line ~16419), `perform_desync` (line ~16149)
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart/src/tag_manager/mod.rs` — `SyncGroup`, `create_group`, `create_group_auto`
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart-app/src/lib.rs` — pre-render `sync_from_group_primitives` loop (lines ~2999–3060)
- `C:/Users/VA PC/CODING/ML_TRADING/nemo/mylittlechart/crates/chart/src/drawing/manager.rs` — `sync_from_group_primitives` (line ~792)
