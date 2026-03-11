# mylittlechart

Multi-exchange trading terminal with GPU-accelerated rendering. Version 0.2.8.

## Workspace Crates

| Crate | Purpose |
|-------|---------|
| `chart` | Chart rendering, panel system, user profile, indicators layout |
| `chart-app` | ChartApp logic, action enums, input handling, tick() |
| `chart-app-vello` | App entry point, AppState, event loop, window management |
| `indicators` | 480+ technical indicators (SMA, EMA, RSI, MACD, etc.) |
| `sidebar-content` | Sidebar panel content definitions |
| `alerts` | Alert system (price, indicator, drawing crossing) |
| `alert-delivery` | Alert delivery via Telegram bot |
| `live-data` | DataBridge, WebSocket live feed, broadcast channels |
| `zengeld-server` | Internal agent API server (localhost:17420) |
| `instanced-context` | wgpu instanced renderer backend |
| `vello-context` | Vello GPU renderer backend (default) |
| `vello-cpu-context` | Vello CPU fallback renderer |
| `vello-hybrid-context` | Vello hybrid GPU/CPU renderer |
| `tiny-skia-context` | TinySkia software renderer |
| `updater` | OTA updates, heartbeat, OAuth, cloud sync, E2E encryption |

## Build & Run

```bash
cargo build --release
cargo run --release

# Standalone build (no server communication)
cargo build --release --features standalone
```

## Architecture

- **Pipelined render**: Dedicated GPU thread with double-buffered scenes
- **Per-window parallel scene build** via `thread::scope`
- **Action queue pattern**: Window pushes typed enum → App drains in about_to_wait()
- **AppState** is canonical owner of all user-level data
- **Indicator computation** parallelized with rayon

## Rendering Backends

| Backend | Status | Description |
|---------|--------|-------------|
| Vello GPU | Default | Vector graphics via compute shaders |
| Instanced wgpu | Experimental | 3 draw calls (quads/lines/text) |
| Vello CPU | Fallback | Software rendering via Vello |
| Vello Hybrid | Experimental | GPU compute + CPU fallback |
| TinySkia | Fallback | Pure software rasterizer |

## Client Modes

| Mode | Description |
|------|-------------|
| Standalone | Zero network calls. All data local. Default for new installs. |
| Connected | Syncs with chart.example — cloud sync, OTA updates, telemetry |

## Cloud Features (Connected mode)

- **OTA auto-updates** with Ed25519 signature verification
- **Cloud sync** of presets, watchlists, templates (AES-256-GCM encrypted)
- **E2E encryption** option (PBKDF2 → HKDF → AES-256-GCM)
- **API key management** via dashboard + agent-initiated device code flow
- **Build attestation** — server rejects unofficial/modified builds
- **Telemetry** — opt-in usage analytics (toggleable in Settings)

## Agent API

Local axum server on `localhost:17420`. Requires API key (from chart.example or local generation).

Permissions: `read_bars`, `read_indicators`, `read_viewport`, `read_config`, `execute_trades`, `manage_alerts`.

## Key Files

| File | Purpose |
|------|---------|
| `chart-app-vello/src/main.rs` | App, AppState, event loop, save_all() |
| `chart-app/src/lib.rs` | ChartApp, action enums, tick() |
| `chart-app/src/input.rs` | All user interaction handlers |
| `chart/src/panel_app.rs` | ChartPanelApp, chart rendering |
| `chart/src/user_profile/profile.rs` | UserProfile, ClientMode, SyncState |
| `updater/src/lib.rs` | Updater loop, OTA check, cloud sync |
| `updater/src/verify.rs` | Ed25519 OTA signature verification |
| `updater/src/cloud_sync.rs` | Push/pull sync pipeline |
| `updater/src/e2e_crypto.rs` | E2E encryption primitives |
| `updater/src/attest.rs` | Build attestation headers |
| `live-data/src/bridge.rs` | DataBridge, broadcast channel |

## Dependencies

Uses local patches for development (stripped in CI):
- `uzor` — 2D rendering engine
- `digdigdig3` — Exchange connectors

## License

MIT OR Apache-2.0
