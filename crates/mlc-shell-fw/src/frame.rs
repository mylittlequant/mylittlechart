//! One window frame of the existing app: chrome strip, chart, modal layer, toasts.
//!
//! The paint order of the desktop shell's tiny-skia branch
//! (`mlc-app-shell-vello` `render/scene_builder.rs`), except that the chart goes
//! before the chrome strip: its glass effects blur what lies above its top edge, and
//! the GPU shell keeps the chrome in a layer of its own. Shared by the
//! uzor-framework host ([`crate::HostedApp`]) and the reference shots
//! (`mlc-ref-shots`), so the two paint the very same calls.

use mlc_app_input_shell::chrome;
use mlc_core::engine::render::RenderContext as MlcRenderContext;

/// The window chrome as the shell keeps it: theme colours and the open tabs.
pub fn sync_chrome(state: &mut chrome::ChromeState, app: &mlc_app::ChartApp) {
    let theme = app.panel_app.theme_manager.current();
    let c = &mut state.colors;
    c.background = theme.chart.background.clone();
    c.icon_normal = theme.colors.text_primary.clone();
    c.icon_hover = theme.colors.accent.clone();
    c.button_hover = theme.colors.button_bg_hover.clone();
    c.close_hover = theme.colors.danger.clone();
    c.separator = theme.colors.toolbar_divider.clone();
    c.tab_accent = theme.colors.accent.clone();
    c.cta_bg = theme.colors.accent.clone();
    c.cta_bg_hover = theme.colors.accent_hover.clone();
    c.tooltip_bg = theme.colors.button_bg.clone();
    c.tooltip_text = theme.colors.text_primary.clone();
    let active = &app.panel_app.active_preset_id;
    let presets = app.core.presets();
    let mut tabs: Vec<chrome::Tab> = app
        .panel_app
        .open_tabs
        .iter()
        .filter_map(|id| presets.get(id).map(|p| chrome::Tab { id: p.id.clone(), name: p.name.clone(), active: &p.id == active }))
        .collect();
    drop(presets);
    for t in &mut tabs {
        let base = t.name.clone();
        t.name = app.tab_display_name(&t.id, &base);
    }
    if tabs.is_empty() {
        tabs.push(chrome::Tab { id: "__fallback__".into(), name: "Untitled".into(), active: true });
    } else if !tabs.iter().any(|t| t.active) {
        tabs[0].active = true;
    }
    state.tabs = tabs;
}

/// What a frame paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Layers {
    /// The whole window.
    All,
    /// Only the chrome strip (to inspect it alone).
    ChromeOnly,
}

/// Paint one frame of `app` at `w`×`h` into `ctx`.
pub fn paint_window(app: &mut mlc_app::ChartApp, state: &mut chrome::ChromeState, ctx: &mut dyn MlcRenderContext, w: f64, h: f64, now: u64, layers: Layers) {
    sync_chrome(state, app);
    ctx.set_fill_color("#131722");
    ctx.fill_rect(0.0, 0.0, w, h);
    if layers == Layers::All {
        ctx.save();
        ctx.translate(0.0, chrome::CHROME_HEIGHT);
        app.render(ctx, now, false);
        ctx.restore();
    }
    ctx.set_fill_color(&state.colors.background);
    ctx.fill_rect(0.0, 0.0, w, chrome::CHROME_HEIGHT);
    chrome::update_tab_widths(ctx, state, w, &app.input_coordinator);
    let skeleton = app.panel_app.user_settings_state.profile_manager_mandatory;
    chrome::render(ctx, state, w, skeleton, &app.input_coordinator);
    if let Some(backdrop) = app.panel_app.dim_backdrop_color() {
        chrome::render_backdrop_dim(ctx, w, state.flavor, &backdrop);
    }
    if layers == Layers::ChromeOnly {
        return;
    }
    ctx.save();
    ctx.translate(0.0, chrome::CHROME_HEIGHT);
    app.render_modal_layer(ctx, now);
    ctx.restore();
    ctx.save();
    ctx.translate(0.0, chrome::CHROME_HEIGHT);
    app.render_toasts(ctx, now);
    ctx.restore();
}

/// One step of the app's clock, as the desktop shell and the headless harness run it:
/// tick, lay out at the chart size, run the deferred preset work.
pub fn pump(app: &mut mlc_app::ChartApp, now_ms: u64, w: f64, chart_h: f64) {
    app.tick(now_ms);
    app.prepare_frame(w, chart_h);
    app.drain_pending_cold_apply();
    app.drain_pending_cure_after_preset_load();
}
