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

    /// Read the primitive's own `data.id`, if `json` parses as an object
    /// with one. `None` for a snapshot this crate did not produce
    /// (defensive, same fallback [`Self::with_id`] uses).
    pub fn id(&self) -> Option<u64> {
        let value: serde_json::Value = serde_json::from_str(&self.json).ok()?;
        value.get("data")?.get("id")?.as_u64()
    }

    /// Rewrite every identifier this snapshot embeds that names something
    /// ELSE in the same import batch: its own `data.id`, `data.window_id`
    /// (the chart window it was created on), `data.origin_id` (the
    /// primitive it was synced-cloned from), and — for the Elliott family
    /// only — the top-level `link.parent_id` / `follows` fields.
    ///
    /// `primitive_id_map` / `window_id_map` are old-id → new-id lookups the
    /// caller builds once for the WHOLE preset being imported
    /// (`ChartPreset::remap_ids_for_import`). A reference whose OLD id is
    /// not a key in the matching map names something OUTSIDE this import
    /// batch — already unresolvable once (a cross-window sync clone whose
    /// origin window did not come along, say) — and is cleared rather than
    /// left dangling at a stale, now-foreign id, the same rule
    /// [`crate::drawing::reassign_primitive_ids_remapped`] already applies
    /// to a live primitive's wave links.
    pub fn remap_ids(
        &self,
        primitive_id_map: &std::collections::HashMap<u64, u64>,
        window_id_map: &std::collections::HashMap<u64, u64>,
    ) -> Self {
        fn remap_or_clear(
            obj: &mut serde_json::Map<String, serde_json::Value>,
            key: &str,
            id_map: &std::collections::HashMap<u64, u64>,
        ) {
            let Some(old) = obj.get(key).and_then(|v| v.as_u64()) else {
                return;
            };
            let replacement = match id_map.get(&old) {
                Some(&new_id) => serde_json::Value::from(new_id),
                None => serde_json::Value::Null,
            };
            obj.insert(key.to_string(), replacement);
        }

        let Ok(mut value) = serde_json::from_str::<serde_json::Value>(&self.json) else {
            return self.clone();
        };
        let Some(top) = value.as_object_mut() else {
            return self.clone();
        };
        if let Some(data) = top.get_mut("data").and_then(|d| d.as_object_mut()) {
            if let Some(old_id) = data.get("id").and_then(|v| v.as_u64()) {
                if let Some(&new_id) = primitive_id_map.get(&old_id) {
                    data.insert("id".to_string(), serde_json::Value::from(new_id));
                }
            }
            remap_or_clear(data, "window_id", window_id_map);
            remap_or_clear(data, "origin_id", primitive_id_map);
        }
        remap_or_clear(top, "follows", primitive_id_map);
        if let Some(link) = top.get_mut("link").and_then(|l| l.as_object_mut()) {
            remap_or_clear(link, "parent_id", primitive_id_map);
        }
        let Ok(json) = serde_json::to_string(&value) else {
            return self.clone();
        };
        Self { type_id: self.type_id.clone(), json }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snap(json: &str) -> PrimitiveSnapshot {
        PrimitiveSnapshot { type_id: "trend_line".to_string(), json: json.to_string() }
    }

    #[test]
    fn id_reads_the_embedded_data_id() {
        let s = snap(r#"{"data":{"id":7,"window_id":3}}"#);
        assert_eq!(s.id(), Some(7));
    }

    #[test]
    fn id_is_none_for_unparseable_json() {
        let s = snap("not json");
        assert_eq!(s.id(), None);
    }

    #[test]
    fn remap_ids_rewrites_id_window_id_and_origin_id() {
        let s = snap(r#"{"data":{"id":1,"window_id":10,"origin_id":2}}"#);
        let mut prim_map = std::collections::HashMap::new();
        prim_map.insert(1u64, 101u64);
        prim_map.insert(2u64, 102u64);
        let mut win_map = std::collections::HashMap::new();
        win_map.insert(10u64, 110u64);

        let remapped = s.remap_ids(&prim_map, &win_map);
        let v: serde_json::Value = serde_json::from_str(&remapped.json).unwrap();
        assert_eq!(v["data"]["id"], 101);
        assert_eq!(v["data"]["window_id"], 110);
        assert_eq!(v["data"]["origin_id"], 102);
    }

    #[test]
    fn remap_ids_clears_references_outside_the_batch() {
        // window_id/origin_id name ids NOT present in either map — cleared,
        // not left pointing at a foreign id.
        let s = snap(r#"{"data":{"id":1,"window_id":999,"origin_id":888}}"#);
        let mut prim_map = std::collections::HashMap::new();
        prim_map.insert(1u64, 101u64);
        let win_map = std::collections::HashMap::new();

        let remapped = s.remap_ids(&prim_map, &win_map);
        let v: serde_json::Value = serde_json::from_str(&remapped.json).unwrap();
        assert_eq!(v["data"]["id"], 101);
        assert!(v["data"]["window_id"].is_null());
        assert!(v["data"]["origin_id"].is_null());
    }

    #[test]
    fn remap_ids_rewrites_elliott_wave_link_and_follows() {
        let s = snap(r#"{"data":{"id":5},"link":{"parent_id":1,"slot":"a"},"follows":2}"#);
        let mut prim_map = std::collections::HashMap::new();
        prim_map.insert(5u64, 205u64);
        prim_map.insert(1u64, 201u64);
        prim_map.insert(2u64, 202u64);
        let win_map = std::collections::HashMap::new();

        let remapped = s.remap_ids(&prim_map, &win_map);
        let v: serde_json::Value = serde_json::from_str(&remapped.json).unwrap();
        assert_eq!(v["data"]["id"], 205);
        assert_eq!(v["link"]["parent_id"], 201);
        assert_eq!(v["link"]["slot"], "a");
        assert_eq!(v["follows"], 202);
    }
}
