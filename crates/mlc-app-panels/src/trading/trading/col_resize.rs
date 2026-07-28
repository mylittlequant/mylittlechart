//! Draggable column widths for the trading tables (owner spec 2026-07-28:
//! Positions / Orders / Trades columns must live-resize).
//!
//! RAM-only (never persisted): `widths` stays `None` until the first drag,
//! so an untouched table keeps adapting proportionally to the panel width;
//! the moment the user drags a boundary the current pixel widths are
//! captured and stay fixed (the last column always takes the remainder).

use crate::panel_theme::PanelTheme;
use crate::render::RenderContext;

/// Per-table column-resize state. `widths` covers every column EXCEPT the
/// last (which is the remainder of the row).
#[derive(Clone, Debug, Default)]
pub struct ColumnResize {
    /// User-resized widths (px) of all-but-last columns. `None` =
    /// proportional defaults.
    pub widths: Option<Vec<f32>>,
    /// Available row width cached by `resolve()` each render — drags only
    /// receive pixel deltas (see `TradingPanel::handle_drag_move`), so this
    /// is how the first drag recomputes the defaults it captures.
    pub last_avail: std::cell::Cell<f32>,
}

impl ColumnResize {
    /// Minimum column width a boundary drag can shrink to.
    pub const MIN_W: f32 = 28.0;

    /// Widths to render with: the user-resized set when present (length
    /// re-checked defensively against `defaults`), else `defaults`. Caches
    /// `avail` for later drags.
    pub fn resolve(&self, avail: f32, defaults: &[f32]) -> Vec<f32> {
        self.last_avail.set(avail);
        match &self.widths {
            Some(v) if v.len() == defaults.len() => v.clone(),
            _ => defaults.to_vec(),
        }
    }

    /// Apply a boundary drag: boundary `idx` sits at the right edge of
    /// column `idx`; dragging by `dx` px resizes that column, clamped to
    /// [`Self::MIN_W`]. The first drag captures `defaults` as the base.
    pub fn drag(&mut self, idx: usize, dx: f32, defaults: &[f32]) {
        let v = self.widths.get_or_insert_with(|| defaults.to_vec());
        if v.len() != defaults.len() {
            *v = defaults.to_vec();
        }
        if let Some(w) = v.get_mut(idx) {
            *w = (*w + dx).max(Self::MIN_W);
        }
    }

    /// Back to proportional defaults (trading-container modal "Reset
    /// layout").
    pub fn reset(&mut self) {
        self.widths = None;
    }
}

/// Parse a `"{panel}:colsep:{idx}"` local id into the boundary index.
pub fn colsep_index(local_id: &str, panel: &str) -> Option<usize> {
    local_id
        .strip_prefix(panel)?
        .strip_prefix(":colsep:")?
        .parse()
        .ok()
}

/// Draw hover feedback + register the draggable boundary widgets for a
/// table's columns. `bounds_x` are the boundary x positions (right edge of
/// each resizable column); widgets span the full table height so the grab
/// target is generous. MUST be called at the END of the panel's `render()`
/// so the thin zones registered here win the hit-test over the row/body
/// widgets (later registration wins — same layering trick the OrdersLog
/// history scroll zone uses).
pub fn draw_col_separators(
    ctx: &mut dyn RenderContext,
    coordinator: &mut uzor::InputCoordinator,
    slot_prefix: &str,
    panel: &str,
    bounds_x: &[f32],
    y: f32,
    h: f32,
    theme: &PanelTheme,
) {
    use uzor::input::Sense;
    for (i, bx) in bounds_x.iter().enumerate() {
        let id = format!("{}:{}:colsep:{}", slot_prefix, panel, i);
        if coordinator.is_hovered(&uzor::WidgetId::from(id.as_str())) {
            ctx.set_fill_color(&theme.accent);
            ctx.fill_rect((*bx - 0.5) as f64, y as f64, 1.0, h as f64);
        }
        coordinator.register(
            id.as_str(),
            uzor::Rect::new((*bx - 3.0) as f64, y as f64, 6.0, h as f64),
            Sense::DRAG | Sense::HOVER,
        );
    }
}
