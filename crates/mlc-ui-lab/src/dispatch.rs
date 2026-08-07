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

use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::ui::modal_settings::{
    PrimitiveSettingsState, PrimitiveSettingsTab, ResizableModal, PRIMITIVE_SETTINGS_FIELD,
};
use mlc_app_layout::ui::widgets::{ResizeEdge, STANDARD_PALETTE};
use mlc_core::drawing::primitives_v2::config::{PropertyType, PropertyValue};
use mlc_core::drawing::{LineStyle, PrimitiveText, TextAlign};
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::templates::PrimitiveTemplate;
use uzor::input::keyboard::events::KeyCode;
use uzor::input::{InputCoordinator, LayerId, Sense, TextAction, WidgetResponse};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

fn tab_from_id(id: &str) -> Option<PrimitiveSettingsTab> {
    PrimitiveSettingsTab::all().iter().copied().find(|t| t.id() == id)
}

// =============================================================================
// Editable text sub-fields (stroke width, coordinates, level ratio, text)
// =============================================================================

/// Every inline numeric/text field this modal can focus and type into.
/// Unifies what used to be a single hardcoded `"stroke_width_value"` check —
/// same shared `PRIMITIVE_SETTINGS_FIELD` buffer, one discriminant per kind.
enum EditableField {
    StrokeWidth,
    CoordPrice(usize),
    CoordBar(usize),
    LevelValue(usize),
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
        if let Some(rest) = id.strip_prefix("level_") {
            if let Some(idx_str) = rest.strip_suffix("_value") {
                return idx_str.parse::<usize>().ok().map(Self::LevelValue);
            }
        }
        None
    }

    fn field_id(&self) -> String {
        match self {
            Self::StrokeWidth => "stroke_width_value".to_string(),
            Self::CoordPrice(i) => format!("coord_{i}_price"),
            Self::CoordBar(i) => format!("coord_{i}_bar"),
            Self::LevelValue(i) => format!("level_{i}_value"),
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
            Self::LevelValue(i) => prim
                .level_configs()
                .and_then(|c| c.get(*i).map(|l| format!("{:.3}", l.level)))
                .unwrap_or_default(),
            Self::TextFontSize => prim
                .data()
                .text
                .as_ref()
                .map(|t| format!("{:.0}", t.font_size))
                .unwrap_or_else(|| "14".to_string()),
            Self::TextContent => prim.data().text.as_ref().map(|t| t.content.clone()).unwrap_or_default(),
        }
    }

    fn commit(&self, text: &str, prim: &mut dyn Primitive) {
        match self {
            Self::StrokeWidth => {
                if let Ok(v) = text.parse::<f64>() {
                    if v > 0.0 {
                        prim.data_mut().width = v;
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
            Self::LevelValue(i) => {
                if let Ok(v) = text.parse::<f64>() {
                    if let Some(mut configs) = prim.level_configs() {
                        if let Some(cfg) = configs.get_mut(*i) {
                            cfg.level = v;
                        }
                        prim.set_level_configs(configs);
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

/// Apply the currently-focused text field's buffer back into the primitive
/// it belongs to, then clear focus. Called on Enter (`TextAction::Commit`),
/// Escape (`TextAction::Cancel`, just clears focus) and on blur (click
/// elsewhere while a field is focused).
pub fn commit_active_field(state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive, coord: &mut InputCoordinator) {
    let Some(field_id) = state.active_field_id.take() else { return };
    if let Some(field) = EditableField::from_id(&field_id) {
        let wid = WidgetId::from(PRIMITIVE_SETTINGS_FIELD);
        let text = coord.text_fields().text(&wid).to_string();
        field.commit(&text, prim);
    }
    coord.text_fields_mut().blur();
}

fn start_editing_field(
    field: EditableField,
    state: &mut PrimitiveSettingsState,
    prim: &dyn Primitive,
    coord: &mut InputCoordinator,
) {
    let wid = WidgetId::from(PRIMITIVE_SETTINGS_FIELD);
    let seed = field.seed_text(prim);
    coord.text_fields_mut().set_text(&wid, &seed);
    coord.focus_text_field(&wid);
    state.active_field_id = Some(field.field_id());
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
    if let Some(rest) = field.strip_prefix("level_") {
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

fn cycle_line_style(prim: &mut dyn Primitive) {
    let cur = prim.data().style;
    let idx = LINE_STYLE_ORDER.iter().position(|s| *s == cur).unwrap_or(0);
    prim.data_mut().style = LINE_STYLE_ORDER[(idx + 1) % LINE_STYLE_ORDER.len()];
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

fn toggle_select_dropdown(kind: &str, prop_id: &str, state: &mut PrimitiveSettingsState) {
    let target = (kind.to_string(), prop_id.to_string());
    if state.open_select_dropdown.as_ref() == Some(&target) {
        state.open_select_dropdown = None;
    } else {
        state.open_select_dropdown = Some(target);
    }
}

/// A deferred dropdown option row: `"{kind}_prop_option:{prop_id}:{value}"`.
fn apply_prop_option(rest: &str, kind: &str, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive) -> bool {
    let Some((prop_id, value)) = rest.split_once(':') else { return false };
    let applied = apply_prop_value(kind, prop_id, &PropertyValue::String(value.to_string()), prim);
    state.open_select_dropdown = None;
    applied
}

fn apply_level_prop_primary_click(prop_id: &str, prim: &mut dyn Primitive) -> bool {
    let prop = prim.level_properties().into_iter().find(|p| p.id == prop_id);
    let Some(prop) = prop else { return false };
    match prop.prop_type {
        PropertyType::Boolean => {
            let cur = prop.value.as_bool().unwrap_or(false);
            prim.apply_level_property(prop_id, &PropertyValue::Boolean(!cur))
        }
        PropertyType::Select { ref options } => {
            let cur = prop.value.as_string().unwrap_or("");
            let idx = options.iter().position(|o| o.value == cur).unwrap_or(0);
            match options.get((idx + 1) % options.len().max(1)) {
                Some(next) => prim.apply_level_property(prop_id, &PropertyValue::String(next.value.clone())),
                None => false,
            }
        }
        _ => false,
    }
}

fn toggle_level_visible(prim: &mut dyn Primitive, level_idx: usize) -> bool {
    let Some(mut configs) = prim.level_configs() else { return false };
    let Some(cfg) = configs.get_mut(level_idx) else { return false };
    cfg.visible = !cfg.visible;
    prim.set_level_configs(configs)
}

// =============================================================================
// Text tab default UI (bold/italic/position grid — used when the primitive
// has no `text_properties()` override, e.g. Fib Retracement)
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
    let mut tmpl = PrimitiveTemplate::from_primitive_data(prim.data(), &name);
    if let Some(configs) = prim.level_configs() {
        tmpl.level_configs = configs;
    }
    ctx.templates.push(tmpl);
}

fn apply_template(id: &str, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive, ctx: &ClickCtx) {
    let Some(tmpl) = ctx.templates.iter().find(|t| t.id == id).cloned() else { return };
    tmpl.apply_to_primitive_data(prim.data_mut());
    if tmpl.has_levels() {
        prim.set_level_configs(tmpl.level_configs.clone());
    }
    state.applied_template_id = Some(tmpl.id.clone());
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
            commit_active_field(state, prim, coord);
        }
    }

    // Close a stale line-style dropdown if the click isn't part of it.
    if state.open_line_style_dropdown && item_id != "line_style_menu" && !item_id.starts_with("line_style_option:") {
        state.open_line_style_dropdown = false;
    }
    // Close a stale select dropdown if the click isn't part of it.
    if let Some((kind, id)) = state.open_select_dropdown.clone() {
        let menu_id = format!("{kind}_prop_menu:{id}");
        let option_prefix = format!("{kind}_prop_option:{id}:");
        if item_id != menu_id && !item_id.starts_with(&option_prefix) {
            state.open_select_dropdown = None;
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

    if item_id == format!("{NS}:close") || item_id == format!("{NS}:ok") {
        state.close();
        return true;
    }
    if item_id == format!("{NS}:template") {
        state.template_dropdown_open = !state.template_dropdown_open;
        return true;
    }
    if let Some(rest) = item_id.strip_prefix(&format!("{NS}:tab:")) {
        if let Some(tab) = tab_from_id(rest) {
            state.set_tab(tab);
            return true;
        }
    }

    if let Some(field) = EditableField::from_id(item_id) {
        start_editing_field(field, state, prim, coord);
        return true;
    }

    if item_id == "line_style" {
        cycle_line_style(prim);
        return true;
    }
    if item_id == "line_style_menu" {
        state.open_line_style_dropdown = !state.open_line_style_dropdown;
        return true;
    }
    if let Some(style_str) = item_id.strip_prefix("line_style_option:") {
        prim.data_mut().style = LineStyle::from_str(style_str);
        state.open_line_style_dropdown = false;
        return true;
    }

    if let Some(prop_id) = item_id.strip_prefix("style_prop_menu:") {
        toggle_select_dropdown("style", prop_id, state);
        return true;
    }
    if let Some(rest) = item_id.strip_prefix("style_prop_option:") {
        return apply_prop_option(rest, "style", state, prim);
    }
    if let Some(prop_id) = item_id.strip_prefix("style_prop:") {
        return apply_prop_primary_click("style", prop_id, state, prim, ctx);
    }

    if let Some(prop_id) = item_id.strip_prefix("text_prop_menu:") {
        toggle_select_dropdown("text", prop_id, state);
        return true;
    }
    if let Some(rest) = item_id.strip_prefix("text_prop_option:") {
        return apply_prop_option(rest, "text", state, prim);
    }
    if let Some(prop_id) = item_id.strip_prefix("text_prop:") {
        return apply_prop_primary_click("text", prop_id, state, prim, ctx);
    }

    if let Some(prop_id) = item_id.strip_prefix("level_prop_menu:") {
        toggle_select_dropdown("level", prop_id, state);
        return true;
    }
    if let Some(rest) = item_id.strip_prefix("level_prop_option:") {
        return apply_prop_option(rest, "level", state, prim);
    }
    if let Some(prop_id) = item_id.strip_prefix("level_prop:") {
        return apply_level_prop_primary_click(prop_id, prim);
    }

    if let Some(rest) = item_id.strip_prefix("level_") {
        if let Some(idx_str) = rest.strip_suffix("_visible") {
            if let Ok(idx) = idx_str.parse::<usize>() {
                return toggle_level_visible(prim, idx);
            }
        }
        if idx_from_suffix(rest, "_color").is_some() || idx_from_suffix(rest, "_fill").is_some() {
            open_color_picker_for(&format!("level_{rest}"), state, prim, ctx);
            return true;
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

    false
}

fn idx_from_suffix(rest: &str, suffix: &str) -> Option<usize> {
    rest.strip_suffix(suffix).and_then(|s| s.parse::<usize>().ok())
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
        return; // dual-handle sliders — not wired, see the implementation report.
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
        apply_slider_value(&fid, value, prim);
    }
}

fn apply_slider_value(field_id: &str, value: f64, prim: &mut dyn Primitive) {
    if field_id == "stroke_width" {
        if value > 0.0 {
            prim.data_mut().width = value;
        }
        return;
    }
    if let Some(prop_id) = field_id.strip_prefix("style_prop:") {
        prim.apply_style_property(prop_id, &PropertyValue::Number(value));
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
/// shared `TextFieldStore`), or the shared inline sub-field.
pub fn apply_char(ch: char, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive, coord: &mut InputCoordinator) {
    if state.color_picker.hex_editing {
        apply_hex_char(ch, state, prim);
        return;
    }
    if state.active_field_id.is_none() {
        return;
    }
    let action = coord.on_char(ch);
    if let TextAction::Commit(_) = action {
        commit_active_field(state, prim, coord);
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
pub fn apply_key(key: uzor::input::keyboard::KeyPress, state: &mut PrimitiveSettingsState, prim: &mut dyn Primitive, coord: &mut InputCoordinator) {
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
        if state.open_line_style_dropdown {
            state.open_line_style_dropdown = false;
            return;
        }
        if state.open_select_dropdown.is_some() {
            state.open_select_dropdown = None;
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
        commit_active_field(state, prim, coord);
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
