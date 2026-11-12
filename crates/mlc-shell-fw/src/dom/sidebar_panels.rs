//! The right sidebar's other panels (alerts, events, object tree, signals,
//! monitoring, screener) as uzor DOM bodies under the shared header
//! ([`super::sidebar`]).
//!
//! Each body is the content of the panel's scroll container (`dom_scroll`), laid out
//! where `mlc-app-sidebar-content`'s renderer put each piece, with the app's widget
//! ids, so hover comes from the DOM and presses reach the app as before.

use mlc_app_sidebar_content::render::{format_event_time, format_price_smart};
use mlc_app_sidebar_content::state::RightSidebarPanel;
use mlc_core::state::icons::Icon;
use uzor::ui::dom::El;

use super::sidebar::{Sidebar, SCROLLBAR, VIEWPORT};

const SCROLLBAR_W: f64 = 8.0;
/// The text box of [`base_text`]: taller than any line it holds.
const AT_H: f64 = 40.0;
/// Where the DOM puts the alphabetic baseline below the middle of a text box, per px
/// of font size: it centres the line on Inter's ascent + descent (1984 + 494 of
/// 2048) and draws at the ascent.
const DOM_BASELINE: f64 = 1984.0 / 2048.0 - (1984.0 + 494.0) / 2048.0 / 2.0;

/// How a run sits on its anchor `x`.
#[derive(Clone, Copy, PartialEq)]
pub enum At {
    L,
    C,
    R,
}

/// The font size of a canvas font string.
fn px_of(font: &str) -> f64 {
    font.split_whitespace().find_map(|p| p.strip_suffix("px").and_then(|n| n.parse().ok())).unwrap_or(12.0)
}

/// The ascent the app's text renderer uses for `font` (fontdue's line metrics:
/// Roboto's hhea 1900/2048, JetBrains Mono 1020/1000, PT Root UI 950/1000).
fn ascent(font: &str) -> f64 {
    let l = font.to_ascii_lowercase();
    let r = if l.contains("monospace") || l.contains("mono") {
        1.02
    } else if l.contains("pt root") {
        0.95
    } else {
        1900.0 / 2048.0
    };
    px_of(font) * r
}

/// `s` anchored at `x` by `at` with its alphabetic baseline at `base`, exactly where
/// the app's `fill_text` puts that run: the DOM hands the renderer the same anchor and
/// alignment (a box centred on, or ending at, the anchor), so the pen is the same to
/// the bit.
pub fn base_text(s: &str, x: f64, at: At, base: f64, font: &str, color: &str) -> El {
    const W: f64 = 1024.0;
    let top = base - AT_H / 2.0 - DOM_BASELINE * px_of(font);
    let (left, align) = match at {
        At::L => (x, "left"),
        At::C => (x - W / 2.0, "center"),
        At::R => (x - W, "right"),
    };
    El::new("div.s-at")
        .css(format!("left: {left}px; top: {top}px; width: {W}px; text-align: {align}; {}; color: {color}", super::sidebar::font_css(font)))
        .text(s.to_string())
}

/// The app's `fill_text(s, x, y)` with `TextBaseline::Top`.
pub fn top_text(s: &str, x: f64, y: f64, at: At, font: &str, color: &str, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
    runs(s, x, at, y + ascent(font), font, color, measure)
}

/// The app's `fill_text(s, x, y)` with `TextBaseline::Middle`.
pub fn mid_text(s: &str, x: f64, y: f64, at: At, font: &str, color: &str, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
    runs(s, x, at, y + ascent(font) / 2.0, font, color, measure)
}

/// [`base_text`], except that a run with spaces the DOM would collapse (leading,
/// trailing or doubled) is drawn as its pieces at the app's pen offsets.
fn runs(s: &str, x: f64, at: At, base: f64, font: &str, color: &str, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
    if !(s.starts_with(' ') || s.contains("  ")) {
        return base_text(s, x, at, base, font, color);
    }
    let left = match at {
        At::L => x,
        At::C => x - measure(font, s) / 2.0,
        At::R => x - measure(font, s),
    };
    let mut el = El::new("div.s-abs").css("left: 0px; top: 0px; width: 0px; height: 0px");
    let mut i = 0;
    for piece in s.split(' ') {
        if !piece.is_empty() {
            el = el.child(base_text(piece, left + measure(font, &s[..i]), At::L, base, font, color));
        }
        i += piece.len() + 1;
    }
    el
}

/// A filled box.
pub fn rect(l: f64, t: f64, w: f64, h: f64, color: &str) -> El {
    El::new("div.s-fill").css(format!("left: {l}px; top: {t}px; width: {w}px; height: {h}px; background: {color}"))
}

/// An icon box at `l, t`, `size` square, in `color`.
pub fn icon_at(icon: &str, l: f64, t: f64, size: f64, color: &str) -> El {
    El::new("div.s-abs").css(format!("left: {l}px; top: {t}px; width: {size}px; height: {size}px; color: {color}")).svg(icon)
}

/// A hit box carrying the app's widget id.
pub fn hit(id: impl Into<String>, l: f64, t: f64, w: f64, h: f64) -> El {
    El::new("div.s-hit").id(id.into()).css(format!("left: {l}px; top: {t}px; width: {w}px; height: {h}px"))
}

/// The app's `word_wrap_text` with the app's measure.
pub fn word_wrap(text: &str, max_w: f64, font: &str, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<String> {
    if text.is_empty() {
        return vec![String::new()];
    }
    let mut lines = Vec::new();
    for paragraph in text.split('\n') {
        let mut current = String::new();
        for word in paragraph.split(' ') {
            let candidate = if current.is_empty() { word.to_string() } else { format!("{current} {word}") };
            if measure(font, &candidate) <= max_w {
                current = candidate;
            } else {
                if !current.is_empty() {
                    lines.push(std::mem::take(&mut current));
                }
                if measure(font, word) > max_w {
                    let mut buf = String::new();
                    for ch in word.chars() {
                        buf.push(ch);
                        if measure(font, &buf) > max_w {
                            buf.pop();
                            if !buf.is_empty() {
                                lines.push(std::mem::take(&mut buf));
                            }
                            buf = ch.to_string();
                        }
                    }
                    current = buf;
                } else {
                    current = word.to_string();
                }
            }
        }
        lines.push(current);
    }
    lines
}

/// The panel body under the header: what is pinned, then the scroll container with
/// the panel's content and the scrollbar.
pub fn body(mut root: El, w: &Sidebar<'_>, measure: &mut dyn FnMut(&str, &str) -> f64) -> El {
    let t = w.theme;
    let rw = w.rect.width;
    let content_w = rw - SCROLLBAR_W;
    let vt = w.viewport.y - w.rect.y;
    let vh = w.viewport.height;
    // The border's other half shows where nothing paints over it: beside the body
    // (the screener's pinned rows cover it).
    let bt = if w.state.right_panel == RightSidebarPanel::Screener { vt } else { super::sidebar::HEADER_H };
    root = root.child(El::new("div.s-fill").css(format!(
        "left: 0px; top: {bt}px; width: 1px; height: {}px; opacity: 0.5; background: {}",
        w.rect.height - bt,
        t.separator
    )));
    if w.state.right_panel == RightSidebarPanel::Screener {
        root = root.children(screener_head(w, content_w, measure));
    }
    let content = match w.state.right_panel {
        RightSidebarPanel::Alerts => alerts(w, content_w, measure),
        RightSidebarPanel::Events => events(w, content_w, measure),
        RightSidebarPanel::ObjectTree => object_tree(w, content_w, measure),
        RightSidebarPanel::Signals => signals(w, content_w, measure),
        RightSidebarPanel::Monitoring => monitoring(w, content_w, measure),
        RightSidebarPanel::Screener => screener(w, content_w, measure),
        _ => Vec::new(),
    };
    let h = w.app_content_h.max(0.0);
    let content = El::new("div.s-content").css(format!("width: {content_w}px; height: {h}px")).children(content);
    root = root.child(El::new("div.s-view").id(VIEWPORT).css(format!("top: {vt}px; width: {rw}px; height: {vh}px")).child(content));
    if h > vh + 0.5 {
        root = root.child(El::new("div.s-scroll").css(format!("left: {content_w}px; top: {vt}px; height: {vh}px")).slot(SCROLLBAR));
    }
    root
}

fn empty(lines: &[&str], content_w: f64, step: f64, measure: &mut dyn FnMut(&str, &str) -> f64, color: &str) -> Vec<El> {
    lines.iter().enumerate().map(|(i, l)| top_text(l, content_w / 2.0, i as f64 * step, At::C, "12px sans-serif", color, measure)).collect()
}

/// `render_alert_items`: rows grouped by source (price, drawing, indicator).
fn alerts(w: &Sidebar<'_>, content_w: f64, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<El> {
    use mlc_core::alert::{AlertSource, AlertStatus};
    let t = w.theme;
    let s = w.state;
    if s.alert_items.is_empty() {
        return empty(&["No alerts configured", "Click + to add an alert"], content_w, 20.0, measure, &t.item_text_muted);
    }
    let mut groups: [(&str, &str, Vec<&mlc_core::alert::AlertItem>); 3] =
        [("price", "Price Alerts", Vec::new()), ("drawing", "Drawing Alerts", Vec::new()), ("indicator", "Indicator Alerts", Vec::new())];
    for item in &s.alert_items {
        let g = match &item.source {
            AlertSource::Price { .. } => 0,
            AlertSource::Drawing { .. } => 1,
            _ => 2,
        };
        groups[g].2.push(item);
    }
    let (item_h, section_h, pad, icon, dot) = (54.0, 24.0, 8.0, 14.0, 6.0);
    let mut out = Vec::new();
    let mut y = 0.0;
    for (suffix, label, items) in &groups {
        if items.is_empty() {
            continue;
        }
        out.push(hit(format!("alert_grp:{suffix}"), 0.0, y, content_w, section_h));
        let font = "bold 11px sans-serif";
        out.push(mid_text("\u{25BC}", pad, y + section_h / 2.0, At::L, font, &t.item_text_muted, measure));
        out.push(mid_text(&format!("{label} ({})", items.len()), pad + 14.0, y + section_h / 2.0, At::L, font, &t.item_text_muted, measure));
        y += section_h;
        for item in items {
            let row_id = format!("alert_{}", item.id);
            let del_id = format!("alert_delete_{}", item.id);
            let hovered = w.hov(&row_id) || w.hov(&del_id);
            let bg = if hovered { format!("; background: {}", t.item_bg_hover) } else { String::new() };
            out.push(El::new("div.s-hit").id(row_id).css(format!("left: 4px; top: {y}px; width: {}px; height: {item_h}px{bg}", content_w - 8.0)));
            let bar = match item.status {
                AlertStatus::Active => t.accent.as_str(),
                AlertStatus::Triggered => "#ff9800",
                _ => t.item_text_muted.as_str(),
            };
            out.push(rect(pad, y + 4.0, 4.0, item_h - 8.0, bar));
            let name = if item.name.is_empty() { item.source_display() } else { item.name.clone() };
            out.push(top_text(&name, pad + 12.0, y + 6.0, At::L, "12px sans-serif", &t.item_text, measure));
            let cond = if item.condition.requires_second_price() {
                format!("{} {} - {}", item.condition.display_name(), format_price_smart(item.price), format_price_smart(item.price2))
            } else {
                format!("{} @ {}", item.condition.display_name(), format_price_smart(item.price))
            };
            out.push(top_text(&cond, pad + 12.0, y + 23.0, At::L, "10px sans-serif", &t.item_text_muted, measure));
            let sym = item.symbol();
            let venue = mlc_core::venue_label(item.exchange());
            let line3 = match (venue.is_empty(), item.timeframe.is_empty()) {
                (true, true) => sym.to_string(),
                (false, true) => format!("{sym}:{venue}"),
                (true, false) => format!("{sym}:{}", item.timeframe),
                (false, false) => format!("{sym}:{venue}:{}", item.timeframe),
            };
            out.push(top_text(&line3, pad + 12.0, y + 37.0, At::L, "9px sans-serif", &t.item_text_muted, measure));
            let dot_color = match item.status {
                AlertStatus::Active => "#4caf50",
                AlertStatus::Triggered => "#ff9800",
                AlertStatus::Paused => "#9e9e9e",
                AlertStatus::Expired => "#f44336",
            };
            out.push(rect(content_w - pad - icon - 8.0 - dot, y + (item_h - dot) / 2.0, dot, dot, dot_color));
            let (ix, iy) = (content_w - pad - icon - 4.0, y + (item_h - icon) / 2.0);
            let ic = if hovered { &t.item_text_active } else { &t.item_text_muted };
            out.push(icon_at(Icon::Close.svg(), ix, iy, icon, ic).id(del_id));
            y += item_h;
        }
        y += 8.0;
    }
    out
}

/// `render_events_items`: the system event log, newest first.
fn events(w: &Sidebar<'_>, content_w: f64, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<El> {
    let t = w.theme;
    let s = w.state;
    if s.system_event_items.is_empty() {
        let msg = "Nothing yet — sign-ins, shares, tier changes and sync results land here.";
        let lines = word_wrap(msg, (content_w - 32.0).max(60.0), "12px sans-serif", measure);
        let lines: Vec<&str> = lines.iter().map(String::as_str).collect();
        return empty(&lines, content_w, 16.0, measure, &t.item_text_muted);
    }
    let pad = 8.0;
    let text_x = pad + 12.0;
    let body_max = (content_w - pad * 2.0 - 12.0).max(20.0);
    let mut out = Vec::new();
    let mut y = 0.0;
    for (k, ev) in s.system_event_items.iter().enumerate() {
        let row_id = format!("events:row:{}", ev.id);
        // The app wraps before setting the row's font: the first body is measured in
        // the header button's 11px, the others in the previous row's 10px.
        let wrap_font = if k == 0 { "11px sans-serif" } else { "10px sans-serif" };
        let body = word_wrap(&ev.body, body_max, wrap_font, measure);
        let item_h = 20.0 + 16.0 + body.len().min(2) as f64 * 14.0 + pad * 2.0;
        let bg = if w.hov(&row_id) { format!("; background: {}", t.item_bg_hover) } else { String::new() };
        out.push(El::new("div.s-hit").id(row_id).css(format!("left: 4px; top: {y}px; width: {}px; height: {item_h}px{bg}", content_w - 8.0)));
        if !ev.read {
            out.push(rect(pad, y + 4.0, 4.0, item_h - 8.0, &t.accent));
        }
        let line1 = format!("{:?}  ·  {}", ev.kind, format_event_time(ev.at_ms, w.now_ms));
        out.push(top_text(&line1, text_x, y + 6.0, At::L, "10px sans-serif", &t.item_text_muted, measure));
        out.push(top_text(&ev.title, text_x, y + 20.0, At::L, "bold 12px sans-serif", &t.item_text, measure));
        for (i, line) in body.iter().take(2).enumerate() {
            let shown = if body.len() > 2 && i == 1 { format!("{}…", line.trim_end()) } else { line.clone() };
            out.push(top_text(&shown, text_x, y + 34.0 + i as f64 * 14.0, At::L, "10px sans-serif", &t.item_text_muted, measure));
        }
        y += item_h + 2.0;
    }
    out
}

/// `#rrggbb` at `a` (0..1) as `rgba()` (the app's `#rrggbbAA` fills).
pub fn alpha(hex: &str, a: f64) -> String {
    let h = hex.trim_start_matches('#');
    let c = |i: usize| u8::from_str_radix(h.get(i..i + 2).unwrap_or("00"), 16).unwrap_or(0);
    format!("rgba({}, {}, {}, {a})", c(0), c(2), c(4))
}

/// `render_object_tree_items`: sections (group, window, loose), key groups, rows
/// with their buttons; memory rows after a rule.
fn object_tree(w: &Sidebar<'_>, content_w: f64, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<El> {
    use mlc_app_sidebar_content::types::ObjectItemState;
    use mlc_core::state::command::ObjectCategory;
    let t = w.theme;
    let items = &w.state.object_tree_items;
    if items.is_empty() {
        return empty(&["No objects on chart", "Draw something to see it here"], content_w, 20.0, measure, &t.item_text_muted);
    }
    let (item_h, pad, key_h, icon) = (32.0, 8.0, 18.0, 14.0);
    let mut out = Vec::new();
    let mut y = 0.0;
    for section in [Some("Group"), Some("Window"), None] {
        let idx: Vec<usize> = items.iter().enumerate().filter(|(_, i)| i.section.as_deref() == section).map(|(k, _)| k).collect();
        if idx.is_empty() {
            continue;
        }
        if let Some(label) = section {
            if y > 0.0 {
                out.push(rect(pad, y + 3.0, content_w - pad * 2.0, 1.0, &t.item_text_muted));
                y += 8.0;
            }
            out.push(mid_text(label, pad, y + 10.0, At::L, "bold 11px sans-serif", &t.item_text, measure));
            y += 22.0;
        }
        let mut keys: Vec<String> = Vec::new();
        for &k in &idx {
            let key = items[k].key_label();
            if !keys.contains(&key) {
                keys.push(key);
            }
        }
        for key in &keys {
            out.push(mid_text(key, pad + 4.0, y + key_h / 2.0, At::L, "10px sans-serif", &t.item_text_muted, measure));
            y += key_h;
            let mut memory_rule = false;
            for &k in idx.iter().filter(|&&k| &items[k].key_label() == key) {
                let item = &items[k];
                if !item.is_interactive() && !memory_rule {
                    memory_rule = true;
                    let ind = pad + 4.0;
                    out.push(rect(ind, y + 8.0, content_w - ind * 2.0, 1.0, &t.item_text_muted));
                    out.push(mid_text("\u{2014} Memory \u{2014}", ind, y + 16.0, At::L, "10px sans-serif", &t.item_text_muted, measure));
                    y += 26.0;
                }
                let prefix = match item.category {
                    ObjectCategory::Indicator => "ind",
                    ObjectCategory::Compare => "cmp",
                    _ => "drw",
                };
                let tag = match (&item.item_state, item.section.as_deref()) {
                    (ObjectItemState::Memory, Some("Group")) => "grpmem",
                    (ObjectItemState::Memory, Some("Window")) => "winmem",
                    (ObjectItemState::Memory, _) => "mem",
                    (_, Some("Group")) => "grp",
                    (_, Some("Window")) => "win",
                    _ => "flt",
                };
                let slug = if item.item_state == ObjectItemState::Memory { format!("{}_{prefix}", item.key_label().replace(':', "_")) } else { prefix.to_string() };
                let wp = format!("{tag}_{slug}");
                let item_id = format!("{wp}_{}", item.id);
                let is_drawing = item.category != ObjectCategory::Indicator && item.category != ObjectCategory::Compare;
                let step = icon + 4.0;
                let iy = y + (item_h - icon) / 2.0;
                let del_x = content_w - pad - icon;
                let (set_x, alert_x) = (del_x - step, del_x - 2.0 * step);
                let (vis_x, lock_x) = (del_x - 3.0 * step, del_x - 4.0 * step);
                let name_x = if item.color.is_some() { pad + 24.0 } else { pad };
                let del_id = format!("{wp}_delete_{}", item.id);
                if item.is_interactive() {
                    let set_id = format!("{wp}_settings_{}", item.id);
                    let alert_id = format!("{wp}_alert_{}", item.id);
                    let vis_id = format!("{wp}_vis_{}", item.id);
                    let lock_id = is_drawing.then(|| format!("{wp}_lock_{}", item.id));
                    let hovered = [&item_id, &del_id, &set_id, &alert_id, &vis_id].iter().any(|i| w.hov(i)) || lock_id.as_deref().is_some_and(|l| w.hov(l));
                    let bg = if item.selected {
                        format!("; background: {}", alpha(&t.accent, 64.0 / 255.0))
                    } else if hovered {
                        format!("; background: {}", t.item_bg_hover)
                    } else {
                        String::new()
                    };
                    out.push(El::new("div.s-hit").id(item_id).css(format!("left: 4px; top: {y}px; width: {}px; height: {item_h}px{bg}", content_w - 8.0)));
                    if let Some(c) = &item.color {
                        out.push(rect(pad, y + 8.0, 16.0, 16.0, c));
                    }
                    let row = if item.visible { &t.item_text } else { &t.item_text_hidden };
                    out.push(mid_text(&item.name, name_x, y + item_h / 2.0, At::L, "12px sans-serif", row, measure));
                    let del_c = if w.hov(&del_id) { "#ff5252" } else { row.as_str() };
                    out.push(icon_at(Icon::Close.svg(), del_x, iy, icon, del_c).id(del_id));
                    out.push(icon_at(Icon::Settings.svg(), set_x, iy, icon, row).id(set_id));
                    let bell = if item.category == ObjectCategory::Compare {
                        t.item_text_hidden.as_str()
                    } else if item.has_alert {
                        t.accent.as_str()
                    } else {
                        row.as_str()
                    };
                    out.push(icon_at(Icon::Alert.svg(), alert_x, iy, icon, bell).id(alert_id));
                    let eye = if item.visible { Icon::Eye } else { Icon::EyeOff };
                    out.push(icon_at(eye.svg(), vis_x, iy, icon, row).id(vis_id));
                    if let Some(lid) = lock_id {
                        let (li, lc) = if item.locked { (Icon::Lock, t.item_text.as_str()) } else { (Icon::Unlock, row.as_str()) };
                        out.push(icon_at(li.svg(), lock_x, iy, icon, lc).id(lid));
                    }
                } else {
                    if let Some(c) = &item.color {
                        out.push(rect(pad, y + 8.0, 16.0, 16.0, c));
                    }
                    out.push(mid_text(&item.name, name_x, y + item_h / 2.0, At::L, "12px sans-serif", &t.item_text_muted, measure));
                    if item.is_deletable() {
                        let c = if w.hov(&del_id) { "#ff5252" } else { t.item_text_muted.as_str() };
                        out.push(icon_at(Icon::Close.svg(), del_x, iy, icon, c).id(del_id));
                    }
                }
                y += item_h;
            }
            y += 8.0;
        }
    }
    out
}

/// The `#rrggbbAA` fill the app makes by appending `aa` (hex) to a theme colour.
pub fn hexa(hex: &str, aa: &str) -> String {
    alpha(hex, u8::from_str_radix(aa, 16).unwrap_or(255) as f64 / 255.0)
}

/// `render_indicator_signals`: the total, then each indicator's group with its rows
/// (at most 8 shown; the host leaves groups that scroll to the app).
fn signals(w: &Sidebar<'_>, content_w: f64, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<El> {
    let t = w.theme;
    let s = w.state;
    let sig = &s.indicator_signals;
    if sig.groups.is_empty() {
        return empty(&["No indicator signals", "Add indicators with signals enabled"], content_w, 20.0, measure, &t.item_text_muted);
    }
    let (head_h, row_h, pad) = (28.0, 24.0, 8.0);
    let mut out = Vec::new();
    let mut y = 0.0;
    out.push(mid_text(&format!("Total: {} signals", sig.total_count), pad, y + 10.0, At::L, "11px sans-serif", &t.item_text_muted, measure));
    y += 24.0;
    for g in &sig.groups {
        let collapsed = s.collapsed_signal_groups.contains(&g.instance_id);
        let gid = format!("signal_group_{}", g.instance_id);
        let bg = if w.hov(&gid) { format!("; background: {}", t.item_bg_hover) } else { String::new() };
        out.push(El::new("div.s-hit").id(gid).css(format!("left: 4px; top: {y}px; width: {}px; height: {head_h}px{bg}", content_w - 8.0)));
        let arrow = if collapsed { Icon::ChevronRight } else { Icon::ChevronDown };
        out.push(icon_at(arrow.svg(), pad, y + head_h / 2.0 - 5.0, 10.0, &t.item_text));
        out.push(mid_text(&g.indicator_name, pad + 16.0, y + head_h / 2.0, At::L, "12px sans-serif", &t.item_text, measure));
        let (bw, bh) = (24.0, 20.0);
        let (bx, by) = (content_w - bw - 4.0, y + (head_h - bh) / 2.0);
        out.push(rect(bx, by, bw, bh, &hexa(&t.accent, "30")));
        out.push(mid_text(&g.signals.len().to_string(), bx + bw / 2.0, by + bh / 2.0, At::C, "10px sans-serif", &t.accent, measure));
        y += head_h;
        if !collapsed {
            let rows = g.signals.len().min(8);
            let vh = rows as f64 * row_h;
            let mut view = El::new("div.s-clip").id(format!("signal_group:{}:viewport", g.instance_id)).css(format!(
                "left: 4px; top: {y}px; width: {}px; height: {vh}px; background: {}",
                content_w - 8.0,
                hexa(&t.item_bg_hover, "20")
            ));
            for (i, sg) in g.signals.iter().take(rows).enumerate() {
                let ry = i as f64 * row_h;
                let sid = format!("signal_{}_{}", g.instance_id, sg.bar_index);
                let bg = if w.hov(&sid) { format!("; background: {}", t.item_bg_hover) } else { String::new() };
                view = view.child(El::new("div.s-hit").id(sid).css(format!("left: 0px; top: {ry}px; width: {}px; height: {row_h}px{bg}", content_w - 8.0)));
                let (ic, c) = match sg.direction {
                    1 => (Icon::ArrowUp, "#26a69a"),
                    -1 => (Icon::ArrowDown, "#ef5350"),
                    _ => (Icon::Circle, t.item_text_muted.as_str()),
                };
                // Row content in the panel's x, less the viewport's 4 px.
                let cy = ry + row_h / 2.0;
                view = view.child(icon_at(ic.svg(), pad + 4.0 - 4.0, cy - 5.5, 11.0, c));
                view = view.child(mid_text(&sg.signal_type, pad + 20.0 - 4.0, cy, At::L, "11px sans-serif", &t.item_text, measure));
                view = view.child(mid_text(&format!("#{}", sg.bar_index), content_w - 4.0 - 4.0, cy, At::R, "11px sans-serif", &t.item_text_muted, measure));
                view = view.child(mid_text(&format!("{:.2}", sg.price), content_w - 50.0 - 4.0, cy, At::R, "11px sans-serif", &t.item_text, measure));
            }
            out.push(view);
            y += vh + 4.0;
        }
        out.push(rect(pad, y, content_w - pad * 2.0, 1.0, &hexa(&t.separator, "40")));
        y += 8.0;
    }
    out
}

/// The app's 1 px stroke along integer `y` from `l` to `r`: half cover on the rows
/// either side.
pub fn hline(l: f64, r: f64, y: f64, color: &str) -> El {
    El::new("div.s-fill").css(format!("left: {l}px; top: {}px; width: {}px; height: 2px; opacity: 0.5; background: {color}", y - 1.0, r - l))
}

/// The app's 1 px outline `move_to(x, y)` → right → down → left → back up (not
/// closed, butt caps, miter joins) on integer corners, as disjoint fills of the
/// coverage it leaves: the sides straddle two pixel rows or columns at half cover,
/// the inner corner pixels are covered by two halves (¾), the three mitred corners
/// add a quarter pixel outside and the open corner (`x, y`) none.
pub fn open_outline(x: f64, y: f64, w: f64, h: f64, color: &str) -> Vec<El> {
    outline(x, y, w, h, color, false)
}

/// [`open_outline`] as the watchlist menu's 10 px checkboxes come out: no cover
/// outside the far corners, and the open corner partly covered (measured off the
/// app's pixels: ⅜ outside, 0.59 / 0.6 beside it).
pub fn checkbox_outline(x: f64, y: f64, w: f64, h: f64, color: &str) -> Vec<El> {
    outline(x, y, w, h, color, true)
}

fn outline(x: f64, y: f64, w: f64, h: f64, color: &str, small: bool) -> Vec<El> {
    let f = |l: f64, t: f64, w: f64, h: f64, a: f64| El::new("div.s-fill").css(format!("left: {l}px; top: {t}px; width: {w}px; height: {h}px; background: {}", alpha(color, a)));
    let (x1, y1) = (x + w, y + h);
    let q = if small { 0.0 } else { 0.25 };
    let mut out = vec![f(x, y - 1.0, w, 1.0, 0.5), f(x1, y - 1.0, 1.0, 1.0, q)];
    if small {
        out.extend([f(x - 1.0, y - 1.0, 1.0, 1.0, 0.3625), f(x, y - 1.0, 1.0, 1.0, 0.1875)]);
    }
    for row in [y, y1 - 1.0] {
        out.extend([
            f(x - 1.0, row, 1.0, 1.0, if small && row == y { 0.6 } else { 0.5 }),
            f(x, row, 1.0, 1.0, 0.75),
            f(x + 1.0, row, w - 2.0, 1.0, 0.5),
            f(x1 - 1.0, row, 1.0, 1.0, 0.75),
            f(x1, row, 1.0, 1.0, 0.5),
        ]);
    }
    out.extend([f(x - 1.0, y + 1.0, 2.0, h - 2.0, 0.5), f(x1 - 1.0, y + 1.0, 2.0, h - 2.0, 0.5)]);
    out.extend([f(x - 1.0, y1, 1.0, 1.0, q), f(x, y1, w, 1.0, 0.5), f(x1, y1, 1.0, 1.0, q)]);
    out
}

/// `render_monitoring_panel` with its Performance tab (`render_performance_panel`);
/// the Connectors tab stays the app's.
fn monitoring(w: &Sidebar<'_>, content_w: f64, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<El> {
    let t = w.theme;
    let perf = &w.state.performance_data;
    let mut out = Vec::new();
    // Tab bar.
    let (bar_h, half) = (32.0, content_w / 2.0);
    out.push(rect(0.0, 0.0, content_w, bar_h, &hexa(&t.separator, "18")));
    out.push(rect(0.0, bar_h - 1.0, content_w, 1.0, &hexa(&t.separator, "40")));
    for (tx, label, active, id) in [(0.0, "Performance", true, "monitoring_section:performance"), (half, "Connectors", false, "monitoring_section:connectors")] {
        let bg = if !active && w.hov(id) { format!("; background: {}", t.item_bg_hover) } else { String::new() };
        out.push(El::new("div.s-hit").id(id).css(format!("left: {tx}px; top: 0px; width: {half}px; height: {}px{bg}", if bg.is_empty() { bar_h } else { bar_h - 1.0 })));
        if active {
            out.push(rect(tx, bar_h - 2.0, half, 2.0, &t.accent));
        }
        out.push(mid_text(label, tx + half / 2.0, bar_h / 2.0, At::C, "12px sans-serif", &t.item_text_muted, measure));
    }
    // Performance.
    let (pad, row_h, gap) = (12.0, 22.0, 16.0);
    let mut y = bar_h + pad;
    let (lx, vx) = (pad, content_w - pad);
    let bar_max_w = content_w - pad * 2.0;
    let text = t.item_text.as_str();
    let accent = "#4a9eff";
    let section = |out: &mut Vec<El>, y: &mut f64, title: &str, measure: &mut dyn FnMut(&str, &str) -> f64| {
        out.push(mid_text(title, lx, *y + row_h / 2.0, At::L, "11px sans-serif", &t.item_text_muted, measure));
        *y += row_h;
        out.push(hline(pad, content_w - pad, *y, &t.separator));
        *y += 6.0;
    };
    let row = |out: &mut Vec<El>, y: &mut f64, label: &str, value: &str, color: &str, measure: &mut dyn FnMut(&str, &str) -> f64| {
        out.push(mid_text(label, lx, *y + row_h / 2.0, At::L, "12px sans-serif", &t.item_text_muted, measure));
        out.push(mid_text(value, vx, *y + row_h / 2.0, At::R, "12px sans-serif", color, measure));
        *y += row_h;
    };
    let control = |out: &mut Vec<El>, y: &mut f64, label: &str, value: &str, id: &str, color: &str, measure: &mut dyn FnMut(&str, &str) -> f64| {
        let bg = if w.hov(id) { format!("; background: {}", t.item_bg_hover) } else { String::new() };
        out.push(El::new("div.s-hit").id(id.to_string()).css(format!("left: 0px; top: {}px; width: {content_w}px; height: {row_h}px{bg}", *y)));
        out.push(mid_text(label, lx, *y + row_h / 2.0, At::L, "12px sans-serif", &t.item_text_muted, measure));
        out.push(mid_text(value, vx, *y + row_h / 2.0, At::R, "12px sans-serif", color, measure));
        *y += row_h;
    };
    let tri = |v: f64, a: f64, b: f64| if v < a { "#4ade80" } else if v < b { "#fbbf24" } else { "#f87171" };
    section(&mut out, &mut y, "RENDERER", measure);
    control(&mut out, &mut y, "Backend", perf.render_backend.label(), "perf:backend", accent, measure);
    y += gap;
    section(&mut out, &mut y, "FRAME TIMING", measure);
    let fps_c = if perf.fps >= 55.0 { "#4ade80" } else if perf.fps >= 30.0 { "#fbbf24" } else { "#f87171" };
    row(&mut out, &mut y, "FPS", &format!("{:.0}", perf.fps), fps_c, measure);
    row(&mut out, &mut y, "Frame Time", &format!("{:.1} ms", perf.frame_time_ms), text, measure);
    row(&mut out, &mut y, "Scene Build", &format!("{}μs", perf.scene_build_us), text, measure);
    row(&mut out, &mut y, "GPU Render", &format!("{}μs", perf.gpu_render_us), text, measure);
    row(&mut out, &mut y, "GPU Present", &format!("{}μs", perf.gpu_present_us), text, measure);
    row(&mut out, &mut y, "Total", &format!("{}μs", perf.scene_build_us + perf.gpu_render_us + perf.gpu_present_us), accent, measure);
    y += gap;
    section(&mut out, &mut y, "SYSTEM", measure);
    row(&mut out, &mut y, "System CPU", &format!("{:.1}%", perf.cpu_usage), tri(perf.cpu_usage as f64, 30.0, 70.0), measure);
    row(
        &mut out,
        &mut y,
        "App CPU",
        &format!("{:.1}% ({:.0}% raw)", perf.process_cpu_normalized, perf.process_cpu),
        tri(perf.process_cpu_normalized as f64, 15.0, 50.0),
        measure,
    );
    if !perf.per_core_cpu.is_empty() {
        let n = perf.per_core_cpu.len().min(16);
        let (bh, bg) = (5.0, 2.0);
        let cell = bar_max_w / 4.0;
        y += 2.0;
        for r in 0..n.div_ceil(4) {
            for c in 0..4 {
                let i = r * 4 + c;
                if i >= n {
                    break;
                }
                let u = (perf.per_core_cpu[i] as f64).clamp(0.0, 100.0);
                let bx = lx + c as f64 * cell;
                out.push(rect(bx, y, cell - bg, bh, &t.separator));
                let fw = (cell - bg) * u / 100.0;
                if fw > 0.0 {
                    out.push(rect(bx, y, fw, bh, tri(u, 30.0, 70.0)));
                }
            }
            y += bh + bg;
        }
        y += 4.0;
    }
    let ram = if perf.ram_total_mb > 0.0 { format!("{:.0} / {:.0} MB", perf.ram_mb, perf.ram_total_mb) } else { format!("{:.0} MB", perf.ram_mb) };
    row(&mut out, &mut y, "RAM", &ram, text, measure);
    if !perf.gpu_name.is_empty() {
        let name = if perf.gpu_name.len() > 28 { &perf.gpu_name[..28] } else { &perf.gpu_name };
        row(&mut out, &mut y, "GPU", name, text, measure);
        if !perf.gpu_driver.is_empty() {
            out.push(mid_text(&perf.gpu_driver, lx, y + row_h / 2.0, At::L, "10px sans-serif", &t.item_text_muted, measure));
            y += row_h;
        }
        if perf.gpu_mem_mb > 0.0 {
            row(&mut out, &mut y, "GPU VRAM", &format!("{:.0} MB", perf.gpu_mem_mb), text, measure);
        }
    }
    row(&mut out, &mut y, "Windows", &perf.window_count.to_string(), text, measure);
    row(&mut out, &mut y, "Total Bars", &perf.total_bars.to_string(), text, measure);
    row(&mut out, &mut y, "WS Connections", &perf.ws_connections.to_string(), text, measure);
    row(&mut out, &mut y, "Connectors", &perf.active_connectors.to_string(), text, measure);
    if perf.lag_events > 0 {
        row(&mut out, &mut y, "Lag Events", &perf.lag_events.to_string(), "#f87171", measure);
    }
    y += gap;
    section(&mut out, &mut y, "PROFILING", measure);
    row(&mut out, &mut y, "Tick", &format!("{}μs", perf.tick_us), text, measure);
    row(&mut out, &mut y, "  Events", &format!("{}μs", perf.event_process_us), text, measure);
    row(&mut out, &mut y, "  Auto Scale", &format!("{}μs", perf.auto_scale_us), text, measure);
    let ind_c = if perf.indicator_recalc_count == 0 {
        text
    } else if perf.indicator_full_count == 0 {
        "#4ade80"
    } else if perf.indicator_incremental_count > 0 {
        "#fbbf24"
    } else {
        "#f87171"
    };
    row(&mut out, &mut y, "  Indicators", &format!("{}μs", perf.indicator_recalc_us), ind_c, measure);
    row(&mut out, &mut y, "    Instances", &perf.indicator_recalc_count.to_string(), text, measure);
    row(&mut out, &mut y, "    Incremental", &perf.indicator_incremental_count.to_string(), "#4ade80", measure);
    row(&mut out, &mut y, "    Full Recalc", &perf.indicator_full_count.to_string(), "#f87171", measure);
    row(&mut out, &mut y, "  OB Panels", &format!("{}μs ({}ev)", perf.orderbook_panel_us, perf.ob_event_count), text, measure);
    row(&mut out, &mut y, "    DOM", &format!("{}μs", perf.dom_panel_us), text, measure);
    row(&mut out, &mut y, "    L2", &format!("{}μs", perf.l2_panel_us), text, measure);
    row(&mut out, &mut y, "    Heatmap", &format!("{}μs", perf.heatmap_panel_us), text, measure);
    row(&mut out, &mut y, "  Trade Panels", &format!("{}μs ({}ev)", perf.trade_panel_us, perf.trade_event_count), text, measure);
    row(&mut out, &mut y, "  Bar Apply", &format!("{}μs", perf.bar_apply_us), text, measure);
    row(&mut out, &mut y, "  Composite", &format!("{}μs", perf.composite_us), text, measure);
    let atw = perf.about_to_wait_us as f64 / 1000.0;
    let atw_c = if atw > 10.0 { "#f87171" } else if atw > 5.0 { "#fbbf24" } else { text };
    row(&mut out, &mut y, "AboutToWait", &format!("{atw:.1}ms"), atw_c, measure);
    y += gap;
    section(&mut out, &mut y, "SETTINGS", measure);
    let fps_l = if perf.fps_limit == 0 { "Unlimited".to_string() } else { perf.fps_limit.to_string() };
    control(&mut out, &mut y, "FPS Limit", &fps_l, "perf:fps_limit", accent, measure);
    let msaa = if perf.msaa_samples == 0 { "Off".to_string() } else { format!("{}x", perf.msaa_samples) };
    control(&mut out, &mut y, "MSAA", &msaa, "perf:msaa", accent, measure);
    control(&mut out, &mut y, "Recalc Mode", &perf.recalc_mode, "perf:recalc_mode", accent, measure);
    let (lc, lt) = if perf.perf_log_enabled { ("#4ade80", "ON") } else { (t.item_text_muted.as_str(), "OFF") };
    control(&mut out, &mut y, "Frame Log", lt, "perf:log_toggle", lc, measure);
    out
}

/// `render_screener_header_rows`: the filter chips and the sortable column header,
/// pinned under the panel header (sidebar coordinates).
fn screener_head(w: &Sidebar<'_>, content_w: f64, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<El> {
    use mlc_app_sidebar_content::render::*;
    let t = w.theme;
    let s = w.state;
    let f = &s.screener_filters;
    let visible = screener_visible_indices(&s.screener_items, f, s.screener_sort_key, s.screener_sort_asc);
    let y0 = super::sidebar::HEADER_H;
    let pad = 8.0;
    let mut out = vec![rect(0.0, y0, content_w, SCREENER_FILTER_ROW_H, &t.background)];
    let (chip_h, gap) = (20.0, 6.0);
    let chip_y = y0 + (SCREENER_FILTER_ROW_H - chip_h) / 2.0;
    let mut x = pad;
    let font = "10px sans-serif";
    let chips = [
        ("screener_filter_change_pct", screener_chip_label_change_pct(f.change_pct), f.change_pct.is_some()),
        ("screener_filter_rsi_above", screener_chip_label_rsi_above(f.rsi_above), f.rsi_above.is_some()),
        ("screener_filter_rsi_below", screener_chip_label_rsi_below(f.rsi_below), f.rsi_below.is_some()),
        ("screener_filter_vol_min", screener_chip_label_vol(f.vol_min), f.vol_min.is_some()),
        ("screener_filter_atr_pct_min", screener_chip_label_atr(f.atr_pct_min), f.atr_pct_min.is_some()),
    ];
    for (id, label, active) in &chips {
        let cw = measure(font, label) + 12.0;
        let hov = w.hov(id);
        let bg = if *active {
            Some(if hov { t.accent.as_str() } else { t.item_bg_active.as_str() })
        } else if hov {
            Some(t.item_bg_hover.as_str())
        } else {
            None
        };
        let bg = bg.map(|c| format!("; border-radius: 4px; background: {c}")).unwrap_or_default();
        out.push(El::new("div.s-hit").id(id.to_string()).css(format!("left: {x}px; top: {chip_y}px; width: {cw}px; height: {chip_h}px{bg}")));
        let c = if *active { &t.item_text } else { &t.item_text_muted };
        out.push(mid_text(label, x + cw / 2.0, chip_y + chip_h / 2.0, At::C, font, c, measure));
        x += cw + gap;
    }
    let m = format!("{} of {} match", visible.len(), s.screener_items.len());
    out.push(mid_text(&m, content_w - pad, chip_y + chip_h / 2.0, At::R, font, &t.item_text_muted, measure));
    out.push(rect(0.0, y0 + SCREENER_FILTER_ROW_H - 1.0, content_w, 1.0, &t.separator));
    let hy = y0 + SCREENER_FILTER_ROW_H;
    out.push(rect(0.0, hy, content_w, SCREENER_COL_HEADER_H, &t.background));
    let n = SCREENER_COL_LABELS.len();
    let col_w = (content_w - pad * 2.0) / n as f64;
    for (i, label) in SCREENER_COL_LABELS.iter().enumerate() {
        let cx = pad + col_w * i as f64;
        let active = s.screener_sort_key == mlc_app_sidebar_content::types::screener_sort_key_for_col(i);
        let arrow = if active { if s.screener_sort_asc { " \u{25B2}" } else { " \u{25BC}" } } else { "" };
        let text = super::sidebar::truncate(measure, font, &format!("{label}{arrow}"), col_w - 2.0);
        let c = if active { &t.item_text } else { &t.item_text_muted };
        let (at, tx) = if i == 0 { (At::L, cx + 2.0) } else { (At::C, cx + col_w / 2.0) };
        out.push(mid_text(&text, tx, hy + SCREENER_COL_HEADER_H / 2.0, at, font, c, measure));
        out.push(hit(format!("screener_sort:{i}"), cx, hy, col_w, SCREENER_COL_HEADER_H));
    }
    out.push(rect(0.0, hy + SCREENER_COL_HEADER_H - 1.0, content_w, 1.0, &t.separator));
    out
}

/// `render_screener_rows`: the filtered, sorted rows.
fn screener(w: &Sidebar<'_>, content_w: f64, measure: &mut dyn FnMut(&str, &str) -> f64) -> Vec<El> {
    use mlc_app_sidebar_content::render::*;
    let t = w.theme;
    let s = w.state;
    if s.screener_items.is_empty() {
        return empty(&["No symbols in watchlist", "Add symbols to the watchlist, then Scan"], content_w, 20.0, measure, &t.item_text_muted);
    }
    let visible = screener_visible_indices(&s.screener_items, &s.screener_filters, s.screener_sort_key, s.screener_sort_asc);
    if visible.is_empty() {
        return empty(&["No symbols match the active filters"], content_w, 20.0, measure, &t.item_text_muted);
    }
    let (pad, row_h) = (8.0, 32.0);
    let col_w = (content_w - pad * 2.0) / SCREENER_COL_LABELS.len() as f64;
    let center = |i: usize| pad + col_w * i as f64 + col_w / 2.0;
    let mut out = Vec::new();
    let mut y = 0.0;
    for &i in &visible {
        let it = &s.screener_items[i];
        let id = format!("screener_row:{}:{}:{}", it.symbol, it.exchange, it.account_type);
        let bg = if w.hov(&id) { format!("; background: {}", t.item_bg_hover) } else { String::new() };
        out.push(El::new("div.s-hit").id(id).css(format!("left: 0px; top: {y}px; width: {content_w}px; height: {row_h}px{bg}")));
        let sym = super::sidebar::truncate(measure, "11px sans-serif", &it.symbol, col_w - 4.0);
        out.push(top_text(&sym, pad, y + 3.0, At::L, "11px sans-serif", &t.item_text, measure));
        let tag = super::sidebar::truncate(measure, "9px sans-serif", &format!("{}:{}", mlc_core::venue_label(&it.exchange), it.account_type), col_w - 4.0);
        out.push(top_text(&tag, pad, y + 17.0, At::L, "9px sans-serif", &t.item_text_muted, measure));
        let my = y + row_h / 2.0;
        let font = "10px sans-serif";
        let mut cell = |out: &mut Vec<El>, c: usize, v: &str, color: &str| out.push(mid_text(v, center(c), my, At::C, font, color, measure));
        cell(&mut out, 1, &format_price_compact(it.last_price), &t.item_text);
        let chg = if it.change_percent > 0.0 {
            &t.success
        } else if it.change_percent < 0.0 {
            &t.danger
        } else {
            &t.item_text_muted
        };
        cell(&mut out, 2, &format!("{:+.1}%", it.change_percent), chg);
        cell(&mut out, 3, &format_volume(it.volume_24h_quote), &t.item_text_muted);
        match it.rsi14 {
            Some(v) => {
                let c = if v >= 70.0 {
                    &t.danger
                } else if v <= 30.0 {
                    &t.success
                } else {
                    &t.item_text
                };
                cell(&mut out, 4, &format!("{v:.0}"), c);
            }
            None => cell(&mut out, 4, "\u{2026}", &t.item_text_muted),
        }
        let opt = |v: Option<f64>, p: usize, sfx: &str| v.map(|v| format!("{v:.p$}{sfx}")).unwrap_or_else(|| "\u{2026}".into());
        cell(&mut out, 5, &opt(it.atr_pct14, 2, "%"), &t.item_text_muted);
        cell(&mut out, 6, &opt(it.dist_high_pct, 1, "%"), &t.item_text_muted);
        cell(&mut out, 7, &opt(it.dist_low_pct, 1, "%"), &t.item_text_muted);
        out.push(rect(0.0, y + row_h - 1.0, content_w, 1.0, &t.separator));
        y += row_h;
    }
    out
}
