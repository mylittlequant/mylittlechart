//! Trading Container modal (owner spec 2026-07-28).
//!
//! Opened by the gear on the container's embedded header toolbar. Mirrors
//! the Tags & Tabs modal shape: draggable frame, left sidebar of sections,
//! content area. The headline section is **MAP** — a scaled card map of
//! every panel currently hosted inside the container (same idea as the
//! Tags & Tabs minimap, cards not a list): center (DOM/Chart), OrderEntry,
//! RiskCalculator, and the bottom strip as three tab segments with the
//! active one highlighted. **SETUP** holds the container-level knobs
//! (center-slot switcher, layout reset).
//!
//! The renderer is data-driven: `mlc-app-layout` cannot see
//! `TradingContainerState` (it lives in `mlc-app-panels`), so the caller
//! flattens the container geometry into [`TcMapPanel`] cards. Hover is read
//! straight off the `InputCoordinator` (live hit-test) — no
//! `hovered_item_id` plumbing.

use uzor::input::Sense;
use uzor::types::Rect as WidgetRect;

use crate::layout::render_chart::FrameTheme;
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme};
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use mlc_core::engine::render::draw_svg_icon;
use mlc_core::engine::render::RenderContext;
use mlc_core::render::{TextAlign, TextBaseline};

// =============================================================================
// Input data
// =============================================================================

/// One card on the MAP section — a panel currently hosted inside the
/// trading container, with its rect in CONTAINER-LOCAL coordinates.
#[derive(Clone, Debug)]
pub struct TcMapPanel {
    /// Widget-id suffix (`tc_modal:map:{id}` is registered for it).
    pub id: String,
    /// Card headline (e.g. "DOM", "Order Entry", "Positions (2)").
    pub label: String,
    /// Secondary line under the headline (suppressed on small cards).
    pub sub: String,
    /// Container-local rect `(x, y, w, h)`.
    pub rect: (f64, f64, f64, f64),
    /// Accent highlight (active bottom tab).
    pub active: bool,
    /// Whether clicking the card performs an action.
    pub clickable: bool,
}

// =============================================================================
// State
// =============================================================================

/// Which section is active in the modal's left sidebar.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum TcModalSection {
    #[default]
    Map,
    Setup,
}

/// Trading Container modal state (RAM-only).
#[derive(Clone, Debug, Default)]
pub struct TradingContainerModalState {
    pub is_open: bool,
    /// `None` = centered on first paint; preserved across close/reopen.
    pub position: Option<(f64, f64)>,
    pub is_dragging: bool,
    pub drag_offset: Option<(f64, f64)>,
    pub section: TcModalSection,
}

impl TradingContainerModalState {
    pub fn open(&mut self) {
        self.is_open = true;
        self.section = TcModalSection::Map;
    }

    pub fn close(&mut self) {
        self.is_open = false;
        self.is_dragging = false;
        self.drag_offset = None;
    }

    pub fn start_drag(&mut self, mouse_x: f64, mouse_y: f64, modal_x: f64, modal_y: f64) {
        self.is_dragging = true;
        self.drag_offset = Some((mouse_x - modal_x, mouse_y - modal_y));
    }

    pub fn update_drag(&mut self, mouse_x: f64, mouse_y: f64) {
        if let Some((ox, oy)) = self.drag_offset {
            self.position = Some((mouse_x - ox, mouse_y - oy));
        }
    }

    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_offset = None;
    }
}

// =============================================================================
// Result
// =============================================================================

/// Hit-test rectangles returned by [`render_trading_container_modal`].
#[derive(Clone, Debug, Default)]
pub struct TcModalResult {
    pub modal_rect: WidgetRect,
    /// Title bar (drag initiation — `tc_modal:header`).
    pub header_rect: WidgetRect,
}

// =============================================================================
// Layout constants
// =============================================================================

const MODAL_WIDTH: f64 = 480.0;
const MODAL_HEIGHT: f64 = 400.0;
const HEADER_HEIGHT: f64 = 36.0;
const SIDEBAR_WIDTH: f64 = 80.0;
const MODAL_PADDING: f64 = 12.0;
const CLOSE_BTN_SIZE: f64 = 20.0;

// =============================================================================
// Renderer
// =============================================================================

/// Render the Trading Container modal.
///
/// `panels` — the MAP cards (container-local rects); `container_size` — the
/// container's last rendered `(w, h)` (aspect + scale source);
/// `center_label` — current center-slot occupant ("DOM"/"Chart", drives the
/// SETUP switcher's active state); `hist_rows` — per-table history-depth
/// rows `(display label, wire key, active option index)` — the option set
/// itself is fixed (`HIST_OPTIONS`), the app layer maps indices back to its
/// own depth type.
#[allow(clippy::too_many_arguments)]
pub fn render_trading_container_modal(
    ctx: &mut dyn RenderContext,
    state: &TradingContainerModalState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    panels: &[TcMapPanel],
    container_size: (f64, f64),
    center_label: &str,
    hist_rows: &[(String, String, usize)],
    screen_w: f64,
    screen_h: f64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> TcModalResult {
    let mut result = TcModalResult::default();

    let (modal_x, modal_y) = state.position.unwrap_or((
        (screen_w - MODAL_WIDTH) / 2.0,
        (screen_h - MODAL_HEIGHT) / 2.0,
    ));
    let modal_x = modal_x.max(0.0).min((screen_w - MODAL_WIDTH).max(0.0));
    let modal_y = modal_y.max(0.0).min((screen_h - MODAL_HEIGHT).max(0.0));

    result.modal_rect = WidgetRect::new(modal_x, modal_y, MODAL_WIDTH, MODAL_HEIGHT);
    result.header_rect = WidgetRect::new(modal_x, modal_y, MODAL_WIDTH, HEADER_HEIGHT);

    let layer_id = ZLayer::Modal.push_named(input_coordinator, "tc_modal");

    input_coordinator.register_on_layer(
        "tc_modal:header",
        result.header_rect,
        Sense::DRAG,
        &layer_id,
    );
    input_coordinator.register_on_layer(
        "tc_modal:modal_bg",
        result.modal_rect,
        Sense::CLICK,
        &layer_id,
    );

    // ── Frame + header ───────────────────────────────────────────────────
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &frame_theme.toolbar_border,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_active,
        &frame_theme.toolbar_border,
    );
    render_modal_frame_only(ctx, result.modal_rect, &modal_theme, 0.0);

    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rect(modal_x, modal_y, MODAL_WIDTH, HEADER_HEIGHT);

    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Trading Container", modal_x + 16.0, modal_y + HEADER_HEIGHT / 2.0);

    let close_x = modal_x + MODAL_WIDTH - CLOSE_BTN_SIZE - 12.0;
    let close_y = modal_y + (HEADER_HEIGHT - CLOSE_BTN_SIZE) / 2.0;
    draw_svg_icon(
        ctx,
        Icon::Close.svg(),
        close_x,
        close_y,
        CLOSE_BTN_SIZE,
        CLOSE_BTN_SIZE,
        &toolbar_theme.item_text,
    );
    input_coordinator.register_on_layer(
        "tc_modal:close",
        WidgetRect::new(close_x, close_y, CLOSE_BTN_SIZE, CLOSE_BTN_SIZE),
        Sense::CLICK,
        &layer_id,
    );

    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + HEADER_HEIGHT);
    ctx.line_to(modal_x + MODAL_WIDTH, modal_y + HEADER_HEIGHT);
    ctx.stroke();

    // ── Left sidebar ─────────────────────────────────────────────────────
    let sidebar_top = modal_y + HEADER_HEIGHT;
    let sidebar_height = MODAL_HEIGHT - HEADER_HEIGHT;
    let sidebar_items = [
        ("map", "MAP", TcModalSection::Map),
        ("setup", "SETUP", TcModalSection::Setup),
    ];
    let item_h = 40.0;
    let mut sb_y = sidebar_top + (sidebar_height - item_h * sidebar_items.len() as f64) / 2.0;
    for (id, label, section) in &sidebar_items {
        let wid = format!("tc_modal:sidebar:{}", id);
        let is_active = state.section == *section;
        let is_hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
        if is_active {
            ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.accent, 0.20));
            ctx.fill_rounded_rect(modal_x + 4.0, sb_y + 2.0, SIDEBAR_WIDTH - 8.0, item_h - 4.0, 4.0);
        } else if is_hovered {
            ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, 0.08));
            ctx.fill_rounded_rect(modal_x + 4.0, sb_y + 2.0, SIDEBAR_WIDTH - 8.0, item_h - 4.0, 4.0);
        }
        ctx.set_font("bold 11px sans-serif");
        ctx.set_fill_color(if is_active { toolbar_theme.accent.as_str() } else { toolbar_theme.item_text.as_str() });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, modal_x + SIDEBAR_WIDTH / 2.0, sb_y + item_h / 2.0);

        input_coordinator.register_on_layer(
            wid.as_str(),
            WidgetRect::new(modal_x, sb_y, SIDEBAR_WIDTH, item_h),
            Sense::CLICK | Sense::HOVER,
            &layer_id,
        );
        sb_y += item_h;
    }

    ctx.set_stroke_color(&frame_theme.toolbar_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x + SIDEBAR_WIDTH, sidebar_top);
    ctx.line_to(modal_x + SIDEBAR_WIDTH, modal_y + MODAL_HEIGHT);
    ctx.stroke();

    // ── Content ──────────────────────────────────────────────────────────
    let content_x = modal_x + SIDEBAR_WIDTH;
    let content_y = sidebar_top;
    let content_w = MODAL_WIDTH - SIDEBAR_WIDTH;
    let content_h = MODAL_HEIGHT - HEADER_HEIGHT;

    match state.section {
        TcModalSection::Map => render_map_section(
            ctx, toolbar_theme, frame_theme, panels, container_size,
            content_x, content_y, content_w, content_h,
            &layer_id, input_coordinator,
        ),
        TcModalSection::Setup => render_setup_section(
            ctx, toolbar_theme, center_label, hist_rows,
            content_x, content_y, content_w,
            &layer_id, input_coordinator,
        ),
    }

    input_coordinator.pop_layer(&layer_id);
    result
}

// =============================================================================
// MAP section — scaled card map of the hosted panels
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_map_section(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    panels: &[TcMapPanel],
    container_size: (f64, f64),
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
) {
    let (cw, ch) = container_size;
    if cw <= 0.0 || ch <= 0.0 {
        return;
    }

    let map_max_w = content_w - MODAL_PADDING * 2.0;
    let map_max_h = content_h - MODAL_PADDING * 2.0 - 20.0;
    let aspect = cw / ch;
    let (map_w, map_h) = if map_max_w / map_max_h > aspect {
        (map_max_h * aspect, map_max_h)
    } else {
        (map_max_w, map_max_w / aspect)
    };
    let map_x = content_x + (content_w - map_w) / 2.0;
    let map_y = content_y + MODAL_PADDING;
    let sx = map_w / cw;
    let sy = map_h / ch;

    // Map background
    ctx.set_fill_color(&frame_theme.toolbar_bg);
    ctx.fill_rounded_rect(map_x - 2.0, map_y - 2.0, map_w + 4.0, map_h + 4.0, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(map_x - 2.0, map_y - 2.0, map_w + 4.0, map_h + 4.0, 4.0);

    for panel in panels {
        let (px, py, pw, ph) = panel.rect;
        let mx = map_x + px * sx;
        let my = map_y + py * sy;
        let mw = (pw * sx).max(2.0);
        let mh = (ph * sy).max(2.0);

        let wid = format!("tc_modal:map:{}", panel.id);
        let is_hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));

        let fill = if panel.active {
            mlc_core::apply_opacity(&toolbar_theme.accent, if is_hovered { 0.45 } else { 0.30 })
        } else if is_hovered {
            mlc_core::apply_opacity(&toolbar_theme.item_text, 0.14)
        } else {
            mlc_core::apply_opacity(&toolbar_theme.item_text, 0.06)
        };
        ctx.set_fill_color(&fill);
        ctx.fill_rounded_rect(mx + 1.0, my + 1.0, (mw - 2.0).max(1.0), (mh - 2.0).max(1.0), 3.0);

        if panel.active {
            ctx.set_stroke_color(&toolbar_theme.accent);
            ctx.set_stroke_width(1.5);
        } else if is_hovered {
            ctx.set_stroke_color(&toolbar_theme.item_text_muted);
            ctx.set_stroke_width(1.2);
        } else {
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
        }
        ctx.stroke_rounded_rect(mx + 1.0, my + 1.0, (mw - 2.0).max(1.0), (mh - 2.0).max(1.0), 3.0);

        // Labels — headline centered, sub line under it (size-gated like the
        // Tags & Tabs minimap).
        if mw > 34.0 && mh > 14.0 {
            let has_sub = !panel.sub.is_empty() && mh > 34.0;
            let label_y = if has_sub { my + mh / 2.0 - 7.0 } else { my + mh / 2.0 };
            ctx.set_font("bold 10px sans-serif");
            ctx.set_fill_color(
                if panel.active { &toolbar_theme.item_text_active }
                else if is_hovered { &toolbar_theme.item_text_active }
                else { &toolbar_theme.item_text },
            );
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&panel.label, mx + mw / 2.0, label_y);
            if has_sub {
                ctx.set_font("9px sans-serif");
                ctx.set_fill_color(&toolbar_theme.item_text_muted);
                ctx.fill_text(&panel.sub, mx + mw / 2.0, label_y + 13.0);
            }
        }

        let sense = if panel.clickable { Sense::CLICK | Sense::HOVER } else { Sense::HOVER };
        input_coordinator.register_on_layer(
            wid.as_str(),
            WidgetRect::new(mx, my, mw, mh),
            sense,
            layer_id,
        );
    }

    // Footer: live container size
    ctx.set_font("10px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        &format!("{}×{} px", cw.round() as i64, ch.round() as i64),
        content_x + content_w / 2.0,
        map_y + map_h + 12.0,
    );
}

// =============================================================================
// SETUP section — container-level knobs
// =============================================================================

/// The fixed history-depth option labels, in the same order the app layer's
/// `HistoryDepth::OPTIONS` uses — `tc_modal:hist:{key}:{idx}` carries the
/// index.
const HIST_OPTIONS: [&str; 4] = ["Off", "7d", "30d", "All"];

#[allow(clippy::too_many_arguments)]
fn render_setup_section(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    center_label: &str,
    hist_rows: &[(String, String, usize)],
    content_x: f64,
    content_y: f64,
    content_w: f64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
) {
    let left = content_x + MODAL_PADDING;
    let mut row_y = content_y + MODAL_PADDING;

    // ── Center-slot switcher ─────────────────────────────────────────────
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Center panel", left, row_y + 10.0);
    row_y += 26.0;

    let btn_w = 70.0;
    let btn_h = 26.0;
    for (i, (id, label)) in [("dom", "DOM"), ("chart", "Chart")].iter().enumerate() {
        let bx = left + i as f64 * (btn_w + 8.0);
        let wid = format!("tc_modal:center:{}", id);
        let is_active = center_label.eq_ignore_ascii_case(label);
        let is_hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
        let bg = if is_active {
            toolbar_theme.accent.clone()
        } else if is_hovered {
            toolbar_theme.item_bg_hover.clone()
        } else {
            mlc_core::apply_opacity(&toolbar_theme.item_text, 0.06)
        };
        ctx.set_fill_color(&bg);
        ctx.fill_rounded_rect(bx, row_y, btn_w, btn_h, 4.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, bx + btn_w / 2.0, row_y + btn_h / 2.0);

        input_coordinator.register_on_layer(
            wid.as_str(),
            WidgetRect::new(bx, row_y, btn_w, btn_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    row_y += btn_h + 20.0;

    // ── Layout reset ─────────────────────────────────────────────────────
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Layout", left, row_y + 10.0);
    row_y += 26.0;

    let reset_w = 148.0;
    let wid = "tc_modal:reset_layout";
    let is_hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid));
    ctx.set_fill_color(if is_hovered {
        &toolbar_theme.item_bg_hover
    } else {
        &toolbar_theme.item_bg_active
    });
    ctx.fill_rounded_rect(left, row_y, reset_w, btn_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Reset layout", left + reset_w / 2.0, row_y + btn_h / 2.0);
    input_coordinator.register_on_layer(
        wid,
        WidgetRect::new(left, row_y, reset_w, btn_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    row_y += btn_h + 8.0;

    ctx.set_font("10px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        "Restores separators and table columns",
        left,
        row_y + 8.0,
    );
    row_y += 24.0;

    // ── History depth per table (owner spec 2026-07-28) ─────────────────
    // The common gear owns these knobs while the tables live inside the
    // container (their per-panel mini-gears are suppressed here).
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("History", left, row_y + 10.0);
    row_y += 24.0;

    let opt_w = 44.0;
    let opt_h = 20.0;
    let opts_x = content_x + content_w - MODAL_PADDING - opt_w * HIST_OPTIONS.len() as f64 - 4.0 * (HIST_OPTIONS.len() as f64 - 1.0);
    for (label, key, active_idx) in hist_rows {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, left, row_y + opt_h / 2.0);

        for (i, opt) in HIST_OPTIONS.iter().enumerate() {
            let bx = opts_x + i as f64 * (opt_w + 4.0);
            let wid = format!("tc_modal:hist:{}:{}", key, i);
            let is_active = *active_idx == i;
            let is_hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
            let bg = if is_active {
                toolbar_theme.accent.clone()
            } else if is_hovered {
                toolbar_theme.item_bg_hover.clone()
            } else {
                mlc_core::apply_opacity(&toolbar_theme.item_text, 0.06)
            };
            ctx.set_fill_color(&bg);
            ctx.fill_rounded_rect(bx, row_y, opt_w, opt_h, 3.0);
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(opt, bx + opt_w / 2.0, row_y + opt_h / 2.0);

            input_coordinator.register_on_layer(
                wid.as_str(),
                WidgetRect::new(bx, row_y, opt_w, opt_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }
        row_y += opt_h + 6.0;
    }
}
