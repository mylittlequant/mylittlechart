//! Typed service layer over `BackendCore`'s `RwLock`-guarded managers.
//!
//! Doctrine (`docs/mlc/plans/doctrine-cutover-2026-06-27.md` Step 3):
//! `BackendCore` exposes typed services — the ONLY public access path. No
//! `RwLockGuard` escapes outward, ever. Each service module wraps exactly
//! one `BackendCore` field's guard and exposes either explicit named
//! methods (the high-frequency 80%) or a `with_panel(_mut)` closure escape
//! hatch for the long tail. The guard itself never leaves the service
//! struct — only borrows scoped to the service's lifetime (read side) or
//! to the closure call (write side) do.

pub mod drawing;

pub use drawing::{DrawingService, DrawingServiceMut, SwapSymbolResult};
