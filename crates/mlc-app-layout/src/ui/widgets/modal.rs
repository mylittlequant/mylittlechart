//! Modal dialog widget rendering
//!
//! Platform-agnostic modal dialog rendering using RenderContext.

use mlc_core::engine::render::{draw_svg_icon, RenderContext, TextAlign, TextBaseline};
use crate::ui::toolbar_theme::ToolbarTheme;
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
    /// Selected tab / primary button fill.
    pub accent: String,
    /// Text on top of `accent`.
    pub accent_text: String,
    /// Hover fill for a tab or a secondary button.
    pub item_bg_hover: String,
    /// Unselected tab label.
    pub item_text_muted: String,
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
            accent: "#2962ff".to_string(),
            accent_text: "#ffffff".to_string(),
            item_bg_hover: "rgba(255,255,255,0.08)".to_string(),
            item_text_muted: "#9598a1".to_string(),
        }
    }
}

impl ModalTheme {
    /// The one every modal in the app should use: the toolbar owns these
    /// colours already, and passing five loose strings to `from_frame_theme`
    /// is how two modals ended up with different tab and button tints.
    pub fn from_toolbar(toolbar_bg: &str, toolbar_border: &str, tt: &ToolbarTheme) -> Self {
        Self {
            backdrop: "rgba(0,0,0,0.6)".to_string(),
            background: toolbar_bg.to_string(),
            border: toolbar_border.to_string(),
            shadow: "rgba(0,0,0,0.4)".to_string(),
            header_bg: toolbar_bg.to_string(),
            header_text: tt.item_text.clone(),
            header_border: toolbar_border.to_string(),
            footer_bg: toolbar_bg.to_string(),
            footer_border: toolbar_border.to_string(),
            close_button: tt.item_text.clone(),
            close_button_hover: tt.item_text_active.clone(),
            accent: tt.accent.clone(),
            accent_text: tt.item_text_active.clone(),
            item_bg_hover: tt.item_bg_hover.clone(),
            item_text_muted: tt.item_text_muted.clone(),
        }
    }

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
            ..Default::default()
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

/// One entry in a modal's section switch.
#[derive(Clone, Copy, Debug)]
pub struct ModalTab<'a> {
    pub id: &'a str,
    pub label: &'a str,
    /// Rails draw the icon when there is one and the label when there is not.
    pub icon: Option<Icon>,
}

impl<'a> ModalTab<'a> {
    pub fn new(id: &'a str, label: &'a str) -> Self {
        Self { id, label, icon: None }
    }

    pub fn with_icon(id: &'a str, label: &'a str, icon: Icon) -> Self {
        Self { id, label, icon: Some(icon) }
    }
}

/// How a horizontal bar divides its width.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TabFit {
    /// Equal slices across the full width.
    Equal,
    /// Each tab as wide as its own label, packed from the left. Not a style
    /// preference: "Coordinates" and "Text" are not the same word, and the
    /// modals that measure their tabs today would visibly regress under an
    /// equal split.
    Labels,
}

/// A modal's section switch. Orientation is a real choice and both are kept:
/// a rail down the side carries many sections in icons, a bar under the
/// header carries a few and spells them out. What is NOT a choice is their
/// size or their colours.
#[derive(Clone, Copy, Debug)]
pub enum ModalTabs<'a> {
    /// Rail down the left. `TAB_RAIL_W` for icons, `TAB_LIST_W` for labels.
    Vertical { width: f64, items: &'a [ModalTab<'a>], active: usize },
    /// Bar under the header.
    Horizontal { items: &'a [ModalTab<'a>], active: usize, fit: TabFit },
}

/// What sits along the bottom.
#[derive(Clone, Copy, Debug)]
pub enum ModalFooter<'a> {
    /// No bar. For pickers, menus and popovers — those are not dialogs.
    None,
    /// The object already exists and every edit applies as it is made, so
    /// the only action left is to leave: one OK, in the same place in every
    /// modal. `template` is the label of the secondary button on the left
    /// where the modal belongs to a template family — the current template's
    /// name where there is one, the word Template where the button only opens
    /// a menu — and nothing where the modal has no templates at all.
    Editor { template: Option<&'a str> },
    /// The object does not exist yet — this button is what brings it into
    /// existence, and the ✕ aborts.
    Creator { label: &'a str },
}

/// Chrome sizes. They live here so that two modals cannot disagree about
/// them, which is exactly what happened while each one carried its own
/// constants: headers of 36 / 40 / 44, rails of 48 / 80, bars of 32 / 40.
pub const HEADER_H: f64 = 44.0;
pub const FOOTER_H: f64 = 52.0;
pub const TAB_RAIL_W: f64 = 48.0;
pub const TAB_LIST_W: f64 = 80.0;
pub const TAB_BAR_H: f64 = 32.0;
const TAB_ITEM_H: f64 = 44.0;
const TAB_FONT: &str = "11px sans-serif";
const TAB_PAD_H: f64 = 12.0;
const TAB_GAP: f64 = 2.0;
const BTN_H: f64 = 28.0;
const OK_W: f64 = 80.0;

/// What the shell needs to draw a modal. Note what is NOT here: total height.
/// It is derived from `content_height`, which is the one number the body can
/// actually measure — a modal that sizes itself from an enum cannot host a
/// list whose length it does not know.
#[derive(Clone, Debug)]
pub struct ModalShellSpec<'a> {
    /// Widget-id namespace. The shell names its layer `{ns}` and registers
    /// `{ns}:header`, `{ns}:modal_bg`, `{ns}:close`, `{ns}:tab:{id}`,
    /// `{ns}:ok` / `{ns}:template` / `{ns}:create`; the body registers ids of
    /// its own on the same layer.
    pub ns: &'a str,
    /// Name of the layer, when it must differ from `ns`. The layer name is a
    /// routing key — `close_topmost_known_modal_layer` matches on it — and
    /// three modals already ship a layer name that is not their id prefix
    /// (`primitive_settings` against `prim_settings`). Defaults to `ns`.
    pub layer_name: Option<&'a str>,
    pub title: &'a str,
    pub width: f64,
    /// Height of the body between tabs and footer, padding excluded — what
    /// the body would LIKE. What it gets is this clamped by
    /// `max_content_height`.
    pub content_height: f64,
    /// Ceiling on the body. Above it the frame stops growing and the excess
    /// becomes `ModalShell::overflow` for the body to scroll. A modal with a
    /// fixed height sets this and `content_height` to the same number.
    pub max_content_height: Option<f64>,
    pub tabs: Option<ModalTabs<'a>>,
    pub footer: ModalFooter<'a>,
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
    /// A draggable, centred settings surface whose only action is to leave —
    /// what nearly every modal in this codebase is.
    pub fn new(ns: &'a str, title: &'a str, width: f64, content_height: f64) -> Self {
        Self {
            ns,
            layer_name: None,
            title,
            width,
            content_height,
            max_content_height: None,
            tabs: None,
            footer: ModalFooter::Editor { template: None },
            placement: ModalPlacement::Center,
            layer: ZLayer::Modal,
            header_height: HEADER_H,
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

    /// Keep an existing layer name while the widget ids take a shorter one.
    pub fn layer_named(mut self, name: &'a str) -> Self {
        self.layer_name = Some(name);
        self
    }

    /// Stop the frame growing past this body height; the rest is overflow.
    pub fn with_max_content_height(mut self, max: f64) -> Self {
        self.max_content_height = Some(max);
        self
    }

    pub fn with_tabs(mut self, tabs: ModalTabs<'a>) -> Self {
        self.tabs = Some(tabs);
        self
    }

    pub fn with_footer(mut self, footer: ModalFooter<'a>) -> Self {
        self.footer = footer;
        self
    }

    /// An anchored surface: no drag, no footer, and it hugs the control that
    /// opened it. A popover is not a dialog.
    pub fn anchored(mut self, x: f64, y: f64) -> Self {
        self.placement = ModalPlacement::Anchor(x, y);
        self.draggable = false;
        self.footer = ModalFooter::None;
        self
    }

    fn tab_bar_h(&self) -> f64 {
        match self.tabs {
            Some(ModalTabs::Horizontal { .. }) => TAB_BAR_H,
            _ => 0.0,
        }
    }

    fn rail_w(&self) -> f64 {
        match self.tabs {
            Some(ModalTabs::Vertical { width, .. }) => width,
            _ => 0.0,
        }
    }

    fn footer_h(&self) -> f64 {
        match self.footer {
            ModalFooter::None => 0.0,
            _ => FOOTER_H,
        }
    }

    /// What the body actually gets: what it asked for, under the ceiling.
    fn viewport_height(&self) -> f64 {
        match self.max_content_height {
            Some(max) => self.content_height.min(max),
            None => self.content_height,
        }
    }

    /// Total frame height: chrome plus whatever the body said it needs.
    fn height(&self) -> f64 {
        self.header_height
            + self.tab_bar_h()
            + self.padding * 2.0
            + self.viewport_height()
            + self.footer_h()
    }
}

/// The geometry the shell settled on. The body draws inside `content`.
#[derive(Clone, Debug, Default)]
pub struct ModalShell {
    pub frame: WidgetRect,
    pub header: WidgetRect,
    /// The VIEWPORT the body draws into — not the height it asked for.
    pub content: WidgetRect,
    /// How much taller the body wanted to be than `content`. Zero means it
    /// fits; anything else is the body's to scroll and clip.
    pub overflow: f64,
    pub tabs: Option<WidgetRect>,
    /// Where each tab landed, by the id the caller gave it.
    pub tab_rects: Vec<(String, WidgetRect)>,
    /// Where the ✕ landed, when the modal has one.
    pub close_btn: Option<WidgetRect>,
    pub footer: Option<WidgetRect>,
    /// Where the footer's template button landed, when there is one. A body
    /// that hangs a menu off it reads this instead of re-deriving the footer
    /// arithmetic and drifting from it.
    pub template_btn: Option<WidgetRect>,
}

/// Draw a modal and hand its body the layer to register on.
///
/// This owns everything that used to be copied into every modal: the layer,
/// the header-drag / background / close registrations, the on-screen clamp,
/// the title, the separators, the tab strip and the footer. A body that uses
/// it never names `ZLayer`, `Icon::Close` or a screen edge, and cannot end up
/// with a header of its own height or an OK button in its own place.
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
    let (shell, layer_id) = begin_modal_shell(ctx, coord, spec, theme, screen);
    body(ctx, coord, &layer_id, &shell);
    coord.pop_layer(&layer_id);
    shell
}

/// Draw the chrome and hand back the geometry and the layer, leaving the body
/// — and the `pop_layer` that closes it — to the caller.
///
/// Prefer [`render_modal_shell`]: its closure makes the pop impossible to
/// forget. This form exists for the settings modals whose bodies run to
/// hundreds of lines, where wrapping the body in a closure would be a
/// reindentation of the whole file for no structural gain. Both draw exactly
/// the same chrome — this IS the implementation of the closure form.
pub fn begin_modal_shell(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    spec: &ModalShellSpec<'_>,
    theme: &ModalTheme,
    screen: (f64, f64),
) -> (ModalShell, LayerId) {
    let (screen_w, screen_h) = screen;
    let height = spec.height();

    // Placement, then the clamp. Anchored surfaces keep a small margin so a
    // gear at the very edge still opens something fully visible.
    let centre = ((screen_w - spec.width) / 2.0, (screen_h - height) / 2.0);
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
    let footer_h = spec.footer_h();
    let footer = (footer_h > 0.0)
        .then(|| WidgetRect::new(x, frame.bottom() - footer_h, spec.width, footer_h));

    let rail_w = spec.rail_w();
    let bar_h = spec.tab_bar_h();
    let tabs_rect = match spec.tabs {
        Some(ModalTabs::Vertical { width, .. }) => Some(WidgetRect::new(
            x,
            header.bottom(),
            width,
            frame.height - spec.header_height - footer_h,
        )),
        Some(ModalTabs::Horizontal { .. }) => {
            Some(WidgetRect::new(x, header.bottom(), spec.width, bar_h))
        }
        None => None,
    };
    let viewport_h = spec.viewport_height();
    let content = WidgetRect::new(
        x + rail_w + spec.padding,
        header.bottom() + bar_h + spec.padding,
        spec.width - rail_w - spec.padding * 2.0,
        viewport_h,
    );
    let shell = ModalShell {
        frame,
        header,
        content,
        overflow: (spec.content_height - viewport_h).max(0.0),
        tabs: tabs_rect,
        tab_rects: Vec::new(),
        close_btn: None,
        footer,
        template_btn: None,
    };
    let mut shell = shell;

    let layer_id = spec.layer.push_named(coord, spec.layer_name.unwrap_or(spec.ns));
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
        let hit = WidgetRect::new(cx - 4.0, cy - 4.0, CLOSE + 8.0, CLOSE + 8.0);
        coord.register_on_layer(close_id.as_str(), hit, Sense::CLICK | Sense::HOVER, &layer_id);
        shell.close_btn = Some(hit);
    }

    hairline(ctx, &theme.header_border, header.x, header.bottom(), header.right());

    if let (Some(tabs), Some(rect)) = (spec.tabs, tabs_rect) {
        shell.tab_rects = draw_tabs(ctx, coord, &layer_id, spec.ns, tabs, rect, theme);
    }

    if let Some(f) = footer {
        hairline(ctx, &theme.footer_border, f.x, f.y, f.right());
        shell.template_btn = draw_footer(ctx, coord, &layer_id, spec, f, theme);
    }

    (shell, layer_id)
}

fn hairline(ctx: &mut dyn RenderContext, color: &str, x0: f64, y: f64, x1: f64) {
    ctx.set_stroke_color(color);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x0, y);
    ctx.line_to(x1, y);
    ctx.stroke();
}

/// The width a `TabFit::Labels` bar needs, insets included. A modal that
/// sizes itself to fit its tabs must ask the same code that lays them out —
/// two copies of this arithmetic is how a tab strip ends up one pixel wider
/// than the modal carrying it.
pub fn labelled_tabs_width(ctx: &mut dyn RenderContext, items: &[ModalTab<'_>]) -> f64 {
    if items.is_empty() {
        return 0.0;
    }
    ctx.set_font(TAB_FONT);
    let labels: f64 = items
        .iter()
        .map(|t| ctx.measure_text(t.label) + TAB_PAD_H * 2.0)
        .sum();
    labels + TAB_GAP * (items.len() - 1) as f64 + TAB_PAD_H * 2.0
}

/// Where each tab sits. Split out because it is the whole difference between
/// the two fits, and because a body that wants to know a tab's rect should
/// ask the same code the shell drew with.
fn tab_rects(
    ctx: &mut dyn RenderContext,
    items: &[ModalTab<'_>],
    rect: WidgetRect,
    vertical: bool,
    fit: TabFit,
) -> Vec<WidgetRect> {
    if vertical {
        return (0..items.len())
            .map(|i| WidgetRect::new(rect.x, rect.y + i as f64 * TAB_ITEM_H, rect.width, TAB_ITEM_H))
            .collect();
    }
    match fit {
        TabFit::Equal => {
            let step = rect.width / items.len() as f64;
            (0..items.len())
                .map(|i| WidgetRect::new(rect.x + i as f64 * step, rect.y, step, rect.height))
                .collect()
        }
        TabFit::Labels => {
            let mut x = rect.x + TAB_PAD_H;
            items
                .iter()
                .map(|t| {
                    let w = ctx.measure_text(t.label) + TAB_PAD_H * 2.0;
                    let r = WidgetRect::new(x, rect.y, w, rect.height);
                    x += w + TAB_GAP;
                    r
                })
                .collect()
        }
    }
}

fn draw_tabs(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    ns: &str,
    tabs: ModalTabs<'_>,
    rect: WidgetRect,
    theme: &ModalTheme,
) -> Vec<(String, WidgetRect)> {
    let (items, active, vertical, fit) = match tabs {
        ModalTabs::Vertical { items, active, .. } => (items, active, true, TabFit::Equal),
        ModalTabs::Horizontal { items, active, fit } => (items, active, false, fit),
    };
    if items.is_empty() {
        return Vec::new();
    }
    let mut placed = Vec::with_capacity(items.len());

    // Measuring needs the font the labels will be drawn in, so it is set once
    // here rather than inside the loop.
    ctx.set_font(TAB_FONT);
    let rects = tab_rects(ctx, items, rect, vertical, fit);

    for (i, (tab, r)) in items.iter().zip(rects).enumerate() {
        let id = format!("{}:tab:{}", ns, tab.id);
        let is_active = i == active;
        let hovered = coord.is_hovered(&WidgetId::from(id.as_str()));

        if is_active {
            ctx.set_fill_color(&theme.accent);
            if vertical {
                ctx.fill_rounded_rect(r.x + 4.0, r.y + 2.0, r.width - 8.0, r.height - 4.0, 4.0);
            } else {
                ctx.fill_rect(r.x, r.bottom() - 2.0, r.width, 2.0);
            }
        } else if hovered {
            ctx.set_fill_color(&theme.item_bg_hover);
            ctx.fill_rect(r.x, r.y, r.width, r.height);
        }

        let tint = if is_active && vertical {
            &theme.accent_text
        } else if is_active {
            &theme.accent
        } else if hovered {
            &theme.header_text
        } else {
            &theme.item_text_muted
        };

        match tab.icon {
            Some(icon) if vertical => {
                const S: f64 = 18.0;
                draw_svg_icon(ctx, icon.svg(), r.center_x() - S / 2.0, r.center_y() - S / 2.0, S, S, tint);
            }
            _ => {
                ctx.set_font(TAB_FONT);
                ctx.set_fill_color(tint);
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(tab.label, r.center_x(), r.center_y());
            }
        }

        coord.register_on_layer(id.as_str(), r, Sense::CLICK | Sense::HOVER, layer);
        placed.push((tab.id.to_string(), r));
    }

    if vertical {
        ctx.set_stroke_color(&theme.header_border);
        ctx.set_stroke_width(1.0);
        ctx.begin_path();
        ctx.move_to(rect.right(), rect.y);
        ctx.line_to(rect.right(), rect.bottom());
        ctx.stroke();
    } else {
        hairline(ctx, &theme.header_border, rect.x, rect.bottom(), rect.right());
    }
    placed
}

fn draw_footer(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer: &LayerId,
    spec: &ModalShellSpec<'_>,
    footer: WidgetRect,
    theme: &ModalTheme,
) -> Option<WidgetRect> {
    let btn_y = footer.center_y() - BTN_H / 2.0;
    let mut template_btn = None;

    // The primary action always lands in the same place: hard against the
    // right edge, where the eye already goes.
    let (id, label, primary) = match spec.footer {
        ModalFooter::None => return None,
        ModalFooter::Editor { template } => {
            if let Some(name) = template {
                let t_id = format!("{}:template", spec.ns);
                let t_w = 150.0;
                let t_r = WidgetRect::new(footer.x + spec.padding, btn_y, t_w, BTN_H);
                let hovered = coord.is_hovered(&WidgetId::from(t_id.as_str()));
                ctx.set_fill_color(if hovered { &theme.item_bg_hover } else { &theme.footer_bg });
                ctx.fill_rounded_rect(t_r.x, t_r.y, t_r.width, t_r.height, 4.0);
                ctx.set_stroke_color(&theme.footer_border);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(t_r.x, t_r.y, t_r.width, t_r.height, 4.0);
                ctx.set_font("12px sans-serif");
                ctx.set_fill_color(&theme.header_text);
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(name, t_r.center_x(), t_r.center_y());
                coord.register_on_layer(t_id.as_str(), t_r, Sense::CLICK | Sense::HOVER, layer);
                template_btn = Some(t_r);
            }
            (format!("{}:ok", spec.ns), "OK".to_string(), true)
        }
        ModalFooter::Creator { label } => (format!("{}:create", spec.ns), label.to_string(), true),
    };

    let w = OK_W.max(label.chars().count() as f64 * 8.0 + 24.0);
    let r = WidgetRect::new(footer.right() - spec.padding - w, btn_y, w, BTN_H);
    let hovered = coord.is_hovered(&WidgetId::from(id.as_str()));
    ctx.set_fill_color(if primary && hovered { &theme.item_bg_hover } else { &theme.accent });
    ctx.fill_rounded_rect(r.x, r.y, r.width, r.height, 4.0);
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&theme.accent_text);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&label, r.center_x(), r.center_y());
    coord.register_on_layer(id.as_str(), r, Sense::CLICK | Sense::HOVER, layer);
    template_btn
}

/// Keep an edge on screen. A surface taller than the screen is pinned to the
/// margin rather than pushed off the top by a negative clamp bound.
fn clamp_on_screen(pos: f64, size: f64, screen: f64, margin: f64) -> f64 {
    pos.max(margin).min((screen - size - margin).max(margin))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Height comes from the body plus the chrome the modal actually asked
    /// for — the reason the old widget could not host a real modal.
    #[test]
    fn height_is_chrome_plus_body() {
        let plain = ModalShellSpec::new("t", "Title", 300.0, 100.0);
        assert_eq!(plain.height(), HEADER_H + 32.0 + 100.0 + FOOTER_H);

        // A rail costs width, not height; a bar costs height.
        let rail = plain.clone().with_tabs(ModalTabs::Vertical {
            width: TAB_RAIL_W,
            items: &[],
            active: 0,
        });
        assert_eq!(rail.height(), plain.height());

        let bar = plain
            .clone()
            .with_tabs(ModalTabs::Horizontal { items: &[], active: 0, fit: TabFit::Equal });
        assert_eq!(bar.height(), plain.height() + TAB_BAR_H);
    }

    /// A body taller than its ceiling does not grow the frame — the excess is
    /// handed back as overflow instead. This is what lets a modal have a fixed
    /// height without the shell carrying a height field.
    #[test]
    fn a_body_over_its_ceiling_becomes_overflow_not_height() {
        let fixed = ModalShellSpec::new("t", "Title", 300.0, 900.0).with_max_content_height(400.0);
        assert_eq!(fixed.viewport_height(), 400.0);
        assert_eq!(fixed.height(), HEADER_H + 32.0 + 400.0 + FOOTER_H);

        // Under the ceiling the modal still sizes to its content.
        let small = ModalShellSpec::new("t", "Title", 300.0, 120.0).with_max_content_height(400.0);
        assert_eq!(small.viewport_height(), 120.0);
    }

    /// The layer name is a routing key elsewhere, so it may differ from the
    /// id namespace — and defaults to it when it does not.
    #[test]
    fn the_layer_name_defaults_to_the_namespace() {
        let s = ModalShellSpec::new("prim_settings", "T", 300.0, 100.0);
        assert_eq!(s.layer_name.unwrap_or(s.ns), "prim_settings");
        let renamed = s.layer_named("primitive_settings");
        assert_eq!(renamed.layer_name.unwrap_or(renamed.ns), "primitive_settings");
    }

    /// A picker is not a dialog: no footer, so no footer height.
    #[test]
    fn a_footerless_surface_costs_no_footer_height() {
        let s = ModalShellSpec::new("t", "Title", 300.0, 100.0).with_footer(ModalFooter::None);
        assert_eq!(s.height(), HEADER_H + 32.0 + 100.0);
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

    /// Anchoring turns dragging off and drops the footer: a surface pinned to
    /// a gear has nowhere to be dragged to and nothing to confirm.
    #[test]
    fn anchoring_makes_a_popover_not_a_dialog() {
        let s = ModalShellSpec::new("t", "Title", 300.0, 100.0).anchored(10.0, 20.0);
        assert!(!s.draggable);
        assert!(matches!(s.footer, ModalFooter::None));
        assert_eq!(s.placement, ModalPlacement::Anchor(10.0, 20.0));
    }
}
