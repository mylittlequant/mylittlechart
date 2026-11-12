//! Reference screenshots of the current desktop app for the uzor-framework port.
//!
//! Renders the real `ChartApp` (headless, through `mlc-app-test-harness`) and the
//! window chrome exactly like the desktop shell's tiny-skia backend
//! (`mlc-app-shell-vello` `render/scene_builder.rs`), at a chosen size, into PNGs.
//! The port's screens are compared against these side by side.
//!
//! ```text
//! mlc-ref-shots <out-dir> [WxH ...] [scene ...]   (default 1280x800, every scene)
//! scenes: main chart-settings user-settings chart-browser preset-rename
//! MLC_REF_SYMBOL=BTCUSDT MLC_REF_EXCHANGE=OKX MLC_REF_WAIT_S=20
//! ```
//!
//! Run it with `HOME` pointing at a scratch directory: the app keeps its profile,
//! presets and caches under the home directory, and a fresh one is the state a new
//! user sees.

use std::path::{Path, PathBuf};
use std::time::Duration;

use mlc_app_input_shell::chrome;
use mlc_app_render_context_tiny_skia::TinySkiaChartRenderContext;
use mlc_app_test_harness::headless::HeadlessChartApp;
use mlc_core::engine::render::RenderContext as MlcRenderContext;

/// The window chrome as the shell keeps it: theme colours and the open tabs.
fn sync_chrome(state: &mut chrome::ChromeState, app: &mlc_app::ChartApp) {
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

/// One frame of the window at `w`×`h`: chrome, chart, modal layer, toasts (the
/// tiny-skia branch of the shell's scene builder, without the startup spinner).
fn render(h: &mut HeadlessChartApp, state: &mut chrome::ChromeState, w: u32, hgt: u32, now: u64) -> Vec<u8> {
    sync_chrome(state, &h.app);
    let app = h.inner_mut();
    let mut skia = TinySkiaChartRenderContext::new(w, hgt, 1.0, None, None);
    let ctx: &mut dyn MlcRenderContext = &mut skia;
    ctx.set_fill_color("#131722");
    ctx.fill_rect(0.0, 0.0, w as f64, hgt as f64);
    // The chart first: its glass effects blur what is above its top edge, so the
    // chrome strip goes over it (the GPU shell keeps the chrome in its own layer).
    let chrome_only = std::env::var_os("MLC_REF_CHROME_ONLY").is_some();
    if !chrome_only {
        ctx.save();
        ctx.translate(0.0, chrome::CHROME_HEIGHT);
        app.render(ctx, now, false);
        ctx.restore();
    }
    ctx.set_fill_color(&state.colors.background);
    ctx.fill_rect(0.0, 0.0, w as f64, chrome::CHROME_HEIGHT);
    chrome::update_tab_widths(ctx, state, w as f64, &app.input_coordinator);
    let skeleton = app.panel_app.user_settings_state.profile_manager_mandatory;
    chrome::render(ctx, state, w as f64, skeleton, &app.input_coordinator);
    if let Some(backdrop) = app.panel_app.dim_backdrop_color() {
        chrome::render_backdrop_dim(ctx, w as f64, state.flavor, &backdrop);
    }
    if chrome_only {
        return flatten(skia.inner().pixels(), [0x13, 0x17, 0x22]);
    }
    ctx.save();
    ctx.translate(0.0, chrome::CHROME_HEIGHT);
    app.render_modal_layer(ctx, now);
    ctx.restore();
    ctx.save();
    ctx.translate(0.0, chrome::CHROME_HEIGHT);
    app.render_toasts(ctx, now);
    ctx.restore();
    flatten(skia.inner().pixels(), [0x13, 0x17, 0x22])
}

/// Premultiplied RGBA → opaque RGBA over `bg`, as the window's own background shows
/// through the chrome's translucent strip.
fn flatten(px: &[u8], bg: [u8; 3]) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len());
    for p in px.chunks_exact(4) {
        let a = p[3] as u32;
        for c in 0..3 {
            out.push((p[c] as u32 + bg[c] as u32 * (255 - a) / 255).min(255) as u8);
        }
        out.push(255);
    }
    out
}

fn save_png(path: &Path, w: u32, h: u32, rgba: &[u8]) -> std::io::Result<()> {
    let f = std::io::BufWriter::new(std::fs::File::create(path)?);
    let mut enc = png::Encoder::new(f, w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut wr = enc.write_header().map_err(std::io::Error::other)?;
    wr.write_image_data(rgba).map_err(std::io::Error::other)?;
    Ok(())
}

fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.split_once(['x', 'X'])?;
    Some((w.trim().parse().ok()?, h.trim().parse().ok()?))
}

/// What the window shows in a reference shot.
const SCENES: &[&str] = &["main", "chart-settings", "user-settings", "chart-browser", "preset-rename"];

fn open_scene(app: &mut mlc_app::ChartApp, scene: &str) {
    match scene {
        "chart-settings" => app.open_settings(),
        "user-settings" => app.open_user_settings(),
        "chart-browser" => app.open_chart_browser(),
        "preset-rename" => app.open_preset_rename(),
        _ => {}
    }
}

fn main() {
    let mut args = std::env::args().skip(1);
    let out = PathBuf::from(args.next().unwrap_or_else(|| "ref-shots".into()));
    let rest: Vec<String> = args.collect();
    let mut sizes: Vec<(u32, u32)> = rest.iter().filter_map(|a| parse_size(a)).collect();
    if sizes.is_empty() {
        sizes.push((1280, 800));
    }
    let scenes: Vec<&str> = {
        let named: Vec<&str> = rest.iter().map(String::as_str).filter(|a| SCENES.contains(a)).collect();
        if named.is_empty() { SCENES.to_vec() } else { named }
    };
    std::fs::create_dir_all(&out).expect("out dir");
    let symbol = std::env::var("MLC_REF_SYMBOL").unwrap_or_else(|_| "BTCUSDT".into());
    // Binance answers 451 from some networks; OKX is the default venue here.
    let exchange = std::env::var("MLC_REF_EXCHANGE").unwrap_or_else(|_| "OKX".into());
    let wait = std::env::var("MLC_REF_WAIT_S").ok().and_then(|s| s.parse().ok()).unwrap_or(20u64);
    for &(w, hgt) in &sizes {
        for scene in &scenes {
            // A fresh app per shot: no modal state carries over.
            let mut h = HeadlessChartApp::new(&symbol);
            let mut state = chrome::ChromeState::new("chart-app-vello");
            let chart_h = (hgt as f64 - chrome::CHROME_HEIGHT).max(1.0) as u32;
            h.resize(w, chart_h);
            h.pump_once();
            if let Some(cid) = h.active_chart_id() {
                h.inner_mut().apply_symbol(cid, &symbol, &exchange, "S");
            }
            let loaded = h.pump_until(|h| h.active_chart_id().is_some_and(|c| h.chart_bar_count(c) > 0), Duration::from_secs(wait));
            open_scene(h.inner_mut(), scene);
            for _ in 0..10 {
                h.pump_once();
            }
            let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_millis() as u64).unwrap_or(0);
            let px = render(&mut h, &mut state, w, hgt, now);
            let path = out.join(format!("ref-{scene}-{w}x{hgt}.png"));
            save_png(&path, w, hgt, &px).expect("png");
            println!("{} bars_loaded={loaded}", path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn sizes_parse() {
        assert_eq!(super::parse_size("1280x800"), Some((1280, 800)));
        assert_eq!(super::parse_size("bad"), None);
    }

    #[test]
    fn flatten_puts_translucent_pixels_over_the_background() {
        // Opaque stays, transparent becomes the background, half is in between.
        let px = [10, 20, 30, 255, 0, 0, 0, 0, 50, 50, 50, 128];
        let out = super::flatten(&px, [100, 100, 100]);
        assert_eq!(&out[..4], &[10, 20, 30, 255]);
        assert_eq!(&out[4..8], &[100, 100, 100, 255]);
        assert_eq!(&out[8..12], &[99, 99, 99, 255]);
    }
}
