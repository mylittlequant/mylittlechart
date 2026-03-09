//! Alert management crate for zengeld-terminal.
//!
//! Owns all alert types, the AlertManager, and crossing detection logic.
//! Chart-app and sidebar-content depend on this crate for alert data.

mod types;
mod manager;
mod crossing;

pub use types::{
    AlertCondition,
    AlertItem,
    AlertSource,
    AlertStatus,
    AlertTransport,
    AlertTriggerMode,
    DrawingExtendMode,
};
pub use manager::AlertManager;
pub use crossing::check_crossings;
