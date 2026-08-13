//! Elliott wave chart-gesture bindings — which physical input gesture opens
//! which wave-editing menu (owner ask 2026-08-13: "вопрос только на что это
//! вешать на даблклик или на ПКМ хз, дискуссионно... проектируй так чтобы
//! легко было подставить потом одним переменной хоткея"). Pure data + one
//! lookup method — no input handling, no wiring into any handler. The input
//! layer (a later step, in another crate) is meant to ask this ONE question
//! ("this trigger landed on this kind of target, what does it mean?") and
//! never match on double-click vs right-click itself; that indirection is
//! the entire point of this module existing separately from wherever the
//! gesture is actually consumed.

use serde::{Deserialize, Serialize};

/// A physical input gesture the chart's own input layer can recognize.
///
/// Deliberately a flat enum, not a bitmask or a raw key/button code — every
/// binding in [`GestureBindings`] names ONE of these, and switching which
/// physical gesture a binding uses (e.g. double-click → right-click) is a
/// one-field edit on [`GestureBindings`], never a change to this enum or to
/// whatever eventually matches on it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum GestureTrigger {
    /// Two rapid primary-button clicks on the same target.
    DoubleClick,
    /// A secondary (context-menu) button click.
    RightClick,
}

/// A wave-editing menu a [`GestureTrigger`] can open, once resolved against
/// a [`GestureTarget`] via [`GestureBindings::gesture_for`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ChartGesture {
    /// The per-wave menu (e.g. seed a child into this wave's own slot) —
    /// opened on a hit against one of a structure's own wave SEGMENTS.
    WaveMenu,
    /// The successor menu (e.g. continue this structure with the next
    /// same-degree structure) — opened on a hit against a structure's own
    /// TERMINAL point (its last drawn pivot).
    SuccessorMenu,
    /// The predecessor menu (e.g. build a NEW structure that this one
    /// continues from) — opened on a hit against a structure's own ORIGIN
    /// point (its first drawn pivot). [`SuccessorMenu`](Self::SuccessorMenu)
    /// read backwards (owner ask 2026-08-14: "щас мы чейн можем только с
    /// конца строить, а хорошо бы... делать достройку перед началом
    /// чейна" — a count is read backwards as often as forwards).
    PredecessorMenu,
}

/// The kind of chart element a [`GestureTrigger`] landed on — the other half
/// of the question [`GestureBindings::gesture_for`] answers. A hit-test
/// result (already resolved to "this IS a wave segment", "this IS a
/// terminal point", or "this IS an origin point" by the caller) rather than
/// raw geometry — this module owns no hit-testing of its own.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GestureTarget {
    /// A hit on one of a structure's own wave segments (not its terminal or
    /// origin point).
    WaveSegment,
    /// A hit on a structure's own terminal point — its last drawn pivot,
    /// the point a successor would continue from.
    TerminalPoint,
    /// A hit on a structure's own origin point — its FIRST drawn pivot, the
    /// point a NEW predecessor would continue INTO. The mirror of
    /// [`TerminalPoint`](Self::TerminalPoint); see that variant's own doc.
    OriginPoint,
}

/// Which physical gesture opens which wave-editing menu — the whole
/// configurable surface this module exists for. A settings blob field, not
/// a hardcoded match in the input layer (see the module doc's own "one
/// field" framing).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct GestureBindings {
    /// The gesture that opens [`ChartGesture::WaveMenu`] on a
    /// [`GestureTarget::WaveSegment`] hit.
    #[serde(default = "default_wave_menu_trigger")]
    pub wave_menu: GestureTrigger,
    /// The gesture that opens [`ChartGesture::SuccessorMenu`] on a
    /// [`GestureTarget::TerminalPoint`] hit.
    #[serde(default = "default_successor_menu_trigger")]
    pub successor_menu: GestureTrigger,
    /// The gesture that opens [`ChartGesture::PredecessorMenu`] on a
    /// [`GestureTarget::OriginPoint`] hit.
    #[serde(default = "default_predecessor_menu_trigger")]
    pub predecessor_menu: GestureTrigger,
}

fn default_wave_menu_trigger() -> GestureTrigger {
    GestureTrigger::DoubleClick
}

fn default_successor_menu_trigger() -> GestureTrigger {
    GestureTrigger::DoubleClick
}

fn default_predecessor_menu_trigger() -> GestureTrigger {
    GestureTrigger::DoubleClick
}

impl Default for GestureBindings {
    /// All three menus open on double-click — a STARTING default the owner
    /// explicitly left open to revisit ("дискуссионно"), not a settled
    /// design decision. Stated here as a default meant to be flipped, e.g.
    /// to `RightClick` for one or more menus, once the owner picks —
    /// exactly the one-field edit [`GestureBindings::gesture_for`]'s own doc
    /// describes.
    fn default() -> Self {
        Self {
            wave_menu: default_wave_menu_trigger(),
            successor_menu: default_successor_menu_trigger(),
            predecessor_menu: default_predecessor_menu_trigger(),
        }
    }
}

impl GestureBindings {
    /// The [`ChartGesture`] `trigger` opens when it lands on `target`, under
    /// these bindings — `None` when `trigger` is not the one bound to
    /// whichever menu `target` maps to (e.g. a right-click on a wave segment
    /// while [`Self::wave_menu`] is still bound to [`GestureTrigger::DoubleClick`]).
    ///
    /// The ONE method the input layer calls — it never matches on
    /// `GestureTrigger` itself (see the module doc).
    pub fn gesture_for(&self, trigger: GestureTrigger, target: GestureTarget) -> Option<ChartGesture> {
        match target {
            GestureTarget::WaveSegment if trigger == self.wave_menu => Some(ChartGesture::WaveMenu),
            GestureTarget::TerminalPoint if trigger == self.successor_menu => Some(ChartGesture::SuccessorMenu),
            GestureTarget::OriginPoint if trigger == self.predecessor_menu => Some(ChartGesture::PredecessorMenu),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_bindings_are_all_double_click() {
        let bindings = GestureBindings::default();
        assert_eq!(bindings.wave_menu, GestureTrigger::DoubleClick);
        assert_eq!(bindings.successor_menu, GestureTrigger::DoubleClick);
        assert_eq!(bindings.predecessor_menu, GestureTrigger::DoubleClick);
    }

    #[test]
    fn default_bindings_resolve_all_three_gestures_on_double_click() {
        let bindings = GestureBindings::default();
        assert_eq!(
            bindings.gesture_for(GestureTrigger::DoubleClick, GestureTarget::WaveSegment),
            Some(ChartGesture::WaveMenu)
        );
        assert_eq!(
            bindings.gesture_for(GestureTrigger::DoubleClick, GestureTarget::TerminalPoint),
            Some(ChartGesture::SuccessorMenu)
        );
        assert_eq!(
            bindings.gesture_for(GestureTrigger::DoubleClick, GestureTarget::OriginPoint),
            Some(ChartGesture::PredecessorMenu)
        );
    }

    #[test]
    fn unbound_trigger_resolves_to_none() {
        let bindings = GestureBindings::default();
        // Right-click is not bound to anything under the default — none of
        // the three menus open.
        assert_eq!(bindings.gesture_for(GestureTrigger::RightClick, GestureTarget::WaveSegment), None);
        assert_eq!(bindings.gesture_for(GestureTrigger::RightClick, GestureTarget::TerminalPoint), None);
        assert_eq!(bindings.gesture_for(GestureTrigger::RightClick, GestureTarget::OriginPoint), None);
    }

    #[test]
    fn flipping_one_binding_is_a_one_field_edit() {
        // The whole point of this module: switching wave_menu to right-click
        // is ONE field, and successor_menu/predecessor_menu are untouched.
        let bindings = GestureBindings { wave_menu: GestureTrigger::RightClick, ..GestureBindings::default() };
        assert_eq!(
            bindings.gesture_for(GestureTrigger::RightClick, GestureTarget::WaveSegment),
            Some(ChartGesture::WaveMenu)
        );
        assert_eq!(bindings.gesture_for(GestureTrigger::DoubleClick, GestureTarget::WaveSegment), None);
        assert_eq!(
            bindings.gesture_for(GestureTrigger::DoubleClick, GestureTarget::TerminalPoint),
            Some(ChartGesture::SuccessorMenu)
        );
        assert_eq!(
            bindings.gesture_for(GestureTrigger::DoubleClick, GestureTarget::OriginPoint),
            Some(ChartGesture::PredecessorMenu)
        );
    }

    #[test]
    fn bindings_round_trip_through_json() {
        let bindings = GestureBindings {
            wave_menu: GestureTrigger::RightClick,
            successor_menu: GestureTrigger::DoubleClick,
            predecessor_menu: GestureTrigger::RightClick,
        };
        let json = serde_json::to_string(&bindings).expect("GestureBindings serializes");
        let restored: GestureBindings = serde_json::from_str(&json).expect("GestureBindings deserializes");
        assert_eq!(bindings, restored);
    }

    #[test]
    fn missing_fields_fall_back_to_double_click_default() {
        // A persisted settings blob written before this struct existed —
        // all three fields absent — still loads (`#[serde(default)]` on
        // each).
        let restored: GestureBindings = serde_json::from_str("{}").expect("empty object deserializes via field defaults");
        assert_eq!(restored, GestureBindings::default());
    }
}
