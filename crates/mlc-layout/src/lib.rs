//! mylittlechart frontend layer (`mlc-layout`).
//!
//! Desktop/wasm presentation: layout computation, render config, ui widgets,
//! hit-testing, input handling. Depends on `mlc-core` for backend state and
//! reads it to produce frames. This crate is the V-tier extraction target —
//! frontend code currently living inside `mlc-core` moves here so the backend
//! becomes a clean, platform-agnostic core that a wasm build (and later the
//! uzor-tessera app shell) can compose against.
//!
//! Empty skeleton (Phase D). Frontend modules land in V1-V4.

pub mod chart_render;
pub mod layout;
pub mod ui;
pub mod panel_app;
