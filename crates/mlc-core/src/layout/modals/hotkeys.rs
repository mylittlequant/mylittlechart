//! Hotkeys reference modal renderer.

use crate::engine::render::RenderContext;
use uzor::types::Rect as WidgetRect;
use crate::layout::render_chart::FrameTheme;
use crate::ui::modal_settings::ChartScreenArea;
use crate::ui::widgets::{render_modal, ModalTheme, ModalConfig, ModalSize};
use crate::i18n::{ModalKey, t_modal};

/// Render hotkeys reference modal.
pub fn render_hotkeys_modal(
    ctx: &mut dyn RenderContext,
    screen: ChartScreenArea,
    theme: &FrameTheme,
) {
    use crate::render::{TextAlign, TextBaseline};

    let screen_rect = WidgetRect::new(screen.x, screen.y, screen.width, screen.height);

    let config = ModalConfig::confirmation(t_modal(ModalKey::KeyboardShortcuts))
        .with_size(ModalSize::Custom { width: 600, height: 500 });

    let modal_theme = ModalTheme::from_frame_theme(
        &theme.toolbar_bg,
        &theme.toolbar_border,
        &theme.toolbar_border,
        &theme.toolbar_border,
        &theme.toolbar_border,
    );

    let hotkeys = [
        ("Ctrl+Z",  t_modal(ModalKey::HkUndo)),
        ("Ctrl+Y",  t_modal(ModalKey::HkRedo)),
        ("Ctrl+S",  t_modal(ModalKey::HkSaveTemplate)),
        ("Del",     t_modal(ModalKey::HkDeleteSelected)),
        ("Esc",     t_modal(ModalKey::HkDeselect)),
        ("Space",   t_modal(ModalKey::HkPlayPause)),
        ("/",       t_modal(ModalKey::HkSearchIndicators)),
        ("Alt+S",   t_modal(ModalKey::HkSymbolSearch)),
        ("Ctrl+C",  t_modal(ModalKey::HkCopy)),
        ("Ctrl+V",  t_modal(ModalKey::HkPaste)),
        ("+/-",     t_modal(ModalKey::HkZoom)),
        ("Scroll",  t_modal(ModalKey::HkPan)),
    ];

    render_modal(ctx, &config, screen_rect, &modal_theme, false, |ctx, content_rect| {
        let row_height = 28.0;
        let col_width = content_rect.width / 2.0;

        for (i, (key, action)) in hotkeys.iter().enumerate() {
            let col = i % 2;
            let row = i / 2;
            let x = content_rect.x + col as f64 * col_width;
            let y = content_rect.y + row as f64 * row_height;

            ctx.set_fill_color(&theme.frame_border);
            ctx.fill_rect(x, y, 80.0, 22.0);
            ctx.set_stroke_color(&theme.toolbar_border);
            ctx.stroke_rect(x, y, 80.0, 22.0);

            ctx.set_font("12px monospace");
            ctx.set_fill_color(&theme.toolbar_border);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(key, x + 40.0, y + 11.0);

            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&theme.toolbar_border);
            ctx.set_text_align(TextAlign::Left);
            ctx.fill_text(action, x + 92.0, y + 11.0);
        }
    });
}
