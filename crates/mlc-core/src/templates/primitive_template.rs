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
    config::{level_set_label_defaults, FibLevelConfig, PropertyValue, TimeframeVisibilityConfig},
    // PrimitiveColor, PrimitiveText, TextAlign, and LineStyle are re-exported
    // from the primitives_v2 module (types sub-module is private).
    PrimitiveColor, PrimitiveText, TextAlign, LineStyle,
    Primitive, PrimitiveData, StyleReference,
};
use crate::i18n::{current_language, StyleKey};
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

    /// Per-type LEVEL properties (`Primitive::level_properties`) — the third
    /// declared family, e.g. a pitchfork's `level_mode` / `extend` /
    /// `show_labels`. Captured and restored exactly like the two above.
    ///
    /// Added 2026-08-09, when `level_properties()` was folded into the
    /// modal's generic declared set: before that these controls were
    /// unreachable in the UI, so nothing missed them here either. The moment
    /// they became reachable, a template that did not carry them would have
    /// silently dropped a type's own settings on every save — which is the
    /// one thing a template must never do.
    #[serde(default)]
    pub custom_level_properties: Vec<(String, PropertyValue)>,

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

    // ---- System vs. user --------------------------------------------------
    /// Whether this is a system (built-in) row — synthesized fresh by
    /// [`Self::system_templates`] every time the Template menu needs one,
    /// never persisted, never renameable or deletable. `#[serde(default)]`
    /// so a template already on disk — which predates this field and is,
    /// definitionally, user-saved (a system row is never written to disk)
    /// — loads as `false`.
    #[serde(default)]
    pub builtin: bool,

    // ---- Fill master (level-band fill on/off + opacity) ---------------------
    /// `(show_fill, fill_opacity)` verbatim from [`Primitive::fill_master`],
    /// or `None` for a type that has no band fill of its own.
    ///
    /// These two are real fields on the primitive but deliberately NOT
    /// declared as `ConfigProperty`s (see `fibonacci::retracement`'s own
    /// `fill_master` doc), so `custom_style_properties` — which captures only
    /// declared properties — never saw them. A template that cannot carry
    /// them cannot regulate the band fill at all: applying one left whatever
    /// opacity the target already had, which is how a system template stopped
    /// being "the whole settings" (owner-reported, 2026-08-09). Written back
    /// through the same `apply_style_property` ids the write path already
    /// owns — `"fill"`, and `"fill_opacity"` in PERCENT, which is what that
    /// setter expects.
    #[serde(default)]
    pub fill_master: Option<(bool, f64)>,
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
            custom_level_properties: Vec::new(),
            timeframe_visibility: None,
            builtin: false,
            fill_master: None,
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
            custom_level_properties: Vec::new(),
            // Timeframe visibility IS a flat `PrimitiveData` field, unlike
            // the three above — captured here, not in `from_primitive`.
            timeframe_visibility: data.timeframe_visibility.clone(),
            builtin: false,
            // Like the three above, this one needs the concrete primitive —
            // `from_primitive` fills it.
            fill_master: None,
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
        tmpl.custom_level_properties = prim
            .level_properties()
            .into_iter()
            .filter(|p| !p.readonly)
            .map(|p| (p.id, p.value))
            .collect();
        tmpl.fill_master = prim.fill_master();
        tmpl
    }

    // ---- System (built-in) templates ---------------------------------------

    /// The [`Self::builtin`] rows this primitive's TYPE offers in the
    /// Template menu — never persisted, never user-editable, recomputed
    /// fresh every call from `prim`'s own current settings.
    ///
    /// `prim` must be a FACTORY instance — a freshly constructed default of
    /// this type (`crate::drawing::PrimitiveRegistry::create`), never the
    /// live primitive being edited: a system row names what THIS TYPE looks
    /// like out of the box, not what the user has since done to one
    /// instance of it. The caller constructs that instance
    /// (`mlc-backend`/`mlc-app-layout` today) rather than this function
    /// reaching into the global registry itself — this keeps the template
    /// value type testable against any `&dyn Primitive` stand-in, and
    /// leaves the "how do I seed a representative instance of this type"
    /// decision at the one or two call sites that already have to make it
    /// for other reasons (creating/previewing a primitive), instead of
    /// growing a second, possibly-diverging copy of it here.
    ///
    /// A type whose [`Primitive::level_set_presets`] is non-empty gets one
    /// row per preset — for the Fibonacci family that is the four canonical
    /// rows `"standard"`/`"extended"`/`"key_only"`/`"all"` — each narrowed/
    /// extended from `prim`'s own seeded ladder to that preset's value list
    /// (see [`narrow_to_target`]). Ids, names and values all come straight
    /// off `prim.level_set_presets()` — never a hardcoded list here, and
    /// never assumed just because `prim` happens to have a ladder at all:
    /// a level-having type with no preset catalogue of its own (Gann,
    /// pitchfork, Fibonacci Time Zones — see that trait method's own doc)
    /// falls to the branch below exactly like a type with no ladder at
    /// all, which is what closes the defect where every level-having type
    /// used to be offered the Fibonacci ratios regardless of whether it
    /// actually had any.
    ///
    /// Any type with no presets — with or without a ladder — gets exactly
    /// one row, `"standard"`, that IS the type's factory look (its own
    /// seeded ladder if it has one, untouched; flat style fields + typed
    /// style/text properties otherwise; nothing to narrow either way).
    ///
    /// Every row also merges
    /// [`crate::drawing::primitives_v2::config::level_set_label_defaults`]
    /// into `custom_style_properties` — a system template is a whole
    /// settings default, not just a level list (owner direction,
    /// 2026-08-09), so it restores the label plate + colour inheritance the
    /// same way applying one already does.
    pub fn system_templates(prim: &dyn Primitive) -> Vec<PrimitiveTemplate> {
        let lang = current_language();
        let presets = prim.level_set_presets();
        if presets.is_empty() {
            vec![Self::system_template(prim, "standard", StyleKey::Standard.get(lang), &[])]
        } else {
            presets
                .iter()
                .map(|preset| Self::system_template(prim, preset.id, (preset.name)(lang), preset.values))
                .collect()
        }
    }

    /// Resolve one [`Self::system_templates`] row by id — the reading
    /// [`StyleReference::System`] names. `None` for an id that matches none
    /// of `factory`'s own current system rows (a stale id left over from a
    /// row set that has since changed, or a plain unknown id).
    pub fn resolve_system(id: &str, factory: &dyn Primitive) -> Option<Self> {
        Self::system_templates(factory).into_iter().find(|t| t.id == id)
    }

    /// One [`Self::system_templates`] row: `prim`'s factory-seeded style and
    /// typed properties verbatim (via [`Self::from_primitive`]), `id`/`name`
    /// overwriting the generated id/name that constructor mints, `builtin`
    /// forced `true`, the ladder narrowed to `target` when non-empty (see
    /// [`narrow_to_target`]), and the label defaults merged in — see
    /// [`Self::system_templates`]'s own doc for why each of these.
    fn system_template(prim: &dyn Primitive, id: &str, name: &str, target: &[f64]) -> Self {
        let mut tmpl = Self::from_primitive(prim, name);
        tmpl.id = id.to_string();
        tmpl.builtin = true;
        if !target.is_empty() {
            tmpl.level_configs = narrow_to_target(&tmpl.level_configs, target);
        }
        // "All levels" ships with NO painting of any kind (owner direction,
        // 2026-08-09): thirty lines is a reference grid, and thirty stacked
        // bands under it is mud, not information. `narrow_to_target` already
        // cleared the per-level colours; this also puts the band fill out on
        // every level AND on the master, so the row reads as a plain ladder
        // the moment it is applied. Every other system row keeps the type's
        // own factory fill.
        if id == ALL_LEVELS_SET_ID {
            for cfg in tmpl.level_configs.iter_mut() {
                cfg.fill_enabled = false;
            }
            if let Some((_, opacity)) = tmpl.fill_master {
                tmpl.fill_master = Some((false, opacity));
            }
        }
        for (prop_id, value) in level_set_label_defaults() {
            match tmpl.custom_style_properties.iter_mut().find(|(pid, _)| pid == prop_id) {
                Some(existing) => existing.1 = value,
                None => tmpl.custom_style_properties.push((prop_id.to_string(), value)),
            }
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
    /// Also stamps [`PrimitiveData::style_reference`] — applying a template
    /// makes it the reference the primitive is compared against from now on
    /// ("Save as…" shows again once the primitive drifts from THIS
    /// template, not from whatever was applied or edited before it).
    /// `StyleReference::System(self.id.clone())` for a [`Self::builtin`]
    /// row, `StyleReference::Template(self.id.clone())` for a user-saved
    /// one — same field, same shape, the two are otherwise applied through
    /// this identical path.
    pub fn apply_to_primitive_data(&self, data: &mut PrimitiveData) {
        data.color = self.color.clone();
        data.width = self.width;
        data.style = self.style;
        data.timeframe_visibility = self.timeframe_visibility.clone();
        data.style_reference = Some(if self.builtin {
            StyleReference::System(self.id.clone())
        } else {
            StyleReference::Template(self.id.clone())
        });

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
        for (id, value) in &self.custom_level_properties {
            prim.apply_level_property(id, value);
        }
        // The band fill master, which no declared property covers — see the
        // `fill_master` field's own doc. `"fill_opacity"`'s setter takes a
        // PERCENT, `Primitive::fill_master` reports a 0.0..=1.0 fraction.
        if let Some((show_fill, opacity)) = self.fill_master {
            prim.apply_style_property("fill", &PropertyValue::Boolean(show_fill));
            prim.apply_style_property("fill_opacity", &PropertyValue::Number(opacity * 100.0));
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
    /// a user template looked up by id, or a system row resolved fresh via
    /// [`Self::resolve_system`] — to decide whether anything has moved
    /// since that reference was established. Both readings use this same function;
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
        // Compared only when the TARGET has a fill master at all, same rule
        // every other section here follows: gate on what `prim` exposes,
        // never on whether this template's own field happens to be empty.
        if let Some(current_fill) = prim.fill_master() {
            match self.fill_master {
                Some((show, opacity)) => {
                    if show != current_fill.0 || (opacity - current_fill.1).abs() > f64::EPSILON {
                        return false;
                    }
                }
                None => return false,
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
        // The third declared family — see `custom_level_properties`. Compared
        // the same way, and for the same reason: a control the modal can
        // reach is a control whose drift must be visible.
        let level_props_match = prim.level_properties().iter().all(|p| {
            self.custom_level_properties
                .iter()
                .find(|(id, _)| id == &p.id)
                .map(|(_, value)| value)
                == Some(&p.value)
        });
        if !level_props_match {
            return false;
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

/// Floating-point tolerance for matching a level value against a system
/// template's target composition — mirrors `fibonacci::retracement`'s own
/// private `LEVEL_SET_MATCH_EPS` (not reusable directly: that constant is
/// private to its module), same value.
const SYSTEM_TEMPLATE_EPS: f64 = 1e-9;

/// The one system-row id that ships unpainted — see [`PrimitiveTemplate::system_template`].
const ALL_LEVELS_SET_ID: &str = "all";

/// Narrow/extend `factory_configs` (a level-having primitive's factory-
/// seeded ladder — whatever fill/colour/group that type's own constructor
/// happens to set, read verbatim from [`Primitive::level_configs`] by
/// [`PrimitiveTemplate::system_template`]) to exactly `target`'s value
/// list.
///
/// Every entry comes out UNIFORM, because a system template is a base look
/// and a base look has no ladder of its own colours (owner direction,
/// 2026-08-09): `width: None` and `style: "solid"` (the primitive's own
/// `data.width` governs thickness), `color: None` (every level draws in the
/// primitive's stroke — per-level hues are what the Levels tab's Палитра
/// MODE is for, not what a base template ships), and the band fill copied
/// from the factory's own first seeded entry so a value the seed never had
/// is filled exactly like the ones it did.
///
/// That last part is the defect this closes: a fresh entry used to be a
/// bare `FibLevelConfig::new`, whose `fill_enabled` is false and whose
/// colour was then computed by a ramp run over a DIFFERENT length than the
/// factory's own — so "All levels" rendered its upper half unfilled and in
/// a visibly different colour sequence from its lower half.
fn narrow_to_target(factory_configs: &[FibLevelConfig], target: &[f64]) -> Vec<FibLevelConfig> {
    // The factory's own fill convention, read off its first seeded entry:
    // some level-having types seed filled bands, others deliberately do not
    // (`fan`/`speed_resistance`/`spiral` — see `default_level_configs_filled`'s
    // own doc), and a fresh entry must follow whichever this type is rather
    // than default to "unfilled" and leave half a ladder bare.
    let (seed_fill_enabled, seed_fill_opacity) = factory_configs
        .first()
        .map(|c| (c.fill_enabled, c.fill_opacity))
        .unwrap_or((false, crate::drawing::primitives_v2::BAND_FILL_OPACITY));

    target
        .iter()
        .map(|&level| {
            let mut cfg = factory_configs
                .iter()
                .find(|c| (c.level - level).abs() < SYSTEM_TEMPLATE_EPS)
                .cloned()
                .unwrap_or_else(|| FibLevelConfig::new(level));
            cfg.visible = true;
            cfg.width = None;
            cfg.style = "solid".to_string();
            cfg.color = None;
            cfg.fill_enabled = seed_fill_enabled;
            cfg.fill_opacity = seed_fill_opacity;
            cfg
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::drawing::primitives_v2::fibonacci::FibRetracement;
    use crate::drawing::primitives_v2::fibonacci::retracement::{
        level_set_id_for_composition, ALL_LEVELS, EXTENDED_LEVELS, KEY_ONLY_LEVELS, LEVEL_SET_PRESETS,
        MAIN_LEVELS,
    };
    use crate::drawing::primitives_v2::gann::GannFan;
    use crate::drawing::primitives_v2::gann::gann_fan::ALL_GANN_ANGLES;
    use crate::drawing::primitives_v2::lines::TrendLine;

    /// A fib retracement's system rows are the four Fibonacci presets,
    /// unchanged by the move off `system_templates`' own hardcoded id/value
    /// lookup onto `Primitive::level_set_presets` — this is the "pure move"
    /// the fix promises for the Fibonacci family: same ids, same names,
    /// same narrowed value lists as before.
    #[test]
    fn fib_retracement_system_templates_has_four_presets_with_same_values() {
        let fib = FibRetracement::new(0, 100.0, 1_000, 200.0, "#F7B93E");
        let rows = PrimitiveTemplate::system_templates(&fib);

        assert_eq!(rows.len(), 4);
        assert_eq!(
            rows.iter().map(|t| t.id.as_str()).collect::<Vec<_>>(),
            vec!["standard", "extended", "key_only", "all"]
        );

        let lang = current_language();
        for (row, preset) in rows.iter().zip(LEVEL_SET_PRESETS.iter()) {
            assert_eq!(row.id, preset.id);
            assert_eq!(row.name, (preset.name)(lang));
            let levels: Vec<f64> = row.level_configs.iter().map(|c| c.level).collect();
            assert_eq!(levels, preset.values);
        }

        // Spot-check the raw value tables directly too — the assertion above
        // already covers them transitively via `LEVEL_SET_PRESETS`, but this
        // pins the actual numbers so a future edit to the shared preset
        // table cannot silently drag the fib family's own rows along with it
        // unnoticed.
        let standard: Vec<f64> = rows[0].level_configs.iter().map(|c| c.level).collect();
        assert_eq!(standard, MAIN_LEVELS);
        let extended: Vec<f64> = rows[1].level_configs.iter().map(|c| c.level).collect();
        assert_eq!(extended, EXTENDED_LEVELS);
        let key_only: Vec<f64> = rows[2].level_configs.iter().map(|c| c.level).collect();
        assert_eq!(key_only, KEY_ONLY_LEVELS);
        let all: Vec<f64> = rows[3].level_configs.iter().map(|c| c.level).collect();
        assert_eq!(all, ALL_LEVELS);
    }

    /// A Gann fan has its own level ladder (`level_configs()` is `Some`) but
    /// no Fibonacci preset catalogue of its own — `level_set_presets()`
    /// stays the trait default (empty). It must get exactly one system
    /// row, and that row's ladder must be Gann's OWN factory angles, never
    /// narrowed toward the Fibonacci ratios it never had — the defect this
    /// closes.
    #[test]
    fn gann_fan_system_templates_has_one_row_with_own_ladder_not_narrowed_to_fib() {
        let gann = GannFan::new(0, 100.0, 1_000, 200.0, "#00ff00");
        assert!(gann.level_set_presets().is_empty());

        let rows = PrimitiveTemplate::system_templates(&gann);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "standard");

        let levels: Vec<f64> = rows[0].level_configs.iter().map(|c| c.level).collect();
        assert_eq!(levels, ALL_GANN_ANGLES);
        // Never the fib ladder this type never owned.
        assert_ne!(levels, MAIN_LEVELS);
    }

    /// A primitive with no level ladder at all — unaffected by this fix,
    /// still exactly one "standard" row with no levels to narrow.
    #[test]
    fn no_ladder_primitive_still_gets_one_standard_row() {
        let trend_line = TrendLine::new(0, 100.0, 1_000, 200.0, "#2962ff");
        assert!(trend_line.level_configs().is_none());
        assert!(trend_line.level_set_presets().is_empty());

        let rows = PrimitiveTemplate::system_templates(&trend_line);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].id, "standard");
        assert!(!rows[0].has_levels());
    }

    /// `level_set_id_for_composition` names a fib composition against the
    /// Fibonacci family's own preset table, and returns `None` for the
    /// identical values matched against a type that offers no presets at
    /// all (e.g. a Gann fan) — there is nothing to match against, so
    /// nothing is named.
    #[test]
    fn level_set_id_for_composition_none_without_presets() {
        let composition = MAIN_LEVELS.to_vec();

        assert_eq!(level_set_id_for_composition(LEVEL_SET_PRESETS, &composition), Some("standard"));

        let gann = GannFan::new(0, 100.0, 1_000, 200.0, "#00ff00");
        assert_eq!(level_set_id_for_composition(gann.level_set_presets(), &composition), None);
    }
}
