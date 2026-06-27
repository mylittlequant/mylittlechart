//! Helper for restoring drawing primitives from a snapshot into a live
//! `DrawingManager`.
//!
//! `ChartContainer::take_drawings_snapshot_for_symbol` returns the cached
//! `Vec<PrimitiveSnapshot>` without touching any backend manager —
//! mlc-core does not own one.  This helper closes the loop: rebuild
//! concrete primitives through the global `PrimitiveRegistry` and push
//! them into the panel's live `DrawingManager`.

use mlc_core::drawing::{DrawingManager, PrimitiveSnapshot};
use mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry;

/// Rebuild each cached snapshot through the registry and push it into
/// `dm` as an external primitive.  Snapshots whose `type_id` is no
/// longer registered are silently skipped.
pub fn restore_drawings_into_manager(snaps: &[PrimitiveSnapshot], dm: &mut DrawingManager) {
    let Ok(reg) = PrimitiveRegistry::global().read() else { return };
    for snap in snaps {
        if let Some(prim) = reg.from_json(&snap.type_id, &snap.json) {
            dm.add_external_primitive(prim);
        }
    }
}
