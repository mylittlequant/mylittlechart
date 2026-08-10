//! State for the concept-modal renderer (`super::render`).
//!
//! Interactive [`Row`](super::spec::Row) values — `Chk`/`Chk3`/`Mode`/
//! `Pills`/`Icons`/`Stepper` — are seeded from `spec.rs`'s own defaults the
//! moment a target opens ([`open_type`]/[`open_modal`]) and mutated in place
//! thereafter through the accessor/mutator pairs below (`Ladder`'s own
//! per-level checkbox and `Table`'s own per-cell `Check` toggle are
//! interactive too, but sub-keyed under a row's own base key rather than
//! seeded as a whole `RowValue` — see `render_collections`'s own doc).
//! `render.rs`/`render_collections.rs` never read a `spec.rs` default for
//! one of these directly — only through this state — so a toggled value
//! survives a re-render without re-deriving "is this still the default".
//!
//! Wiring click events into these mutators (parsing an `InputCoordinator`
//! response into a `(target, key)` pair) is the NEXT task's job — this file
//! only has to make the mutation itself correct and cheap to call once that
//! wiring exists.

use std::collections::HashMap;

use mlc_app_layout::ui::scroll_widget::ScrollState;

use super::spec::{ConceptModal, ConceptTab, ConceptType, Row};

/// Which archetype surface is currently open — one of the 14 object-bound
/// [`ConceptType`]s or one of the 9 [`ConceptModal`]s. Both id spaces are
/// `spec.rs`'s own fixed, approved data (never user input), so a bare id
/// plus which kind it names is enough to look the target back up in the
/// registry (`super::render`'s own `lookup_target`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConceptTarget {
    Type(&'static str),
    Modal(&'static str),
}

impl ConceptTarget {
    pub fn id(&self) -> &'static str {
        match self {
            ConceptTarget::Type(id) | ConceptTarget::Modal(id) => id,
        }
    }
}

/// One interactive row's live value — the subset of [`Row`] variants this
/// task draws as real, clickable widgets (`Chk`/`Chk3`/`Mode`/`Pills`).
/// Every other `Row` variant reads its display value straight off
/// `spec.rs` and never appears here.
#[derive(Clone, Debug)]
pub enum RowValue {
    Chk(bool),
    /// One entry per [`Row::Chk3`] item, in the same order — kept as a
    /// `Vec` rather than `[bool; 3]` so this state never has to assume the
    /// trio is exactly three (the spec's own doc says it always is; this
    /// state does not need to re-assert that to stay correct).
    Chk3(Vec<bool>),
    /// Selected segment index — the identical "one of N, by index" value
    /// [`Row::Mode`] and [`Row::Pills`] both carry, despite drawing
    /// different widgets. [`Row::Icons`] reuses this too — an icon grid
    /// selection is "one of N, by index" as well, just drawn as a wrapping
    /// grid instead of one line.
    Segment(usize),
    /// [`Row::Stepper`]'s own live count.
    Stepper(i32),
}

/// State for the concept-modal renderer: which target is open, the active
/// tab per target, every interactive row's live value, and the body's own
/// scroll/drag geometry.
#[derive(Clone, Debug, Default)]
pub struct ConceptModalState {
    /// The target currently drawn, or `None` — mirrors
    /// `PrimitiveSettingsState::primitive_id`'s own "no target, nothing to
    /// draw" convention.
    pub open: Option<ConceptTarget>,
    /// Active tab id, keyed by target id (`ConceptType::id`/
    /// `ConceptModal::id`). A target not yet opened this session has no
    /// entry — [`active_tab_id`](Self::active_tab_id) falls back to that
    /// target's own first tab, the same "stale beats nothing, first tab
    /// beats stale" rule `primitive_settings::resolve_active_tab` uses for
    /// the real modal.
    active_tab: HashMap<&'static str, &'static str>,
    /// Live value of every interactive row, keyed
    /// `"{target_id}:{tab_id}:{group_index}:{row_index}"` (see
    /// [`row_key`]). Seeded from `spec.rs`'s own defaults on open
    /// ([`open_type`](Self::open_type)/[`open_modal`](Self::open_modal)),
    /// so a key is always present for a row this state has ever drawn —
    /// every accessor below still falls back to a caller-supplied default
    /// for the rare case a key was never seeded (a render against a target
    /// that was never opened through this state).
    row_values: HashMap<String, RowValue>,
    /// Body scroll position — the same `ScrollState`/`ScrollableContainer`
    /// pairing every other modal in this app uses.
    pub scroll: ScrollState,
    /// Position the user dragged this modal to, or `None` for the default
    /// centred placement.
    pub position: Option<(f64, f64)>,
    /// Size the user dragged this modal to, or `None` for the
    /// content-derived default.
    pub size: Option<(f64, f64)>,
}

impl ConceptModalState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    pub fn close(&mut self) {
        self.open = None;
    }

    /// Open one of the 14 object-bound archetypes.
    pub fn open_type(&mut self, concept_type: &ConceptType) {
        self.open_target(ConceptTarget::Type(concept_type.id), &concept_type.tabs);
    }

    /// Open one of the 9 application-level modals.
    pub fn open_modal(&mut self, concept_modal: &ConceptModal) {
        self.open_target(ConceptTarget::Modal(concept_modal.id), &concept_modal.tabs);
    }

    fn open_target(&mut self, target: ConceptTarget, tabs: &[ConceptTab]) {
        self.open = Some(target);
        self.scroll.reset();
        self.position = None;
        self.size = None;
        let id = target.id();
        self.active_tab.insert(id, first_tab_id(tabs));
        seed_rows(&mut self.row_values, id, tabs);
    }

    /// `active_tab`'s value for `target_id` if it names one of `tabs`,
    /// otherwise `tabs`' own first entry — mirrors
    /// `primitive_settings::resolve_active_tab`'s fallback rule. Reads back
    /// `""` rather than indexing on an empty `tabs`, which never happens in
    /// practice (every `ConceptType`/`ConceptModal` in `spec.rs` carries at
    /// least one tab) but must not panic if it ever did.
    pub fn active_tab_id<'a>(&self, target_id: &str, tabs: &'a [ConceptTab]) -> &'a str {
        if let Some(stored) = self.active_tab.get(target_id) {
            if let Some(tab) = tabs.iter().find(|t| t.id == *stored) {
                return tab.id;
            }
        }
        tabs.first().map(|t| t.id).unwrap_or("")
    }

    pub fn set_active_tab(&mut self, target_id: &'static str, tab_id: &'static str) {
        self.active_tab.insert(target_id, tab_id);
        self.scroll.reset();
    }

    // =========================================================================
    // Row value accessors/mutators — one pair per interactive `Row` shape.
    // =========================================================================

    pub fn chk(&self, key: &str, default: bool) -> bool {
        match self.row_values.get(key) {
            Some(RowValue::Chk(v)) => *v,
            _ => default,
        }
    }

    pub fn toggle_chk(&mut self, key: &str, default: bool) {
        let next = !self.chk(key, default);
        self.row_values.insert(key.to_string(), RowValue::Chk(next));
    }

    /// `default_items` supplies the fallback when `key` was never seeded —
    /// each entry's own `bool` half, in order.
    pub fn chk3(&self, key: &str, default_items: &[(String, bool)]) -> Vec<bool> {
        match self.row_values.get(key) {
            Some(RowValue::Chk3(v)) => v.clone(),
            _ => default_items.iter().map(|(_, on)| *on).collect(),
        }
    }

    pub fn toggle_chk3(&mut self, key: &str, index: usize, default_items: &[(String, bool)]) {
        let mut values = self.chk3(key, default_items);
        if let Some(v) = values.get_mut(index) {
            *v = !*v;
        }
        self.row_values.insert(key.to_string(), RowValue::Chk3(values));
    }

    pub fn segment(&self, key: &str, default: usize) -> usize {
        match self.row_values.get(key) {
            Some(RowValue::Segment(v)) => *v,
            _ => default,
        }
    }

    pub fn set_segment(&mut self, key: &str, index: usize) {
        self.row_values.insert(key.to_string(), RowValue::Segment(index));
    }

    pub fn stepper(&self, key: &str, default: i32) -> i32 {
        match self.row_values.get(key) {
            Some(RowValue::Stepper(v)) => *v,
            _ => default,
        }
    }

    pub fn bump_stepper(&mut self, key: &str, default: i32, delta: i32) {
        let next = self.stepper(key, default) + delta;
        self.row_values.insert(key.to_string(), RowValue::Stepper(next));
    }
}

/// The stable id one row's live value is keyed under —
/// `"{target_id}:{tab_id}:{group_index}:{row_index}"`, exactly the shape
/// the launching brief specifies.
pub fn row_key(target_id: &str, tab_id: &str, group_index: usize, row_index: usize) -> String {
    format!("{target_id}:{tab_id}:{group_index}:{row_index}")
}

fn first_tab_id(tabs: &[ConceptTab]) -> &'static str {
    tabs.first().map(|t| t.id).unwrap_or("")
}

/// Populate `row_values` with every interactive row's default straight off
/// `spec.rs` — walks every tab/group/row exactly once per open, so a value
/// this state has never drawn simply has no key (its accessor falls back to
/// a caller-supplied default instead, see the accessors above).
fn seed_rows(row_values: &mut HashMap<String, RowValue>, target_id: &str, tabs: &[ConceptTab]) {
    for tab in tabs {
        for (group_index, group) in tab.groups.iter().enumerate() {
            for (row_index, row) in group.rows.iter().enumerate() {
                let value = match row {
                    Row::Chk { on, .. } => Some(RowValue::Chk(*on)),
                    Row::Chk3 { items } => Some(RowValue::Chk3(items.iter().map(|(_, on)| *on).collect())),
                    Row::Mode { selected, .. } | Row::Pills { selected, .. } | Row::Icons { selected, .. } => {
                        Some(RowValue::Segment(*selected))
                    }
                    Row::Stepper { value, .. } => Some(RowValue::Stepper(*value)),
                    _ => None,
                };
                if let Some(value) = value {
                    row_values.insert(row_key(target_id, tab.id, group_index, row_index), value);
                }
            }
        }
    }
}
