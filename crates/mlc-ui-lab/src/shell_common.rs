//! Shared modal-shell setup for every custom body this crate draws. Before
//! the 2026-08-08 port this served both the approved 3-tab design
//! (`crate::approved_design`) and the template management panel
//! (`crate::templates_variant`); the approved design is now the real
//! `render_primitive_settings_modal` in `mlc-app-layout` (which builds its
//! own shell directly), so this module's only remaining caller is the
//! templates panel. It still reuses the exact chrome builders
//! `render_primitive_settings_modal` is built on — header, tab strip,
//! footer — so the frame around a custom body is the real shell, not a
//! drawn approximation of it. Callers hand their own tab strip in
//! (`tab_items`/`active_tab_idx`) rather than this module deriving one from
//! `PrimitiveSettingsTab` — the templates panel has no tab strip at all.
//!
//! This is the ONE place that decides how wide the modal is and where its
//! content box sits (`content_left`/`content_right`) — every row either
//! custom body draws lays out against THIS pair, not a value it recomputes
//! on its own. Before this module existed, each variant's own copy of this
//! preamble grew `modal_width` from the title/tabs/label-column widths
//! alone, with no ceiling against the SECTOR's own width (`screen_w` here
//! is the grid cell `crate::lib::sector_grid` hands the variant, not the
//! full canvas) — on a sector narrower than the ~400px floor the frame
//! simply drew past its own clip, and every row anchored to its right edge
//! went with it. The owner's report of the footer button, the level rows
//! and the opacity label all overrunning the frame were three symptoms of
//! that one missing `.min(screen_w - ...)`. `content_right` also reserves
//! the scrollbar's own width up front (from the SAME `ScrollableConfig`
//! handed to `ScrollableContainer`), so a row drawn flush to it never ends
//! up half under the scrollbar's clip strip once a tall body actually
//! needs to scroll.

use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::layout::render_ui::toolbar_to_widget_theme;
use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::scroll_widget::{ScrollableConfig, ScrollableContainer, ScrollableResult};
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_app_layout::ui::widgets::{
    begin_modal_shell, end_modal_shell, labelled_tabs_width, ModalFooter, ModalPlacement, ModalShell,
    ModalShellSpec, ModalTab, ModalTabs, ModalTheme, TabFit, WidgetTheme,
};
use mlc_app_layout::ui::Icon;
use mlc_core::engine::render::draw_svg_icon;
use mlc_core::i18n::{t_settings, SettingsKey};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::templates::PrimitiveTemplate;
use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::types::Rect as WidgetRect;

/// The body's own inset from the modal frame.
pub const BODY_PADDING: f64 = 12.0;
/// Clear space kept between the modal frame and the sector's own clip edge
/// once `modal_width` is capped against `screen_w`.
const SECTOR_MARGIN: f64 = 8.0;
/// Floor the frame may shrink to on a sector too narrow for the usual
/// 400px minimum — small enough to still host a title and a close button,
/// never zero or negative.
const MIN_MODAL_W: f64 = 200.0;
/// Width of the scrollbar strip `ScrollableContainer` reserves — kept as
/// one constant so `content_right` and the `ScrollableConfig` handed to
/// the container can never disagree about it.
const SCROLLBAR_WIDTH: f64 = 10.0;
const SCROLLBAR_PADDING: f64 = 3.0;
/// How far the top/bottom edge fade reaches into the scrollable content.
const SCROLL_FADE_H: f64 = 16.0;

/// Everything a custom Levels-tab body needs after the shell is up: the
/// shell geometry (for the drag/resize/footer bookkeeping the render loop
/// already does the same way for every variant), the layer to register
/// widgets on, and — the one pair every row must lay out against —
/// `content_left`/`content_right`.
pub struct TabBodyShell {
    pub shell: ModalShell,
    pub layer_id: LayerId,
    pub modal_theme: ModalTheme,
    pub widget_theme: WidgetTheme,
    pub content_left: f64,
    pub content_right: f64,
    pub scrollable: ScrollableContainer,
    pub row_y: f64,
}

/// Build the modal frame for a custom body and open its scrollable content
/// viewport. `content_height` is what the body's own rows add up to (its
/// `rows_height` equivalent) — the shell clamps it to a per-sector ceiling
/// and reports the difference as scroll overflow, exactly like
/// `render_primitive_settings_modal` does for every other tab.
/// `tab_items` may be empty (the templates panel has no tab strip at all).
#[allow(clippy::too_many_arguments)]
pub fn begin_tab_body_shell(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    screen_w: f64,
    screen_h: f64,
    default_modal_y: f64,
    state: &PrimitiveSettingsState,
    title: &str,
    tab_items: &[ModalTab<'_>],
    active_tab_idx: usize,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    content_height: f64,
    result: &mut PrimitiveSettingsResult,
) -> TabBodyShell {
    let label_width = 170.0;
    let close_btn_size = 20.0;
    let title_close_gap = 12.0;

    let tabs_row_width = labelled_tabs_width(ctx, tab_items);

    ctx.set_font("13px sans-serif");
    let title_text = title.to_string();
    let title_width = ctx.measure_text(&title_text);

    let min_header_width = title_width + title_close_gap + close_btn_size + BODY_PADDING * 2.0;
    let min_content_width = label_width + 150.0 + BODY_PADDING * 2.0;
    let modal_width = min_header_width
        .max(tabs_row_width)
        .max(min_content_width)
        .max(400.0)
        .min((screen_w - SECTOR_MARGIN * 2.0).max(MIN_MODAL_W));

    let modal_theme = ModalTheme::from_toolbar(&frame_theme.toolbar_bg, &toolbar_theme.separator, toolbar_theme);
    let max_content_height = (screen_h - 160.0).clamp(80.0, 460.0);

    let mut spec = ModalShellSpec {
        padding: BODY_PADDING,
        placement: ModalPlacement::At(Some(
            state.position.unwrap_or(((screen_w - modal_width) / 2.0, default_modal_y)),
        )),
        ..ModalShellSpec::new("prim_settings", &title_text, modal_width, content_height)
            .with_max_content_height(max_content_height)
            .layer_named("primitive_settings")
            .with_footer(ModalFooter::Editor { template: Some(t_settings(SettingsKey::ButtonTemplate)) })
    };
    if !tab_items.is_empty() {
        spec = spec.with_tabs(ModalTabs::Horizontal { items: tab_items, active: active_tab_idx, fit: TabFit::Labels });
    }

    let (shell, layer_id) = begin_modal_shell(ctx, coord, &spec, &modal_theme, (screen_w, screen_h));

    let modal_x = shell.frame.x;
    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result.tab_rects = shell.tab_rects.clone();
    result.total_content_height = content_height;
    result.viewport_height = shell.content.height;
    result.content_rect = WidgetRect::new(modal_x, shell.content.y, shell.frame.width, shell.content.height);

    let scroll_config = ScrollableConfig {
        scrollbar_width: SCROLLBAR_WIDTH,
        scrollbar_padding: SCROLLBAR_PADDING,
        always_show_scrollbar: false,
    };
    let content_left = modal_x + BODY_PADDING;
    let content_right = modal_x + modal_width - BODY_PADDING - scroll_config.scrollbar_width;
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

    let scrollable = ScrollableContainer::new(shell.content, &state.scroll, Some(scroll_config));
    scrollable.begin(ctx);
    let row_y = scrollable.content_y();

    TabBodyShell { shell, layer_id, modal_theme, widget_theme, content_left, content_right, scrollable, row_y }
}

/// Close the scrollable viewport, register the scrollbar/template-dropdown/
/// content-item hit regions, and draw the footer. Mirrors
/// `render_primitive_settings_modal`'s own tail (same public builders, same
/// widget-id shapes) so `crate::dispatch` — written against those ids, not
/// against that function — drives these variants for free.
#[allow(clippy::too_many_arguments)]
pub fn end_tab_body_shell(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    body: TabBodyShell,
    result: &mut PrimitiveSettingsResult,
    content_height: f64,
    state: &PrimitiveSettingsState,
    templates: &[PrimitiveTemplate],
    toolbar_theme: &ToolbarTheme,
) {
    let TabBodyShell { shell, layer_id, modal_theme, widget_theme, scrollable, .. } = body;
    let viewport = shell.content;
    let scroll_result = scrollable.end(ctx, content_height, &widget_theme);
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;

    if scroll_result.has_scrollbar {
        draw_scroll_boundary(ctx, viewport, &scroll_result, state.scroll.offset, content_height, &modal_theme, toolbar_theme);
    }

    coord.register_on_layer(
        "prim_settings:scroll_viewport",
        uzor::types::Rect::new(viewport.x, viewport.y, viewport.width, viewport.height),
        Sense::SCROLL,
        &layer_id,
    );
    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        coord.register_on_layer("prim_settings:scrollbar_handle", inflated, Sense::DRAG, &layer_id);
    }
    if let Some(ref track_rect) = result.scrollbar_track_rect {
        coord.register_on_layer(
            "prim_settings:scrollbar_track",
            uzor::types::Rect::new(track_rect.x, track_rect.y, track_rect.width, track_rect.height),
            Sense::CLICK,
            &layer_id,
        );
    }

    if let (Some(template_btn), true) = (shell.template_btn, state.template_dropdown_open) {
        draw_template_dropdown(ctx, coord, &layer_id, template_btn, templates, toolbar_theme, result);
    }

    let slider_field_ids: std::collections::HashSet<&str> =
        result.slider_tracks.iter().map(|s| s.field_id.as_str()).collect();
    for (item_id, item_rect) in &result.content_items {
        if item_id == "template_dropdown_menu" {
            continue;
        }
        let sense = if slider_field_ids.contains(item_id.as_str()) { Sense::DRAG | Sense::SCROLL } else { Sense::CLICK | Sense::HOVER };
        coord.register_on_layer(
            format!("prim_settings:item:{item_id}"),
            uzor::types::Rect::new(item_rect.x, item_rect.y, item_rect.width, item_rect.height),
            sense,
            &layer_id,
        );
    }

    let end_spec = ModalShellSpec {
        footer: ModalFooter::Editor { template: Some(t_settings(SettingsKey::ButtonTemplate)) },
        ..ModalShellSpec::new("prim_settings", "", 0.0, 0.0)
    };
    end_modal_shell(ctx, coord, &end_spec, &modal_theme, &shell, &layer_id);
}

/// Make the scrollable body read as bounded. `ScrollableContainer`'s own
/// `draw_scrollbar` (`mlc-app-layout/src/ui/scroll_widget.rs`, out of this
/// crate's scope) paints only the handle, at 50% opacity, with no track
/// underneath it — there is nothing on screen to say where the scrollbar
/// starts or how long it is until the handle itself happens to be
/// scrolled into view. This adds the missing track, plus a fade at
/// whichever edge still has content past it, from data this function
/// already has (`scroll_result`, the live scroll offset).
fn draw_scroll_boundary(
    ctx: &mut dyn RenderContext,
    viewport: WidgetRect,
    scroll_result: &ScrollableResult,
    scroll_offset: f64,
    content_height: f64,
    modal_theme: &ModalTheme,
    toolbar_theme: &ToolbarTheme,
) {
    if let Some(track) = scroll_result.track_rect {
        ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.separator, 0.5));
        ctx.fill_rounded_rect(track.x, track.y, track.width, track.height, track.width / 2.0);
    }

    let fade_h = SCROLL_FADE_H.min(viewport.height / 2.0);
    let bg = modal_theme.background.as_str();
    let transparent = mlc_core::apply_opacity(bg, 0.0);
    let max_scroll = (content_height - viewport.height).max(0.0);

    if scroll_offset > 0.5 {
        ctx.begin_path();
        ctx.rect(viewport.x, viewport.y, viewport.width, fade_h);
        ctx.fill_linear_gradient(&[(0.0, bg), (1.0, transparent.as_str())], viewport.x, viewport.y, viewport.x, viewport.y + fade_h);
    }
    if scroll_offset < max_scroll - 0.5 {
        ctx.begin_path();
        ctx.rect(viewport.x, viewport.bottom() - fade_h, viewport.width, fade_h);
        ctx.fill_linear_gradient(&[(0.0, transparent.as_str()), (1.0, bg)], viewport.x, viewport.bottom() - fade_h, viewport.x, viewport.bottom());
    }
}

/// The footer's template dropdown (Save as… / Apply default / the template
/// list, each with its own delete icon) — pixel-identical to what
/// `render_primitive_settings_modal` draws for baseline, lifted here once
/// so variants B and C do not each carry their own copy.
#[allow(clippy::too_many_arguments)]
fn draw_template_dropdown(
    ctx: &mut dyn RenderContext,
    coord: &mut InputCoordinator,
    layer_id: &LayerId,
    template_btn: WidgetRect,
    templates: &[PrimitiveTemplate],
    toolbar_theme: &ToolbarTheme,
    result: &mut PrimitiveSettingsResult,
) {
    let button_y = template_btn.y;
    let button_height = template_btn.height;
    let template_btn_width = template_btn.width;
    let template_btn_x = template_btn.x;
    let opt_h = 28.0;
    let sep_h = 1.0;
    let delete_btn_w = 24.0;
    let fixed_rows = 2_usize;
    let tmpl_rows = templates.len().max(1);
    let total_h = opt_h * (fixed_rows + tmpl_rows) as f64 + sep_h + 6.0;
    let menu_w = template_btn_width.max(180.0);
    let dd_x = template_btn_x;
    let menu_y = button_y + button_height + 2.0;

    ctx.set_fill_color(&toolbar_theme.dropdown_bg);
    ctx.fill_rounded_rect(dd_x, menu_y, menu_w, total_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(dd_x, menu_y, menu_w, total_h, 4.0);

    coord.register_on_layer(
        "prim_settings:item:template_dropdown_menu".to_string(),
        uzor::types::Rect::new(dd_x, menu_y, menu_w, total_h),
        Sense::CLICK,
        layer_id,
    );

    let mut row_y = menu_y + 3.0;

    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::SaveAsTemplate), dd_x + 8.0, row_y + opt_h / 2.0);
    result.content_items.push(("template_save_as".to_string(), WidgetRect::new(dd_x, row_y, menu_w, opt_h)));
    row_y += opt_h;

    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("11px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(t_settings(SettingsKey::ApplyDefault), dd_x + 8.0, row_y + opt_h / 2.0);
    result.content_items.push(("template_default".to_string(), WidgetRect::new(dd_x, row_y, menu_w, opt_h)));
    row_y += opt_h;

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(dd_x + 4.0, row_y);
    ctx.line_to(dd_x + menu_w - 4.0, row_y);
    ctx.stroke();
    row_y += sep_h + 2.0;

    if templates.is_empty() {
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(t_settings(SettingsKey::NoTemplates), dd_x + 8.0, row_y + opt_h / 2.0);
    } else {
        for tmpl in templates {
            let row_id = format!("template_option:{}", tmpl.id);
            let del_id = format!("template_delete:{}", tmpl.id);

            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&tmpl.name, dd_x + 8.0, row_y + opt_h / 2.0);

            let del_x = dd_x + menu_w - delete_btn_w - 4.0;
            let del_y = row_y + (opt_h - 24.0) / 2.0;
            draw_svg_icon(ctx, Icon::Close.svg(), del_x, del_y, 24.0, 24.0, &toolbar_theme.item_text_muted);

            let name_w = menu_w - delete_btn_w - 8.0;
            result.content_items.push((row_id, WidgetRect::new(dd_x, row_y, name_w, opt_h)));
            result.content_items.push((del_id, WidgetRect::new(del_x, del_y, delete_btn_w, 24.0)));

            row_y += opt_h;
        }
    }

    result.content_items.push(("template_dropdown_menu".to_string(), WidgetRect::new(dd_x, menu_y, menu_w, total_h)));
}
