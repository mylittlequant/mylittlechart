//! Serialized form of a single drawing primitive.
//!
//! Lives next to the `Primitive` trait so both runtime caches
//! (`mlc-core::state::ChartContainer::symbol_drawings`) and the app-side
//! preset (`mlc-app::preset::snapshots`) can speak about the same type.

use serde::{Deserialize, Serialize};

use crate::drawing::primitives_v2::Primitive;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimitiveSnapshot {
    /// Primitive type identifier (e.g. `"trend_line"`, `"fib_retracement"`).
    pub type_id: String,
    /// Full primitive state serialized to JSON by the primitive itself.
    pub json: String,
}

impl PrimitiveSnapshot {
    pub fn from_primitive(prim: &dyn Primitive) -> Self {
        Self {
            type_id: prim.type_id().to_string(),
            json: prim.to_json(),
        }
    }

    /// Rewrite the embedded `data.id` to `new_id`, leaving every other
    /// field untouched. Every concrete primitive type serializes its
    /// shared header under a field literally named `data`
    /// (`pub data: PrimitiveData`) — a generic JSON patch works for any
    /// registered type without downcasting.
    ///
    /// Used by cold-load history reconciliation
    /// (`Command::set_prim_id`): a preset reload can re-number a
    /// primitive's id, and a stale undo/redo command's own snapshot must
    /// follow the same renumbering or a restored `CreatePrimitive` would
    /// resurrect the primitive under its OLD id, colliding with (or
    /// orphaned from) whatever the reload already assigned. Returns an
    /// unchanged clone when `json` does not parse as an object with a
    /// `data` object — defensive, not expected to ever trigger for a
    /// snapshot this crate itself produced.
    pub fn with_id(&self, new_id: u64) -> Self {
        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&self.json) else {
            return self.clone();
        };
        if let Some(data) = value.get_mut("data").and_then(|d| d.as_object_mut()) {
            data.insert("id".to_string(), serde_json::Value::from(new_id));
        } else {
            return self.clone();
        }
        let Ok(json) = serde_json::to_string(&value) else {
            return self.clone();
        };
        Self { type_id: self.type_id.clone(), json }
    }
}
