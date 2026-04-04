//! Overlay tab headers for split panel leaves.
//!
//! Provides a lightweight tab header drawn at the top-left of each leaf in a
//! split chart panel.  The header shows `SYMBOL · TF`, an optional color tag
//! square, and (on hover) a gear/settings menu indicator.
//!
//! Visual style mirrors `render_panel_header()` from
//! `zengeld-terminal-core::ui::render::panel_render` but is simplified — no
//! exchange name, no connection status dot.

use crate::render::{RenderContext, TextAlign, TextBaseline, draw_svg_icon};
use crate::ui::icons::ICON_SETTINGS;
use crate::ui::toolbar_render::ToolbarTheme;

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Convert an RGBA `[0.0, 1.0]` array to a CSS hex string `"#rrggbbaa"`.
fn rgba_hex(rgba: [f32; 4]) -> String {
    let r = (rgba[0].clamp(0.0, 1.0) * 255.0) as u8;
    let g = (rgba[1].clamp(0.0, 1.0) * 255.0) as u8;
    let b = (rgba[2].clamp(0.0, 1.0) * 255.0) as u8;
    let a = (rgba[3].clamp(0.0, 1.0) * 255.0) as u8;
    format!("#{:02x}{:02x}{:02x}{:02x}", r, g, b, a)
}

/// Parse a CSS hex color (`#RRGGBB` or `#RRGGBBAA`) and return an `rgba()`
/// string with the given `alpha` override (0.0 – 1.0).
///
/// If parsing fails the input string is returned unchanged.
fn hex_with_alpha(hex: &str, alpha: f64) -> String {
    let h = hex.trim_start_matches('#');
    if h.len() < 6 {
        return hex.to_string();
    }
    let r = u8::from_str_radix(&h[0..2], 16).unwrap_or(0);
    let g = u8::from_str_radix(&h[2..4], 16).unwrap_or(0);
    let b = u8::from_str_radix(&h[4..6], 16).unwrap_or(0);
    format!("rgba({},{},{},{:.2})", r, g, b, alpha)
}

// ---------------------------------------------------------------------------
// Public types
// ---------------------------------------------------------------------------

/// Which interactive zone within an overlay tab header is hovered.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum LeafTabHoverZone {
    /// No zone hovered.
    #[default]
    None,
    /// The text / body of the tab.
    Body,
    /// The small color-tag square.
    ColorTag,
    /// The gear/settings menu icon.
    GearMenu,
}

/// Hit zones returned after rendering a leaf tab header.
///
/// All rects are `[x, y, w, h]` in absolute screen coordinates.
#[derive(Clone, Debug, Default)]
pub struct LeafTabHitZones {
    /// Full tab rect `[x, y, w, h]`.
    pub tab_rect: [f64; 4],
    /// Color tag square rect `[x, y, w, h]`.
    pub color_tag_rect: [f64; 4],
    /// Gear-menu rect `[x, y, w, h]`.
    pub dots_rect: [f64; 4],
}

// ---------------------------------------------------------------------------
// Render function
// ---------------------------------------------------------------------------

/// Height of a leaf overlay tab (used by other overlays for vertical positioning).
pub const LEAF_TAB_HEIGHT: f64 = 24.0;

/// Render an overlay tab header at the top of a split leaf.
///
/// Draws a compact `SYMBOL · TF · EXCHANGE · ACCT` label with an optional
/// color-tag square and (when hovered) a three-dot menu indicator.  The active
/// leaf receives a slightly brighter background and a 2 px left accent bar.
///
/// # Arguments
/// * `ctx`           — Mutable render context.
/// * `x`, `y`        — Top-left corner of the tab in screen coordinates.
/// * `max_width`     — Maximum width available for the tab.
/// * `symbol`        — Symbol string, e.g. `"BTCUSDT"`.
/// * `timeframe`     — Timeframe label, e.g. `"1H"`.
/// * `exchange`      — Exchange name, e.g. `"Binance"`.  May be empty.
/// * `account_type`  — Account type short label, e.g. `"S"` for Spot,
///                     `"FC"` for Futures Cross.  Always shown.
/// * `is_active`     — Whether this leaf is the active (focused) one.
/// * `hovered_zone`  — Which interactive zone is currently hovered.
/// * `color_tag`     — Optional RGBA color for the tag square.
/// * `toolbar_theme` — Theme providing semantic colors for tabs.
///
/// # Returns
/// [`LeafTabHitZones`] describing the clickable regions within the tab.
pub fn render_leaf_tab(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    max_width: f64,
    symbol: &str,
    timeframe: &str,
    exchange: &str,
    account_type: &str,
    is_active: bool,
    hovered_zone: LeafTabHoverZone,
    color_tag: Option<[f32; 4]>,
    toolbar_theme: &ToolbarTheme,
) -> LeafTabHitZones {
    // ── Constants ────────────────────────────────────────────────────────────
    const TAB_HEIGHT: f64 = LEAF_TAB_HEIGHT;
    const LEFT_PAD: f64 = 6.0;
    const RIGHT_PAD: f64 = 4.0;
    const TAG_GAP: f64 = 4.0;
    /// Color tag square — same as font size for visual alignment.
    const TAG_SIZE: f64 = 12.0;
    const TAG_CONTAINER_W: f64 = TAG_SIZE;
    const DOTS_CONTAINER_W_EXPANDED: f64 = 20.0;
    const ACCENT_BAR_W: f64 = 2.0;

    // Collapse the dots container when the tab is not hovered — saves visible space.
    let dots_w = if hovered_zone != LeafTabHoverZone::None {
        DOTS_CONTAINER_W_EXPANDED
    } else {
        0.0
    };

    let center_y = (y + TAB_HEIGHT / 2.0).round();

    // ── Text ─────────────────────────────────────────────────────────────────
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);

    let display_text = if exchange.is_empty() {
        format!("{} · {} · {}", symbol, timeframe, account_type)
    } else {
        format!("{} · {} · {} · {}", symbol, timeframe, exchange, account_type)
    };

    // Available width for the text container.
    let max_text_w = (max_width - LEFT_PAD - TAG_GAP - TAG_CONTAINER_W - dots_w - RIGHT_PAD).max(0.0);

    let measured_w = ctx.measure_text(&display_text);
    let text_container_w = measured_w.min(max_text_w);

    // Total tab width.
    let tab_width = (LEFT_PAD + text_container_w + TAG_GAP + TAG_CONTAINER_W + dots_w + RIGHT_PAD).min(max_width);

    // Early-out when there is not enough room for any text.
    if text_container_w < 24.0 {
        return LeafTabHitZones::default();
    }

    // Derived x positions.
    let text_x = (x + LEFT_PAD).round();
    let tag_container_x = text_x + text_container_w + TAG_GAP;
    let dots_container_x = tag_container_x + TAG_CONTAINER_W;

    // ── Background ───────────────────────────────────────────────────────────
    // Active leaf: slightly brighter — use toolbar background at 0.82 opacity.
    // Inactive leaf: same base color at 0.70 opacity (darker appearance).
    let bg_color = if is_active {
        hex_with_alpha(&toolbar_theme.background, 0.82)
    } else {
        hex_with_alpha(&toolbar_theme.background, 0.70)
    };
    ctx.set_fill_color(&bg_color);
    ctx.fill_rounded_rect(x, y, tab_width, TAB_HEIGHT, 2.0);

    // Active leaf: 2 px left accent bar.
    if is_active {
        let accent = hex_with_alpha(&toolbar_theme.accent, 0.92);
        ctx.set_fill_color(&accent);
        ctx.fill_rect(x, y, ACCENT_BAR_W, TAB_HEIGHT);
    }

    // ── Hover highlight ───────────────────────────────────────────────────────
    // White semi-transparent overlays are universal — they work on any theme.
    match hovered_zone {
        LeafTabHoverZone::Body => {
            ctx.draw_hover_rect(text_x, y, tab_width - LEFT_PAD, TAB_HEIGHT, "rgba(255,255,255,0.06)");
        }
        LeafTabHoverZone::ColorTag => {
            ctx.draw_hover_rect(tag_container_x, y, TAG_CONTAINER_W, TAB_HEIGHT, "rgba(255,255,255,0.08)");
        }
        LeafTabHoverZone::GearMenu => {
            ctx.draw_hover_rect(dots_container_x, y, dots_w, TAB_HEIGHT, "rgba(255,255,255,0.08)");
        }
        LeafTabHoverZone::None => {}
    }

    // ── Text rendering ────────────────────────────────────────────────────────
    // Truncate text if it overflows the container.
    let final_text = if measured_w > text_container_w {
        let mut t = display_text.clone();
        // Binary-like truncation: pop chars until it fits (leave room for ellipsis).
        while ctx.measure_text(&t) > text_container_w - 10.0 && t.len() > 1 {
            t.pop();
        }
        format!("{}…", t)
    } else {
        display_text
    };

    // Main text color: brighter for active, dimmer for inactive.
    let text_color = if is_active {
        &toolbar_theme.item_text_active
    } else {
        &toolbar_theme.item_text_muted
    };
    ctx.set_fill_color(text_color);
    ctx.fill_text(&final_text, text_x, center_y);

    // ── Color tag square ──────────────────────────────────────────────────────
    let tag_x = tag_container_x;
    let tag_y = center_y - TAG_SIZE / 2.0;
    let tag_hex = match color_tag {
        Some(c) => rgba_hex(c),
        None => hex_with_alpha(&toolbar_theme.item_text_muted, 0.50),
    };
    ctx.set_fill_color(&tag_hex);
    ctx.fill_rounded_rect(tag_x, tag_y, TAG_SIZE, TAG_SIZE, 2.0);

    // ── Gear menu (shown when any zone is hovered) ────────────────────────────
    if hovered_zone != LeafTabHoverZone::None {
        let gear_size = 14.0_f64;
        let gear_x = dots_container_x + (dots_w - gear_size) / 2.0;
        let gear_y = center_y - gear_size / 2.0;
        let gear_color = hex_with_alpha(&toolbar_theme.item_text, 0.90);
        draw_svg_icon(ctx, ICON_SETTINGS, gear_x, gear_y, gear_size, gear_size, &gear_color);
    }

    // ── Return hit zones ──────────────────────────────────────────────────────
    LeafTabHitZones {
        tab_rect: [x, y, tab_width, TAB_HEIGHT],
        color_tag_rect: [tag_container_x, y, TAG_CONTAINER_W, TAB_HEIGHT],
        dots_rect: [dots_container_x, y, dots_w, TAB_HEIGHT],
    }
}
