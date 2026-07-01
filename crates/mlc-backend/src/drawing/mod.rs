//! Drawing logic: `DrawingManager` + facade. Value types (the `Primitive`
//! trait, concrete primitives, `PrimitiveData`, `PrimitiveSnapshot`,
//! `PropertyValue`, etc.) live in `mlc-core::drawing`; this module owns the
//! per-bubble stateful manager that mutates the bubble's primitive slice.
//!
//! `DrawingManager` itself is `pub(crate)`-surfaced only — app code accesses
//! it exclusively through the `Drawings` / `DrawingsMut` facade (see
//! `BackendCore::drawings` / `drawings_mut` in
//! `crates/mlc-backend/src/lib.rs`).

mod manager;
mod facade;

pub use manager::{DragType, DrawingManager, DrawingState, PrimitiveListItem};
pub use facade::{BubbleDrawingView, Drawings, DrawingsMut, SwapSymbolResult};
