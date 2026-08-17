//! Shared per-wave custom-label OVERRIDE machinery for all five Elliott
//! types (`elliott_impulse`/`elliott_correction`/`elliott_triangle`/
//! `elliott_double_combo`/`elliott_triple_combo`) — the design's own
//! `wave_tabs()` "Custom Labels" table (`ConceptKey::CustomLabelsGroupTitle`
//! / `OverrideHint`, `mlc-ui-lab`'s `spec.rs`), the first declaring user of
//! `PropertyType::Table`.
//!
//! One production point per concern (this wave's own hard rule): each
//! type's own `rendered_wave_labels()` stays the ONLY place a wave's own
//! on-chart label is decided (notation, then the truncation mark where that
//! applies, then this override, in that order) — this module never computes
//! a label itself, only turns the CURRENT rendered set into a declared
//! property ([`wave_label_override_property`]) and parses a cell click back
//! into a row write ([`apply_wave_label_override`]).

use super::super::config::{
    parse_table_cell_id, ConfigProperty, PropertyCategory, PropertyGroup, PropertyValue, TableCell,
    CUSTOM_WAVE_LABELS_PROPERTY_ID,
};
use crate::i18n::{current_language, ConceptKey};

/// Build the `custom_wave_labels` declared property every Elliott type's
/// own `style_properties()` appends — one row per point index, `Wave`#
/// (inert) + `Label` (editable, seeded with what THIS index actually
/// renders right now — `rendered`, override or not, the SAME strings a
/// chart draws). `rendered.len()` decides the row count, so a 4-point
/// correction gets 4 rows and a 9-point triple combo gets 9, with no
/// per-type array size to keep in sync here.
pub(super) fn wave_label_override_property(rendered: &[String]) -> ConfigProperty {
    let lang = current_language();
    let head =
        vec![ConceptKey::WaveColumnHeader.get(lang).to_string(), ConceptKey::LabelColumnHeader.get(lang).to_string()];
    let rows: Vec<Vec<TableCell>> = rendered
        .iter()
        .enumerate()
        .map(|(i, label)| vec![TableCell::Text(i.to_string()), TableCell::EditableText(label.clone())])
        .collect();
    ConfigProperty::table(CUSTOM_WAVE_LABELS_PROPERTY_ID, ConceptKey::CustomLabelsGroupTitle.get(lang), head, rows)
        .with_category(PropertyCategory::LabelsText)
        .with_order(25)
        .in_group(PropertyGroup::CustomWaveLabels)
}

/// Parse and apply a `custom_wave_labels` cell click — `id` is whatever
/// `apply_style_property` receives (already the bare id, past the
/// `style_prop:`/`text_prop:`/`level_prop:` family-prefix strip every host
/// already does before calling in). Returns `Some(true)`/`Some(false)` when
/// `id` DOES name this property (write applied / rejected, e.g. an
/// out-of-range row or a non-string value), `None` when it does not — the
/// caller falls through to its own `match id` in that case, exactly the
/// same "not recognised here" contract every other `apply_style_property`
/// arm follows.
///
/// `labels` is the declaring type's own `custom_wave_labels` field —
/// grown to `point_count` on first write (never shrunk) so a later row
/// index is always in range; an empty string clears that row's own
/// override back to `None` (the label falls back to whatever
/// `wave_labels()` already produces for it).
pub(super) fn apply_wave_label_override(
    id: &str,
    value: &PropertyValue,
    labels: &mut Vec<Option<String>>,
    point_count: usize,
) -> Option<bool> {
    let (base, row, col) = parse_table_cell_id(id)?;
    if base != CUSTOM_WAVE_LABELS_PROPERTY_ID || col != 1 {
        return None;
    }
    let Some(text) = value.as_string() else { return Some(false) };
    if row >= point_count {
        return Some(false);
    }
    if labels.len() < point_count {
        labels.resize(point_count, None);
    }
    labels[row] = if text.is_empty() { None } else { Some(text.to_string()) };
    Some(true)
}

/// Apply `overrides` (the declaring type's own `custom_wave_labels`) on top
/// of `computed` (that type's own `wave_labels()`, already turned into
/// owned strings) — the shared LAST step every `rendered_wave_labels()`
/// calls, after notation resolution and (impulse only) the truncation
/// mark. An override fully REPLACES the computed string at its own index;
/// an absent or `None` entry changes nothing, so a structure with no
/// overrides renders byte for byte as before this field existed.
pub(super) fn apply_overrides(computed: &mut [String], overrides: &[Option<String>]) {
    for (i, custom) in overrides.iter().enumerate() {
        if let (Some(text), Some(slot)) = (custom, computed.get_mut(i)) {
            *slot = text.clone();
        }
    }
}
