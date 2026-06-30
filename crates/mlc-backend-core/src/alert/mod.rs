//! Alert logic: AlertManager + crossing detection.  Value types live
//! in `mlc-core::alert`; this module owns the manager that mutates
//! the bubble's alert slice.

mod manager;
mod crossing;
pub use manager::AlertManager;
pub use crossing::check_crossings;
