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
}
