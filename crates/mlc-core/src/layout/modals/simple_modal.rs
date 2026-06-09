//! Simple informational modal renderer.

use crate::engine::render::RenderContext;
use uzor::types::Rect as WidgetRect;
use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::ChartScreenArea;
use crate::ui::widgets::{render_modal, ModalTheme, ModalConfig, ModalSize};

/// Render a simple informational modal centered on screen.
pub fn render_simple_modal(
    ctx: &mut dyn RenderContext,
    screen: ChartScreenArea,
    title: &str,
    message: &str,
    theme: &FrameTheme,
) {
    use crate::render::{TextAlign, TextBaseline};

    let screen_rect = WidgetRect::new(screen.x, screen.y, screen.width, screen.height);

    let config = ModalConfig::confirmation(title)
        .with_size(ModalSize::Custom { width: 400, height: 200 });

    let modal_theme = ModalTheme::from_frame_theme(
        &theme.toolbar_bg,
        &theme.toolbar_border,
        &theme.toolbar_border,
        &theme.toolbar_border,
        &theme.toolbar_border,
    );

    render_modal(ctx, &config, screen_rect, &modal_theme, false, |ctx, content_rect| {
        ctx.set_font("13px sans-serif");
        ctx.set_fill_color(&theme.toolbar_border);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(message, content_rect.x + content_rect.width / 2.0, content_rect.y + content_rect.height / 2.0);
    });
}
