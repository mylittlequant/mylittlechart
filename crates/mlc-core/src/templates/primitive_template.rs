//! Template for drawing primitive style settings.
//!
//! A [`PrimitiveTemplate`] captures the *style* of an existing primitive (color,
//! width, line style, text formatting, Fibonacci levels, etc.) so that the same
//! look can be applied to new primitives of the same type.  Positional data and
//! content (text) are intentionally excluded — the user fills those in at
//! draw time.
//!
//! # Gating: the target primitive's capability, not the template's payload
//!
//! [`PrimitiveTemplate::apply_to_primitive`] and
//! [`PrimitiveTemplate::matches_primitive`] decide whether to touch the level
//! ladder / typed style / typed text properties by asking the TARGET
//! primitive what it supports, never by asking whether this template's own
//! vector happens to be empty. An empty vector on the template is not "skip
//! this part" — it is a real captured value ("this primitive had no levels
//! / no custom properties set when the template was saved") and applying it
//! is a real, visible write, including a write that clears whatever the
//! target primitive currently holds.
//!
//! This exists to fix a stale-template trap: a template saved by an earlier
//! build of this feature, before the typed payload existed, deserializes
//! today with legitimately empty `level_configs`/`custom_style_properties`/
//! `custom_text_properties` — indistinguishable, field-for-field, from a
//! template honestly saved from a primitive type with no levels or no
//! custom properties. There is no version marker on a template file to
//! tell the two apart, and none is being added retroactively — a marker
//! only helps templates saved after it exists, and the owner confirms
//! there are no saved templates in general circulation to migrate; adding
//! discard-by-guesswork logic here would misfire on every legitimately
//! sparse template (e.g. a plain trend line, which has no levels at all)
//! and silently delete user data instead. So this module does not attempt
//! to detect or discard a stale template. Instead, gating on the
//! primitive's capability makes the previously silent bug loud: applying a
//! stale template now visibly does something (it clears the ladder / resets
//! whatever properties the primitive exposes) rather than being an inert
//! click, so the owner sees the mismatch immediately and can re-save the
//! template on the spot — "Save as…" is a two-second fix once the problem
//! is visible, where a silent no-op was not fixable by the user at all.

use serde::{Deserialize, Serialize};

use crate::drawing::primitives_v2::{
    config::{FibLevelConfig, PropertyValue, TimeframeVisibilityConfig},
    // PrimitiveColor, PrimitiveText, TextAlign, and LineStyle are re-exported
    // from the primitives_v2 module (types sub-module is private).
    PrimitiveColor, PrimitiveText, TextAlign, LineStyle,
    Primitive, PrimitiveData, StyleReference,
};
use crate::utils::unix_timestamp_parts;

// =============================================================================
// PrimitiveTemplate
// =============================================================================

/// Saved style configuration for a drawing primitive.
///
/// Contains everything needed to reproduce a primitive's *appearance* but not
/// its geometry or text content — including the level ladder and the
/// primitive's typed style/text properties. Apply to an existing primitive
/// with [`PrimitiveTemplate::apply_to_primitive`] (or
/// [`PrimitiveTemplate::apply_to_primitive_data`] for just the flat fields,
/// when only a `PrimitiveData` is available).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimitiveTemplate {
    /// Unique identifier generated at creation time.
    /// Format: `"ptmpl_{unix_secs}_{nanos_suffix}"`.
    pub id: String,
    /// User-visible display name for this template.
    pub name: String,
    /// Primitive type this template applies to (e.g. `"trend_line"`, `"fib_retracement"`).
    pub type_id: String,

    // ---- Core style --------------------------------------------------------
    /// Stroke and optional fill color.
    pub color: PrimitiveColor,
    /// Line width in pixels.
    pub width: f64,
    /// Line dash style.
    pub style: LineStyle,

    // ---- Text formatting (content is not saved — user provides it) ---------
    /// Font size in pixels.
    #[serde(default)]
    pub text_font_size: Option<f64>,
    /// Text color override (hex string).  `None` means inherit stroke color.
    #[serde(default)]
    pub text_color: Option<String>,
    /// Bold text.
    #[serde(default)]
    pub text_bold: Option<bool>,
    /// Italic text.
    #[serde(default)]
    pub text_italic: Option<bool>,
    /// Horizontal alignment string (`"start"`, `"center"`, `"end"`).
    #[serde(default)]
    pub text_h_align: Option<String>,
    /// Vertical alignment string (`"start"`, `"center"`, `"end"`).
    #[serde(default)]
    pub text_v_align: Option<String>,

    // ---- Level configuration (Fibonacci, Gann, etc.) -----------------------
    /// Per-level configuration for multi-level primitives.
    #[serde(default)]
    pub level_configs: Vec<FibLevelConfig>,

    // ---- Type-specific style properties -------------------------------------
    /// Per-type style properties (e.g. a pitchfork's `level_mode`, a fib's
    /// `extend_left`/`extend_right`/`show_trend_line`), captured via
    /// [`Primitive::style_properties`] and restored via
    /// [`Primitive::apply_style_property`]. Populated by [`Self::from_primitive`]
    /// (empty from [`Self::from_primitive_data`], which has no primitive to
    /// query). `#[serde(default)]` so an existing template on disk without
    /// this key loads with an empty vector rather than failing to parse.
    #[serde(default)]
    pub custom_style_properties: Vec<(String, PropertyValue)>,

    /// Per-type text properties (e.g. a table's `rows_count`, a note's
    /// `content` formatting toggles), captured via
    /// [`Primitive::text_properties`] and restored via
    /// [`Primitive::apply_text_property`]. Distinct from the flat
    /// `text_font_size`/`text_color`/... fields above, which cover the
    /// shared `PrimitiveText` block rather than per-type text settings.
    #[serde(default)]
    pub custom_text_properties: Vec<(String, PropertyValue)>,

    // ---- Timeframe visibility ----------------------------------------------
    /// Which timeframes this primitive shows on — [`PrimitiveData::timeframe_visibility`]
    /// verbatim. `None` means "all timeframes" (the primitive's own default,
    /// see [`crate::drawing::primitives_v2::config::TimeframeVisibilityConfig::is_visible_on`]'s
    /// fallback), the same as an unset flat style field: captured, applied,
    /// and compared exactly like `color`/`width`/`style` above, never gated
    /// on the target's capability (every primitive carries this field,
    /// unlike the level ladder or the typed style/text properties, which
    /// only some primitive types expose). `#[serde(default)]` so a template
    /// saved before this field existed loads as "all timeframes" rather
    /// than failing to parse.
    #[serde(default)]
    pub timeframe_visibility: Option<TimeframeVisibilityConfig>,
}

impl PrimitiveTemplate {
    // ---- Constructors ------------------------------------------------------

    /// Create a template with sensible defaults for the given `type_id`.
    pub fn new_with_defaults(type_id: &str, name: &str) -> Self {
        let (secs, nanos) = unix_timestamp_parts();
        Self {
            id: format!("ptmpl_{}_{}", secs, nanos),
            name: name.to_string(),
            type_id: type_id.to_string(),
            color: PrimitiveColor::default(),
            width: 1.0,
            style: LineStyle::default(),
            text_font_size: None,
            text_color: None,
            text_bold: None,
            text_italic: None,
            text_h_align: None,
            text_v_align: None,
            level_configs: Vec::new(),
            custom_style_properties: Vec::new(),
            custom_text_properties: Vec::new(),
            timeframe_visibility: None,
        }
    }

    /// Extract style fields from an existing [`PrimitiveData`] into a new template.
    ///
    /// The template name is taken from `name`.  Positional data and text content
    /// are not copied.  Level configuration and per-type style/text properties
    /// are not available from a bare `PrimitiveData` (they live on the concrete
    /// primitive impl) — use [`Self::from_primitive`] to capture those too.
    pub fn from_primitive_data(data: &PrimitiveData, name: &str) -> Self {
        let (secs, nanos) = unix_timestamp_parts();

        let (text_font_size, text_color, text_bold, text_italic, text_h_align, text_v_align) =
            if let Some(text) = &data.text {
                (
                    Some(text.font_size),
                    text.color.clone(),
                    Some(text.bold),
                    Some(text.italic),
                    Some(text.h_align.as_str().to_string()),
                    Some(text.v_align.as_str().to_string()),
                )
            } else {
                (None, None, None, None, None, None)
            };

        Self {
            id: format!("ptmpl_{}_{}", secs, nanos),
            name: name.to_string(),
            type_id: data.type_id.clone(),
            color: data.color.clone(),
            width: data.width,
            style: data.style,
            text_font_size,
            text_color,
            text_bold,
            text_italic,
            text_h_align,
            text_v_align,
            // Level configs and both typed-property vectors are not
            // available on PrimitiveData directly (they live in the concrete
            // primitive impl) — from_primitive fills all three.
            level_configs: Vec::new(),
            custom_style_properties: Vec::new(),
            custom_text_properties: Vec::new(),
            // Timeframe visibility IS a flat `PrimitiveData` field, unlike
            // the three above — captured here, not in `from_primitive`.
            timeframe_visibility: data.timeframe_visibility.clone(),
        }
    }

    /// Extract EVERYTHING a template can capture from a live primitive: the
    /// flat style fields [`Self::from_primitive_data`] already reads, PLUS
    /// the level ladder ([`Primitive::level_configs`]) and the primitive's
    /// typed style/text properties ([`Primitive::style_properties`] /
    /// [`Primitive::text_properties`]). Readonly properties are skipped —
    /// nothing to restore, mirrors [`Primitive::apply_style_property`]'s own
    /// unwritable contract for them.
    ///
    /// This is the constructor the "Save as…" UI path uses;
    /// [`Self::from_primitive_data`] stays as the flat-fields-only building
    /// block it wraps, for callers that only ever have a `PrimitiveData`
    /// snapshot (no `dyn Primitive` to query further).
    pub fn from_primitive(prim: &dyn Primitive, name: &str) -> Self {
        let mut tmpl = Self::from_primitive_data(prim.data(), name);
        if let Some(configs) = prim.level_configs() {
            tmpl.level_configs = configs;
        }
        tmpl.custom_style_properties = prim
            .style_properties()
            .into_iter()
            .filter(|p| !p.readonly)
            .map(|p| (p.id, p.value))
            .collect();
        if let Some(props) = prim.text_properties() {
            tmpl.custom_text_properties = props
                .into_iter()
                .filter(|p| !p.readonly)
                .map(|p| (p.id, p.value))
                .collect();
        }
        tmpl
    }

    // ---- Application -------------------------------------------------------

    /// Apply the template's style to an existing [`PrimitiveData`].
    ///
    /// Style, timeframe visibility, and text formatting are written;
    /// positional data, visibility, lock state, sync mode, and text content
    /// are left untouched. Level configuration and per-type style/text
    /// properties live on the concrete primitive impl, not on
    /// `PrimitiveData` — use [`Self::apply_to_primitive`] to write those too.
    ///
    /// Also stamps [`PrimitiveData::style_reference`] to
    /// `StyleReference::Template(self.id.clone())` — applying a template
    /// makes it the reference the primitive is compared against from now
    /// on ("Save as…" shows again once the primitive drifts from THIS
    /// template, not from whatever was applied or edited before it).
    pub fn apply_to_primitive_data(&self, data: &mut PrimitiveData) {
        data.color = self.color.clone();
        data.width = self.width;
        data.style = self.style;
        data.timeframe_visibility = self.timeframe_visibility.clone();
        data.style_reference = Some(StyleReference::Template(self.id.clone()));

        // Apply text formatting if the primitive already has a text block.
        if let Some(text) = &mut data.text {
            apply_text_template(text, self);
        }
    }

    /// Apply EVERYTHING this template carries to a live primitive: the flat
    /// style fields [`Self::apply_to_primitive_data`] already writes, PLUS
    /// the level ladder and the typed style/text properties.
    ///
    /// The ladder is written whenever `prim` itself supports one
    /// ([`Primitive::level_configs`] returns `Some`), regardless of whether
    /// this template's own `level_configs` happens to be empty — gated on
    /// the TARGET's capability, not on what this template holds. A
    /// level-capable primitive always gets `self.level_configs` written
    /// verbatim, which legitimately clears its ladder when the template's
    /// own is empty — that is deliberate, see the module-level note on why
    /// an apply is never a silent no-op.
    ///
    /// Each style/text property is applied one at a time through the
    /// primitive's own [`Primitive::apply_style_property`] /
    /// [`Primitive::apply_text_property`], which silently ignores any id it
    /// does not recognize — so applying a template saved from one primitive
    /// type onto a different type only carries over whatever id happens to
    /// make sense on the target, never forces anything foreign onto it, and
    /// never panics.
    pub fn apply_to_primitive(&self, prim: &mut dyn Primitive) {
        self.apply_to_primitive_data(prim.data_mut());
        if prim.level_configs().is_some() {
            prim.set_level_configs(self.level_configs.clone());
        }
        // Load-bearing order: this iterates `custom_style_properties` in
        // CAPTURED order (`from_primitive`'s `style_properties().into_iter()
        // ...collect()`, itself the primitive's own declaration-push order —
        // never re-sorted by `ConfigProperty::order`), not applied-in-any-
        // order. That is what keeps a label's "inherit line colour" toggle
        // (`"label_color_inherit"`, `LabelStyle::style_properties`) correct
        // after a template round-trip: every caller of
        // `LabelStyle::style_properties` pushes its own `"label_color"` row
        // first and only THEN extends with the label-style block, whose own
        // first entry is `"label_color_inherit"` — so the inherit row is
        // always captured, and therefore always applied, strictly after
        // `"label_color"`. `LabelStyle::apply_property`'s inherit-ON branch
        // writes `self.color = None` unconditionally, so applying it last
        // is what lets a saved "inherit" template restore the link instead
        // of the `"label_color"` write two steps earlier silently pinning a
        // concrete colour back in.
        for (id, value) in &self.custom_style_properties {
            prim.apply_style_property(id, value);
        }
        for (id, value) in &self.custom_text_properties {
            prim.apply_text_property(id, value);
        }
    }

    /// Returns `true` if this template stores level configuration.
    pub fn has_levels(&self) -> bool {
        !self.level_configs.is_empty()
    }

    /// Whether `data`'s FLAT style fields already match everything this
    /// template itself would capture into those same fields if it were
    /// saved fresh from `data` right now — the base check
    /// [`Self::matches_primitive`] builds on. Deliberately stops at the flat
    /// fields (never `level_configs`/`custom_style_properties`/
    /// `custom_text_properties`, none of which a bare `PrimitiveData` can
    /// answer for) — see [`Self::matches_primitive`] for the full check that
    /// also covers those.
    pub fn matches_primitive_data(&self, data: &PrimitiveData) -> bool {
        if self.color != data.color {
            return false;
        }
        if (self.width - data.width).abs() > f64::EPSILON {
            return false;
        }
        if self.style != data.style {
            return false;
        }
        if self.timeframe_visibility != data.timeframe_visibility {
            return false;
        }
        match &data.text {
            Some(text) => {
                self.text_font_size == Some(text.font_size)
                    && self.text_color == text.color
                    && self.text_bold == Some(text.bold)
                    && self.text_italic == Some(text.italic)
                    && self.text_h_align.as_deref() == Some(text.h_align.as_str())
                    && self.text_v_align.as_deref() == Some(text.v_align.as_str())
            }
            None => {
                self.text_font_size.is_none()
                    && self.text_color.is_none()
                    && self.text_bold.is_none()
                    && self.text_italic.is_none()
                    && self.text_h_align.is_none()
                    && self.text_v_align.is_none()
            }
        }
    }

    /// Full drift check. On top of [`Self::matches_primitive_data`]'s flat
    /// fields (which now include timeframe visibility), this also compares
    /// the level ladder and the typed style/text properties, so an edit to
    /// ONLY the ladder, ONLY timeframe visibility, or only a per-type toggle
    /// like a fib's `show_trend_line` — correctly reports as drift. `true`
    /// means `prim`'s live settings equal what `self` captured; `false`
    /// means they have diverged.
    ///
    /// Two callers, same question asked two different ways:
    /// [`crate::templates::PrimitiveTemplate`] rows in the merged Template
    /// menu list ask it of a SAVED template ("is this primitive already
    /// running this saved look?"); the settings modal's own "Save as…"
    /// visibility (`mlc-app-layout::layout::modals::primitive_settings`)
    /// asks it of whatever [`PrimitiveData::style_reference`] resolves to —
    /// a template looked up by id, or a system level-set preset's own
    /// fixed definition — to decide whether anything has moved since that
    /// reference was established. Both readings use this same function;
    /// only the object on the left of the comparison changes.
    ///
    /// Every section below gates on what `prim` itself exposes, never on
    /// whether this template's own field happens to be empty — see the
    /// [`Self::apply_to_primitive`] doc for why. A level-capable primitive's
    /// ladder is ALWAYS compared, even against an empty
    /// `self.level_configs` (that then correctly reads as drift, since the
    /// template captured no ladder for a primitive that has one); a
    /// primitive with no ladder at all (`level_configs()` returns `None`)
    /// is never compared, because there is nothing on either side to
    /// compare. `custom_style_properties`/`custom_text_properties` gate the
    /// identical way, keyed off `prim.style_properties()` /
    /// `prim.text_properties()` — the primitive's own currently-exposed
    /// property ids — rather than off `self`'s vectors.
    pub fn matches_primitive(&self, prim: &dyn Primitive) -> bool {
        if !self.matches_primitive_data(prim.data()) {
            return false;
        }
        if let Some(current_levels) = prim.level_configs() {
            if self.level_configs != current_levels {
                return false;
            }
        }
        let current_style_props = prim.style_properties();
        let style_props_match = current_style_props.iter().all(|p| {
            self.custom_style_properties
                .iter()
                .find(|(id, _)| id == &p.id)
                .map(|(_, value)| value)
                == Some(&p.value)
        });
        if !style_props_match {
            return false;
        }
        if let Some(current_text_props) = prim.text_properties() {
            let text_props_match = current_text_props.iter().all(|p| {
                self.custom_text_properties
                    .iter()
                    .find(|(id, _)| id == &p.id)
                    .map(|(_, value)| value)
                    == Some(&p.value)
            });
            if !text_props_match {
                return false;
            }
        }
        true
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Write template text fields onto an existing [`PrimitiveText`].
fn apply_text_template(text: &mut PrimitiveText, tmpl: &PrimitiveTemplate) {
    if let Some(size) = tmpl.text_font_size {
        text.font_size = size;
    }
    if tmpl.text_color.is_some() {
        text.color = tmpl.text_color.clone();
    }
    if let Some(bold) = tmpl.text_bold {
        text.bold = bold;
    }
    if let Some(italic) = tmpl.text_italic {
        text.italic = italic;
    }
    if let Some(ref align) = tmpl.text_h_align {
        text.h_align = TextAlign::from_str(align);
    }
    if let Some(ref align) = tmpl.text_v_align {
        text.v_align = TextAlign::from_str(align);
    }
}
