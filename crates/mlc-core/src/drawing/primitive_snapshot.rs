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
}
