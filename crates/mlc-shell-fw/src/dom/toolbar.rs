//! The app's toolbars (top control strip, left drawing rail, right sidebar rail) as
//! uzor DOM.
//!
//! Was: `mlc-app-layout` `toolbar_core::draw_toolbar_with_icons`, which lays out and
//! draws every item by hand. Now the app still lays the toolbar out (its own code,
//! with drawing switched off: `toolbar_core::capture`), so hit rects, hover, the
//! dropdown anchors and every click stay the app's, and this module paints the result
//! as a DOM tree: one box per item at the rect the app gave it, its icon as the same
//! SVG through the DOM's icon painter, its label, its chevron, the separators.
//! The boxes carry no ids: presses go on to the app's own hit test.
//!
//! Covered: the toolbars `toolbar_core::capture::host_can_paint` accepts (no scroll
//! chevrons; separators, spacers, icon buttons, buttons, dropdowns, split icon
//! buttons) on a context without backdrop blur. Any other toolbar is drawn by the app
//! as before. The mascot ("Bot") icon is the app's own drawing in a paint slot.

use mlc_app_layout::ui::toolbar_core::capture::CapturedToolbar;
use mlc_app_layout::ui::toolbar_core::{stroke_compensated_icon, ToolbarItem, ToolbarOrientation};
use uzor::types::Rect;
use uzor::ui::dom::El;

/// Paint-slot key prefix of an item the app draws itself (the mascot).
pub const SLOT: &str = "tb:own:";
/// Paint-slot key prefix of a backdrop blur (the context's own
/// `draw_blur_background`, a no-op unless the theme blurs), where the old drawing
/// called it: under the bar, and under a lit item when the context blurs.
pub const BLUR: &str = "tb:blur:";

/// The rule between toolbar `t` (at window rect `o`) and the chart, in window
/// coordinates: the drawing rail's right edge, the control strip's bottom edge, the
/// right rail's left edge. `chart_toolbar_render_toolbars` draws these after all the
/// toolbars, so the host paints them after its toolbars too ([`edge_tree`]).
pub fn edge(t: &CapturedToolbar, o: Rect) -> Option<Rect> {
    match t.slot {
        "left" => Some(Rect::new(o.x + o.width - 1.0, o.y, 1.0, o.height)),
        "top" => Some(Rect::new(o.x, o.y + o.height - 1.0, o.width, 1.0)),
        "right" => Some(Rect::new(o.x, o.y, 1.0, o.height)),
        _ => None,
    }
}

/// One edge rule as a DOM box filling its rect.
pub fn edge_tree(r: Rect) -> El {
    El::new("div.t-rule").css(format!("width: {}px; height: {}px", r.width, r.height))
}

/// The toolbar's colours, as CSS.
pub fn css(t: &CapturedToolbar) -> String {
    let th = &t.theme;
    format!(
        "div.t-bar {{ position: relative; color: {ink}; font-size: 13px; font-family: sans-serif; overflow: hidden }}
div.t-bg {{ position: absolute; left: 0px; top: 0px; background: {bg} }}
div.t-item {{ position: absolute; color: {ink} }}
div.t-item.hov {{ color: {ink_hover} }}
div.t-item.on, div.t-item.side {{ color: {ink_active} }}
div.t-face {{ position: absolute; left: 0px; top: 0px; border-radius: 4px }}
div.t-item.hov > div.t-face {{ background: {hover} }}
div.t-item.on > div.t-face {{ background: {active} }}
div.t-item.side > div.t-face {{ left: 3px; border-radius: 0px; background: {active} }}
div.t-blur {{ position: absolute; left: 0px; top: 0px }}
div.t-item.side > div.t-blur {{ left: 3px }}
div.t-mark {{ position: absolute; left: 0px; top: 0px; width: 3px; background: {accent} }}
div.t-icon {{ position: absolute }}
div.t-glyph {{ position: absolute; shape-rendering: geometricPrecision }}
div.t-text {{ position: absolute; display: flex; align-items: center; white-space: nowrap; padding-top: 2px }}
div.t-strike {{ position: absolute; height: 1px; background: currentColor }}
div.t-sep {{ position: absolute; background: {sep} }}
div.t-rule {{ background: {sep} }}",
        bg = th.background,
        ink = th.item_text,
        hover = th.item_bg_hover,
        ink_hover = th.item_text_hover,
        active = th.item_bg_active,
        ink_active = th.item_text_active,
        accent = th.accent,
        sep = th.separator,
    )
}

fn item_of<'a>(t: &'a CapturedToolbar, id: &str) -> Option<&'a ToolbarItem> {
    t.config.sections.iter().flat_map(|s| &s.items).find(|i| i.id() == Some(id))
}

fn at(r: Rect, o: Rect) -> String {
    format!("left: {}px; top: {}px; width: {}px; height: {}px", r.x - o.x, r.y - o.y, r.width, r.height)
}

fn icon_box(name: &str, r: Rect, o: Rect) -> El {
    let e = El::new("div.t-icon").css(at(r, o));
    match mlc_app_layout::ui::icons::icon_svg(name) {
        Some(svg) => e.svg(stroke_compensated_icon(svg, r.width).into_owned()),
        None => e,
    }
}

/// A label whose left edge is `x` (window), on the item's centre line.
fn text(label: &str, x: f64, item: Rect, o: Rect) -> El {
    El::new("div.t-text").css(format!("left: {}px; top: {}px; height: {}px", x - o.x, item.y - o.y, item.height)).child(El::new("span").text(label.to_string()))
}

/// A filled or stroked polyline in a whole-pixel box, the pen kept exact.
fn glyph(points: &[(f64, f64)], fill: bool, pen: f64, o: Rect) -> El {
    let x0 = points.iter().map(|p| p.0).fold(f64::INFINITY, f64::min).floor() - 2.0;
    let y0 = points.iter().map(|p| p.1).fold(f64::INFINITY, f64::min).floor() - 2.0;
    let x1 = points.iter().map(|p| p.0).fold(f64::NEG_INFINITY, f64::max).ceil() + 2.0;
    let y1 = points.iter().map(|p| p.1).fold(f64::NEG_INFINITY, f64::max).ceil() + 2.0;
    let (w, h) = (x1 - x0, y1 - y0);
    let mut d = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        d.push_str(&format!("{}{} {}", if i == 0 { "M" } else { "L" }, x - x0, y - y0));
    }
    let paint = if fill { "fill=\"currentColor\" stroke=\"none\"".to_string() } else { format!("fill=\"none\" stroke=\"currentColor\" stroke-width=\"{pen}\"") };
    if fill {
        d.push('Z');
    }
    El::new("div.t-glyph")
        .css(format!("left: {}px; top: {}px; width: {w}px; height: {h}px", x0 - o.x, y0 - o.y))
        .svg(format!(r#"<svg viewBox="0 0 {w} {h}" {paint}><path d="{d}"/></svg>"#))
}

fn state_class(active: bool, hovered: bool) -> &'static str {
    if active {
        "div.t-item.on"
    } else if hovered {
        "div.t-item.hov"
    } else {
        "div.t-item"
    }
}

/// The toolbar `t` at `o` (its window rect). `measure` measures a 13px label.
/// Returns the tree and the paint slots it asks for: `(key, icon, color)`.
pub fn tree(t: &CapturedToolbar, o: Rect, measure: &mut dyn FnMut(&str) -> f64) -> (El, Vec<(String, String, String)>) {
    let cfg = &t.config;
    let th = &t.theme;
    let horizontal = matches!(cfg.orientation, ToolbarOrientation::Horizontal);
    let hov = t.hovered.as_deref();
    let mut slots = Vec::new();
    let full = format!("width: {}px; height: {}px", o.width, o.height);
    let mut root = El::new("div.t-bar")
        .css(full.clone())
        .child(El::new("div.t-blur").slot(format!("{BLUR}{}:bar", t.slot)).css(full.clone()))
        .child(El::new("div.t-bg").css(full));
    // A lit item's face: the blur, then the fill (`draw_hover_rounded_rect` /
    // `draw_active_rounded_rect` / `draw_sidebar_active_item`).
    let face = |b: El, key: &str, w: f64, h: f64, lit: bool, side: bool| -> El {
        if !lit {
            return b;
        }
        let fw = if side { w - 3.0 } else { w };
        b.child(El::new("div.t-blur").slot(format!("{BLUR}{}:{key}", t.slot)).css(format!("width: {fw}px; height: {h}px")))
            .child(El::new("div.t-face").css(format!("width: {fw}px; height: {h}px")))
    };
    // Item rects are in the app's coordinates; `o` is the window rect of `t.rect`.
    let shift = |r: &mlc_app_layout::ui::toolbar_core::WidgetRect| Rect::new(r.x - t.rect.x + o.x, r.y - t.rect.y + o.y, r.width, r.height);
    for sep in &t.result.separator_rects {
        root = root.child(El::new("div.t-sep").css(at(shift(sep), o)));
    }
    let rects = &t.result.item_rects;
    for (id, r) in rects {
        let r = shift(r);
        let Some(item) = item_of(t, id) else { continue };
        let hovered = hov == Some(id.as_str());
        match item {
            ToolbarItem::IconButton { icon, active, disabled, .. } => {
                let side = !horizontal && th.sidebar_style && *active && !*disabled;
                let mut b = if side {
                    face(El::new("div.t-item.side").css(at(r, o)).child(El::new("div.t-mark").css(format!("height: {}px", r.height))), id, r.width, r.height, true, true)
                } else {
                    let spec = if *disabled { "div.t-item" } else { state_class(*active, hovered) };
                    face(El::new(spec).css(at(r, o)), id, r.width, r.height, spec != "div.t-item", false)
                };
                let size = if matches!(id.as_str(), "slot_dom" | "slot_tape" | "slot_l2" | "slot_big_trades" | "slot_liq" | "screener") {
                    (cfg.icon_size + 4.0).max(20.0)
                } else {
                    cfg.icon_size
                };
                let ir = Rect::new(r.x + r.width / 2.0 - size / 2.0, r.y + r.height / 2.0 - size / 2.0, size, size);
                if icon.name() == "Bot" {
                    let key = format!("{SLOT}{}:{id}", t.slot);
                    let color = if *disabled { &th.item_text } else if *active { &th.item_text_active } else if hovered { &th.item_text_hover } else { &th.item_text };
                    slots.push((key.clone(), icon.name().to_string(), color.clone()));
                    // The mascot is drawn 36 px wide, centred on the icon box.
                    let m = 36.0_f64.max(size);
                    let mr = Rect::new(ir.x + ir.width / 2.0 - m / 2.0, ir.y + ir.height / 2.0 - m / 2.0, m, m);
                    b = b.child(El::new("div.t-icon").slot(key).css(at(mr, r)));
                } else {
                    b = b.child(icon_box(icon.name(), ir, r));
                }
                root = root.child(b);
            }
            ToolbarItem::Button { icon, text: label, active, disabled, strikethrough, .. } => {
                let spec = if *disabled { "div.t-item" } else { state_class(*active, hovered) };
                let mut b = face(El::new(spec).css(at(r, o)), id, r.width, r.height, spec != "div.t-item", false);
                let cx = r.x + r.width / 2.0;
                if let Some(icon) = icon {
                    b = b.child(icon_box(icon.name(), Rect::new(cx - cfg.icon_size / 2.0, r.y + r.height / 2.0 - cfg.icon_size / 2.0, cfg.icon_size, cfg.icon_size), r));
                }
                if let Some(label) = label {
                    let tw = measure(label);
                    let x = if icon.is_some() { r.x + cfg.icon_size + 4.0 } else { cx - tw / 2.0 };
                    b = b.child(text(label, x, r, r));
                    if *strikethrough {
                        b = b.child(El::new("div.t-strike").css(at(Rect::new(x - 1.0, r.y + r.height / 2.0 - 0.5, tw + 2.0, 1.0), r)));
                    }
                }
                root = root.child(b);
            }
            ToolbarItem::Dropdown { icon, text: label, active, show_chevron, .. } => {
                let tw = label.as_deref().map(&mut *measure).unwrap_or(0.0);
                let reserve = if label.is_some() {
                    if icon.is_some() { cfg.icon_size + 8.0 } else { 8.0 }
                } else {
                    4.0 + cfg.icon_size
                };
                let w = r.width.max(reserve + tw + if *show_chevron { 14.0 } else { 8.0 });
                let hr = Rect::new(r.x, r.y, w, r.height);
                let spec = state_class(*active, hovered);
                let mut b = face(El::new(spec).css(at(hr, o)), id, hr.width, hr.height, spec != "div.t-item", false);
                if let Some(icon) = icon {
                    b = b.child(icon_box(icon.name(), Rect::new(r.x + 4.0, r.y + r.height / 2.0 - cfg.icon_size / 2.0, cfg.icon_size, cfg.icon_size), hr));
                }
                if let Some(label) = label {
                    b = b.child(text(label, r.x + if icon.is_some() { cfg.icon_size + 8.0 } else { 8.0 }, r, hr));
                }
                if *show_chevron {
                    let (x, y, s) = (r.x + r.width - 10.0, r.y + r.height / 2.0, 4.0);
                    b = b.child(glyph(&[(x - s, y - s / 2.0), (x, y + s / 2.0), (x + s, y - s / 2.0)], true, 0.0, hr));
                }
                root = root.child(b);
            }
            ToolbarItem::SplitIconButton { icon, active, .. } => {
                let menu = format!("{id}_menu");
                let chev = rects.iter().find(|(k, _)| *k == menu).map(|(_, c)| shift(c));
                let full = match chev {
                    Some(c) => Rect::new(r.x, r.y, c.x + c.width - r.x, r.height),
                    None => r,
                };
                let spec = state_class(*active, hovered || hov == Some(menu.as_str()));
                let mut b = face(El::new(spec).css(at(full, o)), id, full.width, full.height, spec != "div.t-item", false);
                let size = cfg.icon_size.min(r.width - 2.0);
                b = b.child(icon_box(icon.name(), Rect::new(r.x + r.width / 2.0 - size / 2.0, r.y + r.height / 2.0 - size / 2.0, size, size), full));
                if let Some(c) = chev {
                    let (x, y, s) = (c.x + c.width / 2.0, c.y + c.height / 2.0, 2.5);
                    b = b.child(glyph(&[(x - s, y - s / 2.0), (x, y + s / 2.0), (x + s, y - s / 2.0)], false, 1.2, full));
                }
                root = root.child(b);
            }
            _ => {}
        }
    }
    (root, slots)
}
