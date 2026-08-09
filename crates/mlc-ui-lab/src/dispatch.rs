//! Click / drag / keyboard dispatch glue for the primitive settings modal.
//!
//! This module is the one piece the render function (`render_primitive_settings_modal`)
//! deliberately does NOT own: it draws widgets and registers their hit
//! regions on the `InputCoordinator`, but "what a click/drag on THIS widget id
//! means" is application logic that normally lives in
//! `mlc-app/src/input/settings.rs` (out of scope for this lab — see
//! `CLAUDE.md`/crate contracts). What follows is that glue, written directly
//! against the real `PrimitiveSettingsState` fields and the real `Primitive`
//! trait setters (`apply_style_property`, `set_level_configs`, `set_points`,
//! `data_mut()`, ...) — no widget is reimplemented here, only "which state
//! changes when this known id is clicked/dragged".
//!
//! Since the 2026-08-08 hard-cutover port, every sector calls the same
//! `render_primitive_settings_modal` (see `crate::variants::VARIANTS`), so
//! there is only one id family to route: the Levels tab's own value-keyed
//! `level_v{value}_*` / `levels_*` and the Placement tab's `tf_{class}_*` /
//! `placement_time_mode:*` are parsed and applied through
//! `mlc_app_layout::layout::modals::primitive_settings_intent` — the SAME
//! module `mlc-app/src/input/settings.rs` calls for its own storage-backed
//! copy — everything else (`stroke_color`, `style_prop:*`, `text_*`,
//! `coord_*`, `line_style`, `template_*`) is unchanged, generic dispatch
//! this file already had.

use mlc_app_layout::layout::modals::primitive_settings_intent::{
    add_level, apply_global_style, apply_global_width, apply_levels_intent, apply_placement_intent, apply_tf_bound,
    parse_levels_click, parse_placement_click, rename_level, LevelsIntent, PlacementIntent,
};
use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::ui::modal_settings::{
    PrimitiveSettingsState, PrimitiveSettingsTab, ResizableModal, PRIMITIVE_SETTINGS_FIELD,
};
use mlc_app_layout::ui::widgets::{ResizeEdge, STANDARD_PALETTE};
use mlc_core::drawing::primitives_v2::config::{PropertyType, PropertyValue};
use mlc_core::drawing::{LineStyle, PrimitiveText, TextAlign};
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::TimeframeVisibilityConfig;
use mlc_core::templates::PrimitiveTemplate;
use uzor::input::keyboard::events::KeyCode;
use uzor::input::{InputCoordinator, LayerId, Sense, TextAction, WidgetResponse};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

const EPS: f64 = 1e-9;

fn tab_from_id(id: &str) -> Option<PrimitiveSettingsTab> {
    PrimitiveSettingsTab::all().iter().copied().find(|t| t.id() == id)
}

// =============================================================================
// Editable text sub-fields (stroke width, coordinates, level ratio, text)
// =============================================================================

/// Every inline numeric/text field this modal's own renderer focuses and
/// types into that is NOT one of the Levels/Placement tabs' own value/
/// class-keyed ids (those go through `primitive_settings_intent` — see
/// `commit_active_field`).
enum EditableField {
    StrokeWidth,
    CoordPrice(usize),
    CoordBar(usize),
    TextFontSize,
    TextContent,
}

impl EditableField {
    fn from_id(id: &str) -> Option<Self> {
        match id {
            "stroke_width_value" => return Some(Self::StrokeWidth),
            "text_font_size" => return Some(Self::TextFontSize),
            "text_content" => return Some(Self::TextContent),
            _ => {}
        }
        if let Some(rest) = id.strip_prefix("coord_") {
            if let Some(idx_str) = rest.strip_suffix("_price") {
                return idx_str.parse::<usize>().ok().map(Self::CoordPrice);
            }
            if let Some(idx_str) = rest.strip_suffix("_bar") {
                return idx_str.parse::<usize>().ok().map(Self::CoordBar);
            }
        }
        None
    }

    fn field_id(&self) -> String {
        match self {
            Self::StrokeWidth => "stroke_width_value".to_string(),
            Self::CoordPrice(i) => format!("coord_{i}_price"),
            Self::CoordBar(i) => format!("coord_{i}_bar"),
            Self::TextFontSize => "text_font_size".to_string(),
            Self::TextContent => "text_content".to_string(),
        }
    }

    fn seed_text(&self, prim: &dyn Primitive) -> String {
        match self {
            Self::StrokeWidth => format!("{}", prim.data().width),
            Self::CoordPrice(i) => {
                prim.points().get(*i).map(|p| format!("{:.4}", p.1)).unwrap_or_default()
            }
            Self::CoordBar(i) => prim.points().get(*i).map(|p| format!("{}", p.0)).unwrap_or_default(),
            Self::TextFontSize => prim
                .data()
                .text
                .as_ref()
                .map(|t| format!("{:.0}", t.font_size))
                .unwrap_or_else(|| "14".to_string()),
            Self::TextContent => prim.data().text.as_ref().map(|t| t.content.clone()).unwrap_or_default(),
        }
    }

    fn commit(&self, text: &str, prim: &mut dyn Primitive, state: &PrimitiveSettingsState) {
        match self {
            Self::StrokeWidth => {
                if let Ok(v) = text.parse::<f64>() {
                    if v > 0.0 {
                        prim.data_mut().width = v;
                        // Global writer (item 3, 2026-08-09) — see
                        // `primitive_settings_intent::apply_global_width`'s
                        // own doc.
                        if let Some(mut configs) = prim.level_configs() {
                            apply_global_width(&mut configs, state, v);
                            prim.set_level_configs(configs);
                        }
                    }
                }
            }
            Self::CoordPrice(i) => {
                if let Ok(v) = text.parse::<f64>() {
                    let mut points = prim.points();
                    if let Some(p) = points.get_mut(*i) {
                        p.1 = v;
                        prim.set_points(&points);
                    }
                }
            }
            Self::CoordBar(i) => {
                if let Ok(v) = text.parse::<i64>() {
                    let mut points = prim.points();
                    if let Some(p) = points.get_mut(*i) {
                        p.0 = v;
                        prim.set_points(&points);
                    }
                }
            }
            Self::TextFontSize => {
                if let Ok(v) = text.parse::<f64>() {
                    if v > 0.0 {
                        ensure_text_block(prim).font_size = v;
                    }
                }
            }
            Self::TextContent => ensure_text_block(prim).content = text.to_string(),
        }
    }
}

fn ensure_text_block(prim: &mut dyn Primitive) -> &mut PrimitiveText {
    prim.data_mut().text.get_or_insert_with(PrimitiveText::default)
}

/// Apply the currently-focused text field's buffer back into whatever it
/// belongs to, then clear focus. Called on Enter (`TextAction::Commit`),
/// Escape (`TextAction::Cancel`, just clears focus) and on blur (click
/// elsewhere while a field is focused).
///
/// Several kinds of field share the one buffer this modal ever focuses
/// (`PRIMITIVE_SETTINGS_FIELD`): the generic sub-fields (`EditableField`,
/// committed into `prim`), a template's display name
/// (`"template_name:{id}"`, into `templates` — `crate::templates_variant`),
/// and the Levels/Placement tabs' own level-value rename, "+ добавить
/// уровень" field, and timeframe range bounds (all into `prim`/`state` via
/// `mlc_app_layout::layout::modals::primitive_settings_intent`). The footer
/// settings-search field (`primitive_settings::SEARCH_FIELD_ID`, moved out
/// of the Вид tab 2026-08-08) commits nothing: filtering already reads the
/// live buffer during render, before any commit happens.
pub fn commit_active_field(
    state: &mut PrimitiveSettingsState,
    prim: &mut dyn Primitive,
    coord: &mut InputCoordinator,
    templates: &mut Vec<PrimitiveTemplate>,
) {
    let Some(field_id) = state.active_field_id.take() else { return };
    let wid = WidgetId::from(PRIMITIVE_SETTINGS_FIELD);
    let text = coord.text_fields().text(&wid).to_string();

    if let Some(tmpl_id) = field_id.strip_prefix("template_name:") {
        let trimmed = text.trim();
        if !trimmed.is_empty() {
            if let Some(tmpl) = templates.iter_mut().find(|t| t.id == tmpl_id) {
                tmpl.name = trimmed.to_string();
            }
        }
    } else if field_id == mlc_app_layout::layout::modals::primitive_settings::SEARCH_FIELD_ID {
        // Ephemeral — nothing to persist, see the doc comment above.
    } else if field_id == "levels_add_value" {
        if let Ok(v) = text.trim().parse::<f64>() {
            if let Some(mut configs) = prim.level_configs() {
                add_level(&mut configs, state, v);
                prim.set_level_configs(configs);
            }
        }
    } else if let Some(key) = field_id.strip_prefix("level_v").and_then(|s| s.strip_suffix("_value")) {
        if let (Ok(old_level), Ok(new_level)) = (key.parse::<f64>(), text.trim().parse::<f64>()) {
            if let Some(mut configs) = prim.level_configs() {
                rename_level(&mut configs, state, old_level, new_level);
                prim.set_level_configs(configs);
            }
        }
    } else if field_id.starts_with("tf_") && (field_id.ends_with("_min") || field_id.ends_with("_max")) {
        if let Ok(val) = text.trim().parse::<u32>() {
            let mut tf = prim.data().timeframe_visibility.clone().unwrap_or_else(TimeframeVisibilityConfig::all);
            apply_tf_bound(&mut tf, &field_id, val);
            prim.data_mut().timeframe_visibility = Some(tf);
        }
    } else if let Some(field) = EditableField::from_id(&field_id) {
        field.commit(&text, prim, state);
    }
    coord.text_fields_mut().blur();
}

fn start_editing_field(
    field: EditableField,
    state: &mut PrimitiveSettingsState,
    prim: &dyn Primitive,
    coord: &mut InputCoordinator,
) {
    start_editing_raw(&field.field_id(), &field.seed_text(prim), state, coord);
}

/// Focus the shared inline-edit buffer on `field_id`, seeded with
/// `seed`. The one place every "click to start typing" affordance in this
/// modal goes through — baseline's own sub-fields (via `EditableField`),
/// a template's rename (`crate::templates_variant`), and the approved
/// design's level-value rename / "+ добавить уровень" / search /
/// timeframe bounds all funnel through this, so it is real, typed
/// keyboard input (cursor, selection, backspace, arrows) through the
/// exact pipeline `render_one_frame`'s `TextInput`/`KeyDown` handlers
/// already drive — never a parallel, faked text control.
pub fn start_editing_raw(field_id: &str, seed: &str, state: &mut PrimitiveSettingsState, coord: &mut InputCoordinator) {
    let wid = WidgetId::from(PRIMITIVE_SETTINGS_FIELD);
    coord.text_fields_mut().set_text(&wid, seed);
    coord.focus_text_field(&wid);
    state.active_field_id = Some(field_id.to_string());
}

// =============================================================================
// Colour targets (every swatch this modal draws)
// =============================================================================

fn current_color_for_field(field: &str, prim: &dyn Primitive) -> String {
    match field {
        "stroke_color" => return prim.data().color.stroke.clone(),
        "fill_color" => {
            return prim.data().color.fill.clone().unwrap_or_else(|| prim.data().color.stroke.clone())
        }
        "text_color" => {
            return prim
                .data()
                .text
                .as_ref()
                .and_then(|t| t.color.clone())
                .unwrap_or_else(|| prim.data().color.stroke.clone())
        }
        _ => {}
    }
    if let Some(prop_id) = field.strip_prefix("style_prop:") {
        return prim
            .style_properties()
            .into_iter()
            .find(|p| p.id == prop_id)
            .and_then(|p| p.value.as_color().map(str::to_string))
            .unwrap_or_else(|| prim.data().color.stroke.clone());
    }
    if let Some(prop_id) = field.strip_prefix("text_prop:") {
        return prim
            .text_properties()
            .into_iter()
            .flatten()
            .find(|p| p.id == prop_id)
            .and_then(|p| p.value.as_color().map(str::to_string))
            .unwrap_or_else(|| prim.data().color.stroke.clone());
    }
    if let Some(rest) = field.strip_prefix("level_v") {
        // `_fill_color`/`_fill` checked BEFORE the plain `_color` suffix:
        // `"..._fill_color".strip_suffix("_color")` also succeeds (it just
        // leaves an invalid `"..._fill"` key), so checking the longer,
        // fill-specific suffixes first is what keeps that from silently
        // mis-parsing the level key.
        if let Some(key) = rest.strip_suffix("_fill_color").or_else(|| rest.strip_suffix("_fill")) {
            if let Ok(level) = key.parse::<f64>() {
                if let Some(c) = find_level_color(prim, level, true) {
                    return c;
                }
            }
        } else if let Some(key) = rest.strip_suffix("_color") {
            if let Ok(level) = key.parse::<f64>() {
                if let Some(c) = find_level_color(prim, level, false) {
                    return c;
                }
            }
        }
    } else if let Some(rest) = field.strip_prefix("level_") {
        if let Some(idx_str) = rest.strip_suffix("_color") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let Some(c) = prim.level_configs().and_then(|cs| cs.get(idx).and_then(|c| c.color.clone())) {
                    return c;
                }
            }
        } else if let Some(idx_str) = rest.strip_suffix("_fill") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let Some(c) = prim.level_configs().and_then(|cs| cs.get(idx).and_then(|c| c.fill_color.clone())) {
                    return c;
                }
            }
        }
    }
    prim.data().color.stroke.clone()
}

fn find_level_color(prim: &dyn Primitive, level: f64, is_fill: bool) -> Option<String> {
    let configs = prim.level_configs()?;
    let cfg = configs.iter().find(|c| (c.level - level).abs() < EPS)?;
    if is_fill { cfg.fill_color.clone() } else { cfg.color.clone() }
}

fn set_level_color(prim: &mut dyn Primitive, idx: usize, color: &str, is_fill: bool) {
    let Some(mut configs) = prim.level_configs() else { return };
    let Some(cfg) = configs.get_mut(idx) else { return };
    if is_fill {
        cfg.fill_color = Some(color.to_string());
        cfg.fill_enabled = true;
    } else {
        cfg.color = Some(color.to_string());
    }
    prim.set_level_configs(configs);
}

fn set_level_color_by_value(prim: &mut dyn Primitive, level: f64, color: &str, is_fill: bool) {
    let Some(mut configs) = prim.level_configs() else { return };
    let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < EPS) else { return };
    if is_fill {
        cfg.fill_color = Some(color.to_string());
        cfg.fill_enabled = true;
    } else {
        cfg.color = Some(color.to_string());
    }
    prim.set_level_configs(configs);
}

fn apply_color_to_target(field: &str, color: &str, prim: &mut dyn Primitive) {
    match field {
        "stroke_color" => {
            prim.data_mut().color.stroke = color.to_string();
            return;
        }
        "fill_color" => {
            prim.data_mut().color.fill = Some(color.to_string());
            return;
        }
        "text_color" => {
            ensure_text_block(prim).color = Some(color.to_string());
            return;
        }
        _ => {}
    }
    if let Some(prop_id) = field.strip_prefix("style_prop:") {
        prim.apply_style_property(prop_id, &PropertyValue::Color(color.to_string()));
    } else if let Some(prop_id) = field.strip_prefix("text_prop:") {
        prim.apply_text_property(prop_id, &PropertyValue::Color(color.to_string()));
    } else if let Some(rest) = field.strip_prefix("level_v") {
        // Same fill-suffix-first ordering as `current_color_for_field` —
        // see its own comment for why.
        if let Some(key) = rest.strip_suffix("_fill_color").or_else(|| rest.strip_suffix("_fill")) {
            if let Ok(level) = key.parse::<f64>() {
                set_level_color_by_value(prim, level, color, true);
            }
        } else if let Some(key) = rest.strip_suffix("_color") {
            if let Ok(level) = key.parse::<f64>() {
                set_level_color_by_value(prim, level, color, false);
            }
        }
    } else if let Some(rest) = field.strip_prefix("level_") {
        if let Some(idx_str) = rest.strip_suffix("_color") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                set_level_color(prim, idx, color, false);
            }
        } else if let Some(idx_str) = rest.strip_suffix("_fill") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                set_level_color(prim, idx, color, true);
            }
        }
    }
}

/// Push the colour picker's current live value back into whatever field it
/// was opened for. Called after every interaction that changes the picker's
/// colour/opacity, so the swatch under it updates live, not just on close.
pub fn commit_color_picker_preview(state: &PrimitiveSettingsState, prim: &mut dyn Primitive) {
    let Some(field) = state.color_picker_field.clone() else { return };
    let color = state.color_picker.get_final_color();
    apply_color_to_target(&field, &color, prim);
}

fn open_color_picker_for(field: &str, state: &mut PrimitiveSettingsState, prim: &dyn Primitive, ctx: &ClickCtx) {
    let current = current_color_for_field(field, prim);
    state.open_color_picker_smart(
        field,
        ctx.anchor.x,
        ctx.anchor.y,
        ctx.anchor.width,
        ctx.anchor.height,
        ctx.screen_w,
        ctx.screen_h,
        Some(&current),
    );
}

// =============================================================================
// Line style
// =============================================================================

const LINE_STYLE_ORDER: [LineStyle; 5] = [
    LineStyle::Solid,
    LineStyle::Dashed,
    LineStyle::Dotted,
    LineStyle::LargeDashed,
    LineStyle::SparseDotted,
];

fn cycle_line_style(prim: &mut dyn Primitive, state: &PrimitiveSettingsState) {
    let cur = prim.data().style;
    let idx = LINE_STYLE_ORDER.iter().position(|s| *s == cur).unwrap_or(0);
    let next = LINE_STYLE_ORDER[(idx + 1) % LINE_STYLE_ORDER.len()];
    prim.data_mut().style = next;
    // Global writer (item 3, 2026-08-09) — see
    // `primitive_settings_intent::apply_global_style`'s own doc.
    if let Some(mut configs) = prim.level_configs() {
        apply_global_style(&mut configs, state, next);
        prim.set_level_configs(configs);
    }
}

// =============================================================================
// Style / text / level Select & Boolean properties
// =============================================================================

/// The "primary" click zone shared by Boolean (toggle), Select (cycle) and
/// Color (open picker) properties — all three register the same
/// `"{kind}_prop:{id}"` id (Number properties are drag-only sliders and
/// never reach here, since their `Sense` has no `CLICK`).
fn apply_prop_primary_click(kind: &str, prop_id: &str, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive, ctx: &ClickCtx) -> bool {
    let prop = match kind {
        "style" => prim.style_properties().into_iter().find(|p| p.id == prop_id),
        "text" => prim.text_properties().into_iter().flatten().find(|p| p.id == prop_id),
        _ => None,
    };
    let Some(prop) = prop else { return false };
    match prop.prop_type {
        PropertyType::Boolean => {
            let cur = prop.value.as_bool().unwrap_or(false);
            let val = PropertyValue::Boolean(!cur);
            apply_prop_value(kind, prop_id, &val, prim)
        }
        PropertyType::Select { ref options } => {
            let cur = prop.value.as_string().unwrap_or("");
            let idx = options.iter().position(|o| o.value == cur).unwrap_or(0);
            match options.get((idx + 1) % options.len().max(1)) {
                Some(next) => apply_prop_value(kind, prop_id, &PropertyValue::String(next.value.clone()), prim),
                None => false,
            }
        }
        PropertyType::Color => {
            let field = format!("{kind}_prop:{prop_id}");
            open_color_picker_for(&field, state, prim, ctx);
            true
        }
        _ => false,
    }
}

fn apply_prop_value(kind: &str, prop_id: &str, val: &PropertyValue, prim: &mut dyn Primitive) -> bool {
    match kind {
        "style" => prim.apply_style_property(prop_id, val),
        "text" => prim.apply_text_property(prop_id, val),
        "level" => prim.apply_level_property(prop_id, val),
        _ => false,
    }
}

// =============================================================================
// Text tab default UI (bold/italic/position grid — used when the primitive
// has no `text_properties()` override, e.g. Fib Retracement — real for it,
// see `mlc-app-layout`'s `primitive_settings_vid` module doc)
// =============================================================================

fn toggle_text_bold(prim: &mut dyn Primitive) {
    let t = ensure_text_block(prim);
    t.bold = !t.bold;
}

fn toggle_text_italic(prim: &mut dyn Primitive) {
    let t = ensure_text_block(prim);
    t.italic = !t.italic;
}

/// `rest` is `"{v_align}_{h_align}"` from `text_pos_{v}_{h}`.
fn apply_text_position(rest: &str, prim: &mut dyn Primitive) -> bool {
    let Some((v, h)) = rest.split_once('_') else { return false };
    let t = ensure_text_block(prim);
    t.v_align = TextAlign::from_str(v);
    t.h_align = TextAlign::from_str(h);
    true
}

// =============================================================================
// Templates (in-lab, in-memory — no persistence layer exists in this crate)
// =============================================================================

/// Per-click context that does not belong on `PrimitiveSettingsState` itself:
/// where the click landed (for smart-positioning the colour picker), the
/// screen size, and the lab's own in-memory template list.
pub struct ClickCtx<'a> {
    pub anchor: WidgetRect,
    pub screen_w: f64,
    pub screen_h: f64,
    pub templates: &'a mut Vec<PrimitiveTemplate>,
}

fn save_current_as_template(prim: &dyn Primitive, ctx: &mut ClickCtx) {
    let count = ctx.templates.iter().filter(|t| t.type_id == prim.type_id()).count();
    let name = format!("Template {}", count + 1);
    // `from_primitive` (not `from_primitive_data`) so the lab's saved
    // template also carries the level ladder and typed style/text
    // properties, not just the flat fields.
    let tmpl = PrimitiveTemplate::from_primitive(prim, &name);
    ctx.templates.push(tmpl);
}

fn apply_template(id: &str, _state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive, ctx: &ClickCtx) {
    let Some(tmpl) = ctx.templates.iter().find(|t| t.id == id).cloned() else { return };
    // `apply_to_primitive` stamps `data.style_reference` itself, which is the
    // only thing the Template menu's current-row highlight reads now.
    tmpl.apply_to_primitive(prim);
}

// =============================================================================
// Deferred-dropdown click patch
// =============================================================================

/// `render_primitive_settings_modal` computes rects for the line-style/select
/// dropdown OPTION rows (pushed into `PrimitiveSettingsResult.content_items`
/// as `line_style_option:*` / `{style,text,level}_prop_option:*`) but never
/// calls `InputCoordinator::register_on_layer` for them: the generic
/// "register every content item" loop inside that function runs BEFORE these
/// deferred rows are appended (they are built after the footer, "to avoid
/// z-order overlap"), so today they are drawn but structurally unreachable by
/// any click, from any caller — confirmed by reading
/// `mlc-app-layout/src/layout/modals/primitive_settings.rs` end-to-end. The
/// real fix is a few lines inside that function, registering each option rect
/// exactly like the main loop does; that file is out of this crate's scope.
///
/// This is the smallest in-crate patch: a same-frame supplementary layer,
/// above the modal but non-modal, carrying ONLY the option rects the render
/// call already computed, under the SAME `prim_settings:item:{id}` shape the
/// rest of this module already expects — so no other dispatch code needs to
/// know this patch exists, and it disappears on its own the moment
/// `mlc-app-layout` registers these rects itself.
pub fn register_dropdown_option_patch(coord: &mut InputCoordinator, result: &PrimitiveSettingsResult) {
    fn is_option(id: &str) -> bool {
        id.starts_with("line_style_option:")
            || id.starts_with("style_prop_option:")
            || id.starts_with("text_prop_option:")
            || id.starts_with("level_prop_option:")
    }
    let options: Vec<(&String, &WidgetRect)> =
        result.content_items.iter().filter(|(id, _)| is_option(id)).map(|(id, r)| (id, r)).collect();
    if options.is_empty() {
        return;
    }
    let layer = LayerId::new("prim_settings_dropdown_patch");
    coord.push_layer(layer.clone(), 4, false);
    for (id, rect) in options {
        coord.register_on_layer(format!("prim_settings:item:{id}"), *rect, Sense::CLICK | Sense::HOVER, &layer);
    }
    coord.pop_layer(&layer);
}

// =============================================================================
// Generic click dispatch (the modal's own layer, namespace "prim_settings")
// =============================================================================

/// Handle one click response's routed id (widget id with the
/// `"prim_settings:item:"` prefix already stripped, if it had one) against
/// the primitive settings modal's known widget ids.
///
/// Returns `true` if the click was recognised and handled.
pub fn apply_primitive_settings_click(
    item_id: &str,
    state: &mut PrimitiveSettingsState,
    prim: &mut dyn Primitive,
    coord: &mut InputCoordinator,
    ctx: &mut ClickCtx,
) -> bool {
    const NS: &str = "prim_settings";

    // Blur/commit whatever text field was active before acting on a new
    // click elsewhere — mirrors the click-away-commits behaviour a real
    // text input has.
    if let Some(active) = state.active_field_id.clone() {
        if active != item_id {
            commit_active_field(state, prim, coord, ctx.templates);
        }
    }

    // Close a stale template dropdown if the click isn't part of it.
    if state.template_dropdown_open {
        let owned = item_id == format!("{NS}:template")
            || item_id == "template_dropdown_menu"
            || item_id == "template_save_as"
            || item_id == "template_default"
            || item_id.starts_with("template_option:")
            || item_id.starts_with("template_delete:");
        if !owned {
            state.template_dropdown_open = false;
        }
    }

    // Close a stale split-control value dropdown (item 1, 2026-08-09
    // restore) if the click is neither its own chevron nor one of its own
    // option rows — mirrors the template-dropdown rule above.
    // `open_value_dropdown` is a single `Option<String>`, so this is also
    // the whole one-open-at-a-time guarantee (see `mlc-app-layout`'s
    // `PrimitiveSettingsState::open_value_dropdown` own doc).
    if state.open_value_dropdown.is_some() && !item_id.ends_with("::menu") && !item_id.contains("::opt:") {
        state.open_value_dropdown = None;
    }

    if item_id == format!("{NS}:close") || item_id == format!("{NS}:ok") {
        state.close();
        return true;
    }
    if item_id == format!("{NS}:template") {
        state.template_dropdown_open = !state.template_dropdown_open;
        state.open_value_dropdown = None;
        return true;
    }
    if let Some(rest) = item_id.strip_prefix(&format!("{NS}:tab:")) {
        if let Some(tab) = tab_from_id(rest) {
            state.set_tab(tab);
            state.open_value_dropdown = None;
            return true;
        }
    }

    if let Some(field) = EditableField::from_id(item_id) {
        start_editing_field(field, state, prim, coord);
        return true;
    }

    // ── Split cycle+chevron control: chevron opens/closes the dropdown ──────
    if let Some(fid) = item_id.strip_suffix("::menu") {
        state.open_value_dropdown = if state.open_value_dropdown.as_deref() == Some(fid) {
            None
        } else {
            Some(fid.to_string())
        };
        return true;
    }

    // ── Split cycle+chevron control: a dropdown option was picked ───────────
    if let Some(sep_idx) = item_id.find("::opt:") {
        let fid = &item_id[..sep_idx];
        let val = &item_id[sep_idx + "::opt:".len()..];
        state.open_value_dropdown = None;
        if fid == "line_style" {
            prim.data_mut().style = LineStyle::from_str(val);
            if let Some(mut configs) = prim.level_configs() {
                apply_global_style(&mut configs, state, prim.data().style);
                prim.set_level_configs(configs);
            }
            return true;
        }
        if let Some(prop_id) = fid.strip_prefix("style_prop:") {
            return apply_prop_value("style", prop_id, &PropertyValue::String(val.to_string()), prim);
        }
        if let Some(prop_id) = fid.strip_prefix("text_prop:") {
            return apply_prop_value("text", prop_id, &PropertyValue::String(val.to_string()), prim);
        }
        return false;
    }

    if item_id == "line_style" {
        cycle_line_style(prim, state);
        return true;
    }

    if let Some(prop_id) = item_id.strip_prefix("style_prop:") {
        return apply_prop_primary_click("style", prop_id, state, prim, ctx);
    }

    if let Some(prop_id) = item_id.strip_prefix("text_prop:") {
        return apply_prop_primary_click("text", prop_id, state, prim, ctx);
    }

    // `level_v{key}_fill` (bare) is NOT reachable here any more: it is now
    // the ladder row's own on/off toggle, parsed by `parse_levels_click`
    // (checked by the caller BEFORE this function — see
    // `apply_approved_design_click`), so `apply_primitive_settings_click`
    // never even sees it. Only the colour-picker ids remain:
    // `level_v{key}_color` (line colour) and `level_v{key}_fill_color`
    // (fill colour, moved into the inspector).
    if let Some(rest) = item_id.strip_prefix("level_v") {
        if let Some(key) = rest.strip_suffix("_fill_color").or_else(|| rest.strip_suffix("_color")) {
            if key.parse::<f64>().is_ok() {
                open_color_picker_for(&format!("level_v{rest}"), state, prim, ctx);
                return true;
            }
        }
    }

    if item_id == "stroke_color" || item_id == "fill_color" || item_id == "text_color" {
        open_color_picker_for(item_id, state, prim, ctx);
        return true;
    }

    if item_id == "text_bold" {
        toggle_text_bold(prim);
        return true;
    }
    if item_id == "text_italic" {
        toggle_text_italic(prim);
        return true;
    }
    if let Some(rest) = item_id.strip_prefix("text_pos_") {
        return apply_text_position(rest, prim);
    }

    if item_id == "template_save_as" {
        save_current_as_template(prim, ctx);
        state.template_dropdown_open = false;
        return true;
    }
    if item_id == "template_default" {
        // Recognised but not implemented: there is no per-type "default
        // template" store in this lab (or in `mlc-core`) to write to.
        state.template_dropdown_open = false;
        return true;
    }
    if let Some(id) = item_id.strip_prefix("template_option:") {
        apply_template(id, state, prim, ctx);
        state.template_dropdown_open = false;
        return true;
    }
    if let Some(id) = item_id.strip_prefix("template_delete:") {
        ctx.templates.retain(|t| t.id != id);
        return true;
    }

    if let Some(rest) = item_id.strip_prefix("templates_panel:") {
        return apply_templates_panel_click(rest, state, prim, coord, ctx);
    }

    false
}

// =============================================================================
// Templates panel (sector C, `crate::templates_variant`) — create / apply /
// rename / delete, the bigger surface `crate::templates_variant` gives the
// same `PrimitiveTemplate` list the footer dropdown above already reads and
// writes.
// =============================================================================

fn apply_templates_panel_click(
    rest: &str,
    state: &mut PrimitiveSettingsState,
    prim: &mut dyn Primitive,
    coord: &mut InputCoordinator,
    ctx: &mut ClickCtx,
) -> bool {
    if rest == "create" {
        save_current_as_template(prim, ctx);
        if let Some(tmpl) = ctx.templates.last() {
            let id = tmpl.id.clone();
            let name = tmpl.name.clone();
            start_editing_raw(&format!("template_name:{id}"), &name, state, coord);
        }
        return true;
    }
    if let Some(id) = rest.strip_prefix("apply:") {
        apply_template(id, state, prim, ctx);
        return true;
    }
    if let Some(id) = rest.strip_prefix("rename:") {
        if let Some(tmpl) = ctx.templates.iter().find(|t| t.id == id) {
            let name = tmpl.name.clone();
            start_editing_raw(&format!("template_name:{id}"), &name, state, coord);
        }
        return true;
    }
    if let Some(id) = rest.strip_prefix("delete:") {
        // The caller (`apply_primitive_settings_click`) already committed
        // and cleared whatever field was active before routing here, for
        // any `item_id` other than that field's own id — a delete click
        // never IS that id, so `active_field_id` cannot still be a rename
        // of the template being deleted by the time this runs. Nothing
        // else to clear.
        ctx.templates.retain(|t| t.id != id);
        return true;
    }
    false
}

// =============================================================================
// The approved design's own ids (Levels tab's `level_v*`/`levels_*`,
// Placement tab's `tf_{class}_*`/`placement_time_mode:*`) — checked BEFORE
// `apply_primitive_settings_click` in the caller (`crate::lib`), for every
// sector uniformly (every sector calls the same real
// `render_primitive_settings_modal` since the 2026-08-08 hard-cutover port,
// see `crate::variants`). Parsing + the storage-agnostic half of applying
// these ids lives in `mlc_app_layout::layout::modals::
// primitive_settings_intent`, shared verbatim with `mlc-app/src/input/
// settings.rs` — see that module's own doc for why. Anything not
// recognised here falls through to the shared dispatch unchanged.
// =============================================================================

/// Returns `true` when `item_id` was one of the approved design's own ids
/// and the click is fully handled.
pub fn apply_approved_design_click(
    item_id: &str,
    state: &mut PrimitiveSettingsState,
    prim: &mut dyn Primitive,
    coord: &mut InputCoordinator,
    ctx: &mut ClickCtx,
) -> bool {
    // ---- Размещение: время/индекс-бара toggle + tf_{class}_toggle /
    // tf_{class}_{min,max} ----
    if item_id.starts_with("placement_time_mode:") || item_id.starts_with("tf_") {
        let tf = prim.data().timeframe_visibility.clone().unwrap_or_else(TimeframeVisibilityConfig::all);
        if let Some(intent) = parse_placement_click(item_id, &tf) {
            if let PlacementIntent::StartTfBoundEditing { field_id, seed } = &intent {
                start_editing_raw(field_id, seed, state, coord);
                return true;
            }
            let mut tf = tf;
            if apply_placement_intent(&intent, state, &mut tf) {
                prim.data_mut().timeframe_visibility = Some(tf);
            }
            return true;
        }
    }

    // ---- Footer settings-search field focus (moved out of Вид 2026-08-08) ----
    if item_id == mlc_app_layout::layout::modals::primitive_settings::SEARCH_FIELD_ID {
        start_editing_raw(item_id, "", state, coord);
        return true;
    }

    // ---- Levels tab: global fill toggle — mirrors the primitive's own
    // fill master (`Primitive::fill_master`) directly, the SAME single
    // source of truth `mlc-app`'s own dedicated block uses (see
    // `mlc_app_layout::layout::modals::primitive_settings_levels::
    // draw_global_opacity_row`'s doc, 2026-08-08 appearance-surface
    // consolidation). Not routed through `parse_levels_click`/
    // `apply_levels_intent` below — that pair only ever mutates
    // `level_configs()`. The WRITE still goes through `apply_style_property`
    // — see `mlc-app`'s own mirror of this block for why that half is
    // unaffected by `FibRetracement` no longer DECLARING `"fill"`. ----
    if item_id == "levels_global_fill_toggle" {
        let current = prim.fill_master().map(|(on, _)| on).unwrap_or(false);
        prim.apply_style_property("fill", &PropertyValue::Boolean(!current));
        return true;
    }

    // ---- Уровни: levels_row_select / levels_add_chip / levels_add_value /
    // levels_color_mode:{mono,palette} / level_v{key}_{visible,fill_toggle,
    // delete,style,value} ----
    if let Some(intent) = parse_levels_click(item_id) {
        match &intent {
            LevelsIntent::StartValueEditing { seed, .. } => start_editing_raw(item_id, seed, state, coord),
            LevelsIntent::StartAddEditing => start_editing_raw(item_id, "", state, coord),
            _ => {
                if let Some(mut configs) = prim.level_configs() {
                    apply_levels_intent(&intent, state, &mut configs);
                    prim.set_level_configs(configs);
                }
            }
        }
        return true;
    }

    let _ = ctx; // reserved: nothing here opens a colour picker directly —
                 // level_v*_color/_fill_color still route through the
                 // shared `apply_primitive_settings_click` path, which
                 // already knows the value-keyed shape (see its own
                 // `level_v` branch) and needs `ClickCtx` for anchor
                 // positioning.
    false
}

// =============================================================================
// Drag (header, resize grips, sliders, scrollbar handle, colour-picker
// opacity slider) — NOT driven by `InputCoordinator::end_frame()` responses.
//
// `end_frame()`'s per-widget `drag_started`/`dragged`/`drag_stopped` fields
// (`uzor` 1.4.x, `input/core/coordinator.rs::end_frame` sections 2–3) only
// ever set `drag_started = true`, repeatedly, every frame the button stays
// down over a `Sense::DRAG` widget — they never call
// `WidgetInputState::drag.start(...)`, so `widget_state.drag.dragging` never
// becomes `Some`, `dragged`/`drag_stopped` are never generated, and every
// drag observed through that path is a same-frame no-op (this is why clicks
// worked and drags did not: a click needs one frame, a drag needs state to
// survive between frames, and nothing carried it). Confirmed by reading
// `end_frame` end-to-end against the shipped `uzor` 1.4.16/1.4.18 source.
//
// The coordinator's OWN answer to this is a second, explicit API —
// `process_drag_start`/`process_drag_move`/`process_drag_end` — documented
// to be "call[ed] ... from a mouse-down handler", which hit-tests itself and
// calls `widget_state.drag.start(...)` directly. This module now drives
// drag entirely through that API, called from the raw DOM
// `PointerDown`/`PointerMoved`/`PointerUp` events in `lib.rs`'s event-drain
// loop — bypassing `end_frame()`'s drag fields for these ids completely.
// =============================================================================

/// Everything a drag needs to know about the modal's current geometry,
/// captured from the most recently RENDERED frame. `PointerDown` arrives
/// during event-draining, before this frame's own render pass runs — same
/// one-frame lag `InputCoordinator::process_drag_start` already has baked
/// in (it hit-tests against widgets registered by the PREVIOUS
/// `end_frame()`/render cycle, for exactly this reason).
#[derive(Clone, Default)]
pub struct DragGeometry {
    pub modal_rect: Option<WidgetRect>,
    pub slider_tracks: Vec<mlc_app_layout::layout::render_frame::SliderTrackInfo>,
    pub scrollbar_track: Option<WidgetRect>,
    pub total_content_height: f64,
    pub viewport_height: f64,
    pub color_opacity_slider_rect: Option<WidgetRect>,
}

/// `id` is whatever `InputCoordinator::process_drag_start` returned —
/// exactly the widget id string as registered (no prefix stripped for the
/// shell ids; `"prim_settings:item:"` stripped below for content items).
pub fn on_drag_start(id: &str, x: f64, y: f64, state: &mut PrimitiveSettingsState, geo: &DragGeometry) {
    if id == "prim_settings:header" {
        if let Some(rect) = geo.modal_rect {
            state.start_drag(x, y, rect.x, rect.y);
        }
        return;
    }
    if let Some(edge_suffix) = id.strip_prefix("prim_settings:resize:") {
        if let (Some(edge), Some(frame)) = (ResizeEdge::from_id(edge_suffix), geo.modal_rect) {
            state.start_resize(edge, (x, y), (frame.x, frame.y), (frame.width, frame.height));
        }
        return;
    }
    if id == "prim_settings:scrollbar_handle" {
        state.scroll.start_drag(y);
        return;
    }
    if id == "color_picker_primitive:opacity_slider" {
        return; // seeded on the first move, which has the rect it needs.
    }
    let routed = id.strip_prefix("prim_settings:item:").unwrap_or(id);
    if routed.starts_with("tf_") {
        return; // timeframe range dragging — not wired, see `primitive_settings_placement`'s module doc (mlc-app-layout).
    }
    if let Some(info) = geo.slider_tracks.iter().find(|t| t.field_id == routed) {
        state.start_slider_drag_from_track(routed, info.track_x, info.track_width, info.min_val, info.max_val);
    }
}

pub fn on_drag_move(id: &str, x: f64, y: f64, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive, geo: &DragGeometry) {
    if id == "prim_settings:header" {
        state.update_drag(x, y);
        return;
    }
    if id.starts_with("prim_settings:resize:") {
        state.update_resize((x, y));
        return;
    }
    if id == "prim_settings:scrollbar_handle" {
        if let Some(track) = geo.scrollbar_track {
            state.scroll.handle_drag(y, track.height, geo.total_content_height, geo.viewport_height);
        }
        return;
    }
    if id == "color_picker_primitive:opacity_slider" {
        if let Some(rect) = geo.color_opacity_slider_rect {
            let t = ((x - rect.x) / rect.width).clamp(0.0, 1.0);
            state.color_picker.set_opacity(t);
            commit_color_picker_preview(state, prim);
        }
        return;
    }
    let routed = id.strip_prefix("prim_settings:item:").unwrap_or(id);
    if geo.slider_tracks.iter().any(|t| t.field_id == routed) {
        state.update_slider_drag_float(x);
    }
}

pub fn on_drag_end(id: &str, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive) {
    if id == "prim_settings:header" {
        state.end_drag();
        return;
    }
    if id.starts_with("prim_settings:resize:") {
        state.end_resize();
        return;
    }
    if id == "prim_settings:scrollbar_handle" {
        state.scroll.end_drag();
        return;
    }
    if id == "color_picker_primitive:opacity_slider" {
        return; // every move already committed live — nothing to finalise.
    }
    if let Some((fid, value, _)) = state.take_slider_drag_value() {
        apply_slider_value(&fid, value, prim, state);
    }
}

/// The Levels tab's own per-level sliders (`level_v{value}_width` /
/// `level_v{value}_fill_opacity`) write straight into that level's own
/// `FibLevelConfig`. `levels_global_opacity` does NOT — since the
/// 2026-08-08 appearance-surface consolidation it writes the PRIMITIVE's
/// own `fill_opacity` style property, the single field every level-ladder
/// primitive's own render actually reads for every band; see
/// `mlc_app_layout::layout::modals::primitive_settings_levels::
/// draw_global_opacity_row`'s own doc for why. No `PrimitiveSettingsState`
/// bookkeeping is touched by any of this any more.
fn apply_slider_value(field_id: &str, value: f64, prim: &mut dyn Primitive, state: &PrimitiveSettingsState) {
    if field_id == "stroke_width" {
        if value > 0.0 {
            prim.data_mut().width = value;
            // Global writer (item 3, 2026-08-09) — see
            // `primitive_settings_intent::apply_global_width`'s own doc.
            if let Some(mut configs) = prim.level_configs() {
                apply_global_width(&mut configs, state, value);
                prim.set_level_configs(configs);
            }
        }
        return;
    }
    if let Some(prop_id) = field_id.strip_prefix("style_prop:") {
        prim.apply_style_property(prop_id, &PropertyValue::Number(value));
        return;
    }
    if let Some(key) = field_id.strip_prefix("level_v").and_then(|s| s.strip_suffix("_width")) {
        if let Ok(level) = key.parse::<f64>() {
            if value > 0.0 {
                if let Some(mut configs) = prim.level_configs() {
                    if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < EPS) {
                        cfg.width = Some(value);
                    }
                    prim.set_level_configs(configs);
                }
            }
        }
        return;
    }
    if let Some(key) = field_id.strip_prefix("level_v").and_then(|s| s.strip_suffix("_fill_opacity")) {
        if let Ok(level) = key.parse::<f64>() {
            if let Some(mut configs) = prim.level_configs() {
                if let Some(cfg) = configs.iter_mut().find(|c| (c.level - level).abs() < EPS) {
                    cfg.fill_opacity = value.clamp(0.0, 1.0);
                }
                prim.set_level_configs(configs);
            }
        }
        return;
    }
    if field_id == "levels_global_opacity" {
        let clamped_pct = value.clamp(0.0, 1.0) * 100.0;
        prim.apply_style_property("fill_opacity", &PropertyValue::Number(clamped_pct));
    }
}

// =============================================================================
// Tab-content scroll
//
// Wheel and the scrollbar TRACK click are single-frame gestures — both stay
// on the `end_frame()` response path (`resp.scrolled` / `resp.clicked`),
// which is exactly what already works for every other click in this crate.
// The scrollbar HANDLE is a drag; it is driven through `on_drag_{start,move,end}`
// above, next to the header/resize/slider drags, for the same reason they are.
// =============================================================================

pub fn apply_scroll_response(
    id: &str,
    resp: &WidgetResponse,
    state: &mut PrimitiveSettingsState,
    pointer: Option<(f64, f64)>,
    total_content_height: f64,
    viewport_height: f64,
    track_rect: Option<WidgetRect>,
) {
    match id {
        "prim_settings:scroll_viewport" => {
            if resp.scrolled {
                state.scroll.handle_wheel(resp.scroll_delta.1, total_content_height, viewport_height);
            }
        }
        "prim_settings:scrollbar_track" => {
            if resp.clicked {
                if let (Some((_, my)), Some(track)) = (pointer, track_rect) {
                    state.scroll.handle_track_click(my, track.y, track.height, total_content_height, viewport_height);
                }
            }
        }
        _ => {}
    }
}

// =============================================================================
// Colour picker popup (its own layer, namespace "color_picker_primitive")
// =============================================================================

/// `cp_id` has the `"color_picker_primitive:"` prefix already stripped.
pub fn apply_color_picker_response(
    cp_id: &str,
    resp: &WidgetResponse,
    state: &mut PrimitiveSettingsState,
    prim: &mut dyn Primitive,
    pointer: Option<(f64, f64)>,
    screen_w: f64,
    screen_h: f64,
) {
    if let Some(idx_str) = cp_id.strip_prefix("swatch:") {
        if resp.clicked {
            if let Ok(idx) = idx_str.parse::<usize>() {
                if let Some(color) = STANDARD_PALETTE.get(idx) {
                    state.color_picker.select_color(color);
                    commit_color_picker_preview(state, prim);
                    state.close_color_picker();
                }
            }
        }
        return;
    }
    match cp_id {
        "plus" => {
            if resp.clicked {
                state.color_picker.open_l2();
                state.color_picker.recalculate_l2_position(screen_w, screen_h);
            }
        }
        "opacity_toggle" => {
            if resp.clicked {
                state.color_picker.toggle_opacity();
                commit_color_picker_preview(state, prim);
            }
        }
        // "opacity_slider" is `Sense::DRAG` — driven through
        // `on_drag_{start,move,end}` above, not through this response path.
        "sv_square" => {
            if resp.clicked {
                if let Some((x, y)) = pointer {
                    let s = ((x - resp.rect.x) / resp.rect.width).clamp(0.0, 1.0);
                    let v = (1.0 - (y - resp.rect.y) / resp.rect.height).clamp(0.0, 1.0);
                    state.color_picker.set_sv(s, v);
                    commit_color_picker_preview(state, prim);
                }
            }
        }
        "hue_bar" => {
            if resp.clicked {
                if let Some((_, y)) = pointer {
                    let h = ((y - resp.rect.y) / resp.rect.height).clamp(0.0, 1.0) * 360.0;
                    state.color_picker.set_hue(h);
                    commit_color_picker_preview(state, prim);
                }
            }
        }
        "hex_input" => {
            if resp.clicked {
                state.color_picker.hex_editing = true;
            }
        }
        "add" => {
            if resp.clicked {
                state.color_picker.add_to_custom();
                state.close_color_picker_one_level();
            }
        }
        "back" => {
            if resp.clicked {
                state.close_color_picker_one_level();
            }
        }
        _ => {} // "popup" — background, swallow.
    }
}

// =============================================================================
// Keyboard
// =============================================================================

/// Forward one character to whichever text buffer currently has focus: the
/// hex-input field inside an open colour picker (its own buffer, not the
/// shared `TextFieldStore`), or the shared inline sub-field (see
/// `commit_active_field`).
pub fn apply_char(
    ch: char,
    state: &mut PrimitiveSettingsState,
    prim: &mut dyn Primitive,
    coord: &mut InputCoordinator,
    templates: &mut Vec<PrimitiveTemplate>,
) {
    if state.color_picker.hex_editing {
        apply_hex_char(ch, state, prim);
        return;
    }
    if state.active_field_id.is_none() {
        return;
    }
    let action = coord.on_char(ch);
    if let TextAction::Commit(_) = action {
        commit_active_field(state, prim, coord, templates);
    }
}

pub fn apply_hex_char(ch: char, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive) {
    state.color_picker.hex_char_input(ch);
    commit_color_picker_preview(state, prim);
}

/// Forward a named key press (Backspace, arrows, Escape, ...) to whichever
/// input currently has focus, and close whatever overlay is topmost on
/// Escape: hex input → colour picker (one level) → line-style dropdown →
/// select dropdown → template dropdown → the shared inline sub-field.
pub fn apply_key(
    key: uzor::input::keyboard::KeyPress,
    state: &mut PrimitiveSettingsState,
    prim: &mut dyn Primitive,
    coord: &mut InputCoordinator,
    templates: &mut Vec<PrimitiveTemplate>,
) {
    use uzor::input::keyboard::KeyPress;

    // Captured once, up front: `key` is not `Copy`, and `coord.on_key(key)`
    // below consumes it.
    let escape = matches!(key, KeyPress::Escape);
    let backspace = matches!(key, KeyPress::Backspace);

    if escape {
        if state.color_picker.hex_editing {
            state.color_picker.hex_editing = false;
            return;
        }
        if state.is_color_picker_open() {
            state.close_color_picker_one_level();
            return;
        }
        if state.template_dropdown_open {
            state.template_dropdown_open = false;
            return;
        }
    }

    if state.color_picker.hex_editing {
        if backspace {
            apply_hex_char('\x08', state, prim);
        }
        return;
    }

    if state.active_field_id.is_none() {
        return;
    }
    let action = coord.on_key(key);
    if escape {
        state.active_field_id = None;
        coord.text_fields_mut().blur();
        return;
    }
    if let TextAction::Commit(_) = action {
        commit_active_field(state, prim, coord, templates);
    }
}

/// `KeyDown`-only path for the colour picker's hex field: Backspace edits it
/// directly (it has no `TextFieldStore` entry to route through
/// `InputCoordinator::on_key`), Enter/Escape blur it.
pub fn apply_hex_key(key: &KeyCode, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive) {
    match key {
        KeyCode::Backspace => apply_hex_char('\x08', state, prim),
        KeyCode::Enter | KeyCode::Escape => state.color_picker.hex_editing = false,
        _ => {}
    }
}
