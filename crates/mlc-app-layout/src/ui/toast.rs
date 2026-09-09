//! Shared toast layout/paint (toast restyle-and-anchor follow-up to §8,
//! owner ask 2026-09-09: "restyle to the MODAL look... reuse the modal
//! frame drawing helper or its constants"). One function both shells call —
//! native and web painters were two independent implementations before
//! this (native's own `render_toasts` in `mlc-app-shell-vello/src/main.rs`,
//! the web shell's own `render_system_toasts` in
//! `mlc-app-shell-web/src/lib.rs`) and neither reused the modal frame.
//!
//! Reused from the modal widget set: [`render_modal_frame_only`] draws the
//! SAME background/border/shadow/corner-radius every plain settings modal
//! draws (chart settings, the community modal — both call it via
//! `begin_modal_shell` with `radius: 0.0`, `ModalTheme::from_toolbar`), the
//! title uses the SAME `"13px sans-serif"` font `begin_modal_shell` draws
//! its own header title in, and the ✕ is the SAME `Icon::Close` svg drawn
//! the same size with the same hover-tint swap `begin_modal_shell` uses for
//! its own close button. What a toast does NOT draw: a header bar — this
//! is `render_modal_frame_only`'s bare frame with a title line and the ✕
//! floated into its top-right corner, never `begin_modal_shell`'s own
//! header fill/hairline.
//!
//! `ChartApp` (`mlc-app`) owns the runtime queue
//! (`system_events::ToastItem`, `active_toasts()`) and every mutation (the
//! ✕, Escape, drag-to-corner) — this module only turns a snapshot of it
//! into pixels and hit zones. Shells convert their own `ToastItem`s into
//! [`ToastFact`]s (computing `remaining` from their own clock) and call
//! [`render_toasts`]; nothing here knows `mlc_alert_delivery` or
//! `mlc_core::SystemEvent` at all.

use mlc_core::engine::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use mlc_core::ToastAnchor;
use uzor::input::{InputCoordinator, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

use crate::ui::widgets::{fit_text_to_width, render_modal_frame_only, wrap_text_to_width, ModalTheme};
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;

/// One toast ready to paint, normalized out of `mlc_app::system_events::
/// ToastItem` by the shell — see this module's own doc for the split.
#[derive(Clone, Debug)]
pub struct ToastFact {
    /// The originating `SystemEvent`/`ToastItem` id — the ✕'s own hit id
    /// and the drag handle's own hit id both key off this, never a slice
    /// index (an index shifts the moment an earlier toast expires or is
    /// dismissed).
    pub id: u64,
    pub title: String,
    pub body: String,
    /// `1.0` = just shown, `0.0` = about to expire — drives the fade over
    /// the final 20% of a toast's life. `mlc_app::system_events::ToastItem::
    /// remaining_fraction` computes this.
    pub remaining: f64,
}

/// Content-driven width floor/ceiling (owner spec: "280–420 px").
const WIDTH_MIN: f64 = 280.0;
const WIDTH_MAX: f64 = 420.0;
/// Body grows up to this many lines before truncating with "…" (owner
/// spec: "up to ~6 lines then truncates").
const MAX_BODY_LINES: usize = 6;

const PADDING: f64 = 12.0;
/// Gap between stacked toasts, and between the title line and the body.
const GAP: f64 = 8.0;
/// Clearance from the screen edge the stack anchors to.
const MARGIN: f64 = 8.0;
/// Below the chrome strip — the clearance every prior toast painter used.
const TOP_CLEARANCE: f64 = 40.0;

const TITLE_FONT: &str = "13px sans-serif";
const BODY_FONT: &str = "12px sans-serif";
const TITLE_LINE_H: f64 = 18.0;
const BODY_LINE_H: f64 = 16.0;
const CLOSE_SIZE: f64 = 14.0;
/// Space the ✕ (plus its own margins) reserves on the title line — the
/// title text itself is fit to `content_w - CLOSE_GUTTER`, never drawn
/// underneath the button.
const CLOSE_GUTTER: f64 = CLOSE_SIZE + 20.0;

/// What one toast measured out to, before it is known where on screen it
/// lands.
struct Measured {
    title_line: String,
    body_lines: Vec<String>,
    w: f64,
    h: f64,
}

fn measure(ctx: &mut dyn RenderContext, fact: &ToastFact) -> Measured {
    let inner_max = (WIDTH_MAX - PADDING * 2.0).max(1.0);
    let inner_min = (WIDTH_MIN - PADDING * 2.0).max(1.0);

    ctx.set_font(TITLE_FONT);
    let title_natural = ctx.measure_text(&fact.title);
    ctx.set_font(BODY_FONT);
    let body_natural = ctx.measure_text(&fact.body);

    let content_w = (title_natural + CLOSE_GUTTER)
        .max(body_natural)
        .clamp(inner_min, inner_max);

    ctx.set_font(TITLE_FONT);
    let title_line = fit_text_to_width(ctx, &fact.title, (content_w - CLOSE_GUTTER).max(20.0));

    ctx.set_font(BODY_FONT);
    let body_lines: Vec<String> = if fact.body.trim().is_empty() {
        Vec::new()
    } else {
        let all = wrap_text_to_width(ctx, &fact.body, content_w);
        let truncated = all.len() > MAX_BODY_LINES;
        let mut lines: Vec<String> = all.into_iter().take(MAX_BODY_LINES).collect();
        if truncated {
            if let Some(last) = lines.last_mut() {
                let ellipsis_w = ctx.measure_text("\u{2026}");
                let avail = (content_w - ellipsis_w).max(0.0);
                let mut s = last.clone();
                while !s.is_empty() && ctx.measure_text(&s) > avail {
                    s.pop();
                }
                s.push('\u{2026}');
                *last = s;
            }
        }
        lines
    };

    let body_h = if body_lines.is_empty() { 0.0 } else { GAP + body_lines.len() as f64 * BODY_LINE_H };
    let h = PADDING * 2.0 + TITLE_LINE_H + body_h;
    let w = content_w + PADDING * 2.0;
    Measured { title_line, body_lines, w, h }
}

/// Draw every toast in `toasts`, stacked away from `anchor`'s corner
/// (oldest nearest the corner, matching the stack order every prior toast
/// painter used), and register each one's ✕ (`toast:close:{id}`) and body
/// drag handle (`toast:drag:{id}`) on the topmost layer
/// ([`ZLayer::Toast`]). `mlc_app::ChartApp`'s click/drag dispatch owns what
/// those ids DO — this only reports geometry.
pub fn render_toasts(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    toasts: &[ToastFact],
    theme: &ModalTheme,
    anchor: ToastAnchor,
    screen: (f64, f64),
) {
    if toasts.is_empty() {
        return;
    }
    let (screen_w, screen_h) = screen;
    let layer_id = ZLayer::Toast.push(coord);

    let grows_up = matches!(anchor, ToastAnchor::BottomLeft | ToastAnchor::BottomRight);
    let on_left = matches!(anchor, ToastAnchor::TopLeft | ToastAnchor::BottomLeft);

    let mut cursor = if grows_up { screen_h - MARGIN } else { TOP_CLEARANCE + MARGIN };

    for fact in toasts {
        // Already-expired entries should have been pruned by the shell
        // before this call — skip defensively rather than draw an
        // invisible box that still eats stack space.
        if fact.remaining <= 0.0 {
            continue;
        }
        let alpha = if fact.remaining < 0.2 { (fact.remaining / 0.2).clamp(0.0, 1.0) } else { 1.0 };

        let m = measure(ctx, fact);
        let y = if grows_up { cursor - m.h } else { cursor };
        if y < 0.0 || y + m.h > screen_h {
            break; // ran out of vertical room — same "don't render off-screen" rule every prior painter had
        }
        let x = if on_left { MARGIN } else { screen_w - m.w - MARGIN };

        draw_one(ctx, coord, &layer_id, fact, &m, x, y, theme, alpha);

        cursor = if grows_up { y - GAP } else { y + m.h + GAP };
    }

    coord.pop_layer(&layer_id);
}

#[allow(clippy::too_many_arguments)]
fn draw_one(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &uzor::input::LayerId,
    fact: &ToastFact,
    m: &Measured,
    x: f64,
    y: f64,
    theme: &ModalTheme,
    alpha: f64,
) {
    let frame = WidgetRect::new(x, y, m.w, m.h);
    let faded = fade_theme(theme, alpha);

    render_modal_frame_only(ctx, frame, &faded, 0.0);

    // Body drag handle — the whole frame, so grabbing anywhere on the toast
    // (except the ✕, registered after and therefore on top — see
    // `end_modal_shell`'s own doc for why last-registered wins a hit) moves
    // the anchor. `Sense::DRAG` only: a toast body has no click action of
    // its own, so there is no plain-click case to disambiguate against.
    let drag_id = format!("toast:drag:{}", fact.id);
    coord.register_on_layer(drag_id.as_str(), frame, Sense::DRAG, layer_id);

    // Title — one line, never wrapped.
    ctx.set_font(TITLE_FONT);
    ctx.set_fill_color(&fade_color(&theme.header_text, alpha));
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(&m.title_line, x + PADDING, y + PADDING);

    // Body — wrapped, muted.
    if !m.body_lines.is_empty() {
        ctx.set_font(BODY_FONT);
        ctx.set_fill_color(&fade_color(&theme.item_text_muted, alpha));
        let body_top = y + PADDING + TITLE_LINE_H + GAP;
        for (i, line) in m.body_lines.iter().enumerate() {
            ctx.fill_text(line, x + PADDING, body_top + i as f64 * BODY_LINE_H);
        }
    }

    // ✕ — top-right corner, drawn like the modal's own close button
    // (`begin_modal_shell`): same size, same hover-tint swap.
    let close_id = format!("toast:close:{}", fact.id);
    let cx = frame.right() - CLOSE_SIZE - 12.0;
    let cy = y + PADDING + (TITLE_LINE_H - CLOSE_SIZE) / 2.0;
    let hovered = coord.is_hovered(&WidgetId::from(close_id.as_str()));
    let tint = if hovered { &theme.close_button_hover } else { &theme.close_button };
    draw_svg_icon(ctx, Icon::Close.svg(), cx, cy, CLOSE_SIZE, CLOSE_SIZE, &fade_color(tint, alpha));
    let hit = WidgetRect::new(cx - 4.0, cy - 4.0, CLOSE_SIZE + 8.0, CLOSE_SIZE + 8.0);
    coord.register_on_layer(close_id.as_str(), hit, Sense::CLICK | Sense::HOVER, layer_id);
}

/// Scale a theme color's alpha for the fade-out window. `mlc_core::
/// apply_opacity` overwrites rather than multiplies an existing alpha, so
/// this is only reached for `alpha < 1.0` (the toast's final 20%) — for the
/// other 80% of its life every color is used exactly as the modal frame
/// itself would draw it.
fn fade_color(color: &str, alpha: f64) -> String {
    if alpha >= 0.999 {
        color.to_string()
    } else {
        mlc_core::apply_opacity(color, alpha)
    }
}

/// [`ModalTheme`] with every color the toast frame draws faded — see
/// [`fade_color`]'s own doc for the approximation this makes.
fn fade_theme(theme: &ModalTheme, alpha: f64) -> ModalTheme {
    if alpha >= 0.999 {
        return theme.clone();
    }
    ModalTheme {
        background: fade_color(&theme.background, alpha),
        border: fade_color(&theme.border, alpha),
        shadow: fade_color(&theme.shadow, alpha),
        ..theme.clone()
    }
}
