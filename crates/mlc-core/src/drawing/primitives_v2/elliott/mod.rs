//! Elliott Wave module - Elliott wave patterns

pub mod elliott_impulse;
pub mod elliott_correction;
pub mod elliott_triangle;
pub mod elliott_double_combo;
pub mod elliott_triple_combo;
pub mod count;
mod ratio;
mod projection;
pub mod guard;
mod wave_hit;

pub use elliott_impulse::ElliottImpulse;
pub use elliott_correction::{ElliottCorrection, CorrectionType};
pub use elliott_triangle::ElliottTriangle;
pub use elliott_double_combo::ElliottDoubleCombo;
pub use elliott_triple_combo::ElliottTripleCombo;
pub use count::{
    WaveSlot, WaveLink, ChildSeed, child_seed, SuccessorSeed, successor_seed, successor_kinds,
    PredecessorSeed, predecessor_seed, predecessor_kinds,
    PromotedShape, promote_chain_shape,
    CHILD_KIND_CANDIDATES, split_kind_id, parent_pattern, kind_pattern, kind_label,
    slot_position, child_kinds, slot_span,
    kind_id_of, conversion_kinds, resample_polyline, slot_ordinal, slot_at_ordinal,
    label_collision_indices,
    Adoption, adoptable_children, adopted_parent_degree,
};
pub use guard::{ChainViolation, Violation, chain_violations, violations, violations_in};
