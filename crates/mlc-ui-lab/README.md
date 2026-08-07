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
