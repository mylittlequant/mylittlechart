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
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    begin_modal_shell, end_modal_shell, ModalFooter, ModalPlacement, ModalShellSpec, ModalTab,
    ModalTabs, ModalTheme,
};
use mlc_core::engine::render::RenderContext;
use mlc_core::render::{TextAlign, TextBaseline};

// =============================================================================
// Input data
// =============================================================================

/// One card on the MAP section — a panel currently hosted inside the
/// trading container, with its rect in VIRTUAL map coordinates (the app
/// layer inflates the thin strips so their labels stay readable at map
/// scale; the renderer scales against the panels' bounding box).
#[derive(Clone, Debug)]
pub struct TcMapPanel {
    /// Widget-id suffix (`tc_modal:map:{id}` is registered for it).
    pub id: String,
    /// Card headline (e.g. "DOM", "Order Entry", "Positions (2)").
    pub label: String,
    /// Secondary line under the headline (suppressed on small cards).
    pub sub: String,
    /// Virtual map rect `(x, y, w, h)`.
    pub rect: (f64, f64, f64, f64),
    /// Accent highlight.
    pub active: bool,
    /// Whether clicking the card body performs an action.
    pub clickable: bool,
    /// Segmented card (owner spec 2026-07-28): the bottom strip is ONE
    /// card carrying N tab buttons `(wire_id, label, active)` — the whole
    /// card is drawn once, each tab registered as
    /// `tc_modal:map:{id}:tab:{wire_id}`. Empty for plain cards.
    pub tabs: Vec<(String, String, bool)>,
    /// Draw a fast-swap button on the card (owner spec 2026-07-28) —
    /// registered as `tc_modal:map:{id}:swap`. Used by the center card
    /// (DOM<->Chart).
    pub swap: bool,
}

impl TcMapPanel {
    /// Plain card constructor (no tabs, no swap).
    pub fn plain(id: &str, label: String, sub: String, rect: (f64, f64, f64, f64), clickable: bool) -> Self {
        Self { id: id.to_string(), label, sub, rect, active: false, clickable, tabs: Vec::new(), swap: false }
    }
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
    /// SETUP section scroll — the section outgrew the fixed modal height
    /// (owner defect 2026-07-29: content spilled past the frame).
    pub scroll: crate::ui::scroll_widget::ScrollState,
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
    /// SETUP section scroll geometry — wheel clamp inputs (0 outside SETUP).
    pub setup_content_height: f64,
    pub setup_viewport_height: f64,
    /// SETUP scrollbar rects — handle drag + track click (None when the
    /// content fits or outside SETUP).
    pub scrollbar_handle_rect: Option<WidgetRect>,
    pub scrollbar_track_rect: Option<WidgetRect>,
}

// =============================================================================
// Layout constants
// =============================================================================

const MODAL_WIDTH: f64 = 480.0;
const MODAL_HEIGHT: f64 = 552.0;
const HEADER_HEIGHT: f64 = 36.0;
const SIDEBAR_WIDTH: f64 = 80.0;
const MODAL_PADDING: f64 = 12.0;

// =============================================================================
// Renderer
// =============================================================================

/// Render the Trading Container modal.
///
/// SETUP-section data, flattened by the app layer (this crate cannot see
/// the panel state types).
#[derive(Clone, Debug, Default)]
pub struct TcSetupData {
    /// Interior layout mode: true = Free docking, false = Fixed v1 regions
    /// (owner spec 2026-07-29) — drives the "Layout mode" switcher.
    pub layout_free: bool,
    /// Current center-slot occupant ("DOM"/"Chart") — drives the switcher.
    pub center_label: String,
    /// Per-table history-depth rows `(display label, wire key, active
    /// option index into HIST_OPTIONS)`.
    pub hist_rows: Vec<(String, String, usize)>,
    /// Ticker-tape entry format: true = full `SYMBOL · EXCHANGE · AT`,
    /// false = symbol only.
    pub tape_full: bool,
    /// Metrics-strip inclusion toggles `(display label, wire key, on)`.
    pub metrics: Vec<(String, String, bool)>,
    /// Whether the tape / metrics strips are CONNECTED (owner spec
    /// 2026-07-29: SETUP configures the live blocks — in Free mode a
    /// disconnected strip's knobs are hidden). Fixed mode passes true.
    pub show_tape: bool,
    pub show_metrics: bool,
    /// Risk-calculator hosting: false = its own panel, true = overlay
    /// popover behind the gauge chip on Order Entry (owner 2026-07-29).
    pub rc_overlay: bool,
    /// Free-mode focus-highlight style: false = accent frame, true =
    /// main-chart-style 2px corner bar (owner 2026-07-29).
    pub focus_bar: bool,
}

/// `panels` — the MAP cards (container-local rects); `container_size` — the
/// container's last rendered `(w, h)` (aspect + scale source); `setup` —
/// the SETUP section's data (see [`TcSetupData`]).
#[allow(clippy::too_many_arguments)]
pub fn render_trading_container_modal(
    ctx: &mut dyn RenderContext,
    state: &TradingContainerModalState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    panels: &[TcMapPanel],
    container_size: (f64, f64),
    setup: &TcSetupData,
    screen_w: f64,
    screen_h: f64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> TcModalResult {
    let mut result = TcModalResult::default();

    // ── Chrome ───────────────────────────────────────────────────────────
    // Frame, header, title, ✕, screen clamp, layer AND the MAP / SETUP rail:
    // that rail is a vertical tab strip, so it is `ModalTabs::Vertical` with
    // no icons — the shell draws the label when a tab carries none, and marks
    // the selected row the way every rail in this app marks it.
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &frame_theme.toolbar_border,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_active,
        &frame_theme.toolbar_border,
    );
    let sidebar_items = [
        ("map", "MAP", TcModalSection::Map),
        ("setup", "SETUP", TcModalSection::Setup),
    ];
    let tab_items: Vec<ModalTab<'_>> = sidebar_items
        .iter()
        .map(|(id, label, _)| ModalTab { id, label, icon: None })
        .collect();
    let active_tab_idx = sidebar_items
        .iter()
        .position(|(_, _, section)| *section == state.section)
        .unwrap_or(0);
    let body_h = MODAL_HEIGHT - HEADER_HEIGHT;
    let spec = ModalShellSpec {
        // The sections inset themselves.
        padding: 0.0,
        header_height: HEADER_HEIGHT,
        placement: ModalPlacement::At(state.position),
        ..ModalShellSpec::new("tc_modal", "Trading Container", MODAL_WIDTH, body_h)
            .with_tabs(ModalTabs::Vertical {
                width: SIDEBAR_WIDTH,
                items: &tab_items,
                active: active_tab_idx,
            })
            .with_footer(ModalFooter::None)
            .with_max_content_height(body_h)
    };
    let (shell, layer_id) =
        begin_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h));

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;

    // ── Content ──────────────────────────────────────────────────────────
    let content_x = shell.content.x;
    let content_y = shell.content.y;
    let content_w = shell.content.width;
    let content_h = shell.content.height;

    match state.section {
        TcModalSection::Map => render_map_section(
            ctx, toolbar_theme, frame_theme, panels, container_size,
            content_x, content_y, content_w, content_h,
            &layer_id, input_coordinator,
        ),
        TcModalSection::Setup => {
            let (setup_ch, setup_vh, handle, track) = render_setup_section(
                ctx, toolbar_theme, setup,
                content_x, content_y, content_w, content_h,
                &layer_id, input_coordinator,
                &state.scroll,
            );
            result.setup_content_height = setup_ch;
            result.setup_viewport_height = setup_vh;
            result.scrollbar_handle_rect = handle;
            result.scrollbar_track_rect = track;
        }
    }

    end_modal_shell(ctx, input_coordinator, &spec, &modal_theme, &shell, &layer_id);
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
    if cw <= 0.0 || ch <= 0.0 || panels.is_empty() {
        return;
    }

    // Scale against the panels' VIRTUAL bounding box (the app layer inflated
    // the thin strips) — `container_size` is only the real px for the footer.
    let vw = panels.iter().map(|p| p.rect.0 + p.rect.2).fold(0.0_f64, f64::max);
    let vh = panels.iter().map(|p| p.rect.1 + p.rect.3).fold(0.0_f64, f64::max);
    if vw <= 0.0 || vh <= 0.0 {
        return;
    }

    let map_max_w = content_w - MODAL_PADDING * 2.0;
    let map_max_h = content_h - MODAL_PADDING * 2.0 - 20.0;
    let aspect = vw / vh;
    let (map_w, map_h) = if map_max_w / map_max_h > aspect {
        (map_max_h * aspect, map_max_h)
    } else {
        (map_max_w, map_max_w / aspect)
    };
    let map_x = content_x + (content_w - map_w) / 2.0;
    let map_y = content_y + MODAL_PADDING;
    let sx = map_w / vw;
    let sy = map_h / vh;

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

        // Segmented card (bottom strip): draw N tab buttons instead of a
        // single label — the card IS one panel, the tabs just switch its
        // active view (owner spec 2026-07-28).
        if !panel.tabs.is_empty() {
            let seg_w = mw / panel.tabs.len() as f64;
            for (i, (wire, label, active)) in panel.tabs.iter().enumerate() {
                let tx = mx + i as f64 * seg_w;
                let twid = format!("tc_modal:map:{}:tab:{}", panel.id, wire);
                let thov = input_coordinator.is_hovered(&uzor::WidgetId::from(twid.as_str()));
                if *active {
                    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.accent, if thov { 0.5 } else { 0.35 }));
                    ctx.fill_rounded_rect(tx + 1.5, my + 1.5, (seg_w - 3.0).max(1.0), (mh - 3.0).max(1.0), 2.0);
                } else if thov {
                    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, 0.12));
                    ctx.fill_rounded_rect(tx + 1.5, my + 1.5, (seg_w - 3.0).max(1.0), (mh - 3.0).max(1.0), 2.0);
                }
                if i > 0 {
                    ctx.set_stroke_color(&toolbar_theme.separator);
                    ctx.set_stroke_width(1.0);
                    ctx.begin_path();
                    ctx.move_to(tx, my + 3.0);
                    ctx.line_to(tx, my + mh - 3.0);
                    ctx.stroke();
                }
                if seg_w > 30.0 {
                    ctx.set_font("bold 9px sans-serif");
                    ctx.set_fill_color(if *active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(label, tx + seg_w / 2.0, my + mh / 2.0);
                }
                input_coordinator.register_on_layer(
                    twid.as_str(),
                    WidgetRect::new(tx, my, seg_w, mh),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }
        } else {
            // Labels — headline centered, sub line under it (size-gated like
            // the Tags & Tabs minimap). The swap button steals the top-right
            // corner when present.
            if mw > 34.0 && mh > 14.0 {
                let has_sub = !panel.sub.is_empty() && mh > 34.0;
                let label_y = if has_sub { my + mh / 2.0 - 7.0 } else { my + mh / 2.0 };
                ctx.set_font("bold 10px sans-serif");
                ctx.set_fill_color(
                    if panel.active || is_hovered { &toolbar_theme.item_text_active }
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

        // Fast-swap button (center card, owner spec 2026-07-28): a small
        // top-right chip on the card — one click flips DOM<->Chart.
        if panel.swap && mw > 40.0 && mh > 24.0 {
            let sw = 20.0_f64.min(mw - 6.0);
            let sx0 = mx + mw - sw - 3.0;
            let sy0 = my + 3.0;
            let swid = format!("tc_modal:map:{}:swap", panel.id);
            let shov = input_coordinator.is_hovered(&uzor::WidgetId::from(swid.as_str()));
            ctx.set_fill_color(if shov { &toolbar_theme.accent } else { &toolbar_theme.item_bg_hover });
            ctx.fill_rounded_rect(sx0, sy0, sw, 14.0, 3.0);
            ctx.set_font("9px sans-serif");
            ctx.set_fill_color(&toolbar_theme.item_text_active);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("\u{21c4}", sx0 + sw / 2.0, sy0 + 7.0);
            input_coordinator.register_on_layer(
                swid.as_str(),
                WidgetRect::new(sx0, sy0, sw, 14.0),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }
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
    setup: &TcSetupData,
    content_x: f64,
    content_y: f64,
    content_w: f64,
    content_h: f64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    scroll: &crate::ui::scroll_widget::ScrollState,
) -> (f64, f64, Option<WidgetRect>, Option<WidgetRect>) {
    use crate::ui::scroll_widget::{ScrollableContainer, ScrollableConfig};
    use crate::ui::widgets::types::WidgetTheme;

    // The section outgrew the fixed modal height — clip + wheel-scroll
    // the whole content column, honest scrollbar on overflow (owner
    // defect 2026-07-29: rows spilled past the modal frame).
    let viewport = WidgetRect::new(content_x, content_y, content_w, content_h);
    input_coordinator.register_on_layer(
        "tc_modal:setup_viewport",
        viewport,
        Sense::SCROLL,
        layer_id,
    );
    let scrollable = ScrollableContainer::new(
        viewport,
        scroll,
        Some(ScrollableConfig {
            scrollbar_width: 8.0,
            scrollbar_padding: 4.0,
            always_show_scrollbar: false,
        }),
    );
    scrollable.begin(ctx);
    let content_y0 = scrollable.content_y();

    let center_label = setup.center_label.as_str();
    let hist_rows = &setup.hist_rows;
    let left = content_x + MODAL_PADDING;
    let mut row_y = content_y0 + MODAL_PADDING;

    let btn_w = 70.0;
    let btn_h = 26.0;

    // ── Interior layout mode (owner spec 2026-07-29) ─────────────────────
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Layout mode", left, row_y + 10.0);
    row_y += 26.0;

    for (i, (id, label)) in [("fixed", "Fixed"), ("free", "Free")].iter().enumerate() {
        let bx = left + i as f64 * (btn_w + 8.0);
        let wid = format!("tc_modal:layout:{}", id);
        let is_active = setup.layout_free == (*id == "free");
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

    // ── Center-slot switcher — Fixed-mode concept only (Free spawns
    // panels from the header dropdown instead of swapping a center).
    if !setup.layout_free {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Center panel", left, row_y + 10.0);
        row_y += 26.0;
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
    }

    // ── Risk-calculator hosting (owner spec 2026-07-29): its own panel,
    // or an overlay popover behind the gauge chip on Order Entry.
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Risk Calculator", left, row_y + 10.0);
    row_y += 26.0;

    for (i, (id, label)) in [("panel", "Panel"), ("overlay", "OE Button")].iter().enumerate() {
        let bx = left + i as f64 * (btn_w + 8.0);
        let wid = format!("tc_modal:rc_host:{}", id);
        let is_active = setup.rc_overlay == (*id == "overlay");
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

    // ── Focus-highlight style (owner spec 2026-07-29): accent frame, or
    // the main-chart convention — a 2px corner bar.
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Focus highlight", left, row_y + 10.0);
    row_y += 26.0;

    for (i, (id, label)) in [("frame", "Frame"), ("bar", "Corner bar")].iter().enumerate() {
        let bx = left + i as f64 * (btn_w + 8.0);
        let wid = format!("tc_modal:focus_style:{}", id);
        let is_active = setup.focus_bar == (*id == "bar");
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
    // container (their per-panel mini-gears are suppressed here). Rows
    // arrive pre-filtered to the CONNECTED tables — none connected, no
    // block.
    if !hist_rows.is_empty() {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("History", left, row_y + 10.0);
        row_y += 24.0;
    }

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
    row_y += 12.0;

    // ── Ticker-tape entry format (owner spec 2026-07-28) ────────────────
    if setup.show_tape {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Tape format", left, row_y + 10.0);
        row_y += 24.0;

        for (i, (id, label)) in [("full", "Sym \u{00b7} Ex \u{00b7} AT"), ("short", "Symbol")].iter().enumerate() {
            let fmt_w = 110.0;
            let bx = left + i as f64 * (fmt_w + 8.0);
            let wid = format!("tc_modal:tapefmt:{}", id);
            let is_active = (*id == "full") == setup.tape_full;
            let is_hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
            let bg = if is_active {
                toolbar_theme.accent.clone()
            } else if is_hovered {
                toolbar_theme.item_bg_hover.clone()
            } else {
                mlc_core::apply_opacity(&toolbar_theme.item_text, 0.06)
            };
            ctx.set_fill_color(&bg);
            ctx.fill_rounded_rect(bx, row_y, fmt_w, 22.0, 4.0);
            ctx.set_font("10px sans-serif");
            ctx.set_fill_color(if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text });
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, bx + fmt_w / 2.0, row_y + 11.0);
            input_coordinator.register_on_layer(
                wid.as_str(),
                WidgetRect::new(bx, row_y, fmt_w, 22.0),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }
        row_y += 22.0 + 12.0;
    }

    // ── Metrics-strip inclusion toggles (owner spec 2026-07-28) ─────────
    if setup.show_metrics {
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text("Metrics", left, row_y + 10.0);
    row_y += 24.0;

    let chip_h = 20.0;
    let chip_gap = 6.0;
    let mut chip_x = left;
    for (label, key, on) in &setup.metrics {
        ctx.set_font("10px sans-serif");
        let chip_w = ctx.measure_text(label) + 18.0;
        if chip_x + chip_w > content_x + content_w - MODAL_PADDING {
            chip_x = left;
            row_y += chip_h + chip_gap;
        }
        let wid = format!("tc_modal:metric:{}", key);
        let is_hovered = input_coordinator.is_hovered(&uzor::WidgetId::from(wid.as_str()));
        let bg = if *on {
            toolbar_theme.accent.clone()
        } else if is_hovered {
            toolbar_theme.item_bg_hover.clone()
        } else {
            mlc_core::apply_opacity(&toolbar_theme.item_text, 0.06)
        };
        ctx.set_fill_color(&bg);
        ctx.fill_rounded_rect(chip_x, row_y, chip_w, chip_h, 10.0);
        ctx.set_fill_color(if *on { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted });
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, chip_x + chip_w / 2.0, row_y + chip_h / 2.0);
        input_coordinator.register_on_layer(
            wid.as_str(),
            WidgetRect::new(chip_x, row_y, chip_w, chip_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
        chip_x += chip_w + chip_gap;
    }
    row_y += chip_h + MODAL_PADDING;
    }

    // Measured off the actual rows drawn above — the robust source of
    // truth for the scrollbar (same doctrine as chart_settings).
    let total_content_h = row_y - content_y0;
    let widget_theme = WidgetTheme::default();
    let scroll_result = scrollable.end(ctx, total_content_h, &widget_theme);

    // Register scrollbar handle (inflated ±5px X) for DRAG, track for
    // CLICK — same wiring as chart_settings.
    if let Some(ref hr) = scroll_result.handle_rect {
        let inflated = WidgetRect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "tc_modal:scrollbar_handle",
            inflated,
            Sense::DRAG,
            layer_id,
        );
    }
    if let Some(ref tr) = scroll_result.track_rect {
        input_coordinator.register_on_layer(
            "tc_modal:scrollbar_track",
            WidgetRect::new(tr.x, tr.y, tr.width, tr.height),
            Sense::CLICK,
            layer_id,
        );
    }

    (total_content_h, content_h, scroll_result.handle_rect, scroll_result.track_rect)
}
