# mlc-ui-lab

Wasm UI laboratory. Hosts the **real** `mlc-app-layout` modal builders (primitive
settings, to start) and real `uzor` widgets, driven by a real
`uzor::input::InputCoordinator`, in a standalone browser page — so UI, skins
and themes can be designed and iterated without building or running the whole
app (`mlc-app` / `mlc-backend` / `digdigdig3` are not in this crate's
dependency graph at all).

Interactive from Stage 0: click a tab and it switches, click a checkbox and it
toggles the real `Primitive`, click the stroke-width value and type — the
buffer is a real `uzor::input::TextFieldStore` field, not a picture. A debug
readout line under the modal shows the live values so a toggle/edit is
verifiable without a chart underneath to draw the primitive on.

## Prerequisites

- Rust + wasm32 target: `rustup target add wasm32-unknown-unknown`
- wasm-pack: `cargo install wasm-pack`
- A static file server: `cargo install basic-http-server` or use `npx serve`

## Check (no build)

From `nemo/mylittlechart/`:

```sh
cargo check --target wasm32-unknown-unknown -p mylittlechart-ui-lab
```

Native (compiles as an empty lib — the whole crate body is
`#[cfg(target_arch = "wasm32")]`-gated):

```sh
RUSTFLAGS="-D warnings" cargo check --workspace --all-targets
```

## Build

From `nemo/mylittlechart/`:

```sh
wasm-pack build crates/mlc-ui-lab --dev --target web --out-dir pkg
```

`--dev` skips wasm-opt (fast iteration; the lab is a dev tool, not a
release artifact). This produces `crates/mlc-ui-lab/pkg/{mlc_ui_lab.js,
mlc_ui_lab_bg.wasm, ...}`, which `index.html` already points at
(`./pkg/mlc_ui_lab.js`) — no copy step needed, unlike `mlc-app-shell-web`
whose `index.html` gets copied alongside an out-of-tree `dist/` output.

## Serve

Port **17498** (17499 is the app stand — see `nemo/CLAUDE.md`).

```sh
cd crates/mlc-ui-lab
basic-http-server --addr 127.0.0.1:17498 .
# or:
npx serve -l 17498 .
```

Open `http://localhost:17498`.

## Architecture

```
WebWindowProvider::from_id("chart")   ← DOM event source (uzor-window-web,
                                          published crate) → Vec<PlatformEvent>
LabRenderContext                      ← uzor-render-canvas2d + mlc_core::render::RenderContext
                                          adapter (src/render_context.rs)
InputCoordinator                      ← real uzor input state machine
PrimitiveRegistry::global()           ← real factory, "fib_retracement"
render_primitive_settings_modal(..)   ← the REAL mlc-app-layout modal builder

requestAnimationFrame loop (src/lib.rs):
  provider.poll_events() → EventProcessor::process (pointer/scroll/modifiers)
                          → TextInput/KeyDown routed to InputCoordinator's
                            TextFieldStore via src/dispatch.rs
  coordinator.begin_frame(input_state)
  theme::draw_lab_bar(..)             ← lab-only chrome: theme presets + reopen
  render_primitive_settings_modal(..) ← draws + registers widget hit regions
  draw_debug_readout(..)              ← live-state text line
  coordinator.end_frame() → Vec<(WidgetId, WidgetResponse)>
  dispatch::apply_primitive_settings_click(..) per clicked id
```

`src/dispatch.rs` is the one piece of glue this lab had to write itself: it
is the smallest amount of "what does a click on this known widget id mean"
logic needed to prove the modal live, written directly against
`PrimitiveSettingsState` fields and the real `Primitive` trait setters
(`apply_style_property`, `set_level_configs`, `data_mut()`). It does not
reimplement any widget — see `nemo/CLAUDE.md`'s handoff notes / the
launching agent's brief for the full feasibility writeup.

## What is live vs still static (Stage 0)

Live: Style/Text/Coordinates/Levels/Visibility tab switching, every Boolean
`style_prop:*` / `text_prop:*` checkbox (via `apply_style_property` /
`apply_text_property`), Fibonacci level visibility checkboxes
(`level_N_visible`, via `set_level_configs`), the stroke-width text field
(click to focus, type, Enter/blur commits into `Primitive::data_mut().width`),
modal close, theme preset switching (`ThemeManager::presets()`, all shipped
presets), reopening the modal after close.

Still static (registered/drawn by the real modal, hit regions exist, but
this lab's dispatch does not act on them yet): color swatches / the color
picker popup, the line-style dropdown, Select-type dropdowns, Number-type
sliders other than stroke width, the Coordinates tab's point fields, the
template dropdown, header drag-to-move, tab-content scroll.

## The application shell (waves 2-4)

Past Stage 0, the lab also hosts a stub of the real app's own chrome
(`src/shell/`) below whichever surface is currently on top of it: top/left/
right/bottom toolbars, an empty chart pane, and the Object Tree side panel
(`src/shell/object_tree.rs`) — one row per spawned primitive (type icon,
name, five row actions). The left rail spawns one of every registered
primitive type; the Object Tree starts pre-populated with one of each
(mirrors `sim.js`'s own `spawnAll()`).

Two independent modal families sit on top of the shell, both real
`mlc-app-layout` renderers, each in its own `InputCoordinator` scoped
region (`objtree_modal` / `app_modal`) sized to the chart pane:

- **Tree family** — click a row's gear (or the bottom toolbar's own gear)
  to open that object's real `render_primitive_settings_modal` (the SAME
  builder the four-sector grid above uses, drawn full-pane instead of
  sectored). `src/lib.rs`'s `open_tree_modal`.
- **Application family** (`src/app_modals.rs`) — the other fifteen
  `mlc-app-layout` modal renderers (chart/indicator settings, watchlist,
  alerts, search, tags & tabs, trading container, ...), reached through the
  top toolbar's "Modals" button (`shell::launcher_menu`, a two-section
  dropdown: object types above, application modals below — click either to
  open it). `src/lib.rs`'s `open_app_modal`.

Only one of the two families is open at a time (opening either closes the
other); the launcher menu and Esc close whichever is topmost. Neither
family ever reaches the Object Tree sidebar's own pixels in the first
place — both are drawn under `ctx.translate(pane_rect.x, pane_rect.y)` and
handed `pane_rect.width`/`pane_rect.height` in place of the full screen
(`shell::chart_pane_rect` — the same rect `shell::draw_shell` returns), so
they size and centre themselves ENTIRELY inside the chart pane, mirroring
the shape the real app's own `ChartModalLayout::chart_x`/`ind_screen_w`
split uses for Chart Settings / Indicator Settings, narrowed here to the
pane instead of the window. Object Tree rows are therefore pixel-identical
whether a modal is open or not — not because anything is cut, but because
nothing a modal draws ever lands under the sidebar to begin with.

## Community page (`?page=community`)

A second, separate page in this SAME crate (`crate::community`, own render
loop, no `LabState`) that hosts the REAL `render_community_modal`
(`mlc-app-layout`'s `layout::modals::community` — Forum + Messages) against
hand-built `CommunityState` fixtures, so the Community modal can be judged
against the approved concept mockups (`designs/2026-09-23-forum/
forum_concept_v1.html`, `designs/2026-09-24-messenger/
messenger_concept_v1.html`) without signing in anywhere. Renders full-window
at a fixed 1280×800 (`#chart` gets that CSS size regardless of the hosting
browser window — see `index.html`'s own `body.community` rule), Dark theme
by default.

```
http://localhost:17498/?page=community&state=forum-index
http://localhost:17498/?page=community&state=forum-thread
http://localhost:17498/?page=community&state=messages-locked
http://localhost:17498/?page=community&state=messages-chat
```

`state` defaults to `forum-index` for an unrecognised or missing value.
`&theme=<preset>` switches the `ThemeManager` preset (`ThemeManager::
presets()` — `dark`/`light`/`high_contrast`/`high_contrast_mono`/`mascot`).
This crate's own `cloud` feature requirement lives on the `mlc-app-layout`
dependency in `Cargo.toml` (unconditional, not a lab-local feature — see
that line's own comment) since `layout::modals::community` only exists
behind `mlc-app-layout`'s `cloud` feature.

One finding for whoever compares a screenshot against the concept mockups:
`DmConversationUi::last_message_preview`/`last_message_mine` are real public
fields the fixture populates, but the chat-list row's own preview line never
reads them today — `dm_messenger_provider.rs`'s `reads::rooms` hardcodes
`RoomSummary::last_event: None` regardless (that type's own doc already
names the gap). `DmConversationUi::peer_rank`/`peer_fingerprint` are NOT
affected — the open conversation's own header reads those two straight off
`state.dm.conversations`, not through the provider.

## Agent control plane (`window.__LAB`)

Mirrors `sim/sim.js`'s own `window.__SIM` (see that file's own doc):
functions, callable from a CDP eval, that drive and inspect the lab without
a mouse. No HTTP endpoint, no socket — everything round-trips through the
wasm module already loaded in the page.

`src/lib.rs` exports `#[wasm_bindgen]` free functions
(`lab_modals`/`lab_open_type`/`lab_tab`/`lab_open_app`/`lab_close`/
`lab_snapshot`/`lab_metrics`/`lab_dump_types`), each returning a
JSON-formatted `String` (this crate has no serde dependency — the field
set is small and hand-escaped). `index.html`'s own bootstrap
`<script type="module">` imports them by name and assembles `window.__LAB`
around them, parsing each JSON string back into a plain object:

```js
window.__LAB.modals()
// → { objects: ["fib_retracement", "gann_fan", ...],       // 14, spec order
//     application: ["chart_settings", "indicator_settings", ...],  // 16, legacy
//     concept_application: ["chart_settings", "indicator_settings", ...] }  // 9

window.__LAB.open_type("gann_fan", "levels")
// opens the CONCEPT modal (approved-design surface,
// `concept::render::render_concept_modal`) for gann_fan, switches to its
// "levels" tab. Tried first against the 14 object archetypes, then the 9
// application archetypes — the SAME call opens either.
// → { ok: true, id: "gann_fan", type_id: "gann_fan", tab: "levels" }
// The production, live-`Primitive`-backed modal this call used to open is
// still reachable in the UI (the shell's own launcher "Production modals"
// section), just not through this control-plane entry point any more.

window.__LAB.tab("labels")
// switches the ALREADY-OPEN concept modal's active tab — the live
// counterpart to open_type's own tab argument.
// → { ok: true, id: "gann_fan", tab: "labels" }

window.__LAB.open_app("chart_settings")
// unchanged — opens the legacy, 16-entry AppModalKind application modal.
// → { ok: true, id: "chart_settings" }

window.__LAB.close()
// closes whichever overlay is topmost
// (launcher > app modal > concept modal > tree modal)
// → { ok: true, closed: "concept" }

window.__LAB.snapshot()
// → { objects: [{id, type_id, display_name, selected}, ...],
//     selected: <id>|null, sidebar_open: bool, launcher_open: bool,
//     modal: null | { family: "concept"|"tree"|"app", id, tab, tabs: [...] } }

window.__LAB.metrics()
// → { rails: { top_toolbar_height, left_toolbar_width,
//              right_toolbar_width, bottom_toolbar_height,
//              right_sidebar_width, lab_bar_height },
//     modal: null | { family, rect: {x,y,width,height},
//                      header_h, tab_bar_h, tab_rail_w, footer_h } }
```

`metrics()`'s `rails` block is the same `mlc_core::types` layout constants
`sim.js`'s own `--mlc-layout-*` CSS custom properties trace to — the
numeric cross-check between the two labs. `modal.rect` is this frame's REAL
measured frame rect for whichever modal is open (the concept family's own
`ConceptModalGeo::modal_rect`, the tree family's own
`PrimitiveSettingsResult::modal_rect`, or an app modal's own
`*Result::modal_rect`-shaped field — `app_modals::AppModalsState::
last_rect`'s own doc has the per-kind field-name list). `header_h`/
`tab_bar_h`/`tab_rail_w`/`footer_h` are the shared
`mlc_app_layout::ui::widgets` shell constants every "app"-family modal
inherits from `ModalShellSpec`; they read `null` for the "concept" and
"tree" families, whose modals are a bespoke shell that does not use them
(their own measured header height is not tracked as of this wave — only
`rect`/position is).
