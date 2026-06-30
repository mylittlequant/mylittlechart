//! Pure value-types for alerts.  Both backend (DataBubble.alerts) and
//! app (UI overlays / sidebar) reference these types.  Logic lives in
//! `mlc-backend-core::alert`.

mod types;
pub use types::*;
