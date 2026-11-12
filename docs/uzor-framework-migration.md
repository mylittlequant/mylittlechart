# mylittlechart on uzor-framework: migration plan

Status: plan and phase 1 (branch `mobile/fw1`). Owner decision: the whole desktop
app moves onto `uzor_framework::App`, not only the phone build. uzor gets patched
and grows new mechanics where mlc needs them.


> The maintained copy of this plan, the audit, the uzor gaps and the progress notes is the
> private repo `HATCHERY-lab/mlc-docs`. The port itself lives in `HATCHERY-lab/mylittlechart-uzor`.

## Pixel-perfect rule (owner, 2026-11-12)

The port matches the current desktop exactly. Every screen is compared side by side with
the old app rendered at the same size:

- `mlc-ref-shots <out> [WxH ...] [scene ...]` renders the real `ChartApp` and the window
  chrome on the tiny-skia context (scenes: main, chart-settings, user-settings,
  chart-browser, preset-rename). Run it with `HOME` at a scratch directory, and with
  `--config 'profile.dev.package.tiny-skia.debug-assertions=false' --config
  'profile.dev.package.uzor-render-tiny-skia.overflow-checks=false'` in debug builds.
- `tools/compare_shots.py <ref.png> <port.png> <out.png> --region name:x,y,w,h` writes
  reference | port | diff and prints the share of differing pixels per region.

uzor gets the mechanics first; a screen is ported only when the parts it needs exist.

## Why

Today the desktop app is its own UI toolkit on top of uzor's low-level pieces:

- it paints every widget by hand (`fill_rect` / `fill_text` with computed rects),
- it hit-tests by hand and routes every click through one `InputCoordinator`
  loop in `mlc-app/src/input`,
- every modal keeps its own geometry, scroll, drag and resize state,
- each shell (`mlc-app-shell-vello`, `mlc-app-shell-web`) runs its own event loop
  and render backend glue.

None of it works on a phone, none of it is testable as a DOM, and every new
screen copies the same hand-made patterns. uzor-framework already gives one App
per product with DOM screens, overlays, docking, text fields, gestures and hosts
for desktop, web, Android and iOS (the HQ client runs on it this way).

## Inventory (desktop UI, `mobile/fw1` base)

Lines of Rust per crate:

| crate | lines | what it is |
|---|---:|---|
| `mlc-app` | 90.5k | `ChartApp`: state, frame lifecycle, persistence, the input loop (`input/` alone 41.6k) |
| `mlc-app-layout` | 88.5k | modal renderers (`layout/modals`, 43.5k), hand-made widgets (`ui/`, 27.9k), chart renderers (`chart_render/`) |
| `mlc-app-panels` | 18.2k | order-flow panels (11) and trading panels (10) |
| `mlc-app-sidebar-content` | 10.7k | right sidebar: watchlist, object tree, alerts, indicators, connectors, monitoring, agents dock |
| `mlc-app-input-shell` | 4.6k | platform events, window chrome (title bar, tabs, account menu), tooltips |
| `mlc-app-shell-vello` | 12.6k | winit + vello runner, multi-window, keychain, telemetry |
| `mlc-app-shell-web` | 10.8k | browser runner, cloud session, persistence |
| `mlc-app-render-context-*` (5) | 1.6k | render backends (vello, vello-cpu, vello-hybrid, tiny-skia, instanced) |
| **total** | **≈237k** | |

Hand-made UI, counted:

- **46 modal files / 35 modal and popup entry points** (`render_*_modal`,
  `*_overlay`, `*_popup`), plus 5 color-picker popups and the context menu.
- **26 widget modules** in `mlc-app-layout/src/ui` (27.9k lines): button, text
  input, single and dual slider, color picker (swatch grid + HSV), dropdown
  (1.7k), scroll container, toggle, radio group, popup, modal frame with tabs and
  resize edges, context menu, toast, chips input, selectable text, avatar,
  count badge, sticker, QR, theme sample, loading hat, mascot animation, clock
  glitch, messenger picker, sync color grid, tag tabs, toolbar core (2.1k).
- **Input loop**: 338 manual `register` calls into the coordinator, 38 hand-written
  hit-test functions, 70 text-field ids (already on uzor's `TextFieldStore`).
- **Hand painting** in the UI crates: 1163 `fill_text`, 535 `fill_rect`.
- Widget uses across the modals: dropdowns 483 references, checkboxes 72,
  toggles 49, text inputs 52 draws, sliders 34, color pickers 12 draws,
  15 scroll-state fields.

Per modal (lines; uses of input / slider / dropdown / checkbox / toggle / color):

| modal | lines | inp | sld | drop | chk | tgl | clr |
|---|---:|---:|---:|---:|---:|---:|---:|
| user_settings (+admin 1455, friends 742) | 3872 | 17 | 5 | 7 | 7 | 45 | 0 |
| messenger_overlay (+dialogs 712, dm 544) | 3870 | 0 | 0 | 9 | 0 | 4 | 0 |
| chart_settings (+color picker 171) | 2954 | 3 | 13 | 197 | 148 | 6 | 127 |
| forum_v2_overlay | 2331 | 3 | 0 | 14 | 0 | 9 | 0 |
| profile_manager | 1906 | 17 | 0 | 0 | 0 | 15 | 0 |
| checkout | 1905 | 3 | 0 | 7 | 1 | 0 | 0 |
| tags_tabs_modal | 1689 | 0 | 0 | 0 | 0 | 17 | 17 |
| cloud_overlay | 1677 | 0 | 0 | 4 | 0 | 7 | 0 |
| indicator_settings (+color picker 171) | 1556 | 7 | 3 | 52 | 9 | 31 | 30 |
| alert_settings | 1351 | 3 | 0 | 31 | 1 | 19 | 0 |
| primitive_settings + 9 parts (vid, intent, levels, placement, property, labels, cells, palettes) | 7128 | 25 | 13 | 84 | 27 | 59 | 44 |
| watchlist_modal | 1212 | 5 | 0 | 0 | 0 | 0 | 0 |
| search_overlay | 1028 | 3 | 0 | 2 | 0 | 27 | 0 |
| compare_settings (+color picker 173) | 1003 | 0 | 3 | 20 | 9 | 1 | 10 |
| trading_container_modal | 884 | 0 | 0 | 1 | 0 | 2 | 0 |
| markup_engine_settings | 843 | 0 | 0 | 1 | 15 | 5 | 0 |
| chart_browser | 524 | 3 | 0 | 1 | 0 | 0 | 0 |
| welcome_wizard | 517 | 0 | 0 | 1 | 1 | 0 | 0 |
| cloud_login | 476 | 0 | 0 | 1 | 0 | 0 | 0 |
| ewa_engine_settings | 346 | 0 | 0 | 1 | 20 | 5 | 2 |
| community | 314 | 0 | 0 | 0 | 0 | 0 | 0 |
| clock_popup | 313 | 0 | 0 | 3 | 7 | 2 | 0 |
| share_settings / share_open | 284 / 214 | 3 / 3 | 0 | 1 / 4 | 0 | 7 / 0 | 0 |
| context_menu | 257 | 0 | 0 | 1 | 4 | 0 | 0 |
| panel_settings_popup | 234 | 0 | 0 | 2 | 0 | 3 | 0 |
| panel_color_tag | 167 | 0 | 0 | 1 | 0 | 6 | 21 |
| template_name_modal / preset_name_input | 166 / 137 | 3 / 3 | 0 | 1 / 0 | 0 | 0 | 0 |
| indicator_overlay (+dropdown 145) | 134 | 0 | 0 | 6 | 0 | 0 | 0 |

### Hand-made widget → uzor

| mlc widget | uzor replacement | status |
|---|---|---|
| `widgets/button.rs`, `layout_btn_row` | `domkit::button`, flex rows in CSS | exists |
| `widgets/input.rs` + `text_input/` (70 field ids) | DOM `Control::Input` (`domkit::input`, `field`): the framework's text store gives caret, selection, IME, clipboard | exists |
| `toggle_widget.rs` | `domkit::switch` | exists |
| checkbox drawing in modals | `domkit::checkbox` | exists |
| `widgets/radio_group.rs` | `domkit::segmented` (few options) or `domkit::list` with `.sel` | exists |
| `dropdown.rs` (plain) | `domkit::select_trigger` + `select_popover` | exists |
| `dropdown.rs` (searchable, grouped, symbol search) | — | **gap G5** |
| `scroll_widget.rs`, `scroll_state.rs` | framework-owned DOM scroll | **gap G2** |
| `widgets/slider.rs` (single, dual, inline edit) | — | **gap G3** |
| `widgets/color_picker.rs`, `color_picker_state.rs` | — | **gap G4** |
| `widgets/modal.rs` (frame, tabs, resize edges, drag) | `OverlayModel` dialog + `domkit::dialog`, `domkit::tabs`; move/resize missing | **gap G6** |
| `widgets/popup.rs`, `context_menu.rs` | `domkit::menu`, `popover_at`; submenus missing | **gap G12** |
| `toast.rs` | `kit::toasts` / `domkit::toasts` | exists |
| tooltips (`input-shell/tooltip.rs`) | `domkit::tooltip` | exists |
| `chips_input.rs` | — | **gap G11** |
| `selectable_text.rs`, messenger / forum bodies | — | **gap G10** |
| `count_badge.rs`, `avatar_widget.rs` | `domkit::badge`, an `El` with CSS | exists |
| `toolbar_core.rs` (top toolbar, drawing toolbar with flyouts) | DOM row of icon buttons; flyouts and overflow missing | **gap G13** |
| tag tabs, chrome tabs (drag, reorder, scroll) | `domkit::tabs`; reorder missing | **gap G8** |
| trading logs, orders, history, watchlist columns (`col_resize.rs`) | `domkit::table` (static) | **gap G7** |
| numeric settings fields (precision, min/max, step) | — | **gap G9** |
| chart, order book, footprint, heatmap, tapes, sticker, mascot, clock glitch, QR, theme sample | painters in `El::paint_slot` (`DomBuilder::paint_slots`) | exists (1.5.36) |
| watchlist, symbol lists | `VirtualList` + `search_bar` / `search_filter` | exists (1.5.36) |
| panel grid, docking (`uzor::panels` LeafId ×88) | uzor-framework docking (`DockPanel`, `LayoutCmd`) | exists; grid presets **gap G14** |
| title bar, account menu | `ChromeModel` (framework chrome) | exists; tab tear-off **gap G8** |
| 5 render-context crates | `uzor-render-hub` through the framework hosts | exists |
| shell-vello / shell-web event loops | `uzor_framework::host::native::run_native`, `host::run_web_with`, `uzor-window-mobile` | exists |

## Target structure (HQ pattern)

```
mlc-core                  domain: chart engine, indicators, drawings, themes, i18n (as today)
mlc-client-core   (new)   app state without UI: charts, layouts, watchlists, alerts,
                          accounts, persistence, data-bridge wiring; I/O through traits
                          (Storage, Clock, Net, Vault) so native / web / mobile inject theirs
mlc-chart-view            the chart as a paint slot: model, input (gestures + mouse), painter
                          (phase 1: the light painter; phase 2: the full mlc-core engine)
mlc-desktop       (new)   the one uzor_framework::App: screens in DOM, overlays, docking;
                          the phone layout from `mobile::on(width, coarse)`
mlc-shell-native  (new)   winit host (`run_native`), dev-control, tray, keychain
mlc-shell-web2            browser host (`run_web_with`) — later runs mlc-desktop
mlc-shell-mobile          Android / iOS host (`uzor-window-mobile`) — later runs mlc-desktop
```

`mlc-app*`, `mlc-app-shell-*`, `mlc-app-input-shell`, `mlc-app-render-context-*`
stay and keep building until the new app reaches parity, then they go.

Rules carried over from the phone work: every control is a uzor widget or DOM
element (no hand-made inputs or lists), a uzor bug is fixed in uzor (branch, release,
pin `=1.5.x` from crates.io), monochrome Mizu-like look (opaque widgets, glass only
on overlays, a status dot instead of colored pills), Russian UI by default with
English fallback, DOM tests at the target sizes (1280×800 desktop, 390×844 phone)
in both languages, screenshots from the headless host.

## Phases

### Phase 1 — native shell on App (branch `mobile/fw1`)

- `mlc-chart-view`: the chart becomes a reusable `ChartSlot` (source, model,
  gestures + mouse, painter) used by both the phone app and the desktop app.
- `mlc-desktop`: `DesktopApp` — one window 1280×800, framework chrome, a DOM
  toolbar (symbol, venue, status dot, timeframe segmented, reset), the chart in
  `El::paint_slot("chart")`, a watchlist column (search field + list, click to
  switch the symbol). Headless tests: wheel zoom, drag pan, timeframe switch,
  watchlist filter and pick, nothing clipped at 1280×800 and 1024×640 in ru/en.
- `mlc-shell-native`: `mlc-fw` binary, `run_native` with dev-control
  (`--dev` / `MLC_DEV=1`).
- The old app is untouched and still builds.

### Phase 2 — client core and the real chart in the slot

- Extract `mlc-client-core` from `mlc-app` (`chart_panel_state`, `chart_state`,
  `persistence/`, `user_manager/`, data-bridge wiring, alerts, messenger bridges)
  with I/O traits.
- `ChartEngineSlot`: the full chart (mlc-core engine + `mlc-app-layout/chart_render`:
  all chart types, indicators, drawings, scales, crosshair) painted into the paint
  slot through the same `RenderContext`; slot gestures and mouse feed
  `DefaultChartInputHandler` instead of the coordinator. This puts the real chart on
  desktop, web and phone at once.
- Drawing tools input (the drawing session) through the slot.

### Phase 3 — frame of the app

- Chrome (tabs of chart layouts, account menu) on `ChromeModel`.
- Top toolbar and the drawing toolbar in DOM (G13).
- Chart grid and docking on framework docking (G14), sync groups (symbol / timeframe /
  crosshair links) in client core.
- Right sidebar panels as `DockPanel`s: watchlist (VirtualList), object tree, alerts,
  indicators, connectors, monitoring; agents dock on the framework terminal composite.

### Phase 4 — modals, simplest and most used first

1. Name inputs: preset, template, watchlist group (DOM dialog + input).
2. Context menu, panel settings popup, clock popup, share open / settings.
3. Indicator overlay + its dropdown, search overlay (G5 for symbol search).
4. Color pickers (G4) → then compare settings, alert settings.
5. Indicator settings, chart settings (G3, G4, G9 — the largest settings forms).
6. Primitive settings family (9 files).
7. Watchlist modal, tags tabs (G8), trading container, chart browser.
8. Welcome wizard, profile manager, user settings (+admin, friends), checkout.
9. Cloud: login, overlay, community, forum, DM, messenger (G10, G11).

Each modal: a DOM tree builder + a state struct in client core + DOM tests; the old
renderer is deleted when the new one ships.

### Phase 5 — panels

- Order-flow panels (DOM book, footprint, tapes, heatmap, volume profile, big
  trades, liquidations, multi-DOM / multi-tape) as paint slots with their painters
  moved over unchanged.
- Trading panels: order entry and risk calculator as DOM forms; positions, orders,
  history, trade log, metrics as data tables (G7); ticker tape as a paint slot.

### Phase 6 — one app everywhere, delete the old one

- `mlc-shell-web2` and `mlc-shell-mobile` run `mlc-desktop` (phone layout by
  width / coarse pointer, bottom sheets instead of modals on a phone).
- Agent API (`mlc-app-agent-api`) commands become App intents.
- Delete `mlc-app`, `mlc-app-layout` UI parts, `mlc-app-input-shell`,
  `mlc-app-shell-vello`, `mlc-app-shell-web`, `mlc-app-render-context-*`.

## uzor gaps and API drafts

Each gap is a branch from uzor master, reported to uzor+tesserax for a patch
release; mlc pins the release.

- **G1. Paint-slot mouse** — *branch `fix/paint-slot-mouse`*. A paint slot in a DOM
  registers only click + hover + touch, so a desktop chart cannot get the mouse drag
  or the wheel without registering itself a second time.
  `SlotFrame { pressed: Option<(f64,f64)>, wheel: (f64,f64), .. }`, slots register
  `CLICK_AND_DRAG | SCROLL` + touch + gesture (the standalone `kit::paint_slot` too).
- **G2. Framework-owned DOM scroll.** Any element with `overflow: auto|scroll` and an
  id scrolls by itself: the framework keeps the offset per id, routes wheel, trackpad
  1:1, finger fling and the scrollbar, clamps against `scroll_height`.
  `El::scroll_area(id)`, `DomResponse.scroll(id) -> f64`, `DomBuilder::reveal(id, child)`.
  Today the app must feed `scroll_top` itself (modal bodies, lists).
- **G3. DOM range slider.** `Control::Range { min, max, step, value, value2: Option<f64> }`,
  `domkit::slider(id, value, min..max)` / `domkit::range2(id, lo, hi, ..)`, drag on the
  thumb or the track, keyboard arrows, `DomResponse.edited_f64: Vec<(id, f64)>`; an
  inline numeric input next to it (G9).
- **G4. DOM color picker.** `domkit::swatches(id, colors, selected)` (L1 grid with
  recents and "+"), `domkit::color_picker(id, Hsva)` (L2: SV plane + hue strip + alpha
  strip + hex input) built on paint slots with drag; result `Picked(Hsva)`.
- **G5. Searchable select / combobox.** `domkit::combo(id, edit, groups: &[Group],
  open)`: filter input, grouped sections with headers and icons, keyboard
  highlight, virtual body for long lists (symbol search over thousands of tickers).
- **G6. Movable, resizable desktop dialogs.** `OverlayModel::window { movable,
  resizable, min: SizePx, remember: key }`: drag by the header, resize by edges,
  size remembered per key, tab strip in the header.
- **G7. Data table.** `DataTable { columns: Vec<Col { key, width, min, sort }> }` →
  `El` with a sticky header, column resize drag, sort click, virtual body; result
  `{ resized, sorted, clicked_row }`.
- **G8. Drag-to-reorder in DOM.** `El::draggable(group, key)` on rows and tabs,
  `DomResponse.reordered: Option<(group, from, to)>`; tab tear-off into a new window
  as the next step.
- **G9. Numeric field.** `Edit::numeric(min, max, step, precision)`: stepper
  arrows, wheel and drag-to-scrub, validation state.
- **G10. Multi-line and selectable text in DOM.** `Control::TextArea(Edit)` with
  wrapping and growing height; `El::selectable()` for read-only text with selection
  and copy (messages, forum posts).
- **G11. Chips input.** `domkit::chips_input(id, chips, edit)`: tokens with remove
  buttons, Backspace removes the last.
- **G12. Nested menus.** `domkit::menu` items with submenus, check marks and
  shortcut hints, opened on hover / arrow keys.
- **G13. Toolbars.** DOM toolbar with icon buttons, split buttons (button + chevron
  flyout), a vertical variant with flyout groups and favorites, overflow into a "…"
  menu when narrow.
- **G14. Grid layout presets for docking.** `LayoutCmd::ApplyGrid(GridPreset)`
  (1×2, 2×1, 2×2, 1+3 …) over framework docking, keeping panels by id.

## Risks

- **Size.** ≈237k lines of UI; parity takes many releases. The two apps live side
  by side meanwhile, and fixes may need doing twice — keep new features on the new
  app only once a screen is migrated.
- **uzor cadence.** Gaps G2–G14 block screens; each needs a uzor branch, review and
  a patch release before mlc can pin it. Order phase 4 so that work that needs no gap
  goes first.
- **Chart input coupling.** The chart engine's input handler expects the
  coordinator; phase 2 must adapt it to slot frames without behavior changes
  (crosshair, drawing tools, scale drags, magnet).
- **Per-frame DOM cost.** The largest forms (chart settings ~150 controls) rebuild a
  DOM each frame; measure on llvmpipe / phone CPUs, cache trees where state did not
  change.
- **Persistence.** Saved layouts use `uzor::panels` serialization; framework docking
  needs a converter so users keep their layouts.
- **Web size.** The live web build carries the exchange connectors (digdigdig3);
  budget proposal: 300 KB brotli for the demo build, 1.5 MB for live — not yet
  confirmed by the owner.
- **Multi-window and the shared data bridge.** shell-vello shares one DataBridge
  across windows; the framework's multi-window App must keep one bridge.
- **No tests on the old UI.** Parity is checked by screenshots and DOM tests on the
  new side only; keep a checklist per screen.
