//! The app's toolbars (top control strip, left drawing rail, right sidebar rail) as
//! uzor DOM.
//!
//! Was: `mlc-app-layout` `toolbar_core::draw_toolbar_with_icons`, which lays out,
//! draws and hit-tests every item by hand. Now the DOM does all three from the app's
//! toolbar *model* (`ToolbarConfig`: sections, items, sizes, theme): a flex row (or
//! column) with the model's padding and spacing, section separators, an end group
//! pinned to the far edge, item boxes sized by their content (labels are measured by
//! the DOM). Hover is the DOM's own (the id it reports), and each item box carries the
//! app's widget id (`dtb:` / `csb:` / `rtb:` + item id), so a click on it runs the
//! app's existing action for that id ([`HostedApp`](crate::HostedApp)). Nothing of the
//! app's item rects is read.
//!
//! What stays the app's: the model and the actions; its layout pass still runs (with
//! drawing switched off, `toolbar_core::capture`) because its dropdowns and tooltips
//! anchor to its own item rects, and a toolbar it would scroll (overflow) or with item
//! kinds not handled here is drawn by the app as before. The animated mascot ("Bot")
//! is the app's own drawing in a paint slot, and the backdrop blur is the context's
//! own call in a slot (see [`BLUR`]).

use mlc_app_layout::ui::toolbar_core::capture::CapturedToolbar;
use mlc_app_layout::ui::toolbar_core::{stroke_compensated_icon, SectionAlign, ToolbarItem, ToolbarOrientation};
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
div.t-row {{ position: absolute; left: 0px; top: 0px; display: flex; align-items: center }}
div.t-row > div.t-item, div.t-row > div.t-sep {{ position: relative; flex-shrink: 0 }}
div.t-lab {{ display: flex; align-items: center; white-space: nowrap; padding-top: 2px }}
div.t-abs {{ position: absolute; top: 0px; display: flex; align-items: center; white-space: nowrap; padding-top: 2px }}
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
div.t-strike {{ position: absolute; height: 1px; background: currentColor }}
div.t-sep {{ background: {sep} }}
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


/// The DOM id of item `id` in toolbar `slot`: the id the app registers it under.
pub fn dom_id(slot: &str, id: &str) -> String {
    let p = match slot {
        "left" => "dtb",
        "top" => "csb",
        "right" => "rtb",
        _ => "tb",
    };
    format!("{p}:{id}")
}

fn px(r: f64) -> String {
    format!("{r}px")
}

fn icon_el(name: &str, x: f64, y: f64, size: f64) -> El {
    let e = El::new("div.t-icon").css(format!("left: {}; top: {}; width: {}; height: {}", px(x), px(y), px(size), px(size)));
    match mlc_app_layout::ui::icons::icon_svg(name) {
        Some(svg) => e.svg(stroke_compensated_icon(svg, size).into_owned()),
        None => e,
    }
}

/// A filled or stroked polyline, its points relative to the box's top-left; the box
/// is placed by `pos` (CSS insets) and the pen kept exact.
fn glyph(points: &[(f64, f64)], fill: bool, pen: f64, pos: String) -> El {
    let w = points.iter().map(|p| p.0).fold(0.0, f64::max) + 2.0;
    let h = points.iter().map(|p| p.1).fold(0.0, f64::max) + 2.0;
    let mut d = String::new();
    for (i, (x, y)) in points.iter().enumerate() {
        d.push_str(&format!("{}{} {}", if i == 0 { "M" } else { "L" }, x, y));
    }
    let paint = if fill { "fill=\"currentColor\" stroke=\"none\"".to_string() } else { format!("fill=\"none\" stroke=\"currentColor\" stroke-width=\"{pen}\"") };
    if fill {
        d.push('Z');
    }
    El::new("div.t-glyph").css(format!("{pos}; width: {w}px; height: {h}px")).svg(format!(r#"<svg viewBox="0 0 {w} {h}" {paint}><path d="{d}"/></svg>"#))
}

/// A down chevron of half-width `s` centred on `(cx, cy)` of its item, as the old
/// drawing placed it, in a box 2 px larger on every side.
fn chevron(s: f64, fill: bool, pen: f64, pos: impl FnOnce(f64, f64) -> String) -> El {
    let (o, w, h) = (2.0, 2.0 * s + 4.0, s + 4.0);
    glyph(&[(o, o), (o + s, o + s), (o + 2.0 * s, o)], fill, pen, pos(w, h))
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

/// The toolbar `t`, `w`×`h`, from its model. `hovered`: the DOM id under the
/// pointer last frame. `measure` measures a 13px label (the DOM's own font) for the
/// one case the old drawing sized from it: a fixed-width dropdown's highlight.
/// Returns the tree and the paint slots it asks for: `(key, icon, color)`.
pub fn tree(t: &CapturedToolbar, w: f64, h: f64, hovered: Option<&str>, measure: &mut dyn FnMut(&str) -> f64) -> (El, Vec<(String, String, String)>) {
    let cfg = &t.config;
    let th = &t.theme;
    let horizontal = matches!(cfg.orientation, ToolbarOrientation::Horizontal);
    let (is, sp, pad, sep) = (cfg.item_size, cfg.spacing, cfg.padding, cfg.separator_size);
    let mut slots = Vec::new();
    let full = format!("width: {w}px; height: {h}px");
    let mut root = El::new("div.t-bar")
        .css(full.clone())
        .child(El::new("div.t-blur").slot(format!("{BLUR}{}:bar", t.slot)).css(full.clone()))
        .child(El::new("div.t-bg").css(full.clone()));
    // Gap after a box along the main axis.
    let gap = |g: f64| if horizontal { format!("margin-right: {g}px") } else { format!("margin-bottom: {g}px") };
    // A lit item's face: the blur, then the fill (`draw_hover_rounded_rect` /
    // `draw_active_rounded_rect` / `draw_sidebar_active_item`).
    let face = |b: El, key: &str, fw: f64, fh: f64, lit: bool, side: bool| -> El {
        if !lit {
            return b;
        }
        let fw = if side { fw - 3.0 } else { fw };
        b.child(El::new("div.t-blur").slot(format!("{BLUR}{}:{key}", t.slot)).css(format!("width: {fw}px; height: {fh}px")))
            .child(El::new("div.t-face").css(format!("width: {fw}px; height: {fh}px")))
    };
    let separator = |g: f64| {
        if horizontal {
            El::new("div.t-sep").css(format!("width: {sep}px; height: {}px; {}", h * 0.5, gap(g)))
        } else {
            El::new("div.t-sep").css(format!("width: {}px; height: {sep}px; {}", w * 0.7, gap(g)))
        }
    };
    let mut item = |it: &ToolbarItem| -> Option<El> {
        let id = it.id().unwrap_or_default();
        let did = dom_id(t.slot, id);
        let hov = hovered == Some(did.as_str());
        Some(match it {
            ToolbarItem::Separator => separator(sp),
            ToolbarItem::Spacer => El::new("div").css(if horizontal { format!("width: {is}px; height: 1px; flex-shrink: 0") } else { format!("height: {is}px; width: 1px; flex-shrink: 0") }),
            ToolbarItem::IconButton { icon, active, disabled, min_width, .. } => {
                let iw = if horizontal && *min_width > 0.0 { *min_width } else { is };
                let side = !horizontal && th.sidebar_style && *active && !*disabled;
                let (bw, bh) = if side { (w, is) } else { (iw, is) };
                let size = if matches!(id, "slot_dom" | "slot_tape" | "slot_l2" | "slot_big_trades" | "slot_liq" | "screener") { (cfg.icon_size + 4.0).max(20.0) } else { cfg.icon_size };
                let geo = format!("width: {bw}px; height: {bh}px; {}", gap(sp));
                let mut b = if side {
                    face(El::new("div.t-item.side").id(did.clone()).css(geo).child(El::new("div.t-mark").css(format!("height: {bh}px"))), &did, bw, bh, true, true)
                } else {
                    let spec = if *disabled { "div.t-item" } else { state_class(*active, hov) };
                    face(El::new(spec).id(did.clone()).css(geo), &did, bw, bh, spec != "div.t-item", false)
                };
                let (ix, iy) = (bw / 2.0 - size / 2.0, bh / 2.0 - size / 2.0);
                if icon.name() == "Bot" {
                    let key = format!("{SLOT}{}:{id}", t.slot);
                    let color = if *disabled { &th.item_text } else if *active { &th.item_text_active } else if hov { &th.item_text_hover } else { &th.item_text };
                    slots.push((key.clone(), icon.name().to_string(), color.clone()));
                    // The mascot is drawn 36 px wide, centred on the icon box.
                    let m = 36.0_f64.max(size);
                    b = b.child(El::new("div.t-icon").slot(key).css(format!("left: {}px; top: {}px; width: {m}px; height: {m}px", bw / 2.0 - m / 2.0, bh / 2.0 - m / 2.0)));
                } else {
                    b = b.child(icon_el(icon.name(), ix, iy, size));
                }
                b
            }
            ToolbarItem::Button { icon, text: label, active, disabled, min_width, strikethrough, .. } => {
                let spec = if *disabled { "div.t-item" } else { state_class(*active, hov) };
                let left = if icon.is_some() { cfg.icon_size + 4.0 } else { 8.0 };
                let bw = if !horizontal || label.is_none() { is } else if *min_width > 0.0 { *min_width } else { left + label.as_deref().map(&mut *measure).unwrap_or(0.0) + 8.0 };
                let mut b = El::new(spec).id(did.clone()).css(format!("width: {bw}px; height: {is}px; {}", gap(sp)));
                b = face(b, &did, bw, is, spec != "div.t-item", false);
                if let Some(ic) = icon {
                    b = b.child(icon_el(ic.name(), bw / 2.0 - cfg.icon_size / 2.0, is / 2.0 - cfg.icon_size / 2.0, cfg.icon_size));
                }
                if let Some(label) = label {
                    let tw = measure(label);
                    let x = if icon.is_some() { left } else { bw / 2.0 - tw / 2.0 };
                    b = b.child(El::new("div.t-abs").css(format!("left: {x}px; height: {is}px")).child(El::new("span").text(label.clone())));
                    if *strikethrough {
                        b = b.child(El::new("div.t-strike").css(format!("left: {}px; top: {}px; width: {}px", x - 1.0, is / 2.0 - 0.5, tw + 2.0)));
                    }
                }
                b
            }
            ToolbarItem::Dropdown { icon, text: label, active, show_chevron, min_width, .. } => {
                let spec = state_class(*active, hov);
                let left = if label.is_some() { if icon.is_some() { cfg.icon_size + 8.0 } else { 8.0 } } else { 4.0 + cfg.icon_size };
                let right = if *show_chevron { 14.0 } else { 8.0 };
                // A natural box hugs its label (the DOM sizes it); a fixed one keeps its
                // width and its highlight still covers the label, as before.
                let natural = horizontal && *min_width <= 0.0 && label.is_some();
                let tw = label.as_deref().map(&mut *measure).unwrap_or(0.0);
                let bw = if !horizontal || (label.is_none() && *min_width <= 0.0) { is } else if natural { left + tw + right } else { *min_width };
                let hw = bw.max(left + tw + right);
                let mut b = El::new(spec).id(did.clone()).css(format!("width: {bw}px; height: {is}px; {}", gap(sp)));
                b = face(b, &did, hw, is, spec != "div.t-item", false);
                if let Some(ic) = icon {
                    b = b.child(icon_el(ic.name(), 4.0, is / 2.0 - cfg.icon_size / 2.0, cfg.icon_size));
                }
                if let Some(label) = label {
                    let lab = El::new("span").text(label.clone());
                    b = if natural {
                        b.child(El::new("div.t-lab").css(format!("height: {is}px; padding-left: {left}px; padding-right: {right}px")).child(lab))
                    } else {
                        b.child(El::new("div.t-abs").css(format!("left: {left}px; height: {is}px")).child(lab))
                    };
                }
                if *show_chevron {
                    // Tip at (right edge - 10, centre + 2): `x ± 4`, `y ∓ 2`.
                    b = b.child(chevron(4.0, true, 0.0, |cw, ch| format!("position: absolute; left: {}px; top: {}px", bw - 10.0 - cw / 2.0, is / 2.0 - ch / 2.0)));
                }
                b
            }
            ToolbarItem::SplitIconButton { icon, active, .. } => {
                let menu = format!("{did}_menu");
                let spec = state_class(*active, hov || hovered == Some(menu.as_str()));
                let bw = is + 10.0;
                let mut b = face(El::new(spec).id(did.clone()).css(format!("width: {bw}px; height: {is}px; {}", gap(sp))), &did, bw, is, spec != "div.t-item", false);
                let size = cfg.icon_size.min(is - 2.0);
                b = b.child(icon_el(icon.name(), is / 2.0 - size / 2.0, is / 2.0 - size / 2.0, size));
                b.child(
                    El::new("div.t-chev")
                        .id(menu)
                        .css(format!("position: absolute; left: {is}px; top: 0px; width: 10px; height: {is}px"))
                        .child(chevron(2.5, false, 1.2, |cw, ch| format!("position: absolute; left: {}px; top: {}px", 5.0 - cw / 2.0, is / 2.0 - ch / 2.0))),
                )
            }
            _ => return None,
        })
    };
    let n = cfg.sections.len();
    let mut group = |end: bool| -> El {
        let mut row = El::new("div.t-row");
        for (i, s) in cfg.sections.iter().enumerate().filter(|(_, s)| (s.align == SectionAlign::End) == end) {
            for it in &s.items {
                if let Some(e) = item(it) {
                    row = row.child(e);
                }
            }
            if s.show_separator && i < n - 1 {
                row = row.child(separator(2.0 * sp));
            }
        }
        row
    };
    let start = group(false).css(if horizontal {
        format!("width: {w}px; height: {h}px; padding-left: {pad}px")
    } else {
        format!("width: {w}px; height: {h}px; padding-top: {pad}px; flex-direction: column")
    });
    root = root.child(start);
    if cfg.sections.iter().any(|s| s.align == SectionAlign::End && !s.items.is_empty()) {
        // Pinned `end_reserved` (its content + the padding) from the far edge.
        // Its boxes carry their trailing gaps, so the group's own width is the old
        // `end_width`: pinned `padding` from the far edge, it starts where it did.
        let end = group(true);
        let pinned = if horizontal {
            format!("left: auto; right: {pad}px; height: {h}px")
        } else {
            format!("top: auto; bottom: {pad}px; width: {w}px; flex-direction: column")
        };
        root = root.child(end.css(pinned));
    }
    (root, slots)
}
