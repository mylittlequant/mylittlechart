# mlc-app-shell-web

Browser shell for the MLC chart application (Phase 4 wasm spike).

Renders `ChartApp` to `<canvas id="chart">` via the Canvas 2D API.
No winit, no wgpu, no vello-gpu. Backend is swappable via Cargo features.

## Prerequisites

- Rust + wasm-pack: `cargo install wasm-pack`
- A static file server: `cargo install basic-http-server` or use `npx serve`

## Check (no build)

```sh
# From the workspace root (nemo/mylittlechart/):
cargo check -p mylittlechart-app-shell-web --target wasm32-unknown-unknown
```

## Build

```sh
# From the crate directory:
wasm-pack build --target web --out-dir ../../../dist/mlc-app-shell-web
```

Or from the workspace root:

```sh
wasm-pack build crates/mlc-app-shell-web --target web --out-dir ../../dist/mlc-app-shell-web
```

This produces:
```
dist/mlc-app-shell-web/
  mlc_app_shell_web.js       — ES module glue
  mlc_app_shell_web_bg.wasm  — binary
  ...
```

## Serve

Copy `index.html` to the dist directory, then serve:

```sh
cp crates/mlc-app-shell-web/index.html dist/mlc-app-shell-web/
basic-http-server dist/mlc-app-shell-web
# or:
cd dist/mlc-app-shell-web && npx serve .
```

Open `http://localhost:4000` (or whatever port).

## Swapping render backends

The current default is `uzor-render-canvas2d`.

To switch to vello-cpu (once the feature is wired in):
1. Uncomment `uzor-render-vello-cpu` in `Cargo.toml` dependencies.
2. Uncomment the `render-vello-cpu` feature.
3. In `src/lib.rs`, replace `Canvas2dChartRenderContext::new(...)` with the
   vello-cpu context constructor inside `render_frame`.
4. The `mlc_core::render::RenderContext` interface is identical — no other
   changes required.

## Architecture

```
DataBridge::new().await         → Arc<DataBridge>       (dig3-station, wasm-aware)
ChartApp::new_empty(bridge)     → ChartApp               (no profile, no wizard)
WebWindowProvider::from_id("chart")  → PlatformEvents from DOM
Canvas2dChartRenderContext      → mlc_core::RenderContext impl on CanvasRenderingContext2d

requestAnimationFrame loop:
  poll_events() → dispatch to ChartApp input methods
  render_frame() → chart.render(&mut ctx, now_ms, false)
```
