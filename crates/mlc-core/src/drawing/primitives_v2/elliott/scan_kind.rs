//! `EwaScanKind` — the 8 Elliott shapes `mli::ewa::scan::scan_waves` can
//! find, the SAME shape `smc::smc_scan::SurveyStructure` already
//! establishes for the SMC survey's own structure table: one place a
//! stable snake_case WIRE id, a display label, and the `mli::ewa::types::
//! EwaPatternKind` this scanner asks `scan_waves` for map to each other.
//!
//! The 8 kinds are the arc's own verbatim list (`docs/mlc/plans/
//! autodetectors-a2-a4-implementation-2026-09-24.md`, "Grounding
//! correction"): `Impulse`, the 4 diagonal kinds (Leading/Ending ×
//! Contracting/Expanding), `Zigzag`, `Flat`, `Triangle` — never the wider
//! aspirational `EwaScanKind` sketch the arc's own §2 names, which lists
//! kinds `scan_waves` does not implement.
//!
//! Isolated from `elliott_scan.rs` (A2.1, ahead of `ElliottScan` itself,
//! A2.2) per that piece's own "a small `elliott/scan_kind.rs` if it grows
//! i18n-key plumbing worth isolating" call — `label`'s own reuse-vs-new-key
//! reasoning is worth keeping apart from `ElliottScan`'s own struct/render
//! code, and `state::ewa_engine::EwaEngineConfig`'s own seed-default field
//! needs this catalogue's `ALL`/`id` before `ElliottScan` exists at all.

use crate::i18n::{ConceptKey, ConfigKey, Language};
use mli::ewa::types::EwaPatternKind;

/// One of the 8 Elliott shapes `ElliottScan` can survey for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EwaScanKind {
    Impulse,
    LeadingDiagonalContracting,
    LeadingDiagonalExpanding,
    EndingDiagonalContracting,
    EndingDiagonalExpanding,
    Zigzag,
    Flat,
    Triangle,
}

impl EwaScanKind {
    pub const ALL: [Self; 8] = [
        Self::Impulse,
        Self::LeadingDiagonalContracting,
        Self::LeadingDiagonalExpanding,
        Self::EndingDiagonalContracting,
        Self::EndingDiagonalExpanding,
        Self::Zigzag,
        Self::Flat,
        Self::Triangle,
    ];

    /// Stable snake_case wire id, persisted on [`super::elliott_scan::
    /// ElliottScan::enabled_kinds`] — never an enum index, so a saved scan
    /// survives a catalogue that grows (same convention
    /// `smc_scan::SurveyStructure::id`/`patterns::candle_pattern_scan::
    /// CandlePatternScan::enabled_kinds` already use).
    pub fn id(self) -> &'static str {
        match self {
            Self::Impulse => "impulse",
            Self::LeadingDiagonalContracting => "leading_diagonal_contracting",
            Self::LeadingDiagonalExpanding => "leading_diagonal_expanding",
            Self::EndingDiagonalContracting => "ending_diagonal_contracting",
            Self::EndingDiagonalExpanding => "ending_diagonal_expanding",
            Self::Zigzag => "zigzag",
            Self::Flat => "flat",
            Self::Triangle => "triangle",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// The `mli::ewa::types::EwaPatternKind` this scan kind asks
    /// `scan_waves` for. Every variant name here matches its
    /// `EwaPatternKind` counterpart exactly — a plain rename, never a
    /// lossy mapping.
    pub fn as_ewa_kind(self) -> EwaPatternKind {
        match self {
            Self::Impulse => EwaPatternKind::Impulse,
            Self::LeadingDiagonalContracting => EwaPatternKind::LeadingDiagonalContracting,
            Self::LeadingDiagonalExpanding => EwaPatternKind::LeadingDiagonalExpanding,
            Self::EndingDiagonalContracting => EwaPatternKind::EndingDiagonalContracting,
            Self::EndingDiagonalExpanding => EwaPatternKind::EndingDiagonalExpanding,
            Self::Zigzag => EwaPatternKind::Zigzag,
            Self::Flat => EwaPatternKind::Flat,
            Self::Triangle => EwaPatternKind::Triangle,
        }
    }

    /// The reverse of [`Self::as_ewa_kind`] — used to label a resolved hit's
    /// own `EwaPatternKind` back on the Hits table's "Kind" column. `None`
    /// for any `EwaPatternKind` outside the 8 this scan enumerates (never
    /// reachable for a hit `ElliottScan::resolved_scan` actually produced,
    /// since it only ever asks `scan_waves` for kinds drawn from
    /// [`Self::ALL`] — an `Option` return rather than a panic all the same,
    /// per this crate's own "never panic on an unexpected id" rule).
    pub fn from_ewa_kind(kind: EwaPatternKind) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_ewa_kind() == kind)
    }

    /// Display label. Reuses an existing key wherever the wording already
    /// matches an established picker over this SAME 8-shape vocabulary
    /// (`ConceptKey::WaveKind*`, `elliott::count::child_kinds`'s own label
    /// table; `ConfigKey::Correction{Zigzag,Flat}`, `ElliottCorrection`'s
    /// own subtype dropdown) — new `ConceptKey::WaveKind{Leading,Ending}
    /// Diagonal{Contracting,Expanding}` rows only where no prior label
    /// carries the Contracting/Expanding split at all (`ConceptKey::
    /// WaveKindLeadingDiagonal`/`WaveKindEndingDiagonal` name the FAMILY,
    /// not the split).
    pub fn label(self, lang: Language) -> &'static str {
        match self {
            Self::Impulse => ConceptKey::WaveKindImpulse.get(lang),
            Self::LeadingDiagonalContracting => ConceptKey::WaveKindLeadingDiagonalContracting.get(lang),
            Self::LeadingDiagonalExpanding => ConceptKey::WaveKindLeadingDiagonalExpanding.get(lang),
            Self::EndingDiagonalContracting => ConceptKey::WaveKindEndingDiagonalContracting.get(lang),
            Self::EndingDiagonalExpanding => ConceptKey::WaveKindEndingDiagonalExpanding.get(lang),
            Self::Zigzag => ConfigKey::CorrectionZigzag.get(lang),
            Self::Flat => ConfigKey::CorrectionFlat.get(lang),
            Self::Triangle => ConceptKey::WaveKindTriangle.get(lang),
        }
    }
}

/// Every kind's own id — the default `enabled_kinds` a freshly constructed
/// `ElliottScan` seeds from (via `EwaEngineConfig::scan_default_enabled_kinds`)
/// and the fallback an old saved scan missing the field entirely lands on
/// (`ElliottScan::enabled_kinds`'s own `#[serde(default = ...)]`) — same
/// dual role `patterns::candle_pattern_scan::default_all_kind_ids` already
/// plays for its own 34-kind catalogue.
pub fn default_all_kind_ids() -> Vec<String> {
    EwaScanKind::ALL.iter().map(|k| k.id().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn ids_round_trip() {
        for kind in EwaScanKind::ALL {
            assert_eq!(EwaScanKind::from_id(kind.id()), Some(kind));
        }
        assert_eq!(EwaScanKind::from_id("bogus"), None);
    }

    #[test]
    fn as_ewa_kind_and_from_ewa_kind_are_inverse() {
        for kind in EwaScanKind::ALL {
            assert_eq!(EwaScanKind::from_ewa_kind(kind.as_ewa_kind()), Some(kind));
        }
        assert_eq!(EwaScanKind::from_ewa_kind(EwaPatternKind::Xabcd), None);
    }

    #[test]
    fn default_all_kind_ids_covers_every_kind_once() {
        let ids = default_all_kind_ids();
        assert_eq!(ids.len(), EwaScanKind::ALL.len());
        for kind in EwaScanKind::ALL {
            assert_eq!(ids.iter().filter(|id| id.as_str() == kind.id()).count(), 1);
        }
    }

    #[test]
    fn labels_never_panic_in_any_language() {
        for kind in EwaScanKind::ALL {
            for lang in [
                Language::En, Language::Ru, Language::Es, Language::De, Language::Fr,
                Language::Pt, Language::Zh, Language::Ja, Language::Ko, Language::Ar,
                Language::It, Language::Tr, Language::Pl, Language::Uk, Language::Hi,
            ] {
                assert!(!kind.label(lang).is_empty());
            }
        }
    }
}
