# Split / Multi-window Bug Batch

Three independent bugs to fix in one batch. All occur in split (multi-window) mode.

## R1 — Scale A/M/F not clickable in split

**Symptom:** click on the A/M/F corner button in any split leaf does not change the displayed letter or the actual `window.price_scale.scale_mode`. Single window works fine.

**Root cause:** `ChartApp.scale_corner_zones: ScaleCornerHitZones` is a single global value. In split mode the render code at `crates/chart-app/src/lib.rs:2809` returns `ScaleCornerHitZones::default()` (empty rects). `handle_canvas_click` (`crates/chart-app/src/input/modals.rs:1005`) calls `self.scale_corner_zones.hit_test(x, y)` and always gets `ScaleCornerButton::None`. The mode-toggle handler never fires.

**Fix:**
- Replace `ChartApp.scale_corner_zones: ScaleCornerHitZones` with `scale_corner_zones_by_leaf: HashMap<LeafId, ScaleCornerHitZones>`. Single mode populates one entry; split mode populates one per leaf.
- `handle_canvas_click` walks the map and finds the leaf whose zones contain `(x, y)`. On hit: `set_active_leaf(leaf_id)`, then apply the existing A/M/F toggle to that leaf's window (resolve via `window_for_leaf_mut`).
- Both single + split go through one code path. The split-only short-circuit `mod.rs:386 ChartInputTarget::ScaleCorner` becomes dead and can be removed.

## R2 — Deleting active leaf in Tags & Tabs blanks the chart

**Symptom:** in Tags & Tabs modal click the X next to the currently-active leaf. All charts disappear. Should: remaining leaves fill the space, active reassigns to the smallest remaining leaf id.

**Root cause hypothesis (verify):** the delete handler removes the leaf from the docking tree but does not reassign `active_leaf`. Renderer then has `active_leaf = None` (or stale/removed id), so single-window render reads no active window and shows nothing.

**Fix:**
- After `tree.remove_leaf(leaf_id)` in the Tags & Tabs delete handler: if the removed leaf was active, set `active_leaf` to `tree.leaves().keys().min()` (smallest remaining id). If no leaves remain → create one fresh default leaf (or block delete-last entirely).
- This is a 5-line fix in the panel_click delete arm. Audit first to confirm root cause matches.

## R3 — Primitives fall into Memory on load (symbol lost)

**Symptom:** on app start primitives flash visible for 1 frame, then drop into the Object Tree "Memory" section. Inspector shows their `symbol` field is empty but `exchange` + `account_type` are correct. A primitive without a symbol is invalid by construction.

**Root cause hypothesis (verify):** something in the load/restore pipeline strips `primitive.data.symbol`. Candidates:
- `restore_drawings_for_symbol` writes drawings to the window keyed on a `(symbol, exchange, account_type)` triple but discards the symbol when applying.
- Snapshot serialization missed `symbol` on `PrimitiveData` somewhere.
- Migration code wipes the field.
- ChangeSymbol propagation rebuilds primitive with empty symbol.

**Fix path:**
- Audit: grep `primitive.data.symbol = "" / String::new()` + audit `PrimitiveSnapshot` (de)ser + `restore_drawings_for_symbol`. Find the one site that writes empty.
- Fix the offending site to preserve the symbol from the snapshot or the current window context.

## Order of work

1. R1 — well-understood, small fix (~30 LOC). Refactor `scale_corner_zones` to per-leaf map.
2. R2 — verify root cause via reading the delete handler, then small fix.
3. R3 — audit-first (read-only), then targeted fix.

One commit per bug, then a release build + smoke test by the user.
