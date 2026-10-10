//! `HarmonicScanKind` — the 11 harmonic shapes
//! `mli::harmonic::chart_scan::scan_waves_with_harmonic_tolerance` can find, the SAME
//! shape `elliott::scan_kind::EwaScanKind` already establishes for the
//! Elliott survey's own kinds table: one place a stable snake_case WIRE id,
//! a display label, and the `mli::ewa::types::HarmonicKind` this scanner
//! asks for map to each other.
//!
//! The 11 ids match `patterns::xabcd_pattern::XabcdHarmonicKind::as_str`'s
//! own 7 tokens (`gartley`..`shark`) plus `cypher`/`abcd`/`three_drives`/
//! `five_zero` — NOT `xabcd` (unclassified is not a scan target, same
//! reasoning arc §3 states and A0's own piece-2 precedent already encodes
//! for the manual tool). `docs/mlc/plans/
//! autodetectors-a2-a4-implementation-2026-09-24.md`, "A3.3 —
//! `patterns/harmonic_scan.rs`".
//!
//! Isolated from `harmonic_scan.rs` (A3.2, ahead of `HarmonicScan` itself,
//! A3.3) per `elliott::scan_kind`'s own "a small `scan_kind.rs` if it grows
//! i18n-key plumbing worth isolating" precedent — `label`'s own
//! reuse-existing-key reasoning is worth keeping apart from `HarmonicScan`'s
//! own struct/render code, and `state::harmonic_engine::HarmonicEngineConfig`'s
//! own seed-default field needs this catalogue's `ALL`/`id` before
//! `HarmonicScan` exists at all.

use crate::i18n::{ConceptKey, Language, PrimitiveNameKey};
use mli::harmonic::HarmonicKind;

/// One of the 11 harmonic shapes `HarmonicScan` can survey for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HarmonicScanKind {
    Gartley,
    Bat,
    AltBat,
    Butterfly,
    Crab,
    DeepCrab,
    Shark,
    Cypher,
    Abcd,
    ThreeDrives,
    FiveZero,
}

impl HarmonicScanKind {
    pub const ALL: [Self; 11] = [
        Self::Gartley,
        Self::Bat,
        Self::AltBat,
        Self::Butterfly,
        Self::Crab,
        Self::DeepCrab,
        Self::Shark,
        Self::Cypher,
        Self::Abcd,
        Self::ThreeDrives,
        Self::FiveZero,
    ];

    /// Stable snake_case wire id, persisted on
    /// [`super::harmonic_scan::HarmonicScan::enabled_kinds`] — never an enum
    /// index, so a saved scan survives a catalogue that grows (same
    /// convention `elliott::scan_kind::EwaScanKind::id`/
    /// `patterns::xabcd_pattern::XabcdHarmonicKind::as_str` already use).
    pub fn id(self) -> &'static str {
        match self {
            Self::Gartley => "gartley",
            Self::Bat => "bat",
            Self::AltBat => "alt_bat",
            Self::Butterfly => "butterfly",
            Self::Crab => "crab",
            Self::DeepCrab => "deep_crab",
            Self::Shark => "shark",
            Self::Cypher => "cypher",
            Self::Abcd => "abcd",
            Self::ThreeDrives => "three_drives",
            Self::FiveZero => "five_zero",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// The `mli::ewa::types::HarmonicKind` this scan kind asks
    /// `scan_waves_with_harmonic_tolerance` for. Every variant name here
    /// matches its `HarmonicKind` counterpart exactly — a plain rename,
    /// never a lossy mapping.
    pub fn as_harmonic_kind(self) -> HarmonicKind {
        match self {
            Self::Gartley => HarmonicKind::Gartley,
            Self::Bat => HarmonicKind::Bat,
            Self::AltBat => HarmonicKind::AltBat,
            Self::Butterfly => HarmonicKind::Butterfly,
            Self::Crab => HarmonicKind::Crab,
            Self::DeepCrab => HarmonicKind::DeepCrab,
            Self::Shark => HarmonicKind::Shark,
            Self::Cypher => HarmonicKind::Cypher,
            Self::Abcd => HarmonicKind::Abcd,
            Self::ThreeDrives => HarmonicKind::ThreeDrives,
            Self::FiveZero => HarmonicKind::FiveZero,
        }
    }

    /// The reverse of [`Self::as_harmonic_kind`] — used to label a resolved hit's
    /// own `HarmonicKind` back on the Hits table's "Kind" column. `None`
    /// for any `HarmonicKind` outside the 11 this scan enumerates (never
    /// reachable for a hit `HarmonicScan::resolved_scan` actually produced,
    /// since it only ever asks `scan_waves_with_harmonic_tolerance` for
    /// kinds drawn from [`Self::ALL`] — an `Option` return rather than a
    /// panic all the same, per this crate's own "never panic on an
    /// unexpected id" rule).
    pub fn from_harmonic_kind(kind: HarmonicKind) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_harmonic_kind() == kind)
    }

    /// Display label. The 7 Carney-named kinds reuse `ConceptKey::
    /// HarmonicKind*` — the SAME label `patterns::xabcd_pattern::
    /// XabcdHarmonicKind`'s own `pattern_kind` dropdown already shows for
    /// these exact proper nouns. `Cypher`/`Abcd`/`ThreeDrives`/`FiveZero`
    /// reuse their own manual primitive's `PrimitiveNameKey` — the SAME
    /// name that primitive's own toolbar row already carries, so the Hits
    /// table's "Kind" column and the toolbar agree on one name per shape.
    pub fn label(self, lang: Language) -> &'static str {
        match self {
            Self::Gartley => ConceptKey::HarmonicKindGartley.get(lang),
            Self::Bat => ConceptKey::HarmonicKindBat.get(lang),
            Self::AltBat => ConceptKey::HarmonicKindAltBat.get(lang),
            Self::Butterfly => ConceptKey::HarmonicKindButterfly.get(lang),
            Self::Crab => ConceptKey::HarmonicKindCrab.get(lang),
            Self::DeepCrab => ConceptKey::HarmonicKindDeepCrab.get(lang),
            Self::Shark => ConceptKey::HarmonicKindShark.get(lang),
            Self::Cypher => PrimitiveNameKey::CypherPattern.get(lang),
            Self::Abcd => PrimitiveNameKey::AbcdPattern.get(lang),
            Self::ThreeDrives => PrimitiveNameKey::ThreeDrives.get(lang),
            Self::FiveZero => PrimitiveNameKey::FiveZeroPattern.get(lang),
        }
    }
}

/// Every kind's own id — the default `enabled_kinds` a freshly constructed
/// `HarmonicScan` seeds from (via `HarmonicEngineConfig::
/// scan_default_enabled_kinds`) and the fallback an old saved scan missing
/// the field entirely lands on (`HarmonicScan::enabled_kinds`'s own
/// `#[serde(default = ...)]`) — same dual role `elliott::scan_kind::
/// default_all_kind_ids` already plays for `ElliottScan`'s own catalogue.
/// Named `_harmonic_` (not a bare `default_all_kind_ids`) so `pub use
/// patterns::*`/`pub use elliott::*` at the `primitives_v2` crate root never
/// glob-collide on the same unqualified name.
pub fn default_all_harmonic_kind_ids() -> Vec<String> {
    HarmonicScanKind::ALL.iter().map(|k| k.id().to_string()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::i18n::Language;

    #[test]
    fn ids_round_trip() {
        for kind in HarmonicScanKind::ALL {
            assert_eq!(HarmonicScanKind::from_id(kind.id()), Some(kind));
        }
        assert_eq!(HarmonicScanKind::from_id("bogus"), None);
        assert_eq!(HarmonicScanKind::from_id("xabcd"), None, "unclassified is not a scan target");
    }

    #[test]
    fn as_harmonic_kind_and_from_harmonic_kind_are_inverse() {
        for kind in HarmonicScanKind::ALL {
            assert_eq!(HarmonicScanKind::from_harmonic_kind(kind.as_harmonic_kind()), Some(kind));
        }
        assert_eq!(HarmonicScanKind::from_harmonic_kind(HarmonicKind::Xabcd), None);
    }

    #[test]
    fn default_all_harmonic_kind_ids_covers_every_kind_once() {
        let ids = default_all_harmonic_kind_ids();
        assert_eq!(ids.len(), HarmonicScanKind::ALL.len());
        for kind in HarmonicScanKind::ALL {
            assert_eq!(ids.iter().filter(|id| id.as_str() == kind.id()).count(), 1);
        }
    }

    #[test]
    fn labels_never_panic_in_any_language() {
        for kind in HarmonicScanKind::ALL {
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
