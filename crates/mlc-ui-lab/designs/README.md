# Design archive

One folder per `YYYY-MM-DD-<modal-type>`. It holds the **concept** artifacts for
that design: the lab's Rust tab bodies as they looked when the owner approved
them, and the HTML renders they were prototyped from.

## Pipeline (owner rule, 2026-08-10)

Every MLC design decision walks the same three stages, and nothing skips a stage:

1. **Concept** — HTML in this archive, one file per modal, all of its tabs inside.
   Shown to the owner, approved or sent back.
2. **Interactive prototype** — Rust in `mlc-ui-lab/src`, rebuilt to wasm, so the
   owner can click it. Approved or sent back.
3. **Implementation** — rewritten (not copied) into the real crates.

A new take on an approved design is a **new version file next to the old one**
(`*_v2.html`), never an edit of the previous one and never a deletion.

**Nothing here is compiled.** These files sit outside `src/` on purpose — the
lab must never carry a second live implementation of a modal it can host for
real. They are the reference you re-port from and diff against.

**Never delete a design file from the lab.** When a concept graduates into the
app, copy it here first, then remove it from `src/`. A design that only exists
in git history is a design nobody will find. This folder exists because one was
deleted during the 2026-08-08 port and had to be recovered from `HEAD`.

## Folders

- `2026-08-08-primitive-settings/` — the approved 3-tab primitive settings modal
  (**Вид · Уровни · Размещение**), replacing the old five tabs
  (Style/Text/Coordinates/Levels/Visibility).
  - `approved_design.rs` — composer: builds its own three tabs, delegates to the
    three bodies, wraps them in the shared tab-body shell.
  - `approved_vid.rs` — Вид (old Style + Text merged), with the settings search.
  - `approved_levels.rs` — Уровни: набор selector, palette ramps, global
    fill-between slider, the ladder, the in-place row inspector, add-level row.
  - `approved_placement.rs` — Размещение (old Coordinates + Visibility merged).
  - `design_state.rs`, `level_presets.rs`, `palettes.rs`, `shell_common.rs`,
    `dispatch.rs` — the state, presets, palettes, shell and id→intent mapping
    these three bodies were built against.
  - `fib_modal.html` — the HTML prototype all three tabs were designed from.
  - `levels_panel.html`, `tabs_redesign.html`, `archetypes.html` — the earlier
    studies: level-row variants, the five-tabs-to-three argument, and the
    archetype survey (Elliott / Position / Gann / Table) showing the three-tab
    shape holds without a fourth tab.
