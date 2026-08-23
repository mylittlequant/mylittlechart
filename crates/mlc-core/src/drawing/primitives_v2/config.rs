//! Primitive Configuration System
//!
//! Provides a unified way to configure primitives through:
//! - Inline toolbar (quick settings: color, width, style)
//! - Context menu (clone, delete, lock, visibility, layer order)
//! - Settings modal (full configuration with tabs: Style, Coordinates, Visibility)
//!
//! # Architecture
//!
//! Each primitive exposes its configurable properties through the `Primitive` trait's
//! `style_properties()` / `level_properties()` / `text_properties()` methods, applied
//! back via `DrawingManager::apply_property` (dispatched by `PropertyKind`).
//! Properties are described as `ConfigProperty` which includes:
//! - Property ID and display name
//! - Property type (color, number, boolean, select, levels, etc.)
//! - Current value
//! - Constraints (min/max, options, etc.)

use serde::{Deserialize, Serialize};
use crate::i18n::{
    Language, WaveDegreeKey, ConfigKey, LabelPositionKey, current_language,
    ConceptKey, PrimitiveNameKey, SettingsKey, ToolbarMenuKey, UserSettingsKey,
};

// =============================================================================
// Elliott Wave Degree and Notation
// =============================================================================

/// Elliott Wave degree - determines the notation style for wave labels
/// From largest (multi-century) to smallest (minutes)
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum WaveDegree {
    /// Multi-century timeframe
    Supermillennium,
    /// Multi-century timeframe
    Millennium,
    /// Multi-century timeframe
    Submillennium,
    /// Multi-decade (50+ years)
    GrandSupercycle,
    /// Multi-decade (years to decades)
    Supercycle,
    /// One to several years
    Cycle,
    /// Months to a year
    Primary,
    /// Weeks to months
    #[default]
    Intermediate,
    /// Weeks
    Minor,
    /// Days
    Minute,
    /// Hours
    Minuette,
    /// Minutes
    Subminuette,
    /// Sub-minute
    Micro,
    /// Sub-minute
    Submicro,
    /// Smallest observable
    Miniscule,
}

impl WaveDegree {
    /// Get impulse wave labels (1-5) for this degree
    pub fn impulse_labels(&self) -> [&'static str; 5] {
        match self {
            WaveDegree::Supermillennium => ["(((I)))", "(((II)))", "(((III)))", "(((IV)))", "(((V)))"],
            WaveDegree::Millennium => ["((I))", "((II))", "((III))", "((IV))", "((V))"],
            WaveDegree::Submillennium => ["(I)", "(II)", "(III)", "(IV)", "(V)"],
            WaveDegree::GrandSupercycle => ["I", "II", "III", "IV", "V"],  // Large Roman, no brackets
            WaveDegree::Supercycle => ["(I)", "(II)", "(III)", "(IV)", "(V)"],  // Circled Roman
            WaveDegree::Cycle => ["I", "II", "III", "IV", "V"],  // Roman numerals
            WaveDegree::Primary => ["①", "②", "③", "④", "⑤"],  // Circled numbers
            WaveDegree::Intermediate => ["(1)", "(2)", "(3)", "(4)", "(5)"],  // Parenthesized
            WaveDegree::Minor => ["1", "2", "3", "4", "5"],  // Plain numbers
            WaveDegree::Minute => ["i", "ii", "iii", "iv", "v"],  // Lowercase Roman
            WaveDegree::Minuette => ["(i)", "(ii)", "(iii)", "(iv)", "(v)"],  // Parenthesized lowercase
            WaveDegree::Subminuette => ["((i))", "((ii))", "((iii))", "((iv))", "((v))"],  // Double parenthesized
            WaveDegree::Micro => ["i", "ii", "iii", "iv", "v"],  // Tiny lowercase Roman
            WaveDegree::Submicro => ["(i)", "(ii)", "(iii)", "(iv)", "(v)"],
            WaveDegree::Miniscule => ["((i))", "((ii))", "((iii))", "((iv))", "((v))"],
        }
    }

    /// Get corrective wave labels (A-B-C) for this degree
    pub fn corrective_labels(&self) -> [&'static str; 3] {
        match self {
            WaveDegree::Supermillennium => ["(((A)))", "(((B)))", "(((C)))"],
            WaveDegree::Millennium => ["((A))", "((B))", "((C))"],
            WaveDegree::Submillennium => ["(A)", "(B)", "(C)"],
            WaveDegree::GrandSupercycle => ["A", "B", "C"],  // Large letters
            WaveDegree::Supercycle => ["(A)", "(B)", "(C)"],  // Circled
            WaveDegree::Cycle => ["A", "B", "C"],  // Plain uppercase
            WaveDegree::Primary => ["Ⓐ", "Ⓑ", "Ⓒ"],  // Circled letters
            WaveDegree::Intermediate => ["(A)", "(B)", "(C)"],  // Parenthesized
            WaveDegree::Minor => ["A", "B", "C"],  // Plain uppercase
            WaveDegree::Minute => ["a", "b", "c"],  // Lowercase
            WaveDegree::Minuette => ["(a)", "(b)", "(c)"],  // Parenthesized lowercase
            WaveDegree::Subminuette => ["((a))", "((b))", "((c))"],  // Double parenthesized
            WaveDegree::Micro => ["a", "b", "c"],
            WaveDegree::Submicro => ["(a)", "(b)", "(c)"],
            WaveDegree::Miniscule => ["((a))", "((b))", "((c))"],
        }
    }

    /// Get triangle wave labels (A-E) for this degree
    pub fn triangle_labels(&self) -> [&'static str; 5] {
        match self {
            WaveDegree::Supermillennium => ["(((A)))", "(((B)))", "(((C)))", "(((D)))", "(((E)))"],
            WaveDegree::Millennium => ["((A))", "((B))", "((C))", "((D))", "((E))"],
            WaveDegree::Submillennium => ["(A)", "(B)", "(C)", "(D)", "(E)"],
            WaveDegree::GrandSupercycle => ["A", "B", "C", "D", "E"],
            WaveDegree::Supercycle => ["(A)", "(B)", "(C)", "(D)", "(E)"],
            WaveDegree::Cycle => ["A", "B", "C", "D", "E"],
            WaveDegree::Primary => ["Ⓐ", "Ⓑ", "Ⓒ", "Ⓓ", "Ⓔ"],
            WaveDegree::Intermediate => ["(A)", "(B)", "(C)", "(D)", "(E)"],
            WaveDegree::Minor => ["A", "B", "C", "D", "E"],
            WaveDegree::Minute => ["a", "b", "c", "d", "e"],
            WaveDegree::Minuette => ["(a)", "(b)", "(c)", "(d)", "(e)"],
            WaveDegree::Subminuette => ["((a))", "((b))", "((c))", "((d))", "((e))"],
            WaveDegree::Micro => ["a", "b", "c", "d", "e"],
            WaveDegree::Submicro => ["(a)", "(b)", "(c)", "(d)", "(e)"],
            WaveDegree::Miniscule => ["((a))", "((b))", "((c))", "((d))", "((e))"],
        }
    }

    /// Get combo wave labels (W-X-Y or W-X-Y-X-Z) for this degree
    pub fn combo_labels(&self) -> [&'static str; 5] {
        match self {
            WaveDegree::Supermillennium => ["(((W)))", "(((X)))", "(((Y)))", "(((X)))", "(((Z)))"],
            WaveDegree::Millennium => ["((W))", "((X))", "((Y))", "((X))", "((Z))"],
            WaveDegree::Submillennium => ["(W)", "(X)", "(Y)", "(X)", "(Z)"],
            WaveDegree::GrandSupercycle => ["W", "X", "Y", "X", "Z"],
            WaveDegree::Supercycle => ["(W)", "(X)", "(Y)", "(X)", "(Z)"],
            WaveDegree::Cycle => ["W", "X", "Y", "X", "Z"],
            WaveDegree::Primary => ["Ⓦ", "Ⓧ", "Ⓨ", "Ⓧ", "Ⓩ"],
            WaveDegree::Intermediate => ["(W)", "(X)", "(Y)", "(X)", "(Z)"],
            WaveDegree::Minor => ["W", "X", "Y", "X", "Z"],
            WaveDegree::Minute => ["w", "x", "y", "x", "z"],
            WaveDegree::Minuette => ["(w)", "(x)", "(y)", "(x)", "(z)"],
            WaveDegree::Subminuette => ["((w))", "((x))", "((y))", "((x))", "((z))"],
            WaveDegree::Micro => ["w", "x", "y", "x", "z"],
            WaveDegree::Submicro => ["(w)", "(x)", "(y)", "(x)", "(z)"],
            WaveDegree::Miniscule => ["((w))", "((x))", "((y))", "((x))", "((z))"],
        }
    }

    /// Get display name for this degree using current language
    pub fn display_name(&self) -> &'static str {
        self.display_name_for(current_language())
    }

    /// Get display name for this degree in specified language
    pub fn display_name_for(&self, lang: Language) -> &'static str {
        self.i18n_key().get(lang)
    }

    /// Get i18n key for this degree
    pub fn i18n_key(&self) -> WaveDegreeKey {
        match self {
            WaveDegree::Supermillennium => WaveDegreeKey::Supermillennium,
            WaveDegree::Millennium => WaveDegreeKey::Millennium,
            WaveDegree::Submillennium => WaveDegreeKey::Submillennium,
            WaveDegree::GrandSupercycle => WaveDegreeKey::GrandSupercycle,
            WaveDegree::Supercycle => WaveDegreeKey::Supercycle,
            WaveDegree::Cycle => WaveDegreeKey::Cycle,
            WaveDegree::Primary => WaveDegreeKey::Primary,
            WaveDegree::Intermediate => WaveDegreeKey::Intermediate,
            WaveDegree::Minor => WaveDegreeKey::Minor,
            WaveDegree::Minute => WaveDegreeKey::Minute,
            WaveDegree::Minuette => WaveDegreeKey::Minuette,
            WaveDegree::Subminuette => WaveDegreeKey::Subminuette,
            WaveDegree::Micro => WaveDegreeKey::Micro,
            WaveDegree::Submicro => WaveDegreeKey::Submicro,
            WaveDegree::Miniscule => WaveDegreeKey::Miniscule,
        }
    }

    /// Get all degrees in order from largest to smallest
    pub fn all() -> &'static [WaveDegree] {
        &[
            WaveDegree::Supermillennium,
            WaveDegree::Millennium,
            WaveDegree::Submillennium,
            WaveDegree::GrandSupercycle,
            WaveDegree::Supercycle,
            WaveDegree::Cycle,
            WaveDegree::Primary,
            WaveDegree::Intermediate,
            WaveDegree::Minor,
            WaveDegree::Minute,
            WaveDegree::Minuette,
            WaveDegree::Subminuette,
            WaveDegree::Micro,
            WaveDegree::Submicro,
            WaveDegree::Miniscule,
        ]
    }

    /// Get serialization value
    pub fn as_str(&self) -> &'static str {
        match self {
            WaveDegree::Supermillennium => "supermillennium",
            WaveDegree::Millennium => "millennium",
            WaveDegree::Submillennium => "submillennium",
            WaveDegree::GrandSupercycle => "grand_supercycle",
            WaveDegree::Supercycle => "supercycle",
            WaveDegree::Cycle => "cycle",
            WaveDegree::Primary => "primary",
            WaveDegree::Intermediate => "intermediate",
            WaveDegree::Minor => "minor",
            WaveDegree::Minute => "minute",
            WaveDegree::Minuette => "minuette",
            WaveDegree::Subminuette => "subminuette",
            WaveDegree::Micro => "micro",
            WaveDegree::Submicro => "submicro",
            WaveDegree::Miniscule => "miniscule",
        }
    }

    /// Parse from string
    pub fn from_str(s: &str) -> Option<Self> {
        match s {
            "supermillennium" => Some(WaveDegree::Supermillennium),
            "millennium" => Some(WaveDegree::Millennium),
            "submillennium" => Some(WaveDegree::Submillennium),
            "grand_supercycle" => Some(WaveDegree::GrandSupercycle),
            "supercycle" => Some(WaveDegree::Supercycle),
            "cycle" => Some(WaveDegree::Cycle),
            "primary" => Some(WaveDegree::Primary),
            "intermediate" => Some(WaveDegree::Intermediate),
            "minor" => Some(WaveDegree::Minor),
            "minute" => Some(WaveDegree::Minute),
            "minuette" => Some(WaveDegree::Minuette),
            "subminuette" => Some(WaveDegree::Subminuette),
            "micro" => Some(WaveDegree::Micro),
            "submicro" => Some(WaveDegree::Submicro),
            "miniscule" => Some(WaveDegree::Miniscule),
            _ => None,
        }
    }

    /// One step FINER (smaller) along the 15-degree ladder, e.g. `Primary`
    /// -> `Intermediate`. Saturates at [`WaveDegree::Miniscule`], the finest
    /// degree — never wraps and never panics.
    ///
    /// Used by the Elliott markup tree (`elliott::count::child_seed`) to
    /// compute the degree a NESTED child wave must carry: one step finer
    /// than its parent's own degree, per [`Self::all`]'s canonical
    /// coarsest-to-finest ordering.
    pub fn finer(&self) -> WaveDegree {
        let all = Self::all();
        let idx = all.iter().position(|d| d == self).unwrap_or(0);
        all[(idx + 1).min(all.len() - 1)]
    }

    /// One step COARSER (larger) along the 15-degree ladder, e.g.
    /// `Intermediate` -> `Primary`. Saturates at
    /// [`WaveDegree::Supermillennium`], the coarsest degree — never wraps
    /// and never panics.
    pub fn coarser(&self) -> WaveDegree {
        let all = Self::all();
        let idx = all.iter().position(|d| d == self).unwrap_or(0);
        all[idx.saturating_sub(1)]
    }
}

#[cfg(test)]
mod wave_degree_tests {
    use super::*;

    #[test]
    fn finer_steps_toward_miniscule() {
        assert_eq!(WaveDegree::Primary.finer(), WaveDegree::Intermediate);
        assert_eq!(WaveDegree::Intermediate.finer(), WaveDegree::Minor);
    }

    #[test]
    fn finer_saturates_at_miniscule() {
        assert_eq!(WaveDegree::Miniscule.finer(), WaveDegree::Miniscule);
    }

    #[test]
    fn coarser_steps_toward_supermillennium() {
        assert_eq!(WaveDegree::Intermediate.coarser(), WaveDegree::Primary);
        assert_eq!(WaveDegree::Primary.coarser(), WaveDegree::Cycle);
    }

    #[test]
    fn coarser_saturates_at_supermillennium() {
        assert_eq!(WaveDegree::Supermillennium.coarser(), WaveDegree::Supermillennium);
    }
}

// =============================================================================
// Wave Notation Style
// =============================================================================

/// A wave structure's own label NUMERAL FAMILY, independent of
/// [`WaveDegree`] — the design's own `wave_tabs()` Notation row (owner ask
/// 2026-08-16: "the analyst cannot choose arabic where the degree dictates
/// roman"). `FollowDegree` (the default) is a REAL variant, not a stand-in
/// for one of the other three: it defers entirely to the structure's own
/// `WaveDegree::impulse_labels()` table, so a drawing saved before this
/// field existed loads and renders byte-for-byte unchanged. The other
/// three replace that table with ONE FIXED family — see
/// [`Self::positional_table`] — regardless of degree.
///
/// Elliott-Impulse ONLY (owner correction 2026-08-17): the four
/// letter-labelled types (correction A-B-C, triangle A-E, double/triple
/// combo W-X-Y-Z) do NOT carry this field — see
/// `ElliottImpulse::notation_style`'s own field doc for why offering
/// "arabic" there would be a correctness defect, not a layout one.
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum NotationStyle {
    #[default]
    FollowDegree,
    Arabic,
    Roman,
    Circled,
}

impl NotationStyle {
    /// Wire value for [`PropertyValue::String`] round-trips — the same
    /// convention `WaveDegree::as_str`/`from_str` already use.
    pub fn as_str(&self) -> &'static str {
        match self {
            NotationStyle::FollowDegree => "follow_degree",
            NotationStyle::Arabic => "arabic",
            NotationStyle::Roman => "roman",
            NotationStyle::Circled => "circled",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "arabic" => NotationStyle::Arabic,
            "roman" => NotationStyle::Roman,
            "circled" => NotationStyle::Circled,
            _ => NotationStyle::FollowDegree,
        }
    }

    /// The five wave-count labels this family draws (1st wave, 2nd wave,
    /// ...) — the exact same glyphs the design's own pills row shows
    /// literally (`1 2 3 4 5` / `I II III IV V` / `① ② ③ ④ ⑤`), and the
    /// same numbers `ElliottImpulse`'s own five waves are always counted
    /// with. `None` for `FollowDegree`, which has no fixed table of its
    /// own — `ElliottImpulse::wave_labels` matches the variant first and
    /// only reaches this for the other three, so a `FollowDegree` impulse
    /// keeps reading its own `WaveDegree` table exactly as before this
    /// field existed.
    pub fn positional_table(&self) -> Option<[&'static str; 5]> {
        match self {
            NotationStyle::FollowDegree => None,
            NotationStyle::Arabic => Some(["1", "2", "3", "4", "5"]),
            NotationStyle::Roman => Some(["I", "II", "III", "IV", "V"]),
            NotationStyle::Circled => Some(["①", "②", "③", "④", "⑤"]),
        }
    }

    /// Which fixed family a `WaveDegree`'s own DRAWN table currently
    /// belongs to — resolves `FollowDegree`'s EFFECTIVE notation for the
    /// pills row's own highlighted segment (owner correction 2026-08-17: a
    /// freshly placed structure defaults to `FollowDegree` and previously
    /// lit no pill at all, though the chart is visibly showing one of the
    /// three families right now). Never used to change what
    /// `wave_labels()` computes — declaration-time display only.
    ///
    /// A degree's own table wraps the FAMILY in decoration — brackets and
    /// case — that is not part of the family itself: `Intermediate`'s `(1)`
    /// is still arabic, `Minute`'s `i` is still (lowercase) roman,
    /// `Supermillennium`'s `(((I)))` is still roman. `undecorated` strips
    /// surrounding `()`/`[]` and whitespace and folds case before
    /// comparing against ARABIC/ROMAN/CIRCLED (sliced to `labels.len()`),
    /// so the comparison is against the family, not its dressing — every
    /// one of `WaveDegree::impulse_labels()`'s 15 tables resolves to
    /// exactly one of the three (verified by
    /// `elliott_impulse`'s own `notation_style_label_tests`).
    ///
    /// `None` only for a table this normalization still can't place —
    /// today that never happens for `impulse_labels()`, the only caller
    /// (letter-labelled tables — corrective/triangle/combo — are never
    /// passed here; see `ElliottImpulse::notation_style`'s own field doc
    /// for why the row lives on that type alone).
    pub fn family_of(labels: &[&str]) -> Option<Self> {
        const ARABIC: [&str; 5] = ["1", "2", "3", "4", "5"];
        const ROMAN: [&str; 5] = ["I", "II", "III", "IV", "V"];
        const CIRCLED: [&str; 5] = ["①", "②", "③", "④", "⑤"];

        // Strip the decoration a WaveDegree table wraps its own family in
        // (surrounding parens/brackets, whitespace) and fold case — leaves
        // the family itself (`I`, `1`, `①`) bare for comparison.
        fn undecorated(label: &str) -> String {
            label
                .chars()
                .filter(|c| !matches!(c, '(' | ')' | '[' | ']') && !c.is_whitespace())
                .flat_map(char::to_uppercase)
                .collect()
        }

        let n = labels.len().min(5);
        let stripped: Vec<String> = labels[..n].iter().map(|l| undecorated(l)).collect();
        let matches_family = |family: &[&str; 5]| stripped.iter().map(String::as_str).eq(family[..n].iter().copied());

        if matches_family(&ARABIC) {
            Some(NotationStyle::Arabic)
        } else if matches_family(&ROMAN) {
            Some(NotationStyle::Roman)
        } else if matches_family(&CIRCLED) {
            Some(NotationStyle::Circled)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod notation_style_tests {
    use super::*;

    #[test]
    fn follow_degree_is_the_default_and_has_no_positional_table() {
        assert_eq!(NotationStyle::default(), NotationStyle::FollowDegree);
        assert_eq!(NotationStyle::FollowDegree.positional_table(), None);
    }

    #[test]
    fn wire_round_trip_for_every_variant() {
        for style in [NotationStyle::FollowDegree, NotationStyle::Arabic, NotationStyle::Roman, NotationStyle::Circled] {
            assert_eq!(NotationStyle::from_str(style.as_str()), style);
        }
    }

    #[test]
    fn family_of_matches_through_decoration() {
        assert_eq!(NotationStyle::family_of(&["1", "2", "3", "4", "5"]), Some(NotationStyle::Arabic), "bare arabic");
        assert_eq!(NotationStyle::family_of(&["(1)", "(2)", "(3)", "(4)", "(5)"]), Some(NotationStyle::Arabic), "parenthesized arabic — Intermediate's own table");
        assert_eq!(NotationStyle::family_of(&["I", "II", "III", "IV", "V"]), Some(NotationStyle::Roman), "bare roman — GrandSupercycle/Cycle's own table");
        assert_eq!(NotationStyle::family_of(&["i", "ii", "iii", "iv", "v"]), Some(NotationStyle::Roman), "lowercase roman — Minute/Micro's own table");
        assert_eq!(NotationStyle::family_of(&["(((I)))", "(((II)))", "(((III)))", "(((IV)))", "(((V)))"]), Some(NotationStyle::Roman), "triple-parenthesized roman — Supermillennium's own table");
        assert_eq!(NotationStyle::family_of(&["①", "②", "③"]), Some(NotationStyle::Circled), "a shorter table matches the family's own prefix");
        assert_eq!(NotationStyle::family_of(&["A", "B", "C"]), None, "letters never equal a digit family, decorated or not");
    }

    /// Owner correction 2026-08-17: after normalization, every one of
    /// `WaveDegree::impulse_labels()`'s 15 degree tables resolves to
    /// exactly one of the three fixed families — none are left dressed
    /// too heavily to place. Named per-degree checks for a representative
    /// few (the plain, the heavily bracketed, the lowercase, the circled)
    /// plus the exhaustive sweep so a future 16th degree with an
    /// unrecognized dressing fails loudly here instead of silently
    /// reading as "no pill lit".
    #[test]
    fn every_impulse_degree_resolves_to_a_family() {
        assert_eq!(NotationStyle::family_of(&WaveDegree::Minor.impulse_labels()), Some(NotationStyle::Arabic));
        assert_eq!(NotationStyle::family_of(&WaveDegree::Intermediate.impulse_labels()), Some(NotationStyle::Arabic));
        assert_eq!(NotationStyle::family_of(&WaveDegree::Cycle.impulse_labels()), Some(NotationStyle::Roman));
        assert_eq!(NotationStyle::family_of(&WaveDegree::GrandSupercycle.impulse_labels()), Some(NotationStyle::Roman));
        assert_eq!(NotationStyle::family_of(&WaveDegree::Minute.impulse_labels()), Some(NotationStyle::Roman), "lowercase roman is still roman");
        assert_eq!(NotationStyle::family_of(&WaveDegree::Supermillennium.impulse_labels()), Some(NotationStyle::Roman), "triple-parenthesized is still roman");
        assert_eq!(NotationStyle::family_of(&WaveDegree::Primary.impulse_labels()), Some(NotationStyle::Circled));

        for degree in WaveDegree::all() {
            let resolved = NotationStyle::family_of(&degree.impulse_labels());
            assert!(resolved.is_some(), "{degree:?}'s own impulse table {:?} resolved to None", degree.impulse_labels());
        }
    }
}

// =============================================================================
// Position Display Unit
// =============================================================================

/// The analyst's chosen unit for every dollar/percent RESULT a position
/// primitive shows (owner-facing gap: `show_pnl` already gates whether a
/// figure is computed, nothing said in WHICH unit). `Currency` (the
/// default) reproduces exactly what every one of those figures showed
/// before this field existed — plain [`super::fmt_price`] amounts,
/// unchanged.
///
/// No `Ticks` variant: [`super::RenderContext`] carries chart geometry
/// only (bar/price coordinate conversion) — no instrument metadata, so no
/// tick size is available to a primitive at render time. Fabricating one
/// would be a made-up number wearing a real unit's name, not an
/// implementation of it — the design's third pill option is left out
/// rather than faked (see `long_position.rs`'s own `style_properties()`
/// doc for the report).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PositionDisplayUnit {
    #[default]
    Currency,
    Percent,
}

impl PositionDisplayUnit {
    pub fn as_str(&self) -> &'static str {
        match self {
            PositionDisplayUnit::Currency => "currency",
            PositionDisplayUnit::Percent => "percent",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "percent" => PositionDisplayUnit::Percent,
            _ => PositionDisplayUnit::Currency,
        }
    }
}

// =============================================================================
// Label Style Configuration
// =============================================================================

/// Style configuration for wave/pattern labels
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct LabelStyle {
    /// Font size in pixels
    #[serde(default = "default_label_font_size")]
    pub font_size: f64,
    /// Font weight: normal or bold
    #[serde(default = "default_font_weight")]
    pub font_weight: String,
    /// Text color (if None, uses primitive stroke color)
    pub color: Option<String>,
    /// Background color for label (if None, no background). Defaults to a
    /// dark translucent plate — labels drawn straight onto candles with no
    /// backing were the single biggest readability defect in this arc; see
    /// `nemo/mylittlechart` label-plumbing pass. `#[serde(default = ...)]`
    /// (rather than only setting it in `Default for LabelStyle`) so JSON
    /// that genuinely lacks the key — pre-dating this field's existence —
    /// also resolves to a plate. Saved drawings that already serialized an
    /// explicit `null` here keep reading back as no background; only a
    /// fresh recreate of the primitive picks up the new default for those.
    #[serde(default = "default_label_background_color")]
    pub background_color: Option<String>,
    /// Background padding in pixels
    #[serde(default = "default_label_padding")]
    pub background_padding: f64,
    /// Background corner radius
    #[serde(default = "default_label_radius")]
    pub background_radius: f64,
    /// Whether the plate draws a border at all. A separate flag, NOT
    /// `border_color.is_some()` (2026-08-09): the UI row for the colour
    /// shows the RESOLVED value (falling back to the label colour when
    /// unset), so a template — which captures declared property values —
    /// wrote that resolved colour back on apply and silently turned the
    /// border ON for every primitive it touched. That is what put a
    /// coloured ring around every fib label and what kept "Save as…" lit
    /// after applying a system template: the applied state could never
    /// equal the template it came from. An `Option` that doubles as an
    /// on/off switch cannot round-trip through a UI that has to render
    /// SOMETHING when it is `None` — so the switch is its own field now.
    /// `#[serde(default)]` = false: drawings carrying a `border_color` from
    /// before this split load with the border off, which is the state their
    /// owner actually chose (nobody ever enabled it deliberately).
    #[serde(default)]
    pub border_enabled: bool,
    /// Border colour. `None` means "use the label's own colour" — a colour
    /// only, never an on/off switch (see [`Self::border_enabled`]).
    pub border_color: Option<String>,
    /// Border width
    #[serde(default = "default_border_width")]
    pub border_width: f64,
    /// Vertical offset from point (negative = above)
    #[serde(default = "default_label_offset")]
    pub offset_y: f64,
}

fn default_label_font_size() -> f64 { 12.0 }
fn default_font_weight() -> String { "normal".to_string() }
fn default_label_background_color() -> Option<String> { Some("#0d0f14cc".to_string()) }
fn default_label_padding() -> f64 { 4.0 }
fn default_label_radius() -> f64 { 3.0 }
fn default_border_width() -> f64 { 1.0 }
fn default_label_offset() -> f64 { 15.0 }

impl Default for LabelStyle {
    fn default() -> Self {
        Self {
            font_size: 12.0,
            font_weight: "normal".to_string(),
            color: None,
            background_color: default_label_background_color(),
            background_padding: 4.0,
            background_radius: 3.0,
            border_enabled: false,
            border_color: None,
            border_width: 1.0,
            offset_y: 15.0,
        }
    }
}

impl LabelStyle {
    /// Create a simple label style with just text color
    pub fn simple(color: Option<&str>) -> Self {
        Self {
            color: color.map(|c| c.to_string()),
            ..Default::default()
        }
    }

    /// Create a label with background
    pub fn with_background(bg_color: &str, text_color: Option<&str>) -> Self {
        Self {
            color: text_color.map(|c| c.to_string()),
            background_color: Some(bg_color.to_string()),
            ..Default::default()
        }
    }

    /// Create a circled/bordered label style (for circled numbers like ①②③)
    pub fn circled(border_color: &str) -> Self {
        Self {
            border_enabled: true,
            border_color: Some(border_color.to_string()),
            border_width: 1.5,
            background_radius: 10.0,
            background_padding: 2.0,
            ..Default::default()
        }
    }

    /// Get CSS-like font string
    pub fn font_string(&self) -> String {
        if self.font_weight == "bold" {
            format!("bold {}px sans-serif", self.font_size)
        } else {
            format!("{}px sans-serif", self.font_size)
        }
    }

    /// Config properties for the parts of the struct beyond `font_size`/
    /// `color` (those two predate this method — every call site already
    /// wires them at fixed orders 13/14 via `ConfigProperty::label_font_size`/
    /// `label_color`). This appends the "inherit line colour" toggle FIRST,
    /// at `order_start` (see [`Self::apply_property`]'s own doc for why it
    /// must be applied after each caller's own `"label_color"` write — this
    /// row's position immediately after that row, whatever order the caller
    /// used for it, is what makes that ordering hold), then the remaining 6
    /// fields — bold, offset, background on/off + color, padding, corner
    /// radius, border on/off + color + width — at `order_start + 1 ..=
    /// order_start + 9`. Callers `.extend()` this into their own
    /// `style_properties()`.
    pub fn style_properties(&self, fallback_color: &str, order_start: i32) -> Vec<ConfigProperty> {
        vec![
            ConfigProperty::boolean("label_color_inherit", ConfigKey::LabelColorInherit.get(current_language()), self.color.is_none())
                .with_category(PropertyCategory::LabelsText).with_order(order_start),
            ConfigProperty::boolean("label_bold", ConfigKey::Bold.get(current_language()), self.font_weight == "bold")
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 1),
            ConfigProperty::number("label_offset", ConfigKey::LabelOffset.get(current_language()), self.offset_y, Some(0.0), Some(60.0))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 2),
            ConfigProperty::boolean("label_background", ConfigKey::ShowBackground.get(current_language()), self.background_color.is_some())
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 3),
            ConfigProperty::color("label_background_color", ConfigKey::LabelBackgroundColor.get(current_language()), self.background_color.as_deref().unwrap_or(fallback_color))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 4),
            ConfigProperty::number("label_padding", ConfigKey::LabelPadding.get(current_language()), self.background_padding, Some(0.0), Some(20.0))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 5),
            ConfigProperty::number("label_radius", ConfigKey::LabelRadius.get(current_language()), self.background_radius, Some(0.0), Some(20.0))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 6),
            ConfigProperty::boolean("label_border", ConfigKey::LabelBorder.get(current_language()), self.border_enabled)
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 7),
            ConfigProperty::color("label_border_color", ConfigKey::LabelBorderColor.get(current_language()), self.border_color.as_deref().unwrap_or(fallback_color))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 8),
            ConfigProperty::number("label_border_width", ConfigKey::LabelBorderWidth.get(current_language()), self.border_width, Some(0.5), Some(6.0))
                .with_category(PropertyCategory::LabelsText).with_order(order_start + 9),
        ]
    }

    /// Same rows as [`Self::style_properties`], with the W2 group model
    /// applied (`docs/mlc/plans/archetype-modals-into-the-app-2026-08-11.md`):
    /// the colour-inherit/bold/offset trio joins `label_group` — each
    /// archetype's own "level/point/wave/… label" group, chosen by the
    /// caller (`LevelLabels` for the ladder archetypes, `WaveLabels` for
    /// Elliott waves, `PointLabels` for harmonic patterns, `PositionLabels`
    /// for long/short positions, `TextGroup` for measurement rulers) — and
    /// the background/rounding/padding quartet joins `PropertyGroup::Plate`,
    /// the SAME group title every archetype's own Labels-tab background
    /// section uses in the approved design (`ConceptKey::Plate`). The
    /// trailing border trio is left ungrouped: the approved design carries
    /// no border row in ANY Plate group for the callers below, so there is
    /// no group title to give it yet (W3/W4).
    pub fn style_properties_grouped(&self, fallback_color: &str, order_start: i32, label_group: PropertyGroup) -> Vec<ConfigProperty> {
        let mut props = self.style_properties(fallback_color, order_start);
        for p in &mut props {
            match p.id.as_str() {
                "label_color_inherit" | "label_bold" | "label_offset" => p.group = Some(label_group),
                // The border rows belong to the plate they outline, not to a
                // headerless run above it — the approved design draws one
                // "Плашка" group holding background, colour, radius, padding
                // and the border that frames them.
                "label_background" | "label_background_color" | "label_padding" | "label_radius"
                | "label_border" | "label_border_color" | "label_border_width" => {
                    p.group = Some(PropertyGroup::Plate);
                }
                _ => {}
            }
        }
        props
    }

    /// Apply a property produced by `style_properties()`. Returns `true` if
    /// `id` was recognized. `font_size`/`color` are NOT handled here — they
    /// stay on the per-primitive `"label_font_size"`/`"label_color"` ids
    /// callers already had; this only owns the ids it mints above.
    ///
    /// `fallback_color` is the primitive's own stroke colour
    /// (`self.data.color.stroke` at every call site) — turning inherit OFF
    /// with no colour chosen yet has nowhere else to source a concrete
    /// value from, and leaving `self.color` at `None` would make the
    /// checkbox look like it did nothing. Turning inherit back ON always
    /// writes `None`, regardless of what colour was pinned before —
    /// `self.color.is_none()` is the checkbox's own read
    /// ([`Self::style_properties`]'s `"label_color_inherit"` row), so this
    /// is the only write that can ever uncheck it again.
    pub fn apply_property(&mut self, id: &str, value: &PropertyValue, fallback_color: &str) -> bool {
        match id {
            "label_color_inherit" => {
                if let Some(v) = value.as_bool() {
                    self.color = if v { None } else { Some(self.color.clone().unwrap_or_else(|| fallback_color.to_string())) };
                    return true;
                }
            }
            "label_bold" => {
                if let Some(v) = value.as_bool() {
                    self.font_weight = if v { "bold".to_string() } else { "normal".to_string() };
                    return true;
                }
            }
            "label_offset" => {
                if let Some(v) = value.as_number() { self.offset_y = v; return true; }
            }
            "label_background" => {
                if let Some(v) = value.as_bool() {
                    self.background_color = if v {
                        Some(self.background_color.clone().unwrap_or_else(|| default_label_background_color().unwrap_or_default()))
                    } else {
                        None
                    };
                    return true;
                }
            }
            "label_background_color" => {
                if let Some(c) = value.as_color() { self.background_color = Some(c.to_string()); return true; }
            }
            "label_padding" => {
                if let Some(v) = value.as_number() { self.background_padding = v.max(0.0); return true; }
            }
            "label_radius" => {
                if let Some(v) = value.as_number() { self.background_radius = v.max(0.0); return true; }
            }
            "label_border" => {
                if let Some(v) = value.as_bool() { self.border_enabled = v; return true; }
            }
            // Sets the COLOUR only. Turning the border on is
            // `label_border`'s job — writing this row must never enable it,
            // or applying any template re-enables the border on every
            // primitive it touches (see `border_enabled`'s own doc).
            "label_border_color" => {
                if let Some(c) = value.as_color() { self.border_color = Some(c.to_string()); return true; }
            }
            "label_border_width" => {
                if let Some(v) = value.as_number() { self.border_width = v.max(0.1); return true; }
            }
            _ => {}
        }
        false
    }
}

/// Property writes a system набор (level-set preset) performs on every
/// primitive's `LabelStyle` when applied — the label-plate half of "шаблоны
/// это настройки в целом" (owner direction, 2026-08-09): a preset restores
/// the label plate to the product default and re-establishes label colour
/// INHERITANCE (`label_color_inherit = true`, i.e. `LabelStyle.color =
/// None` — "use the line's own colour"), not just the level value list.
///
/// Ids are exactly the ones [`LabelStyle::apply_property`] already owns —
/// this mints no new ones — and the numbers are read from the same
/// `default_label_*` functions `LabelStyle::default()` itself uses, so
/// there is one source for them, not a second copy. Does not touch
/// `LabelStyle::default()` or any `default_label_*` value.
pub fn level_set_label_defaults() -> Vec<(&'static str, PropertyValue)> {
    vec![
        ("label_color_inherit", PropertyValue::Boolean(true)),
        ("label_bold", PropertyValue::Boolean(false)),
        ("label_padding", PropertyValue::Number(default_label_padding())),
        ("label_radius", PropertyValue::Number(default_label_radius())),
        ("label_background", PropertyValue::Boolean(true)),
    ]
}

// =============================================================================
// Property Types
// =============================================================================

/// Type of configuration property
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PropertyType {
    /// Color picker (hex string)
    Color,
    /// Numeric value with optional range
    Number {
        min: Option<f64>,
        max: Option<f64>,
        step: Option<f64>,
    },
    /// Integer value with optional range
    Integer {
        min: Option<i32>,
        max: Option<i32>,
    },
    /// Boolean toggle
    Boolean,
    /// Select from predefined options
    Select {
        options: Vec<SelectOption>,
    },
    /// Segmented single-choice control — same closed option list a
    /// `Select` carries (reuses [`SelectOption`], never a parallel shape;
    /// see [`ConfigProperty::pills`]'s own doc) rendered as adjacent
    /// buttons showing every option at once instead of a dropdown hiding
    /// all but one. Wire shape is identical to `Select` — a
    /// `PropertyValue::String` naming one of `options`' own values — so
    /// applying a click on any segment reaches the exact same
    /// `apply_{style,text,level}_property` path an existing `Select` row
    /// already does; only the presentation differs. A row whose own
    /// segments would not fit the modal's current width degrades to
    /// `Select`'s own dropdown control rather than overflow — see
    /// `mlc-app-layout`'s `primitive_settings_property::draw_property_row`'s
    /// `Pills` arm and `primitive_settings_intent::pills_fits`.
    Pills {
        options: Vec<SelectOption>,
    },
    /// Glyph picker (W4) — a closed catalogue of drawable icons, shown as a
    /// grid of clickable cells rather than a text option list (that shape
    /// stays `Select`/`Pills`, both of which carry [`SelectOption`], text
    /// only). Wire shape mirrors `Pills`: a `PropertyValue::String` naming
    /// one `PickerOption::id`, applied through the exact same
    /// `apply_{style,text,level}_property` path — only the option SHAPE
    /// (a glyph, not a label) and the rendered control (a wrapping grid,
    /// not a row of segments) differ. The primitive hands over its OWN
    /// catalogue's entries (e.g. `EmojiType::all()`, `SignType::all()`) —
    /// this type never invents or narrows a catalogue of its own; see
    /// [`ConfigProperty::picker`]'s own doc. `mlc-app-layout`'s
    /// `primitive_settings_property::draw_property_row`'s `Picker` arm
    /// draws it; `primitive_settings_intent::icon_picker_grid` is the pure
    /// cells/columns decision.
    Picker {
        options: Vec<PickerOption>,
    },
    /// Read-only computed-result BLOCK (W4) — the design's own "Result"
    /// group (Position's Risk/Target/R:R, `mlc-ui-lab`'s `Row::Stat`/
    /// `stat()` row constructor) collapsed into ONE declared property
    /// carrying every row it shows, instead of one
    /// `ConfigProperty::text(...).readonly()` per row (pre-W4). Each
    /// [`StatRow`] is a pre-formatted label/value pair plus an optional
    /// [`Tone`] — the value is computed and formatted entirely by the
    /// declaring primitive, through the SAME function its own on-chart
    /// label already calls (see each declarer's own `style_properties()`
    /// doc); this type never computes anything itself, only carries what
    /// it is given.
    ///
    /// The ONE composite kind in this modal that is DISPLAY ONLY, and its
    /// own arm is what makes that true STRUCTURALLY rather than by
    /// convention: `mlc-app-layout`'s
    /// `primitive_settings_property::draw_property_row`'s `Stat` arm never
    /// pushes into `result.content_items` — there is no click id a UI
    /// click could ever produce for a stat row, regardless of whether the
    /// declaring `ConfigProperty` also happens to carry `readonly: true`
    /// (every constructor here does, for the pre-existing
    /// `!prop.readonly` filters `PrimitiveTemplate::from_primitive` /
    /// `DrawingManager::save_last_style_for_primitive` already apply to
    /// every readonly row — a stat block benefits from those for free —
    /// see [`ConfigProperty::stat_block`]'s own doc; but the render
    /// layer's own non-interactivity does not depend on that flag).
    /// [`PropertyValue::Stat`] mirrors this on the write side: it matches
    /// none of `as_string`/`as_bool`/`as_number`/`as_color`, so even a
    /// caller reaching `apply_style_property` directly (bypassing the
    /// render layer entirely) has nothing here it could write through the
    /// usual accessor pattern every implementation uses.
    Stat {
        rows: Vec<StatRow>,
    },
    /// A grid property (W4) — the design's own settings-row `table(...)` /
    /// `Row::Table` (a header of column captions, N rows of typed cells)
    /// turned into a declared property. Unlike every other composite kind
    /// in this modal (`Pills`/`Picker`/`Stat`) this ONE is genuinely
    /// EDITABLE: some cells display, some cells write back — see
    /// [`TableCell`]'s own doc for how a cell decides which it is. The
    /// declaring primitive supplies the cells ALREADY FORMATTED (a
    /// pre-computed ratio, a hex colour, a bool) — this type never
    /// interprets what a cell MEANS, the same discipline `Stat` already
    /// follows for its own rows.
    ///
    /// A cell's own click id is `{field_id}::cell:{row}:{col}` — `row`/
    /// `col` are DATA-row/column indices into `rows` (the header never
    /// registers a click id) — the SAME `::`-separated shape a `Pills`/
    /// `Picker` row's own option id already uses
    /// ([`ConfigProperty::pills`]'s own doc), extended with a row AND a
    /// column instead of a single option value. `mlc-app-layout`'s
    /// `primitive_settings_intent::table_grid` is the ONE place this id is
    /// BUILT; [`parse_table_cell_id`], below, is the ONE place it is
    /// parsed back apart — every declaring primitive's own
    /// `apply_{style,text,level}_property` calls that function instead of
    /// re-splitting the id itself.
    ///
    /// [`ConfigProperty::table`] marks the WHOLE property `readonly` —
    /// same reasoning as [`Self::Stat`]'s own doc: this composite shape has
    /// nothing a template/last-used-style snapshot could usefully reapply
    /// as ONE value, so it is excluded from that capture entirely. An
    /// individual cell's own editability is carried by its OWN
    /// [`TableCell`] variant, completely independent of this flag — see
    /// `mlc-app-layout`'s `primitive_settings_property::draw_property_row`'s
    /// own `Table` arm, checked BEFORE the generic `readonly` branch,
    /// exactly like `Stat`'s.
    Table {
        head: Vec<String>,
        rows: Vec<Vec<TableCell>>,
    },
    /// Line style selector
    LineStyle,
    /// Text input
    Text {
        multiline: bool,
        max_length: Option<usize>,
    },
    /// Fibonacci levels (list of level configs)
    FibLevels,
    /// Coordinate (bar, price)
    Coordinate,
    /// Timeframe visibility settings
    TimeframeVisibility,
}

/// Option for Select property type
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct SelectOption {
    pub value: String,
    pub label: String,
}

impl SelectOption {
    pub fn new(value: &str, label: &str) -> Self {
        Self {
            value: value.to_string(),
            label: label.to_string(),
        }
    }

    /// Create select options for label positions (left, center, right)
    pub fn label_positions() -> Vec<Self> {
        let lang = current_language();
        vec![
            Self::new("left", LabelPositionKey::Left.get(lang)),
            Self::new("center", LabelPositionKey::Center.get(lang)),
            Self::new("right", LabelPositionKey::Right.get(lang)),
        ]
    }

    /// Create select options for vertical positions (top, center, bottom)
    pub fn vertical_positions() -> Vec<Self> {
        let lang = current_language();
        vec![
            Self::new("top", LabelPositionKey::Top.get(lang)),
            Self::new("center", LabelPositionKey::Center.get(lang)),
            Self::new("bottom", LabelPositionKey::Bottom.get(lang)),
        ]
    }
}

/// One entry in a `Picker` property's own catalogue — the glyph-picker
/// counterpart of [`SelectOption`]: same `id`/`label` shape, plus the
/// drawable SVG the grid paints in place of text. Built directly from the
/// owning catalogue type's own `id()`/`label()`/`svg()` (e.g.
/// `EmojiType::all().iter().map(|t| PickerOption::new(t.id(), t.label(), t.svg()))`,
/// see each declaring `style_properties()` for the exact call) — this
/// struct never carries a catalogue of its own, only ferries one type's
/// entries into the shape [`PropertyType::Picker`] needs.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct PickerOption {
    pub id: String,
    pub label: String,
    pub svg: String,
}

impl PickerOption {
    pub fn new(id: &str, label: &str, svg: &str) -> Self {
        Self { id: id.to_string(), label: label.to_string(), svg: svg.to_string() }
    }
}

/// A [`StatRow`]'s own semantic colouring — the approved design's own `cls`
/// (`mlc-ui-lab`'s `concept::spec::Tone`), rebuilt here because `mlc-core`
/// cannot depend on the lab/design crate (crate contract: types depend on
/// nothing above them). `Neutral` is an explicit "no colour" choice, kept
/// distinct from [`StatRow::tone`] being `None` outright — a row can
/// declare "neutral, on purpose" without that reading as an omission; both
/// render identically (the renderer's own `Stat` arm).
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum Tone {
    Good,
    Bad,
    Neutral,
}

/// One line of a [`PropertyType::Stat`] block. `label` and `value` are both
/// already-formatted display strings — see [`PropertyType::Stat`]'s own doc
/// for why this type never computes either. `tone` colours the value only
/// (never the label) — see the renderer's own `Stat` arm.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StatRow {
    pub label: String,
    pub value: String,
    pub tone: Option<Tone>,
}

impl StatRow {
    pub fn new(label: &str, value: &str, tone: Option<Tone>) -> Self {
        Self { label: label.to_string(), value: value.to_string(), tone }
    }

    /// The measurement family's own price-change row (`ConceptKey::PriceChange`)
    /// — shared by `PriceRange`/`PriceDateRange`. No tone: a ruler reports a
    /// distance, it does not judge direction.
    pub fn price_change(value: &str) -> Self {
        Self::new(ConceptKey::PriceChange.get(current_language()), value, None)
    }

    /// The measurement family's own percentage-move row (`ConceptKey::PercentUnit`)
    /// — shared by `PriceRange`/`PriceDateRange`.
    pub fn percent_change(value: &str) -> Self {
        Self::new(ConceptKey::PercentUnit.get(current_language()), value, None)
    }

    /// The measurement family's own elapsed-time row (`ConceptKey::Duration`)
    /// — shared by `DateRange`/`PriceDateRange`.
    pub fn duration(value: &str) -> Self {
        Self::new(ConceptKey::Duration.get(current_language()), value, None)
    }

    /// The position archetype's own "Risk" row (`ConfigKey::PositionRiskLabel`)
    /// — shared by `LongPosition`/`ShortPosition`. Tone `Bad`: a risk
    /// amount is a loss if it materializes.
    pub fn position_risk(value: &str) -> Self {
        Self::new(ConfigKey::PositionRiskLabel.get(current_language()), value, Some(Tone::Bad))
    }

    /// The position archetype's own "Target" row (`ConfigKey::PositionTargetLabel`)
    /// — shared by `LongPosition`/`ShortPosition`. Tone `Good`: a target
    /// amount is a gain if it materializes.
    pub fn position_target(value: &str) -> Self {
        Self::new(ConfigKey::PositionTargetLabel.get(current_language()), value, Some(Tone::Good))
    }

    /// The position archetype's own "R:R" row (`ConfigKey::PositionRiskRewardLabel`)
    /// — shared by `LongPosition`/`ShortPosition`. Untoned: a ratio is
    /// information, not a gain or a loss.
    pub fn position_risk_reward(value: &str) -> Self {
        Self::new(ConfigKey::PositionRiskRewardLabel.get(current_language()), value, None)
    }

    /// The regression trend's own "Slope" row (`ConceptKey::Slope`) — price
    /// change per bar, read straight from
    /// [`crate::drawing::primitives_v2::channels::regression_trend::RegressionFit::slope`].
    /// Untoned: a slope's sign is direction, not a gain or a loss.
    pub fn regression_slope(value: &str) -> Self {
        Self::new(ConceptKey::Slope.get(current_language()), value, None)
    }

    /// The regression trend's own "R²" row. A literal label, not a
    /// [`ConceptKey`]: the approved design's own lab spec (`mlc-ui-lab`'s
    /// `regression_tabs()`, `registry.js`) hardcodes this exact symbol in
    /// every language row it lists rather than translating it — it names a
    /// statistic, not a phrase. Untoned: a fit-quality score is
    /// information, not a gain or a loss.
    pub fn regression_r_squared(value: &str) -> Self {
        Self::new("R²", value, None)
    }
}

/// One [`PropertyType::Table`] cell. Mirrors `mlc-ui-lab`'s own
/// `concept::spec::Cell` shapes (`Text`/`Toned`/`Swatch`/`Check`) with one
/// structural addition: an EDITABLE counterpart for each shape that can
/// carry a real per-row/per-column field on the declaring primitive
/// (`EditableText`/`EditableSwatch`/`EditableCheck`). Inertness is a
/// VARIANT, never a flag beside a shared shape — `mlc-app-layout`'s
/// `primitive_settings_intent::table_grid` (the pure cells/ids decision,
/// see [`PropertyType::Table`]'s own doc) reads a cell's own click id
/// straight off which of these seven it is, so a `Text`/`Toned`/`Swatch`/
/// `Check` cell can never accidentally report one — there is no boolean a
/// future declaration could forget to flip.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum TableCell {
    /// Plain, display-only text — a row header, a segment name, or any
    /// computed reading the declaring primitive does not back with a real
    /// per-row field. Never registers a click id.
    Text(String),
    /// A computed value, coloured by its own semantic reading — the same
    /// [`Tone`] a [`PropertyType::Stat`] row's own value uses. Never
    /// registers a click id.
    Toned(String, Tone),
    /// A single-line editable string, backed by a real per-row/per-column
    /// field. The ONLY `Text`-shaped variant that is ever clickable.
    EditableText(String),
    /// A colour swatch — a fixed legend colour with nothing to edit (e.g. a
    /// hit/miss key). Never registers a click id.
    Swatch(String),
    /// A colour swatch backed by a real per-row/per-column field — opens
    /// the same colour picker a `Color` row's own swatch does.
    EditableSwatch(String),
    /// A checkbox — display-only. Never registers a click id.
    Check(bool),
    /// A checkbox backed by a real per-row/per-column field — toggles
    /// through the same boolean apply path a `Boolean` row's own checkbox
    /// does.
    EditableCheck(bool),
}

/// Parse a `Table` cell's own click id — `{base_id}::cell:{row}:{col}` (see
/// [`PropertyType::Table`]'s own doc) — into `(base_id, row, col)`. The ONE
/// place this shape is parsed apart; `mlc-app-layout`'s
/// `primitive_settings_intent::table_grid` is the ONE place it is BUILT —
/// same string, both directions, so the two can never drift into two
/// independently-typed encodings of the same click.
pub fn parse_table_cell_id(id: &str) -> Option<(&str, usize, usize)> {
    let (base, rest) = id.split_once("::cell:")?;
    let (row_str, col_str) = rest.split_once(':')?;
    Some((base, row_str.parse().ok()?, col_str.parse().ok()?))
}

// =============================================================================
// Property Values
// =============================================================================

/// Value of a configuration property
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub enum PropertyValue {
    Color(String),
    Number(f64),
    Integer(i32),
    Boolean(bool),
    String(String),
    LineStyle(String), // "solid", "dashed", "dotted"
    FibLevels(Vec<FibLevelConfig>),
    Coordinate { bar: f64, price: f64 },
    TimeframeVisibility(TimeframeVisibilityConfig),
    /// Mirrors [`PropertyType::Stat`] — carried here only so `ConfigProperty`
    /// always has SOME `value`, same convention `FibLevels`/`Coordinate`/
    /// `TimeframeVisibility` already follow. Deliberately absent from every
    /// `as_*` accessor below (`as_string`/`as_bool`/`as_number`/`as_color`
    /// all fall through their own `_ => None`) — see `PropertyType::Stat`'s
    /// own doc for why that is load-bearing, not an oversight.
    Stat(Vec<StatRow>),
    /// Mirrors [`PropertyType::Table`] the same way `Stat` mirrors
    /// [`PropertyType::Stat`] — carried only so `ConfigProperty` always has
    /// SOME `value`, deliberately absent from every `as_*` accessor below.
    /// A per-cell write never travels through this variant — see
    /// [`PropertyType::Table`]'s own doc for the id shape a cell's own
    /// click actually uses.
    Table(Vec<Vec<TableCell>>),
}

/// Style / text / level property family.
///
/// Replaces the 3 separate `apply_{style,text,level}_property` manager
/// methods with one intent call (`DrawingManager::apply_property`). The
/// individual `prop_id` stays an open `&str` schema — properties within
/// each family are introspected per-primitive-type via
/// `Primitive::{style,text,level}_properties()` and are genuinely
/// unbounded (new primitive types plug in new properties without touching
/// central code). Only the 3-way family split is closed and stable.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PropertyKind {
    Style,
    Text,
    Level,
}

impl PropertyValue {
    pub fn as_color(&self) -> Option<&str> {
        match self {
            PropertyValue::Color(c) => Some(c),
            _ => None,
        }
    }

    pub fn as_number(&self) -> Option<f64> {
        match self {
            PropertyValue::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_bool(&self) -> Option<bool> {
        match self {
            PropertyValue::Boolean(b) => Some(*b),
            _ => None,
        }
    }

    pub fn as_string(&self) -> Option<&str> {
        match self {
            PropertyValue::String(s) => Some(s),
            PropertyValue::Color(s) => Some(s),
            PropertyValue::LineStyle(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_coordinate(&self) -> Option<(f64, f64)> {
        match self {
            PropertyValue::Coordinate { bar, price } => Some((*bar, *price)),
            _ => None,
        }
    }
}

/// Configuration for a single Fibonacci level
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct FibLevelConfig {
    /// Level value (0.0, 0.236, 0.382, 0.5, 0.618, etc.)
    pub level: f64,
    /// Is this level visible
    pub visible: bool,
    /// Line color (if different from main color)
    pub color: Option<String>,
    /// Line width (if different from main width)
    pub width: Option<f64>,
    /// Line style
    pub style: String,
    /// Fill color for area below this level (to next level down)
    #[serde(default)]
    pub fill_color: Option<String>,
    /// Fill opacity (0.0 to 1.0)
    #[serde(default = "default_fill_opacity")]
    pub fill_opacity: f64,
    /// Whether fill is enabled for this level
    #[serde(default)]
    pub fill_enabled: bool,
    /// Group name for organizing levels (e.g., "base", "fib" for pitchforks)
    /// UI can show group headers and toggle entire groups
    #[serde(default)]
    pub group: Option<String>,
}

use super::fill_defaults::{BAND_FILL_OPACITY, default_band_fill_opacity as default_fill_opacity};

impl FibLevelConfig {
    pub fn new(level: f64) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: "solid".to_string(),
            fill_color: None,
            fill_opacity: BAND_FILL_OPACITY,
            fill_enabled: false,
            group: None,
        }
    }

    pub fn with_style(level: f64, style: &str) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: style.to_string(),
            fill_color: None,
            fill_opacity: BAND_FILL_OPACITY,
            fill_enabled: false,
            group: None,
        }
    }

    /// Create with fill enabled (for default preset with fills)
    pub fn with_fill(level: f64, fill_color: Option<String>, opacity: f64) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: "solid".to_string(),
            fill_color,
            fill_opacity: opacity,
            fill_enabled: true,
            group: None,
        }
    }

    /// Create with group assignment (for pitchfork Base/Fib mode switching)
    pub fn with_group(level: f64, group: &str) -> Self {
        Self {
            level,
            visible: true,
            color: None,
            width: None,
            style: "solid".to_string(),
            fill_color: None,
            fill_opacity: BAND_FILL_OPACITY,
            fill_enabled: false,
            group: Some(group.to_string()),
        }
    }

    /// Set group on existing config
    pub fn in_group(mut self, group: &str) -> Self {
        self.group = Some(group.to_string());
        self
    }
}

/// One named level-set preset a primitive TYPE can offer — the Template
/// menu's system rows ([`crate::templates::PrimitiveTemplate::system_templates`])
/// and the Levels tab's "Набор" selector both narrow/extend a factory ladder
/// to one of these, never to a hardcoded value list of their own.
///
/// A primitive declares its OWN list via `Primitive::level_set_presets`
/// (default: empty). Only a type that actually owns a fixed catalogue of
/// named values returns a non-empty slice, and that slice must be built
/// STRICTLY from the type's own catalogue — never borrowed from another
/// family. Every ladder-having primitive in this crate now owns such a
/// catalogue: the Fibonacci family shares `fibonacci::retracement::
/// LEVEL_SET_PRESETS` (price ratios), the Gann family (`gann_fan`/
/// `gann_box`/`gann_square`+`gann_square_fixed`) each own an angle- or
/// fraction-based catalogue, the pitchfork family shares `pitchforks::
/// pitchfork::LEVEL_SET_PRESETS` (quarter + Fibonacci ratio levels), and
/// `fibonacci::time_zones::LEVEL_SET_PRESETS` is built from the Fibonacci
/// SEQUENCE used as bar-count offsets, not price ratios — narrowing a Gann
/// fan's angles (or any other family's ladder) to a FOREIGN family's value
/// list is exactly the defect this type exists to close (see that trait
/// method's own doc). The default empty slice is now the honest answer
/// only for a primitive with no level ladder at all
/// (`Primitive::level_configs` returns `None`).
#[derive(Clone, Copy)]
pub struct LevelSetPreset {
    /// Preset id, e.g. `"standard"` — the id `StyleReference::System`
    /// (`crate::drawing::StyleReference`) names and
    /// `PrimitiveTemplate::resolve_system` looks a row up by.
    pub id: &'static str,
    /// Display-name resolver. The four presets the Fibonacci family ships
    /// today draw their names from two different key enums —
    /// `StyleKey::Standard`/`StyleKey::Extended` and
    /// `ConfigKey::LevelSetKeyOnly`/`ConfigKey::LevelSetAll` — that happen
    /// to share the exact same `fn(Language) -> &'static str` shape every
    /// localized key already exposes via its own `.get(lang)`. A bare
    /// function pointer is the smallest honest representation that covers
    /// both without forcing one enum to host the other's variants and
    /// without resolving a language at definition time — the language is
    /// supplied at read time, same as every other key lookup in this
    /// codebase.
    pub name: fn(Language) -> &'static str,
    /// The exact level values this preset seeds/narrows a ladder to.
    pub values: &'static [f64],
}

/// Timeframe visibility configuration
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct TimeframeVisibilityConfig {
    /// Show on tick charts
    pub ticks: bool,
    /// Show on second charts (range: 1-59)
    pub seconds: Option<(u32, u32)>,
    /// Show on minute charts (range: 1-59)
    pub minutes: Option<(u32, u32)>,
    /// Show on hour charts (range: 1-24)
    pub hours: Option<(u32, u32)>,
    /// Show on day charts (range: 1-366)
    pub days: Option<(u32, u32)>,
    /// Show on week charts (range: 1-52)
    pub weeks: Option<(u32, u32)>,
    /// Show on month charts (range: 1-12)
    pub months: Option<(u32, u32)>,
    /// Show on range charts
    pub ranges: bool,
}

impl TimeframeVisibilityConfig {
    /// Create config that shows on all timeframes
    pub fn all() -> Self {
        Self {
            ticks: true,
            seconds: Some((1, 59)),
            minutes: Some((1, 59)),
            hours: Some((1, 24)),
            days: Some((1, 366)),
            weeks: Some((1, 52)),
            months: Some((1, 12)),
            ranges: true,
        }
    }

    /// Check if primitive is visible on a specific timeframe
    pub fn is_visible_on(&self, timeframe: &str, value: u32) -> bool {
        match timeframe {
            "tick" | "ticks" => self.ticks,
            "second" | "seconds" | "s" => {
                self.seconds.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "minute" | "minutes" | "m" => {
                self.minutes.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "hour" | "hours" | "h" => {
                self.hours.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "day" | "days" | "d" | "D" => {
                self.days.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "week" | "weeks" | "w" | "W" => {
                self.weeks.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "month" | "months" | "M" => {
                self.months.is_some_and(|(min, max)| value >= min && value <= max)
            }
            "range" | "ranges" => self.ranges,
            _ => true
        }
    }

    /// Check if this config shows on all timeframes
    pub fn is_all(&self) -> bool {
        self.ticks &&
        self.seconds == Some((1, 59)) &&
        self.minutes == Some((1, 59)) &&
        self.hours == Some((1, 24)) &&
        self.days == Some((1, 366)) &&
        self.weeks == Some((1, 52)) &&
        self.months == Some((1, 12)) &&
        self.ranges
    }

    /// Check visibility using a timeframe label string (e.g., "1H", "15m", "1D")
    /// Parses the label and calls is_visible_on() with appropriate type and value
    pub fn is_visible_on_label(&self, label: &str) -> bool {
        if let Some((tf_type, tf_value)) = Self::parse_timeframe_label(label) {
            self.is_visible_on(tf_type, tf_value)
        } else {
            true // Unknown format = show on all
        }
    }

    /// Parse timeframe label (e.g., "1H", "15m", "1D") into (type, value)
    /// Returns None if format is unrecognized
    pub fn parse_timeframe_label(label: &str) -> Option<(&'static str, u32)> {
        let label_lower = label.to_lowercase();
        match label_lower.as_str() {
            // Minutes
            "1m" => Some(("minutes", 1)),
            "3m" => Some(("minutes", 3)),
            "5m" => Some(("minutes", 5)),
            "15m" => Some(("minutes", 15)),
            "30m" => Some(("minutes", 30)),
            "45m" => Some(("minutes", 45)),
            // Hours
            "1h" => Some(("hours", 1)),
            "2h" => Some(("hours", 2)),
            "4h" => Some(("hours", 4)),
            "6h" => Some(("hours", 6)),
            "8h" => Some(("hours", 8)),
            "12h" => Some(("hours", 12)),
            // Days
            "1d" | "d" | "d1" => Some(("days", 1)),
            "3d" => Some(("days", 3)),
            // Weeks
            "1w" | "w" | "w1" => Some(("weeks", 1)),
            // Months
            "1mo" | "m1" => Some(("months", 1)),
            "3mo" => Some(("months", 3)),
            // Seconds
            "1s" => Some(("seconds", 1)),
            "5s" => Some(("seconds", 5)),
            "10s" => Some(("seconds", 10)),
            "15s" => Some(("seconds", 15)),
            "30s" => Some(("seconds", 30)),
            _ => {
                // Try parsing generic format: number + suffix
                let s = label_lower.trim();
                if s.is_empty() { return None; }

                // Find where digits end
                let digit_end = s.chars().take_while(|c| c.is_ascii_digit()).count();
                if digit_end == 0 { return None; }

                let num: u32 = s[..digit_end].parse().ok()?;
                let suffix = &s[digit_end..];

                match suffix {
                    "s" | "sec" | "second" | "seconds" => Some(("seconds", num)),
                    "m" | "min" | "minute" | "minutes" => Some(("minutes", num)),
                    "h" | "hr" | "hour" | "hours" => Some(("hours", num)),
                    "d" | "day" | "days" => Some(("days", num)),
                    "w" | "wk" | "week" | "weeks" => Some(("weeks", num)),
                    "mo" | "month" | "months" => Some(("months", num)),
                    _ => None,
                }
            }
        }
    }
}

// =============================================================================
// Config Property Definition
// =============================================================================

/// Where a property's control renders in the primitive-settings modal —
/// each variant names both a TAB (`Levels`/`Appearance`/`Labels`/
/// `Placement`, encoded by the variant's own name prefix) and a named GROUP
/// within that tab. A primitive declares this per property
/// (`ConfigProperty::category`); the modal buckets by it, sorts within a
/// bucket by `ConfigProperty::order`, and renders each control from its own
/// `PropertyType` — see `primitive_settings_vid`/`_labels`/`_levels`/
/// `_placement` in `mlc-app-layout`. A group nobody declared into renders
/// nothing (no header, no gap): the modal's layout follows the
/// declaration, not the other way around.
///
/// The TAB STRIP itself is derived, not fixed: `mlc-app-layout`'s
/// `available_tabs` builds it from the distinct tabs present among a
/// primitive's own declared properties, plus whichever tabs its bespoke
/// (non-`ConfigProperty`) collections force — a level ladder forces
/// `Levels`, the points table and timeframe chips force `Placement`
/// unconditionally. A tab with nothing in it (declared or bespoke) never
/// appears — a type with no level ladder shows no Levels tab, a type that
/// declares nothing into `AppearanceLine`/`AppearanceFill` and has no base
/// stroke to fall back to (i.e. it has levels, so that fallback lives on
/// the Levels tab instead) shows no Appearance tab either. Tab STRIP order
/// is the enum's own declaration order (`Levels`, `Appearance`, `Labels`,
/// `Placement`), never the order a primitive happened to declare its
/// properties in.
///
/// No variant carries a user-facing string — every group's header resolves
/// through an existing `ConfigKey`/`SettingsKey` at render time, the same
/// way every OTHER display string in this module already does. Replaces the
/// old five-tab vocabulary (`Style`/`Text`/`Coordinates`/`Levels`/
/// `Visibility`/`Inputs`), retired with the 3-tab cutover (2026-08-08); the
/// 3-tab set itself was in turn superseded the same day by this 4-tab,
/// declared-strip vocabulary (`Labels` split out of the old `Appearance`
/// catch-all into a tab of its own) — hard cutover both times, no
/// compatibility variants kept alongside these.
///
/// Extending this (a new group, or a 5th tab) never requires touching the
/// modal's own grouping/sorting/empty-group-suppression code — only
/// `available_tabs` and the tab module that owns the new variant need to
/// learn about it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum PropertyCategory {
    /// Levels tab — the base stroke (colour/width/line style) every level
    /// falls back to when it has no override of its own, plus any other
    /// property that belongs with the ladder rather than the shape's own
    /// line (e.g. a Fibonacci retracement's `extend_left`/`extend_right`/
    /// `show_trend_line` — see its own `style_properties()` doc for why
    /// those moved here out of `AppearanceLine`).
    LevelsBase,
    /// Appearance tab — stroke colour / width / line style, extend
    /// left/right, trend line — everything about how the LINE itself
    /// looks (as opposed to a fill or a label).
    AppearanceLine,
    /// Appearance tab — fill on/off + opacity.
    AppearanceFill,
    /// Appearance tab — its own trailing group of computed, non-editable
    /// results (e.g. a Position's Risk/Target/R:R): every property declared
    /// here MUST also carry `readonly: true` (`ConfigProperty::readonly()`)
    /// — the generic renderer (`mlc-app-layout`'s
    /// `primitive_settings_property::draw_property_row`) reads that flag,
    /// not this category, to decide whether a row is a plain value instead
    /// of a control; this category only decides WHERE the row lands (its
    /// own group, after Line/Fill, headed separately —
    /// `primitive_settings_vid`'s own `draw`/`group_heights`). Order these
    /// properties LAST within the primitive's own declaration (high
    /// `order`) so they always read as results below the editable rows.
    AppearanceStats,
    /// Labels tab — content/position/font/style/pills and every other
    /// text-adjacent control (font size, colour, background, border,
    /// alignment). Its own tab since the 4-tab cutover (2026-08-08) — it
    /// used to share the Appearance tab (`AppearanceLabels`) with the line
    /// and fill groups above.
    LabelsText,
    /// Placement tab — per-point anchor properties, alongside the points
    /// table itself (which is a bespoke collection, not a declared
    /// property — see `primitive_settings_placement`'s own doc).
    PlacementPoints,
    /// Placement tab — timeframe-visibility properties, alongside the
    /// timeframe chip row itself (also bespoke).
    PlacementTimeframes,
}

/// A named sub-section WITHIN one [`PropertyCategory`]'s own bucket — the
/// approved design's own named groups (ОБЩЕЕ / ПАЛИТРЫ / ЗАЛИВКА / СТЕПЕНЬ /
/// НОТАЦИЯ / ЦЕЛЕВЫЕ ОТНОШЕНИЯ / ЗОНЫ / ЛИНИИ / ОТОБРАЖЕНИЕ / РЕЗУЛЬТАТ /
/// ВЕЛИЧИНЫ / ФОРМАТ / …, `mlc-ui-lab`'s `concept::spec::ConceptGroup.title`)
/// given a typed address a declared [`ConfigProperty`] can carry
/// (`ConfigProperty::group`, `ConfigProperty::in_group`). `PropertyCategory`
/// still decides which TAB a property renders on and, by itself, is the
/// property's DEFAULT group — `group: None` renders exactly as before this
/// type existed (the category's own existing unnamed/base block, under
/// whatever fixed header — or lack of one — that category already drew:
/// `AppearanceLine`'s "LINE", `AppearanceFill`'s "FILL",
/// `AppearanceStats`'s "RESULTS", no header at all for `LabelsText`/
/// `LevelsBase`). A `group: Some(_)` property is pulled OUT of that default
/// run into its own named run, with its own header
/// (`primitive_settings_vid::draw_group_header`), positioned after the
/// default run — see `mlc-app-layout`'s
/// `primitive_settings_property::group_runs` for the exact bucketing rule
/// this enables.
///
/// One variant per named group the 14 object-bound archetypes' own tabs use
/// in the approved design (`mlc-ui-lab`'s `concept::spec::{ladder_tabs,
/// wave_tabs, pattern_tabs, position_tabs, measure_tabs, text_tabs,
/// table_tabs, shape_tabs, regression_tabs, brush_tabs, icon_tabs}` — never
/// `app_modals`, which are not `Primitive`-bound and have no `ConfigProperty`
/// to carry a group at all). A handful of the design's own named groups are
/// deliberately NOT here: `tf_group`/`points_group`/`behaviour_group`'s
/// titles (the Placement tab's timeframe-visibility group, points table and
/// lock/hide/sync group) are bespoke collections no primitive has ever
/// declared a `PropertyCategory::Placement*` property into (verified against
/// the whole crate) and structurally never will — the points table and
/// timeframe chips are not `ConfigProperty`-shaped, so there is no row that
/// could ever carry one of these as a `group`; and three groups whose ENTIRE
/// row list in the design is a composite, non-scalar row this crate has no
/// `ConfigProperty`/`PropertyType` shape for at all — `LadderPalettesGroupTitle`
/// (`Row::Ramps`, no fields) and `TargetRatiosGroupTitle`/`ColumnsGroupTitle`
/// (Table archetype's own column list)/`CustomLabelsGroupTitle` (each 100%
/// `Row::Table`) — adding a variant for a group no possible property could
/// ever be tagged with would be inventing vocabulary with no path to use, not
/// giving an existing row an address. The icon picker's own group (`Row::Icons`)
/// and the wave archetype's own custom-labels group (`CustomLabelsGroupTitle`)
/// USED to be in that list — `PropertyType::Picker`/`PropertyType::Table`
/// (both W4) gave them a real `ConfigProperty` shape, so they now have a
/// variant of their own, [`Self::IconGlyph`]/[`Self::CustomWaveLabels`].
///
/// A variant no primitive declares into today draws nothing at all (see
/// `group_runs`'s own doc) — most variants below are exactly that today;
/// W3 is what declares the properties that populate them.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PropertyGroup {
    // ---- Shared across several archetypes ------------------------------
    /// `label_rows()`/`plate_group()`'s own "Plate" group — the label/text
    /// background plate (behind-label toggle, plate colour, rounding,
    /// padding), shared by every archetype whose Labels tab uses
    /// `plate_group()` (ladder/wave/pattern/position) and by Callout's own
    /// differently-shaped but same-titled border/background group.
    Plate,
    /// `line_group(None)`'s own default title — Wave's "Line" group
    /// (`ToolbarMenuKey::Line`), distinct from [`Self::HeaderLines`] and
    /// [`Self::Outline`], which reuse the SAME helper with a different
    /// title argument.
    Line,
    /// `line_group(Some(HeaderLines))` / the position "trade" tab's own
    /// literal `HeaderLines` group — Pattern's and Position's own "Lines"
    /// group (`ToolbarMenuKey::HeaderLines`).
    HeaderLines,
    /// `line_group(Some(Outline))` — Shape's (rectangle/parallel channel)
    /// own "Outline" group (`ConceptKey::Outline`).
    Outline,
    /// `extend_group()` — Shape's and Regression's own Extend-left/right
    /// pair (`ConfigKey::Extend`).
    Extend,

    // ---- Ladder (fib_retracement / gann_fan / pitchfork) ----------------
    /// `ladder_tabs()`'s own "levels" subject-tab base group — colour mode,
    /// base stroke, extend/trend-line (`ConceptKey::LadderGeneralGroupTitle`).
    General,
    /// `ladder_tabs()`'s own "labels" tab group for the per-level label
    /// styling (`ConceptKey::LevelLabelsGroupTitle`).
    LevelLabels,

    // ---- Wave (elliott_impulse) ------------------------------------------
    /// `wave_tabs()`'s own wave-degree dropdown group (`ConceptKey::Degree`).
    Degree,
    /// `wave_tabs()`'s own notation-pills group (`ConceptKey::Notation`).
    Notation,
    /// `wave_tabs()`'s own correction-type dropdown group — hinted
    /// "correction only" in the design (`ConfigKey::CorrectionType`).
    CorrectionType,
    /// `fill_group(Some(FillBetweenWaves))` — Wave's own fill group
    /// (`ConceptKey::FillBetweenWaves`).
    FillBetweenWaves,
    /// `wave_tabs()`'s own "labels" tab group for wave-name label styling
    /// (`ConceptKey::WaveLabelsGroupTitle`).
    WaveLabels,
    /// `wave_tabs()`'s own "labels" tab group for the per-wave custom-label
    /// override table (`ConceptKey::CustomLabelsGroupTitle`) — W4, the
    /// first user of `PropertyType::Table`.
    CustomWaveLabels,

    // ---- Pattern (xabcd_pattern) ------------------------------------------
    /// `pattern_tabs()`'s own harmonic-pattern subtype dropdown
    /// (`ConceptKey::Subtype`).
    Subtype,
    /// `fill_group(Some(PatternBody))` — Pattern's own fill group
    /// (`ConceptKey::PatternBody`).
    PatternBody,
    /// `pattern_tabs()`'s own hit/miss scoring colours
    /// (`ConceptKey::ScoringGroupTitle`).
    Scoring,
    /// `pattern_tabs()`'s own "labels" tab group for point-name label
    /// styling (`ConceptKey::PointLabelsGroupTitle`).
    PointLabels,
    /// `pattern_tabs()`'s own "labels" tab group for on-chart ratio/segment
    /// value display (`ConceptKey::OnChartValuesGroupTitle`).
    OnChartValues,

    // ---- Position (long_position) -----------------------------------------
    /// `position_tabs()`'s own quantity/account-size/risk group
    /// (`ToolbarMenuKey::HeaderVolume`).
    Volume,
    /// `position_tabs()`'s own stop/target zone colours
    /// (`ConceptKey::ZonesGroupTitle`).
    Zones,
    /// `position_tabs()`'s own currency/ticks/percent display group
    /// (`ConceptKey::DisplayGroupTitle`).
    Display,
    /// `position_tabs()`'s own trailing read-only Risk/Target/R:R group
    /// (`ConceptKey::ResultGroupTitle` — singular, distinct wording from the
    /// pre-existing default `PropertyCategory::AppearanceStats` header,
    /// `ConfigKey::GroupResults`, which is plural).
    Result,
    /// `position_tabs()`'s own "labels" tab group — reuses the SAME key as
    /// the tab itself (`ConfigKey::Labels`), exactly as the design does.
    PositionLabels,
    /// `position_tabs()`'s own "what to write on the label" group
    /// (`ConceptKey::WhatToWriteGroupTitle`).
    WhatToWrite,

    // ---- Measurement (price_date_range) ------------------------------------
    /// `measure_tabs()`'s own value-toggle group (`ConceptKey::ValuesGroupTitle`).
    Values,
    /// `measure_tabs()`'s own number-format group (`ConceptKey::FormatGroupTitle`).
    Format,
    /// `measure_tabs()`'s own line/arrow group (`ConceptKey::LineAndArrowGroupTitle`).
    LineAndArrow,
    /// `measure_tabs()`'s own shaded-area group (`ToolbarMenuKey::Area`).
    Area,
    /// `measure_tabs()`'s own "labels" tab text group (`SettingsKey::Text`).
    TextGroup,

    // ---- Text (callout) ------------------------------------------------------
    /// `text_tabs()`'s own free-text content group (also Table's own cell-
    /// grid content group, same key — `ConceptKey::ContentGroupTitle`).
    Content,
    /// `text_tabs()`'s own typeface/size/weight/colour group
    /// (`ConceptKey::FontGroupTitle`).
    Font,
    /// `text_tabs()`'s own alignment/wrap/width group
    /// (`ConceptKey::LayoutGroupTitle`).
    Layout,
    /// `text_tabs()`'s own signpost-tail hint group — titled with the
    /// Signpost primitive's own name, as the design does
    /// (`PrimitiveNameKey::Signpost`).
    SignpostHint,

    // ---- Table --------------------------------------------------------------
    /// `table_tabs()`'s own row/column-count group — titled with the same
    /// key the design reuses for it (`UserSettingsKey::ControlPointSizeLabel`).
    RowsColumns,
    /// `table_tabs()`'s own grid-line/header-row group
    /// (`ConceptKey::GridAndHeaderGroupTitle`).
    GridAndHeader,
    /// `table_tabs()`'s own cell text/background group
    /// (`ConceptKey::TextAndBackgroundGroupTitle`).
    TextAndBackground,

    // ---- Shape (rectangle / parallel_channel) --------------------------------
    /// `shape_tabs()`'s own channel-specific middle-line group, Parallel
    /// Channel only (`ConceptKey::MiddleLine`).
    MiddleLine,
    /// `shape_tabs()`'s own on-shape text group (`ConceptKey::TextOnShape`).
    TextOnShape,

    // ---- Regression (regression_trend) ---------------------------------------
    /// `regression_tabs()`'s own source/period group
    /// (`ConceptKey::CalculationGroupTitle`).
    Calculation,
    /// `regression_tabs()`'s own sigma-band table + opacity group
    /// (`ConceptKey::SigmasGroupTitle`).
    Sigmas,
    /// `regression_tabs()`'s own baseline group (`ToolbarMenuKey::Baseline`).
    Baseline,
    /// `regression_tabs()`'s own trailing computed slope/R² group
    /// (`ConceptKey::MetricsGroupTitle`).
    Metrics,

    // ---- Brush ----------------------------------------------------------------
    /// `brush_tabs()`'s own stroke colour/width/opacity/style group — reuses
    /// the tab's own title (`ConceptKey::StrokeGroupTitle`).
    Stroke,
    /// `brush_tabs()`'s own smoothing/cap/close group
    /// (`UserSettingsKey::ControlPointShapeLabel`).
    PointShape,
    /// `fill_group(Some(FillClosedOutline))` — Brush's own fill group
    /// (`ConceptKey::FillClosedOutline`).
    FillClosedOutline,

    // ---- Icon (flag) ------------------------------------------------------------
    /// `icon_tabs()`'s own glyph-picker group (`Row::Icons` — the design
    /// reuses `SettingsKey::CompareInfoSymbol`, "Symbol"/"Символ", as its
    /// header rather than minting a dedicated key; see [`ConfigProperty::picker`]'s
    /// own doc). W4 — the first group here backed by `PropertyType::Picker`
    /// instead of a bespoke, non-`ConfigProperty` row.
    IconGlyph,
    /// `icon_tabs()`'s own colour/size/outline group (`ConceptKey::Rendering`).
    Rendering,
    /// `icon_tabs()`'s own show/text/size/side group for the label next to
    /// the icon (`ConceptKey::LabelNextToIcon`).
    LabelNextToIcon,
}

impl PropertyGroup {
    /// This group's own header text, resolved through the SAME key the
    /// approved design (`mlc-ui-lab`'s `concept::spec`) calls for it — see
    /// each variant's own doc for which key and why.
    pub fn label(&self) -> &'static str {
        let lang = current_language();
        match self {
            PropertyGroup::Plate => ConceptKey::Plate.get(lang),
            PropertyGroup::Line => ToolbarMenuKey::Line.get(lang),
            PropertyGroup::HeaderLines => ToolbarMenuKey::HeaderLines.get(lang),
            PropertyGroup::Outline => ConceptKey::Outline.get(lang),
            PropertyGroup::Extend => ConfigKey::Extend.get(lang),

            PropertyGroup::General => ConceptKey::LadderGeneralGroupTitle.get(lang),
            PropertyGroup::LevelLabels => ConceptKey::LevelLabelsGroupTitle.get(lang),

            PropertyGroup::Degree => ConceptKey::Degree.get(lang),
            PropertyGroup::Notation => ConceptKey::Notation.get(lang),
            PropertyGroup::CorrectionType => ConfigKey::CorrectionType.get(lang),
            PropertyGroup::FillBetweenWaves => ConceptKey::FillBetweenWaves.get(lang),
            PropertyGroup::WaveLabels => ConceptKey::WaveLabelsGroupTitle.get(lang),
            PropertyGroup::CustomWaveLabels => ConceptKey::CustomLabelsGroupTitle.get(lang),

            PropertyGroup::Subtype => ConceptKey::Subtype.get(lang),
            PropertyGroup::PatternBody => ConceptKey::PatternBody.get(lang),
            PropertyGroup::Scoring => ConceptKey::ScoringGroupTitle.get(lang),
            PropertyGroup::PointLabels => ConceptKey::PointLabelsGroupTitle.get(lang),
            PropertyGroup::OnChartValues => ConceptKey::OnChartValuesGroupTitle.get(lang),

            PropertyGroup::Volume => ToolbarMenuKey::HeaderVolume.get(lang),
            PropertyGroup::Zones => ConceptKey::ZonesGroupTitle.get(lang),
            PropertyGroup::Display => ConceptKey::DisplayGroupTitle.get(lang),
            PropertyGroup::Result => ConceptKey::ResultGroupTitle.get(lang),
            PropertyGroup::PositionLabels => ConfigKey::Labels.get(lang),
            PropertyGroup::WhatToWrite => ConceptKey::WhatToWriteGroupTitle.get(lang),

            PropertyGroup::Values => ConceptKey::ValuesGroupTitle.get(lang),
            PropertyGroup::Format => ConceptKey::FormatGroupTitle.get(lang),
            PropertyGroup::LineAndArrow => ConceptKey::LineAndArrowGroupTitle.get(lang),
            PropertyGroup::Area => ToolbarMenuKey::Area.get(lang),
            PropertyGroup::TextGroup => SettingsKey::Text.get(lang),

            PropertyGroup::Content => ConceptKey::ContentGroupTitle.get(lang),
            PropertyGroup::Font => ConceptKey::FontGroupTitle.get(lang),
            PropertyGroup::Layout => ConceptKey::LayoutGroupTitle.get(lang),
            PropertyGroup::SignpostHint => PrimitiveNameKey::Signpost.get(lang),

            PropertyGroup::RowsColumns => UserSettingsKey::ControlPointSizeLabel.get(lang),
            PropertyGroup::GridAndHeader => ConceptKey::GridAndHeaderGroupTitle.get(lang),
            PropertyGroup::TextAndBackground => ConceptKey::TextAndBackgroundGroupTitle.get(lang),

            PropertyGroup::MiddleLine => ConceptKey::MiddleLine.get(lang),
            PropertyGroup::TextOnShape => ConceptKey::TextOnShape.get(lang),

            PropertyGroup::Calculation => ConceptKey::CalculationGroupTitle.get(lang),
            PropertyGroup::Sigmas => ConceptKey::SigmasGroupTitle.get(lang),
            PropertyGroup::Baseline => ToolbarMenuKey::Baseline.get(lang),
            PropertyGroup::Metrics => ConceptKey::MetricsGroupTitle.get(lang),

            PropertyGroup::Stroke => ConceptKey::StrokeGroupTitle.get(lang),
            PropertyGroup::PointShape => UserSettingsKey::ControlPointShapeLabel.get(lang),
            PropertyGroup::FillClosedOutline => ConceptKey::FillClosedOutline.get(lang),

            PropertyGroup::IconGlyph => SettingsKey::CompareInfoSymbol.get(lang),
            PropertyGroup::Rendering => ConceptKey::Rendering.get(lang),
            PropertyGroup::LabelNextToIcon => ConceptKey::LabelNextToIcon.get(lang),
        }
    }
}

/// A single configurable property
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConfigProperty {
    /// Unique identifier for this property
    pub id: String,
    /// Display name (localized)
    pub name: String,
    /// Property type
    pub prop_type: PropertyType,
    /// Current value
    pub value: PropertyValue,
    /// Category for UI grouping
    pub category: PropertyCategory,
    /// Order within category (lower = first)
    pub order: i32,
    /// Is property read-only
    pub readonly: bool,
    /// Help text / tooltip
    pub tooltip: Option<String>,
    /// Named sub-group WITHIN `category`'s own tab — see [`PropertyGroup`]'s
    /// own doc. `None` (the default — `#[serde(default)]` so drawings
    /// persisted before this field existed still load) renders exactly as
    /// before this field existed: the category's own default run.
    #[serde(default)]
    pub group: Option<PropertyGroup>,
}

/// The position archetype's own Risk/Target/R:R stat-block id — shared
/// between `LongPosition`/`ShortPosition`'s own `style_properties()` (which
/// declares [`ConfigProperty::stat_block`] under this id) and their own
/// `apply_style_property` explicit reject arm (see their own doc), so the
/// two can never drift into two independently-typed copies of the same
/// literal.
pub const POSITION_RESULT_STATS_ID: &str = "position_result_stats";

/// The regression trend archetype's own read-only Slope/R² stat-block id —
/// same role [`POSITION_RESULT_STATS_ID`] plays for the position archetype:
/// shared between `RegressionTrend::style_properties()` (which declares
/// [`ConfigProperty::stat_block`] under this id) and its own
/// `apply_style_property` explicit reject arm, so the two can never drift
/// into two independently-typed copies of the same literal.
pub const REGRESSION_RESULT_STATS_ID: &str = "regression_result_stats";

/// The per-wave custom-label OVERRIDE table's own id — shared by all FIVE
/// Elliott types' own `style_properties()` (each declares
/// [`ConfigProperty::table`] under this id) and their own
/// `apply_style_property` (each parses `id` with [`parse_table_cell_id`]
/// and compares the returned base against this constant), so the two can
/// never drift into two independently-typed copies of the same literal —
/// same role [`POSITION_RESULT_STATS_ID`] plays for the position
/// archetype's own stat block.
pub const CUSTOM_WAVE_LABELS_PROPERTY_ID: &str = "custom_wave_labels";

impl ConfigProperty {
    /// Create a color property
    pub fn color(id: &str, name: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Color,
            value: PropertyValue::Color(value.to_string()),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a number property
    pub fn number(id: &str, name: &str, value: f64, min: Option<f64>, max: Option<f64>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Number { min, max, step: None },
            value: PropertyValue::Number(value),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a boolean property
    pub fn boolean(id: &str, name: &str, value: bool) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Boolean,
            value: PropertyValue::Boolean(value),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a line style property
    pub fn line_style(id: &str, name: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::LineStyle,
            value: PropertyValue::LineStyle(value.to_string()),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a coordinate property
    pub fn coordinate(id: &str, name: &str, bar: f64, price: f64) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Coordinate,
            value: PropertyValue::Coordinate { bar, price },
            category: PropertyCategory::PlacementPoints,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a text content property (multiline)
    pub fn text_content(id: &str, name: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Text { multiline: true, max_length: Some(1000) },
            value: PropertyValue::String(value.to_string()),
            category: PropertyCategory::LabelsText,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a text property (single line)
    pub fn text(id: &str, name: &str, value: &str) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Text { multiline: false, max_length: Some(200) },
            value: PropertyValue::String(value.to_string()),
            category: PropertyCategory::LabelsText,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a select property
    pub fn select(id: &str, name: &str, value: &str, options: Vec<SelectOption>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Select { options },
            value: PropertyValue::String(value.to_string()),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a pills (segmented single-choice) property — mirrors
    /// [`Self::select`] byte-for-byte apart from `prop_type`: same id,
    /// same `PropertyValue::String` wire shape, same options list. See
    /// [`PropertyType::Pills`]'s own doc for why the two are wire-
    /// identical and only the rendered control differs.
    pub fn pills(id: &str, name: &str, value: &str, options: Vec<SelectOption>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Pills { options },
            value: PropertyValue::String(value.to_string()),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a glyph-picker property (W4) — mirrors [`Self::pills`] byte-
    /// for-byte apart from `prop_type`/option shape: same id, same
    /// `PropertyValue::String` wire value (one option's own `id`), same
    /// generic `apply_{style,text,level}_property` reach. See
    /// [`PropertyType::Picker`]'s own doc for why the option shape differs.
    pub fn picker(id: &str, name: &str, value: &str, options: Vec<PickerOption>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Picker { options },
            value: PropertyValue::String(value.to_string()),
            category: PropertyCategory::AppearanceLine,
            order: 0,
            readonly: false,
            tooltip: None,
            group: None,
        }
    }

    /// Create a computed-stat BLOCK (W4) — see [`PropertyType::Stat`]'s own
    /// doc for the shape and why it is structurally read-only regardless of
    /// this constructor's own choices. `rows` is pre-formatted by the
    /// caller; this never computes anything itself.
    ///
    /// `readonly: true` here is a SECOND, independent layer on top of the
    /// render layer's own non-interactivity: it feeds the pre-existing
    /// `!prop.readonly` filters `PrimitiveTemplate::from_primitive` and
    /// `DrawingManager::save_last_style_for_primitive` already apply to
    /// every readonly row, so a stat block is excluded from a saved
    /// template's/last-used-style's own captured properties for free —
    /// never the mechanism a click is blocked by (see `PropertyType::Stat`'s
    /// own doc).
    ///
    /// `name` is the row's own SEARCH text (`primitive_settings.rs`'s
    /// `search_matches`), not a caption drawn anywhere — the block draws
    /// each row's own `StatRow::label`, never a single top-level name — so
    /// this joins every row's label into one substring-searchable string.
    pub fn stat_block(id: &str, rows: Vec<StatRow>) -> Self {
        let name = rows.iter().map(|r| r.label.as_str()).collect::<Vec<_>>().join(" · ");
        Self {
            id: id.to_string(),
            name,
            prop_type: PropertyType::Stat { rows: rows.clone() },
            value: PropertyValue::Stat(rows),
            category: PropertyCategory::AppearanceStats,
            order: 0,
            readonly: true,
            tooltip: None,
            group: None,
        }
    }

    /// Create a grid property (W4) — see [`PropertyType::Table`]'s own doc
    /// for the shape and for why it is `readonly` regardless of how many of
    /// its own cells are individually editable (a cell's own editability is
    /// carried by its OWN [`TableCell`] variant, not by this flag). `rows`
    /// is pre-formatted by the caller — this never computes a cell's own
    /// content, the same discipline [`Self::stat_block`] follows for its
    /// own rows.
    pub fn table(id: &str, name: &str, head: Vec<String>, rows: Vec<Vec<TableCell>>) -> Self {
        Self {
            id: id.to_string(),
            name: name.to_string(),
            prop_type: PropertyType::Table { head, rows: rows.clone() },
            value: PropertyValue::Table(rows),
            category: PropertyCategory::LabelsText,
            order: 0,
            readonly: true,
            tooltip: None,
            group: None,
        }
    }

    /// Set category
    pub fn with_category(mut self, category: PropertyCategory) -> Self {
        self.category = category;
        self
    }

    /// Set order
    pub fn with_order(mut self, order: i32) -> Self {
        self.order = order;
        self
    }

    /// Set tooltip
    pub fn with_tooltip(mut self, tooltip: &str) -> Self {
        self.tooltip = Some(tooltip.to_string());
        self
    }

    /// Set readonly
    pub fn readonly(mut self) -> Self {
        self.readonly = true;
        self
    }

    /// Set the named sub-group this property belongs to within its own
    /// `category` — see [`PropertyGroup`]'s own doc.
    pub fn in_group(mut self, group: PropertyGroup) -> Self {
        self.group = Some(group);
        self
    }

    // =========================================================================
    // i18n-aware property constructors
    // =========================================================================

    /// Create "Show Prices" boolean property with i18n
    pub fn show_prices(value: bool) -> Self {
        Self::boolean("show_prices", ConfigKey::Prices.get(current_language()), value)
    }

    /// Create "Show Levels" boolean property with i18n
    pub fn show_levels(value: bool) -> Self {
        Self::boolean("show_percentages", ConfigKey::Levels.get(current_language()), value)
    }

    /// Create "Show as Percent" boolean property with i18n
    pub fn show_as_percent(value: bool) -> Self {
        Self::boolean("show_as_percent", ConfigKey::Percentages.get(current_language()), value)
    }

    /// Create "Label Position" select property with i18n
    pub fn label_position(value: &str) -> Self {
        Self::select(
            "label_position",
            ConfigKey::LabelPosition.get(current_language()),
            value,
            SelectOption::label_positions(),
        )
    }

    /// Create "Extend Left" boolean property with i18n
    pub fn extend_left(value: bool) -> Self {
        Self::boolean("extend_left", ConfigKey::ExtendLeft.get(current_language()), value)
    }

    /// Create "Extend Right" boolean property with i18n
    pub fn extend_right(value: bool) -> Self {
        Self::boolean("extend_right", ConfigKey::ExtendRight.get(current_language()), value)
    }

    /// Create "Show Trend Line" boolean property with i18n
    pub fn show_trend_line(value: bool) -> Self {
        Self::boolean("show_trend_line", ConfigKey::TrendLine.get(current_language()), value)
    }

    /// Create "Extend Lines" boolean property with i18n
    pub fn extend_lines(value: bool) -> Self {
        Self::boolean("extend_lines", ConfigKey::ExtendLines.get(current_language()), value)
    }

    /// Create "Show Labels" boolean property with i18n
    pub fn show_labels(value: bool) -> Self {
        Self::boolean("show_labels", ConfigKey::ShowLabels.get(current_language()), value)
    }

    /// Create "Show Lines" boolean property with i18n
    pub fn show_lines(value: bool) -> Self {
        Self::boolean("show_lines", ConfigKey::ShowLines.get(current_language()), value)
    }

    /// Create "Show Ratios" boolean property with i18n
    pub fn show_ratios(value: bool) -> Self {
        Self::boolean("show_ratios", ConfigKey::ShowRatios.get(current_language()), value)
    }

    /// Create the Elliott ratio-ribs' own stroke colour property with i18n —
    /// declared next to [`Self::show_ratios`] on every Elliott type (`id`
    /// deliberately distinct from `"stroke_color"`, which the click-dispatch
    /// path special-cases to write `PrimitiveData` directly — this is a
    /// SEPARATE colour, the ribs' own).
    pub fn ratio_rib_color(value: &str) -> Self {
        Self::color("ratio_rib_color", ConceptKey::RatioRibColorLabel.get(current_language()), value)
    }

    /// Create the Elliott ratio-ribs' own stroke width property with i18n.
    pub fn ratio_rib_width(value: f64) -> Self {
        Self::number("ratio_rib_width", ConceptKey::RatioRibWidthLabel.get(current_language()), value, Some(1.0), Some(10.0))
    }

    /// Create the Elliott ratio-ribs' own line-style property with i18n.
    pub fn ratio_rib_style(value: &str) -> Self {
        Self::line_style("ratio_rib_style", ConceptKey::RatioRibStyleLabel.get(current_language()), value)
    }

    /// Create "Show Projections" boolean property with i18n — the
    /// fibonacci-EXTENSION projection toggle (`elliott::projection`), the
    /// forward-looking counterpart to [`Self::show_ratios`] declared next
    /// to it on the four Elliott types that carry it (not
    /// `elliott_triangle` — see `elliott::projection`'s own module doc for
    /// why).
    pub fn show_projections(value: bool) -> Self {
        Self::boolean("show_projections", ConfigKey::ShowProjections.get(current_language()), value)
    }

    /// Create "Truncated fifth" boolean property with i18n — the
    /// per-structure escape hatch for `elliott::guard`'s own
    /// `"wave5_beyond_wave3"` hard rule (owner ruling 2026-08-15), declared
    /// next to [`Self::show_projections`] on `ElliottImpulse`, same
    /// ungrouped run, same "falls out of `style_properties()` for free"
    /// surface — see `ElliottImpulse::truncated_fifth`'s own field doc.
    pub fn truncated_fifth(value: bool) -> Self {
        Self::boolean("truncated_fifth", ConfigKey::TruncatedFifth.get(current_language()), value)
    }

    /// Create "Show Trendlines" boolean property with i18n
    pub fn show_trendlines(value: bool) -> Self {
        Self::boolean("show_trendlines", ConfigKey::ShowTrendlines.get(current_language()), value)
    }

    /// Create "Show Price" boolean property with i18n
    pub fn show_price(value: bool) -> Self {
        Self::boolean("show_price", ConfigKey::ShowPrice.get(current_language()), value)
    }

    /// Create "Show Line" boolean property with i18n
    pub fn show_line(value: bool) -> Self {
        Self::boolean("show_line", ConfigKey::ShowLine.get(current_language()), value)
    }

    /// Create "Show Header" boolean property with i18n
    pub fn show_header(value: bool) -> Self {
        Self::boolean("show_header", ConfigKey::ShowHeader.get(current_language()), value)
    }

    /// Create "Show Neckline" boolean property with i18n
    pub fn show_neckline(value: bool) -> Self {
        Self::boolean("show_neckline", ConfigKey::ShowNeckline.get(current_language()), value)
    }

    /// Create "Show Background" boolean property with i18n
    pub fn show_background(value: bool) -> Self {
        Self::boolean("background", ConfigKey::ShowBackground.get(current_language()), value)
    }

    /// Create "Extend" boolean property with i18n
    pub fn extend(value: bool) -> Self {
        Self::boolean("extend", ConfigKey::Extend.get(current_language()), value)
    }

    /// Create "Upward" boolean property with i18n — `gann_fan`'s own fan
    /// direction toggle (`true` = upward fan, `false` = downward).
    pub fn upward(value: bool) -> Self {
        Self::boolean("upward", ConceptKey::Upward.get(current_language()), value)
    }

    /// Create "Show Grid" boolean property with i18n — `gann_box`'s own
    /// horizontal/vertical grid toggle.
    pub fn show_grid(value: bool) -> Self {
        Self::boolean("show_grid", ConceptKey::ShowGrid.get(current_language()), value)
    }

    /// Create "Show Diagonals" boolean property with i18n — `gann_box`'s
    /// own 1x1 + anti-diagonal toggle.
    pub fn show_diagonals(value: bool) -> Self {
        Self::boolean("show_diagonals", ConceptKey::ShowDiagonals.get(current_language()), value)
    }

    /// Create "Show Cardinal Lines" boolean property with i18n —
    /// `gann_square`'s own horizontal/vertical-through-center toggle.
    pub fn show_cardinal(value: bool) -> Self {
        Self::boolean("show_cardinal", ConceptKey::ShowCardinal.get(current_language()), value)
    }

    /// Create "Show Ordinal Lines" boolean property with i18n —
    /// `gann_square`'s own diagonal toggle.
    pub fn show_ordinal(value: bool) -> Self {
        Self::boolean("show_ordinal", ConceptKey::ShowOrdinal.get(current_language()), value)
    }

    /// Create "Show author & time" boolean property with i18n — `Comment`'s
    /// own `author`/`timestamp` display gate.
    pub fn show_meta(value: bool) -> Self {
        Self::boolean("show_meta", ConceptKey::ShowAuthorTimestamp.get(current_language()), value)
    }

    /// Create "Levels" (show_percentages) boolean property with i18n
    pub fn levels(value: bool) -> Self {
        Self::boolean("show_percentages", ConfigKey::Levels.get(current_language()), value)
    }

    /// Create "Full Circle" boolean property with i18n
    pub fn full_circle(value: bool) -> Self {
        Self::boolean("full_circle", ConfigKey::FullCircle.get(current_language()), value)
    }

    /// Create "Reverse" boolean property with i18n
    pub fn reverse(value: bool) -> Self {
        Self::boolean("reverse", ConfigKey::Reverse.get(current_language()), value)
    }

    /// Create "Closed" boolean property with i18n (free-form shape closure —
    /// last point joins back to the first). Precondition for `fill` to paint
    /// anything on a free-form shape (Polyline/Path); order it above the
    /// "Fill" row in `style_properties()`.
    pub fn closed(value: bool) -> Self {
        Self::boolean("closed", ConfigKey::Closed.get(current_language()), value)
    }

    /// Create "Fill" boolean property with i18n
    pub fn fill(value: bool) -> Self {
        Self::boolean("fill", ConfigKey::Fill.get(current_language()), value)
    }

    /// Create "Fill Opacity" number property with i18n. `value_pct` is a
    /// percentage (0-100) — primitives store opacity as 0.0-1.0 internally,
    /// but the slider renders `{:.0}` with no decimals, so a 0.0-1.0 range
    /// would only ever show "0" or "1".
    pub fn fill_opacity(value_pct: f64) -> Self {
        Self::number("fill_opacity", ConfigKey::FillOpacity.get(current_language()), value_pct, Some(0.0), Some(100.0))
    }

    /// Create "Smooth" boolean property with i18n (path curve smoothing).
    pub fn smooth(value: bool) -> Self {
        Self::boolean("smooth", ConfigKey::Smooth.get(current_language()), value)
    }

    /// Create "Level Mode" select property with i18n for pitchforks
    pub fn level_mode(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "level_mode",
            ConfigKey::LevelMode.get(lang),
            value,
            vec![
                SelectOption::new("both", ConfigKey::AllLevels.get(lang)),
                SelectOption::new("base", ConfigKey::BaseLevels.get(lang)),
                SelectOption::new("fibonacci", ConfigKey::FibonacciLevels.get(lang)),
            ],
        )
    }

    /// Create "Label Font Size" number property with i18n
    pub fn label_font_size(value: f64) -> Self {
        Self::number("label_font_size", ConfigKey::LabelFontSize.get(current_language()), value, Some(8.0), Some(32.0))
    }

    /// Create "Label Color" color property with i18n
    pub fn label_color(value: &str) -> Self {
        Self::color("label_color", ConfigKey::LabelColor.get(current_language()), value)
    }

    /// Create "Label Background Color" color property with i18n
    pub fn label_background_color(value: &str) -> Self {
        Self::color("label_background_color", ConfigKey::LabelBackgroundColor.get(current_language()), value)
    }

    /// Create "Label Padding" number property with i18n
    pub fn label_padding(value: f64) -> Self {
        Self::number("label_padding", ConfigKey::LabelPadding.get(current_language()), value, Some(0.0), Some(20.0))
    }

    /// Create "Label Corner Radius" number property with i18n
    pub fn label_radius(value: f64) -> Self {
        Self::number("label_radius", ConfigKey::LabelRadius.get(current_language()), value, Some(0.0), Some(20.0))
    }

    /// Create "Label Border" boolean property with i18n
    pub fn label_border(value: bool) -> Self {
        Self::boolean("label_border", ConfigKey::LabelBorder.get(current_language()), value)
    }

    /// Create "Label Border Color" color property with i18n
    pub fn label_border_color(value: &str) -> Self {
        Self::color("label_border_color", ConfigKey::LabelBorderColor.get(current_language()), value)
    }

    /// Create "Label Border Width" number property with i18n
    pub fn label_border_width(value: f64) -> Self {
        Self::number("label_border_width", ConfigKey::LabelBorderWidth.get(current_language()), value, Some(0.5), Some(6.0))
    }

    /// Create "Label Offset" number property with i18n
    pub fn label_offset(value: f64) -> Self {
        Self::number("label_offset", ConfigKey::LabelOffset.get(current_language()), value, Some(0.0), Some(60.0))
    }

    /// Create "Quantity" number property with i18n (trading position size)
    pub fn quantity(value: f64) -> Self {
        Self::number("quantity", ConfigKey::Quantity.get(current_language()), value, Some(0.0), Some(1_000_000.0))
    }

    /// Create "Show P&L" boolean property with i18n
    pub fn show_pnl(value: bool) -> Self {
        Self::boolean("show_pnl", ConfigKey::ShowPnl.get(current_language()), value)
    }

    /// Create the position archetype's own display-unit PILLS row
    /// (`position_tabs()`'s own `DisplayGroupTitle` group, W4) — two real
    /// options (`PositionDisplayUnit::{Currency,Percent}`); the design's
    /// third option ("ticks") has no backing field — see
    /// [`PositionDisplayUnit`]'s own doc for why — and is not offered here.
    pub fn position_display_unit_pills(value: &str) -> Self {
        let lang = current_language();
        Self::pills(
            "display_unit",
            ConceptKey::DisplayGroupTitle.get(lang),
            value,
            vec![
                SelectOption::new("currency", ConceptKey::Currency.get(lang)),
                SelectOption::new("percent", ConceptKey::PercentUnit.get(lang)),
            ],
        )
    }

    /// Create "Show Live Label" boolean property with i18n (long/short
    /// position — gates the live-price text label; the live shading layer
    /// and marker line always render regardless of this setting).
    pub fn show_live_label(value: bool) -> Self {
        Self::boolean("show_live_label", ConfigKey::ShowLiveLabel.get(current_language()), value)
    }

    /// Create "Stop Zone Color" color property with i18n (long/short position)
    pub fn stop_zone_color(value: &str) -> Self {
        Self::color("stop_color", ConfigKey::StopZoneColor.get(current_language()), value)
    }

    /// Create "Target Zone Color" color property with i18n (long/short position)
    pub fn target_zone_color(value: &str) -> Self {
        Self::color("target_color", ConfigKey::TargetZoneColor.get(current_language()), value)
    }

    /// Create "Show Price Diff" boolean property with i18n (measurement rulers).
    /// Property id stays `"show_pips"` to match the struct field it drives.
    pub fn show_price_diff(value: bool) -> Self {
        Self::boolean("show_pips", ConfigKey::ShowPriceDiff.get(current_language()), value)
    }

    /// Create "Show Bar Count" boolean property with i18n (measurement rulers)
    pub fn show_bar_count(value: bool) -> Self {
        Self::boolean("show_bars", ConfigKey::ShowBarCount.get(current_language()), value)
    }

    /// Create "Show Duration" boolean property with i18n (date range ruler)
    pub fn show_duration(value: bool) -> Self {
        Self::boolean("show_time", ConfigKey::ShowDuration.get(current_language()), value)
    }

    /// Create "Rotations" number property with i18n (Fibonacci spiral turn count)
    pub fn rotations(value: f64) -> Self {
        Self::number("rotations", ConfigKey::Rotations.get(current_language()), value, Some(0.5), Some(6.0))
    }

    /// Create "Clockwise" boolean property with i18n (Fibonacci spiral direction)
    pub fn clockwise(value: bool) -> Self {
        Self::boolean("clockwise", ConfigKey::Clockwise.get(current_language()), value)
    }

    /// Create "Flip Horizontal" boolean property with i18n (Fibonacci spiral mirroring)
    pub fn flip_horizontal(value: bool) -> Self {
        Self::boolean("flip_horizontal", ConfigKey::FlipHorizontal.get(current_language()), value)
    }

    /// Create "Flip Vertical" boolean property with i18n (Fibonacci spiral mirroring)
    pub fn flip_vertical(value: bool) -> Self {
        Self::boolean("flip_vertical", ConfigKey::FlipVertical.get(current_language()), value)
    }

    /// Create "Wave Degree" select property with i18n
    pub fn wave_degree(value: &str, options: Vec<SelectOption>) -> Self {
        Self::select("degree", ConfigKey::WaveDegree.get(current_language()), value, options)
    }

    /// Create the wave archetype's own notation PILLS row (`wave_tabs()`'s
    /// own Notation group, W4) — three fixed numeral families
    /// (`NotationStyle::{Arabic,Roman,Circled}`), left as bare literals per
    /// the approved design's own "notation samples" convention (never
    /// translated — see `mlc-ui-lab`'s `spec.rs` module doc). `ElliottImpulse`
    /// only (owner correction 2026-08-17) — its labels are numbers, the one
    /// Elliott type where these three families are the real choice; see its
    /// own `notation_style` field doc. `value` is
    /// `declared_notation_style().as_str()` (that method's own doc): a
    /// `FollowDegree` value matches none of the three options, which the
    /// generic Pills renderer already handles as "no segment highlighted"
    /// — there is no fourth pill for it, matching the design's own
    /// three-pill row exactly.
    pub fn notation_style_pills(value: &str) -> Self {
        Self::pills(
            "notation_style",
            ConceptKey::Notation.get(current_language()),
            value,
            vec![
                SelectOption::new("arabic", "1 2 3 4 5"),
                SelectOption::new("roman", "I II III IV V"),
                SelectOption::new("circled", "① ② ③ ④ ⑤"),
            ],
        )
    }

    /// Create "Inverted" boolean property with i18n
    pub fn inverted(value: bool) -> Self {
        Self::boolean("inverted", ConfigKey::Inverted.get(current_language()), value)
    }

    /// Create "Triangle Type" select property with i18n
    pub fn triangle_type(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "triangle_type",
            ConfigKey::TriangleType.get(lang),
            value,
            vec![
                SelectOption::new("symmetrical", ConfigKey::Symmetrical.get(lang)),
                SelectOption::new("ascending", ConfigKey::Ascending.get(lang)),
                SelectOption::new("descending", ConfigKey::Descending.get(lang)),
                SelectOption::new("expanding", ConfigKey::Expanding.get(lang)),
            ],
        )
    }

    /// Create "Correction Type" select property with i18n (Elliott
    /// Correction's `CorrectionType` — Zigzag/Flat/Expanded/Running).
    /// 2026-08-09: previously a real field with zero declared representation.
    pub fn correction_type(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "correction_type",
            ConfigKey::CorrectionType.get(lang),
            value,
            vec![
                SelectOption::new("zigzag", ConfigKey::CorrectionZigzag.get(lang)),
                SelectOption::new("flat", ConfigKey::CorrectionFlat.get(lang)),
                SelectOption::new("expanded", ConfigKey::CorrectionExpanded.get(lang)),
                SelectOption::new("running", ConfigKey::CorrectionRunning.get(lang)),
            ],
        )
    }

    /// Create "Font Size" number property with i18n
    pub fn font_size(value: f64) -> Self {
        Self::number("font_size", ConfigKey::FontSize.get(current_language()), value, Some(8.0), Some(72.0))
    }

    /// Create "Text Color" color property with i18n
    pub fn text_color(value: &str) -> Self {
        Self::color("text_color", ConfigKey::TextColor.get(current_language()), value)
    }

    /// Create "Header Color" color property with i18n
    pub fn header_color(value: &str) -> Self {
        Self::color("header_color", ConfigKey::HeaderColor.get(current_language()), value)
    }

    /// Create "Grid Color" color property with i18n
    pub fn grid_color(value: &str) -> Self {
        Self::color("grid_color", ConfigKey::GridColor.get(current_language()), value)
    }

    /// Create "Header Text Color" color property with i18n
    pub fn header_text_color(value: &str) -> Self {
        Self::color("header_text_color", ConfigKey::HeaderTextColor.get(current_language()), value)
    }

    /// Create "Content" text property with i18n
    pub fn content(value: &str) -> Self {
        Self::text("content", ConfigKey::Content.get(current_language()), value)
    }

    /// Create "Comment" multiline text property with i18n
    pub fn comment(value: &str) -> Self {
        Self::text_content("content", ConfigKey::Comment.get(current_language()), value)
    }

    /// Create "Bold" boolean property with i18n
    pub fn bold(value: bool) -> Self {
        Self::boolean("bold", ConfigKey::Bold.get(current_language()), value)
    }

    /// Create "Italic" boolean property with i18n
    pub fn italic(value: bool) -> Self {
        Self::boolean("italic", ConfigKey::Italic.get(current_language()), value)
    }

    /// Create "Bubble Width" number property with i18n
    pub fn bubble_width(value: f64) -> Self {
        Self::number("bubble_width", ConfigKey::BubbleWidth.get(current_language()), value, Some(50.0), Some(500.0))
    }

    /// Create "Bubble Height" number property with i18n
    pub fn bubble_height(value: f64) -> Self {
        Self::number("bubble_height", ConfigKey::BubbleHeight.get(current_language()), value, Some(30.0), Some(300.0))
    }

    /// Create "Expanded" boolean property with i18n
    pub fn expanded(value: bool) -> Self {
        Self::boolean("expanded", ConfigKey::Expanded.get(current_language()), value)
    }

    /// Create "Direction" select property with i18n for signpost
    pub fn direction(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "direction",
            ConfigKey::Direction.get(lang),
            value,
            vec![
                SelectOption::new("right", ConfigKey::DirectionRight.get(lang)),
                SelectOption::new("left", ConfigKey::DirectionLeft.get(lang)),
                SelectOption::new("up", ConfigKey::DirectionUp.get(lang)),
                SelectOption::new("down", ConfigKey::DirectionDown.get(lang)),
            ],
        )
    }

    /// Create "Rows" number property with i18n for table
    pub fn rows_count(value: f64) -> Self {
        Self::number("rows_count", ConfigKey::Rows.get(current_language()), value, Some(1.0), Some(20.0))
    }

    /// Create "Columns" number property with i18n for table
    pub fn columns_count(value: f64) -> Self {
        Self::number("cols_count", ConfigKey::Columns.get(current_language()), value, Some(1.0), Some(10.0))
    }

    /// Create "Horizontal Align" select property with i18n
    pub fn h_align(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "text_h_align",
            ConfigKey::HorizontalAlign.get(lang),
            value,
            vec![
                SelectOption::new("start", ConfigKey::AlignLeft.get(lang)),
                SelectOption::new("center", ConfigKey::AlignCenter.get(lang)),
                SelectOption::new("end", ConfigKey::AlignRight.get(lang)),
            ],
        )
    }

    /// Create "Vertical Align" select property with i18n
    pub fn v_align(value: &str) -> Self {
        let lang = current_language();
        Self::select(
            "text_v_align",
            ConfigKey::VerticalAlign.get(lang),
            value,
            vec![
                SelectOption::new("start", ConfigKey::AlignTop.get(lang)),
                SelectOption::new("center", ConfigKey::AlignCenter.get(lang)),
                SelectOption::new("end", ConfigKey::AlignBottom.get(lang)),
            ],
        )
    }

    /// Create the text archetype's own horizontal-align PILLS row
    /// (`text_tabs()`'s own Layout group, W4) — same wire id/value/options
    /// shape as [`Self::h_align`] (`text_h_align`, `PropertyValue::String`,
    /// `start`/`center`/`end`), so a click reaches the exact same
    /// `apply_text_property` "text_h_align" arm every text-archetype type
    /// declaring this row already carries; only the on-screen control
    /// differs (adjacent segments instead of a dropdown) and the labels are
    /// the design's own SHORT abbreviations (`AlignLeftShort`/`AlignCenter`/
    /// `AlignRightShort`) rather than `h_align`'s full words.
    pub fn text_align_pills(value: &str) -> Self {
        let lang = current_language();
        Self::pills(
            "text_h_align",
            ConfigKey::HorizontalAlign.get(lang),
            value,
            vec![
                SelectOption::new("start", ConceptKey::AlignLeftShort.get(lang)),
                SelectOption::new("center", ConfigKey::AlignCenter.get(lang)),
                SelectOption::new("end", ConceptKey::AlignRightShort.get(lang)),
            ],
        )
    }
}

// =============================================================================
// Last-used style snapshot
// =============================================================================
//
// `TemplateStyle` is NOT part of the (now-deleted) legacy `SettingsTemplate`
// system — it is the bubble's per-type "last used" style, captured every
// time a primitive of that type is finalized and replayed onto the NEXT one
// drawn (`DrawingManager::save_last_style_for_primitive` /
// `apply_last_style_snapshot` in `mlc-backend`). Unrelated to
// `mlc_core::templates::PrimitiveTemplate` (a named, user-saved or system
// row in the Template menu) — this has no id, is never shown as a menu row,
// and is overwritten unconditionally on every finalize.

/// Style portion of a template
#[derive(Clone, Debug, Serialize, Deserialize)]
#[derive(Default)]
pub struct TemplateStyle {
    /// Main color
    pub color: Option<String>,
    /// Line width
    pub width: Option<f64>,
    /// Line style
    pub line_style: Option<String>,
    /// Fill color
    pub fill_color: Option<String>,
    /// Fill opacity
    pub fill_opacity: Option<f64>,
    /// Show labels
    pub show_labels: Option<bool>,
    /// Show prices
    pub show_prices: Option<bool>,
    /// Extended per-primitive style properties (e.g. show_labels, line_extend,
    /// label_font_size, etc.) captured via `style_properties()` and restored via
    /// `apply_style_property()` on the next creation of the same primitive type.
    #[serde(default)]
    pub style_properties: Vec<(String, PropertyValue)>,
    /// Full level ladder captured via `Primitive::level_configs()`, restored
    /// via `Primitive::set_level_configs()` on the next primitive of the same
    /// type — the last-used-style analogue of `PrimitiveTemplate::level_configs`.
    /// Empty means either "this primitive type has no ladder" or "captured
    /// before this field existed" (`#[serde(default)]`); both read the same
    /// way on apply: the freshly-created primitive's own default ladder is
    /// left alone rather than cleared.
    #[serde(default)]
    pub level_configs: Vec<FibLevelConfig>,
    /// Per-type text properties captured via `Primitive::text_properties()`,
    /// restored via `Primitive::apply_text_property()` — the last-used-style
    /// analogue of `PrimitiveTemplate::custom_text_properties`. Distinct from
    /// `style_properties` above, which only covers `Primitive::style_properties()`.
    #[serde(default)]
    pub text_properties: Vec<(String, PropertyValue)>,
}
