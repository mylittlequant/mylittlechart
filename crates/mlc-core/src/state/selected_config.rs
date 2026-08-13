use crate::i18n::ConceptKey;

/// Configuration for selected primitive (for inline config toolbar)
#[derive(Debug, Clone, Default)]
pub struct SelectedPrimitiveConfig {
    /// Display name (e.g., "Линия тренда")
    pub name: String,
    /// Current stroke color (hex)
    pub color: String,
    /// Current line width
    pub width: f64,
    /// Current line style ("solid", "dashed", "dotted")
    pub style: String,
    /// Is primitive locked
    pub locked: bool,
    /// Text color (hex) - None if no text or uses stroke color
    pub text_color: Option<String>,
    /// Whether this primitive supports text
    pub supports_text: bool,
    /// `(slot_id, label)` pairs for the slots this primitive's own points
    /// can seed a child Elliott wave into (E2 — see
    /// `drawing::primitives_v2::elliott::count`). Empty for every
    /// primitive outside the Elliott family. `slot_id` is
    /// `WaveSlot::id()` — a stable wire token, not the printed notation —
    /// so a UI layer can build a widget action id without depending on the
    /// `WaveSlot` type itself; `label` is `WaveSlot::label()` read at THIS
    /// primitive's own degree (e.g. an Intermediate-degree impulse offers
    /// `("impulse_1", "(1)")` .. `("impulse_5", "(5)")`).
    pub wave_slots: Vec<(String, String)>,
    /// For each slot already present in [`Self::wave_slots`], the legal
    /// child kinds that slot may subdivide into — `(kind_id, label_key)`
    /// pairs (E2's two-step picker: slot first, then which structure goes
    /// in it — see `drawing::primitives_v2::elliott::count::child_kinds`).
    /// `kind_id` is `elliott::count::split_kind_id`'s own fixed
    /// `"type_id[:subtype]"` spelling (e.g. `"elliott_correction:flat"`);
    /// `label_key` is a [`ConceptKey`], NOT a resolved string — this
    /// struct is built in the backend, where the active display language
    /// is not known (every other localized label in the drawing layer is
    /// resolved at render time, in the layer that DOES know it). A slot
    /// whose own `child_kinds` comes back empty never gets an entry here
    /// — nor in `wave_slots` itself; the two lists are built in the same
    /// pass and always name the same set of slots.
    pub wave_slot_kinds: Vec<(String, Vec<(String, ConceptKey)>)>,
    /// `(type_id, display_name)` pairs for the Elliott types that may
    /// validly follow this primitive at its OWN degree — succession, not
    /// subdivision (see `drawing::primitives_v2::elliott::count::
    /// successor_kinds`). Empty for every primitive outside the Elliott
    /// family. `type_id` is the primitive's own registry id (e.g.
    /// `"elliott_correction"`), matching every other `type_id` string in
    /// this codebase; `display_name` is that type's own registered
    /// `PrimitiveMetadata::display_name` — never a second name table.
    pub successor_kinds: Vec<(String, String)>,
    /// Whether the selected primitive's own `follows`-linked chain
    /// currently promotes into one coarser structure (E4 — see
    /// `drawing::primitives_v2::elliott::count::promote_chain_shape`).
    /// `false` for every primitive outside the Elliott family, and for an
    /// Elliott wave whose chain does not (yet) qualify — mixed degree, a
    /// member already nested under another parent, a chain longer than the
    /// parent type's own slot count, or broken alternation.
    pub can_promote: bool,
    /// `(kind_id, label_key, is_current)` triples for every Elliott type
    /// the selected primitive may convert INTO right now, in-place, without
    /// leaving the markup process — the structure's own global type, as
    /// opposed to what nests inside one of its waves ([`Self::wave_slot_kinds`])
    /// or what follows it ([`Self::successor_kinds`]) (owner report
    /// 2026-08-14: "я создал АБЦ как зигзаг, но в процессе понял, что хочу
    /// сделать его плоскостью ... не могу прямо в процессе разметки
    /// переменить глобальное состояние отрезка"). Empty for every primitive
    /// outside the Elliott family (see
    /// `drawing::primitives_v2::elliott::count::conversion_kinds`, whose
    /// output this is built from, one-to-one, via
    /// `count::kind_label`/`count::kind_id_of` — same `kind_id`/`ConceptKey`
    /// shape [`Self::wave_slot_kinds`] already uses, never a resolved
    /// string, for the same reason: this struct is built in the backend,
    /// where the active display language is not known.
    ///
    /// `is_current` marks the ONE entry naming what the structure already
    /// is (`count::conversion_kinds` always includes it, even when the
    /// surrounding grammar would otherwise refuse it) — a UI layer uses
    /// this to disable that row rather than offering a picker option that
    /// would be a no-op.
    pub conversion_kinds: Vec<(String, ConceptKey, bool)>,
}
