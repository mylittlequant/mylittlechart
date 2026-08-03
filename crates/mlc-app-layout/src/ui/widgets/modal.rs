//! Modal dialog widget rendering
//!
//! Platform-agnostic modal dialog rendering using RenderContext.

use mlc_core::engine::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;
use uzor::WidgetId;

/// Modal theme
#[derive(Clone, Debug)]
pub struct ModalTheme {
    pub backdrop: String,
    pub background: String,
    pub border: String,
    pub shadow: String,
    pub header_bg: String,
    pub header_text: String,
    pub header_border: String,
    pub footer_bg: String,
    pub footer_border: String,
    pub close_button: String,
    pub close_button_hover: String,
}

impl Default for ModalTheme {
    fn default() -> Self {
        Self {
            backdrop: "rgba(0,0,0,0.5)".to_string(),
            background: "#1e222d".to_string(),
            border: "#363a45".to_string(),
            shadow: "rgba(0,0,0,0.5)".to_string(),
            header_bg: "#1e222d".to_string(),
            header_text: "#ffffff".to_string(),
            header_border: "#363a45".to_string(),
            footer_bg: "#1e222d".to_string(),
            footer_border: "#363a45".to_string(),
            close_button: "#9598a1".to_string(),
            close_button_hover: "#ffffff".to_string(),
        }
    }
}

impl ModalTheme {
    /// Create ModalTheme from FrameTheme and ToolbarTheme
    ///
    /// This is a convenience function for migrating existing modals that use
    /// FrameTheme/ToolbarTheme to the new unified modal system.
    ///
    /// # Note
    /// FrameTheme is from mlc_core::layout::FrameTheme
    /// ToolbarTheme is from crate::ui::render::ToolbarTheme
    pub fn from_frame_theme(
        toolbar_bg: &str,
        toolbar_border: &str,
        item_text: &str,
        item_text_hover: &str,
        separator: &str,
    ) -> Self {
        Self {
            backdrop: "rgba(0,0,0,0.6)".to_string(),
            background: toolbar_bg.to_string(),
            border: toolbar_border.to_string(),
            shadow: "rgba(0,0,0,0.4)".to_string(),
            header_bg: toolbar_bg.to_string(),
            header_text: item_text.to_string(),
            header_border: separator.to_string(),
            footer_bg: toolbar_bg.to_string(),
            footer_border: separator.to_string(),
            close_button: item_text.to_string(),
            close_button_hover: item_text_hover.to_string(),
        }
    }
}

/// Render modal frame (shadow, blur, background, border) without header/footer
///
/// The bare surface, for modals whose whole layout is bespoke. Anything with
/// a title bar wants [`render_modal_shell`] instead — it draws this frame and
/// owns the chrome around it.
///
/// # Parameters
/// - `ctx` - Render context
/// - `frame_rect` - Modal frame rectangle (position and size)
/// - `theme` - Modal theme
/// - `with_radius` - Whether to use rounded corners (0.0 for minimal style)
pub fn render_modal_frame_only(
    ctx: &mut dyn RenderContext,
    frame_rect: WidgetRect,
    theme: &ModalTheme,
    with_radius: f64,
) {
    let shadow_offset = if with_radius > 0.0 { 4.0 } else { 3.0 };

    // Shadow (subtle)
    ctx.set_fill_color("rgba(0, 0, 0, 0.4)");
    if with_radius > 0.0 {
        ctx.fill_rounded_rect(
            frame_rect.x + shadow_offset,
            frame_rect.y + shadow_offset,
            frame_rect.width,
            frame_rect.height,
            with_radius,
        );
    } else {
        ctx.fill_rect(
            frame_rect.x + shadow_offset,
            frame_rect.y + shadow_offset,
            frame_rect.width,
            frame_rect.height,
        );
    }

    // Blur background (FrostedGlass/LiquidGlass effect)
    ctx.draw_blur_background(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);

    // Background
    ctx.set_fill_color(&theme.background);
    if with_radius > 0.0 {
        ctx.fill_rounded_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height, with_radius);
    } else {
        ctx.fill_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);
    }

    // Border
    ctx.set_stroke_color(&theme.border);
    ctx.set_stroke_width(1.0);
    if with_radius > 0.0 {
        ctx.stroke_rounded_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height, with_radius);
    } else {
        ctx.stroke_rect(frame_rect.x, frame_rect.y, frame_rect.width, frame_rect.height);
    }
}

// =============================================================================
// The modal shell
// =============================================================================

/// Where a modal sits on screen.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ModalPlacement {
    /// Centred.
    Center,
    /// A remembered top-left corner — what a dragged modal carries between
    /// frames. `None` falls back to centred, which is the shape every caller
    /// already had as `pos.unwrap_or(centre)`.
    At(Option<(f64, f64)>),
    /// Under the control that opened it (its bottom-left corner).
    Anchor(f64, f64),
}

/// What the shell needs to draw a modal. Note what is NOT here: total height.
/// It is derived from `content_height`, which is the one number the body can
/// actually measure — a modal that sizes itself from an enum cannot host a
/// list whose length it does not know.
#[derive(Clone, Debug)]
pub struct ModalShellSpec<'a> {
    /// Widget-id namespace. The shell names its layer `{ns}` and registers
    /// `{ns}:header`, `{ns}:modal_bg` and `{ns}:close`; the body registers
    /// ids of its own on the same layer.
    pub ns: &'a str,
    pub title: &'a str,
    pub width: f64,
    /// Height of the body between header and footer, padding excluded.
    pub content_height: f64,
    /// `0.0` = no footer.
    pub footer_height: f64,
    pub placement: ModalPlacement,
    pub layer: ZLayer,
    pub header_height: f64,
    pub padding: f64,
    pub radius: f64,
    /// Register the header as a drag handle. Off for anchored surfaces —
    /// a dropdown pinned to a gear has nowhere to be dragged to.
    pub draggable: bool,
    pub show_close: bool,
}

impl<'a> ModalShellSpec<'a> {
    /// A draggable, centred, closable settings surface — what nearly every
    /// modal in this codebase is.
    pub fn new(ns: &'a str, title: &'a str, width: f64, content_height: f64) -> Self {
        Self {
            ns,
            title,
            width,
            content_height,
            footer_height: 0.0,
            placement: ModalPlacement::Center,
            layer: ZLayer::Modal,
            header_height: 44.0,
            padding: 16.0,
            radius: 0.0,
            draggable: true,
            show_close: true,
        }
    }

    pub fn placed(mut self, placement: ModalPlacement) -> Self {
        self.placement = placement;
        self
    }

    pub fn on_layer(mut self, layer: ZLayer) -> Self {
        self.layer = layer;
        self
    }

    pub fn with_footer(mut self, height: f64) -> Self {
        self.footer_height = height;
        self
    }

    /// An anchored surface: no drag, and it hugs the control that opened it.
    pub fn anchored(mut self, x: f64, y: f64) -> Self {
        self.placement = ModalPlacement::Anchor(x, y);
        self.draggable = false;
        self
    }
}

/// The geometry the shell settled on. The body draws inside `content`; a
/// footer, when asked for, is the body's to fill — it gets the same layer, so
/// its buttons register like any other row.
#[derive(Clone, Debug, Default)]
pub struct ModalShell {
    pub frame: WidgetRect,
    pub header: WidgetRect,
    pub content: WidgetRect,
    pub footer: Option<WidgetRect>,
}

/// Draw a modal and hand its body the layer to register on.
///
/// This owns everything that used to be copied into every modal: the layer,
/// the header-drag / background / close registrations, the on-screen clamp,
/// the title, the separators. A body that uses it never names `ZLayer`,
/// `Icon::Close` or a screen edge.
pub fn render_modal_shell<F>(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    spec: &ModalShellSpec<'_>,
    theme: &ModalTheme,
    screen: (f64, f64),
    body: F,
) -> ModalShell
where
    F: FnOnce(&mut dyn RenderContext, &mut InputCoordinator, &LayerId, &ModalShell),
{
    let (screen_w, screen_h) = screen;
    let height = shell_height(spec);

    // Placement, then the clamp. Anchored surfaces keep a small margin so a
    // gear at the very edge still opens something fully visible.
    let centre = (
        (screen_w - spec.width) / 2.0,
        (screen_h - height) / 2.0,
    );
    let (x, y) = match spec.placement {
        ModalPlacement::Center => centre,
        ModalPlacement::At(pos) => pos.unwrap_or(centre),
        ModalPlacement::Anchor(ax, ay) => (ax, ay),
    };
    let margin = if matches!(spec.placement, ModalPlacement::Anchor(..)) { 4.0 } else { 0.0 };
    let x = clamp_on_screen(x, spec.width, screen_w, margin);
    let y = clamp_on_screen(y, height, screen_h, margin);

    let frame = WidgetRect::new(x, y, spec.width, height);
    let header = WidgetRect::new(x, y, spec.width, spec.header_height);
    let footer = (spec.footer_height > 0.0).then(|| {
        WidgetRect::new(x, frame.bottom() - spec.footer_height, spec.width, spec.footer_height)
    });
    let content = WidgetRect::new(
        x + spec.padding,
        header.bottom() + spec.padding,
        spec.width - spec.padding * 2.0,
        spec.content_height,
    );
    let shell = ModalShell { frame, header, content, footer };

    let layer_id = spec.layer.push_named(coord, spec.ns);
    // The background swallows clicks that land on the modal but on nothing in
    // particular — without it they reach whatever is underneath.
    coord.register_on_layer(
        format!("{}:modal_bg", spec.ns).as_str(),
        frame,
        Sense::CLICK,
        &layer_id,
    );
    if spec.draggable {
        coord.register_on_layer(
            format!("{}:header", spec.ns).as_str(),
            header,
            Sense::DRAG,
            &layer_id,
        );
    }

    render_modal_frame_only(ctx, frame, theme, spec.radius);

    // Header.
    ctx.set_fill_color(&theme.header_bg);
    ctx.fill_rect(header.x, header.y, header.width, header.height);
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(&theme.header_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(spec.title, header.x + spec.padding, header.center_y());

    if spec.show_close {
        const CLOSE: f64 = 16.0;
        let close_id = format!("{}:close", spec.ns);
        let cx = header.right() - CLOSE - 12.0;
        let cy = header.center_y() - CLOSE / 2.0;
        let hovered = coord.is_hovered(&WidgetId::from(close_id.as_str()));
        let tint = if hovered { &theme.close_button_hover } else { &theme.close_button };
        draw_svg_icon(ctx, Icon::Close.svg(), cx, cy, CLOSE, CLOSE, tint);
        coord.register_on_layer(
            close_id.as_str(),
            WidgetRect::new(cx - 4.0, cy - 4.0, CLOSE + 8.0, CLOSE + 8.0),
            Sense::CLICK | Sense::HOVER,
            &layer_id,
        );
    }

    ctx.set_stroke_color(&theme.header_border);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(header.x, header.bottom());
    ctx.line_to(header.right(), header.bottom());
    ctx.stroke();

    if let Some(f) = footer {
        ctx.set_stroke_color(&theme.footer_border);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(f.x, f.y);
        ctx.line_to(f.right(), f.y);
        ctx.stroke();
    }

    body(ctx, coord, &layer_id, &shell);

    coord.pop_layer(&layer_id);
    shell
}

/// Total frame height: chrome plus whatever the body said it needs.
fn shell_height(spec: &ModalShellSpec<'_>) -> f64 {
    spec.header_height + spec.padding * 2.0 + spec.content_height + spec.footer_height
}

/// Keep an edge on screen. A surface taller than the screen is pinned to the
/// margin rather than pushed off the top by a negative clamp bound.
fn clamp_on_screen(pos: f64, size: f64, screen: f64, margin: f64) -> f64 {
    pos.max(margin).min((screen - size - margin).max(margin))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Height comes from the body, not from a preset — the reason the old
    /// widget could not host a real modal.
    #[test]
    fn height_is_chrome_plus_body() {
        let s = ModalShellSpec::new("t", "Title", 300.0, 100.0).with_footer(40.0);
        assert_eq!(shell_height(&s), 44.0 + 32.0 + 100.0 + 40.0);
    }

    /// A remembered position that would hang the modal off an edge is pulled
    /// back rather than trusted.
    #[test]
    fn a_dragged_position_is_clamped_on_screen() {
        assert_eq!(clamp_on_screen(900.0, 300.0, 1000.0, 0.0), 700.0);
        assert_eq!(clamp_on_screen(-40.0, 300.0, 1000.0, 0.0), 0.0);
        assert_eq!(clamp_on_screen(10.0, 300.0, 1000.0, 4.0), 10.0);
    }

    /// Taller than the screen: pinned to the margin, never above it.
    #[test]
    fn an_oversized_modal_stays_at_the_margin() {
        assert_eq!(clamp_on_screen(50.0, 900.0, 400.0, 4.0), 4.0);
        assert_eq!(clamp_on_screen(50.0, 900.0, 400.0, 0.0), 0.0);
    }

    /// Anchoring turns dragging off: a surface pinned to a gear has nowhere
    /// to be dragged to.
    #[test]
    fn anchoring_disables_drag() {
        let s = ModalShellSpec::new("t", "Title", 300.0, 100.0).anchored(10.0, 20.0);
        assert!(!s.draggable);
        assert_eq!(s.placement, ModalPlacement::Anchor(10.0, 20.0));
    }
}
