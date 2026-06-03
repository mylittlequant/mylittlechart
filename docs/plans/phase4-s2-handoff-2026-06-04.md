# Phase 4 — S2 Handoff (Stage 8 migration)

**Date:** 2026-06-04
**Session this hands off TO:** the next agent who will execute S2.
**Read order before touching code:**
1. `docs/plans/phase4-branch-as-group-2026-06-04.md` — Phase 4 north-star design + 8-stage roadmap.
2. **This doc** — exact S2 scope, verified call counts, decision points, gotchas, gates.
3. `docs/plans/chart-container-handoff-stalled-sep-drag.md` — failure modes of the previous Phase 3 attempt. Read so you do not repeat them.

---

## 1. State of the world at handoff

### Commits on `main` branch (mlc)

```
85c9d1e feat(state): Phase 4 branch-as-group types — PanelLeaf, BranchMeta, AcceptPolicy (S1)
d7e418f docs(phase4): branch-as-group design + roadmap (S0)
7e6b87a fix(event_handler): route CursorMoved straight to on_drag_move while mouse_pressed   ← baseline
```

`7e6b87a` is the clean baseline. The Phase 3 attempt (`c62b8c7`, `261af88`, `67539fa`) was
reverted via `git reset --hard 7e6b87a` before S0. There is no merge, no stash, no
in-flight branch — `main` is linear.

Working tree: clean. `git status` is clean. `cargo check -p chart-app-vello` is green at
HEAD `85c9d1e`.

### Companion uzor change

`nemo/uzor` HEAD is `2f62551` (newer than the Phase 4 commit). The relevant commit is:

```
0f39ceb feat(docking): Branch.preserve_if_empty flag + set_branch_preserve_if_empty
```

Local-only, not published. The mlc workspace consumes uzor via `path = "../uzor/uzor"`
(per `nemo/uzor/CLAUDE.md` — local consumers pick up path-dep changes immediately, no
publish needed). **Do NOT bump the nemo submodule pointer, do NOT publish uzor.**

S3 will be the first stage to actually use `set_branch_preserve_if_empty`. S2 does NOT
need it.

---

## 2. S1 deliverable — what landed in `85c9d1e`

New file `crates/chart/src/state/branch.rs` (404 LOC including tests). Re-exported in
`crates/chart/src/state/mod.rs`.

Types added:
- `PanelLeaf` — 13-variant enum (ChartPanel, IndicatorPanel, Dom, L2Tape, TradeTape,
  BigTrades, Footprint, VolumeProfile, LiquidityHeatmap, OrderEntry, PositionManager,
  TradeLog, RiskCalculator). Implements `uzor::panels::DockPanel`.
- `PanelLeafKind` — discriminant enum, used in `AcceptPolicy::Only(HashSet<...>)`.
- `InstanceId(u64)` — newtype over the indicator manager's `instance_id`.
- `BranchKind` — `ChartContainer / TradingContainer / TagGroup / Plain`. Method
  `needs_preservation()` returns `true` for everything except `Plain`.
- `AcceptPolicy` — `Only(HashSet<PanelLeafKind>) | Any`. Method `accepts(kind)`.
  Convenience `chart_container()` constructor.
- `BranchMeta` — `kind + accepts + group_id + symbol + timeframe + exchange +
  account_type + label`. Convenience builders `chart_container() / trading_container()
  / tag_group()`.

Tests: 6/6 in `crates/chart/src/state/branch.rs`.

**Zero consumers** wire these types. S2 begins consumption.

`ChartPanelId` reuses the existing definition in `chart_container_leaf.rs` (not a new
copy). `InstanceId` is the new wrapper.

---

## 3. S2 — exact scope

### Goal

Migrate **canvas-leaf state** out of `ChartContainer` into a new
`ChartPanelState` struct stored in a new map `TradingPanelsStore.chart_panel:
HashMap<ChartPanelId, ChartPanelState>`. Migrate **branch-level state** (symbol,
timeframe, group_id, indicator_source, signal_manager, trade_manager, data_provider,
connection_status, prev_close_price, pre_tag_indicator_ids, stashed_primitives,
stashed_command_history) to a new sidecar map `branch_meta: HashMap<BranchId,
BranchMeta>` on `ChartApp`.

**`ChartContainer` struct does NOT disappear in S2.** It shrinks. It survives as the
host for the inner_layout + sub_panes Vec until S4 retires them. The shrinkage moves
fields out one logical group at a time.

Why split this way: each move is a contained mechanical pass with a clear cargo-check
gate. Doing the whole god-object split in one commit lands ~6000 LOC of mechanical
churn with no intermediate compilable state.

### Verified scope numbers

| Symbol / file | Count | Confidence |
|---|---|---|
| `ChartContainer` references repo-wide | **96 in 25 files** | grep verified |
| `panel_grid.containers()`/`containers_mut()`/`container_for_leaf*`/`active_container*`/`iter_containers` callsites | **559 across 13 files** | strict grep verified (2026-06-04). Earlier "147" undercount was from a non-exhaustive pattern; "~30" from the predecessor's plan doc is 18× off. |
| `crates/chart/src/state/chart_container.rs` LOC | 1916 | wc -l verified |
| `crates/chart/src/state/panel_grid.rs` LOC | 2260 | wc -l verified |
| `crates/chart-app/src/lib.rs` LOC | 6613 | wc -l verified |
| `crates/chart-app/src/input/sync_group.rs` LOC | 1504 + **66 `panel_grid.X` callsites** | wc + grep verified |

The original Phase 4 design doc said "~30 callsites" — predecessor's estimate. The
strict count is **559**. The work is ~18× larger than the original estimate.
Plan accordingly. Many callsites are trivial (`active_container().is_some()` etc.) and
collapse under bridge accessors; budget mental load not pure LOC.

### What "ChartContainer" struct holds today (1916 LOC at HEAD `7e6b87a`)

**71 public fields total.** Groups below cluster them; this is not exhaustive at the
sub-field level (e.g. group G "Interaction state" counts as 8 logical fields but
expands to 14 actual pub fields once you count out `drag_start_x`,
`drag_start_y`, `drag_start_view`, `drag_start_spacing`, `drag_start_price_min`,
`drag_start_price_max`). When estimating per-commit LOC, multiply group field counts
by ~1.5 for actual struct lines, and add 5-15 lines per touched callsite for the
bridge accessor pass.

Field groups (from `chart_container.rs:109` onward):

**A. Identity** (1 field): `id: ChartId`
**B. Display** (1 field): `title: String`
**C. Data** (1 field): `bars: Vec<Bar>`
**D. Viewport + scales** (3 fields): `viewport`, `price_scale`, `time_scale`
**E. Sub-pane container** (5 fields): `sub_panes: Vec<SubPane>`,
`sub_pane_overlay_states`, `sub_pane_overlay_results`, `pane_manager`,
`total_chart_height`, `inner_layout`
**F. Instrument** (4 fields): `symbol`, `timeframe`, `exchange`, `account_type`
**G. Interaction state** (8 fields): `is_active`, `active_pane_index`, `crosshair`,
`kinetic`, `drag_mode`, `last_mouse_pos`, `drag_start_*` (6 sub-fields)
**H. Display options** (5 fields): `grid_options`, `crosshair_options`, `legend`,
`watermark`, `tooltip`
**I. Managers** (5 fields): `drawing_manager`, `signal_manager`, `trade_manager`,
`indicator_source`, `marker_manager`
**J. Series visibility** (12 bool flags + `chart_type: &'static str`)
**K. Scale settings** (1 field): `scale_settings`
**L. Toolbar** (2 fields): `chart_toolbar`, `toolbar_config`
**M. Sync/instrument metadata** (3 fields): `prev_close_price`, `connection_status`,
`group_id`
**N. Sync-group bookkeeping** (3 fields): `pre_tag_indicator_ids`, `stashed_primitives`,
`stashed_command_history`
**O. Cache** (1 field): `symbol_drawings`
**P. Async lifecycle** (5 fields): `pending_symbol_load`, `needs_auto_scale_after_bars`,
`scroll_fetch_in_flight`, `scroll_fetch_started`, `restore_scale_mode`,
`stashed_scale_mode`
**Q. Price + overlay misc** (2 fields): `price_lines`, `compare_overlay`,
`command_history`

### Phase 4 destination

| Group | Phase 4 destination | Confidence |
|---|---|---|
| A — `id: ChartId` | DROP in S2 — replaced by `ChartPanelId` for the canvas leaf and `BranchId` for the container branch. Until S3 starts wiring branches, keep `ChartId` field for backwards-compat callers. | Spec'd in design doc §5 |
| B — `title` | `BranchMeta.label` | Design doc §5 |
| C — `bars` | `ChartPanelState` | Design doc §5 |
| D — viewport/scales | `ChartPanelState` | Design doc §5 |
| E — sub-pane container | **STAYS on ChartContainer until S4**. Owner explicitly committed to retiring `sub_panes` Vec + `inner_layout` only when sub-panes become uzor leaves in S4. Touching this in S2 would re-create the Phase 3 mistake. | Owner instruction, conversation 2026-06-03 |
| F — symbol/timeframe/exchange/account_type | `BranchMeta` | Design doc §5 |
| G — interaction state | `ChartPanelState` (these are per-canvas drag/crosshair) | Design doc §5 |
| H — display options | `ChartPanelState` | Design doc §5 |
| I — managers | drawing_manager → `ChartPanelState`; signal_manager + trade_manager + indicator_source + marker_manager → `BranchMeta` (instrument-level) | Design doc §5 |
| J — series visibility | `ChartPanelState` | Design doc §5 |
| K — scale_settings | `ChartPanelState` | Design doc §5 |
| L — toolbar | `ChartPanelState` (toolbar is chart-leaf chrome) | Design doc §5 |
| M — prev_close_price/connection_status/group_id | `BranchMeta` | Design doc §5 |
| N — sync-group bookkeeping | `BranchMeta` | Design doc §5 |
| O — symbol_drawings | `ChartPanelState` | Design doc §5 |
| P — async lifecycle | `ChartPanelState` | Design doc §5 |
| Q — price_lines/compare_overlay/command_history | `ChartPanelState` | Design doc §5 |

### Recommended S2 commit-by-commit split

The migration is too large for one commit. Split it. Each commit MUST cargo-check
green. Suggested order:

**S2.1 — Add `ChartPanelState` skeleton.** Empty struct in
`crates/chart-app/src/panels_store/chart_panel.rs` (or alongside `panels_store.rs`).
Add field `chart_panel: HashMap<ChartPanelId, ChartPanelState>` to `TradingPanelsStore`.
Add `branch_meta: HashMap<uzor::panels::BranchId, BranchMeta>` to `ChartApp` (path is
`uzor::panels::BranchId`, NOT `uzor::layout::BranchId` — verified via grep at
2026-06-04). Compile.
Zero consumers. ~50 LOC, no risk.

**S2.2 — Move group F (instrument) to BranchMeta.** symbol/timeframe/exchange/
account_type. Add helper `ChartApp::branch_symbol_for(branch_id) -> Option<&str>` etc.
The trick: ChartContainer needs a bridge accessor that reads from `branch_meta` if the
container's branch is mapped, else falls back to its own field. **DO NOT remove the
struct fields yet** — the bridge keeps both in sync for one commit. ~200 LOC + ~30
callsite updates.

**S2.3 — Move group M (prev_close_price/connection_status/group_id) to BranchMeta.**
Same pattern. ~100 LOC.

**S2.4 — Move group N (sync-group stash fields) to BranchMeta.** sync_group.rs is the
heavy consumer (48 callsites of `panel_grid.X`). Read every callsite before touching.
~150 LOC.

**S2.5 — Move group I-instrument (signal_manager, trade_manager, indicator_source,
marker_manager) to BranchMeta.** Largest mechanical pass — these are referenced from
many input handlers. ~300 LOC.

**S2.6 — Define `ChartPanelState` body. Migrate groups C+D+G+H+J+K+L+O+P+Q into it.**
Storage: `panels_store.chart_panel`. ChartContainer keeps `chart_panel_id:
ChartPanelId` to look it up. Bridge accessors on ChartContainer return references into
the panel state. ~400 LOC + ~80 callsite updates.

**S2.7 — Drop the bridge.** Remove the now-dead fields from ChartContainer struct.
Replace bridge accessors with direct lookups. Compile clean. ~100 LOC delete.

**S2.8 — Move drawing_manager from ChartContainer to ChartPanelState.** Separate
commit because drawing_manager is touched in ~30 places and has lifetime entanglement
with the symbol cache. ~150 LOC.

Total: ~1500 LOC of net change across 8 commits. ~110 callsite updates. Each commit
should be self-contained, cargo-check green, with a clear rollback target.

### What NOT to do in S2

1. **DO NOT touch sub-pane state** (group E). That is S4. Keep `sub_panes: Vec<SubPane>`,
   `inner_layout`, `sub_pane_overlay_*` as-is. They live on ChartContainer.
2. **DO NOT replace `ChartId`** with a new container identity. ChartId stays. S3 adds
   `BranchId` alongside; the mapping `chart_id → branch_id` is what S3 builds.
3. **DO NOT delete `ChartContainerLeaf` enum** (the orphan in
   `chart_container_leaf.rs`). It's repurposed in S4. Leave it.
4. **DO NOT alter the uzor docking tree topology.** ChartPanelGrid keeps its
   `DockState<ChartSubPanel>`. S3 converts the payload to `PanelLeaf`. S2 only
   migrates **storage** of ChartContainer fields.
5. **DO NOT rewire input or render paths.** They keep calling
   `panel_grid.active_container()` — the method just returns a thinner ChartContainer.
6. **DO NOT touch IndicatorPanelState** in `panels_store.indicator_panel`. That's
   standalone-panel storage; orthogonal to S2.
7. **DO NOT bump nemo, uzor, or mlc versions.** No publish. No deploy script. Local
   commits only. Owner pushes manually.
8. **DO NOT run `cargo build --release`** between commits. Use `cargo check
   -p chart-app-vello` — incremental run is ~3 s, cold cache ~30-60 s. Release build
   is only at session end for smoke (~3-10 min).

### Critical gotchas verified in code

**G1.** `ChartContainer` has a manual `Clone` via `clone_for_split` (chart_container.rs
near line 567). After S2 moves fields out, `clone_for_split` must read from the new
storage location and write to a freshly-allocated ChartPanelId + branch_meta entry.
Don't forget this. Test: split a chart in the UI, verify both halves still work.

**G2.** `crates/chart/src/preset/snapshots.rs:209-291` builds `ChartWindowSnapshot`
by reading ~30 fields off ChartContainer. After S2, the snapshot capture must read
from the new locations. Each S2.X commit that moves a field must update this snapshot
capture in the same commit, OR the snapshot uses bridge accessors. Easiest: bridge
accessors return the same shape; snapshot code is untouched until S2.7 (drop the
bridge).

**G3.** `sync_group.rs` (66 `panel_grid.X` callsites) heavily uses `window.group_id`,
`window.pre_tag_indicator_ids`, `window.stashed_primitives`,
`window.stashed_command_history`, `window.symbol`, `window.indicator_source`. ALL of
these move to BranchMeta in S2. sync_group.rs needs a careful read pass first to
understand the shape, THEN the migration. Budget 30-60 minutes just for reading
sync_group.rs before touching.

**G4.** `chart_container.rs:1001-1029` — `update_sub_pane_ranges` reads
`self.indicator_source` and writes `self.sub_panes[i].price_min/max`. After S2.5
moves `indicator_source` to BranchMeta, this method takes
`&dyn IndicatorSource` as a parameter instead of accessing `self.indicator_source`.
Same for `set_crosshair_from_bar` and others in this region.

**G5.** Phase 1 (predecessor session) renamed `ChartWindow` → `ChartContainer` across
34 files (commit `cacbdaf`). That rename is still in place. ALL accessors are
`containers()`, `container_for_leaf*`, `active_container*`. Do NOT introduce new names
like `chart_window_for_*` — stay on the existing convention.

**G6.** `prepare.rs:168` and `:851` iterate `containers_mut().iter_mut()` for every
chart. Moving fields to BranchMeta means these iterations split: container fields
become a Vec walk; BranchMeta lookups happen by `chart_id → branch_id` mapping which
does NOT exist until S3. **For S2, branch_meta entries are keyed by ChartId.0** as a
temporary measure (cast `ChartId.0` to `BranchId.0`). S3 renames the keys when uzor
BranchIds enter the picture.

### Gate sequence

After each S2.X commit:
1. `cargo check -p chart-app-vello` — green.
2. `cargo test -p zengeld-chart --lib` — all tests still pass.
3. If feasible: `cargo build --release -p chart-app-vello` once per 2-3 commits.
4. Kill running mlc.exe (`chcp.com 65001 > /dev/null 2>&1; cmd //c "taskkill /F /IM
   mylittlechart.exe /T"`) and relaunch for spot-check that no panic on startup.

Final S2 gate (before S3 starts):
- Owner clicks through: open a chart, switch symbol, switch timeframe, split, close
  split, save preset, reload preset. Visual + behavioral parity with `7e6b87a`. Sub-
  pane drag is allowed to stay as-is (broken from S0); S2 does not fix or break it.

### Estimated S2 effort

Updated after the strict 559-callsite count was verified:

- Reading sync_group.rs + lib.rs + input/* + planning: 2-3 hours.
- S2.1 + S2.2: 2-3 hours (skeleton + first field-group migration with bridge).
- S2.3 + S2.4: 2-3 hours.
- S2.5 (largest — managers): 3-4 hours.
- S2.6 (canvas state migration, ~80 callsites): 4-5 hours.
- S2.7 + S2.8 (bridge drop + drawing_manager): 2-3 hours.
- Total S2: **15-21 hours of focused work**. Realistically two to three sessions.

The previous Phase 3 attempt failed in one over-eager session. Resist the urge to
land S2 in one go. Each S2.X gate is the boundary for "stop work, smoke, decide if
next sub-stage starts now or later".

---

## 4. Standing rules (owner-set, do not violate)

From conversation 2026-06-03 + 2026-06-04:

- **Local commits OK without asking. Push / publish / deploy ONLY on explicit owner
  command.** Never bump nemo submodule pointer. Never publish uzor. Never run
  `scripts/local-release.sh`.
- **Aggressive legacy wipe.** When code is dead, delete it. Don't write
  backwards-compatibility shims unless owner explicitly approves. The exception in S2
  is bridge accessors during 1-commit transitions — those are scaffolding for safe
  refactor, not permanent shims.
- **Russian-only conversational replies.** Code, commits, docs in English. Profanity
  from owner = "switch to think-first, terse, executed-result mode".
- **No agents in parallel on the same working tree.** Sequential agent calls only,
  one tool result at a time, commits between. Worktree isolation does NOT work for
  submodule-heavy refactors here.
- **Build hygiene.** Sub-agents run `cargo check`, not `cargo build`. Coordinator runs
  builds. Release builds only at session-end smoke.
- **mlc on Windows: kill exe before relaunch.** Use `cmd //c "taskkill /F /IM
  mylittlechart.exe /T"`. Never PowerShell `Stop-Process`.
- **Windows codepage:** prefix any Windows-native command (`taskkill`, `tasklist`,
  `wmic`) with `chcp.com 65001 > /dev/null 2>&1;` to get UTF-8 output.
- **No `--no-verify`.** No `git push --force`. No destructive git without explicit
  permission.
- **No .bat / .cmd / .ps1 wrappers.** Use Bash directly via the Bash tool.

---

## 5. Read-this-first checklist for the next agent

Before writing any S2 code:

- [ ] Read `docs/plans/phase4-branch-as-group-2026-06-04.md` end-to-end (438 lines).
- [ ] Read this handoff doc end-to-end.
- [ ] Read `docs/plans/chart-container-handoff-stalled-sep-drag.md` — failure modes of
  the previous attempt. Specifically the "What 3 parallel auditors found" section.
- [ ] Open `crates/chart/src/state/chart_container.rs` and skim all 1916 LOC. Note
  which method bodies touch which field groups.
- [ ] Open `crates/chart-app/src/input/sync_group.rs` and skim all 1504 LOC. Note
  every `.group_id`, `.pre_tag_indicator_ids`, `.stashed_primitives`,
  `.stashed_command_history`, `.symbol`, `.indicator_source` access. Make a list of
  call sites that need updating in S2.4 + S2.5.
- [ ] `cd nemo/mylittlechart && cargo check -p chart-app-vello` — confirm green at
  HEAD `85c9d1e` before any change.
- [ ] `cd nemo/mylittlechart && cargo test -p zengeld-chart --lib state::branch` —
  confirm S1 tests pass (expect 6/6).
- [ ] Open `crates/chart/src/state/branch.rs` and confirm what shipped in S1 — don't
  duplicate types.

---

## 6. What success looks like at the end of S2

- ChartContainer struct shrinks from ~30+ fields to ~5 fields (id, chart_panel_id,
  branch_id, sub_panes Vec, inner_layout, sub_pane_overlay_*, sub_pane_state).
- `ChartPanelState` exists in `panels_store.chart_panel` and owns all
  canvas-leaf state (~25 of the original ChartContainer fields).
- `ChartApp.branch_meta: HashMap<BranchId, BranchMeta>` exists and owns instrument +
  sync state. Keys are `BranchId(chart_id.0)` as a temporary stub until S3.
- `cargo check -p chart-app-vello` is green.
- `cargo test -p zengeld-chart --lib` is green.
- Owner spot-check: open chart, switch symbol, switch timeframe, split, close split,
  save preset, reload preset — all work like at `7e6b87a`.

After S2, S3 can begin: convert outer docking `DockState<ChartSubPanel>` to
`DockState<PanelLeaf>` and start using real uzor BranchIds.

---

End of handoff. Good luck.

— S0+S1 agent
