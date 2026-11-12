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
use mlc_shell_fw::frame::{paint_window, Layers};
use mlc_shell_fw::HostedApp;
use uzor_framework::HeadlessHost;
use uzor_render_tiny_skia::TinySkiaCpuRenderContext;

/// The window chrome as the shell keeps it: theme colours and the open tabs.
fn render(h: &mut HeadlessChartApp, state: &mut chrome::ChromeState, w: u32, hgt: u32, now: u64) -> Vec<u8> {
    let mut skia = TinySkiaChartRenderContext::new(w, hgt, 1.0, None, None);
    let layers = if std::env::var_os("MLC_REF_CHROME_ONLY").is_some() { Layers::ChromeOnly } else { Layers::All };
    paint_window(h.inner_mut(), state, &mut skia, w as f64, hgt as f64, now, layers);
    flatten(skia.inner().pixels(), [0x13, 0x17, 0x22])
}

/// The same app, moved into the uzor-framework host (`mlc-shell-fw`) and painted by
/// the framework's headless host onto a plain uzor tiny-skia context: the port's shot
/// of the same state.
fn render_hosted(app: mlc_app::ChartApp, w: u32, hgt: u32) -> Vec<u8> {
    use uzor::render::{Painter, ShapeHelpers};
    let (mut host, _) = HeadlessHost::new(HostedApp::with_dom(app), HostedApp::runtime_config(w, hgt));
    host.advance(0.017);
    host.advance(0.017);
    let win = host.snapshot().windows[0].id;
    let mut ctx = TinySkiaCpuRenderContext::new(w, hgt, 1.0);
    ctx.set_fill_color("#131722");
    ctx.fill_rect(0.0, 0.0, w as f64, hgt as f64);
    host.paint_window(win, 1.0, &mut ctx).expect("window");
    flatten(ctx.pixmap().data(), [0x13, 0x17, 0x22])
}

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
const SCENES: &[&str] = &[
    "main", "chart-settings", "user-settings", "chart-browser", "preset-rename",
    // A right sidebar panel open (`sb-` + the app's persisted panel key).
    "sb-watchlist", "sb-alerts", "sb-object_tree", "sb-signals", "sb-performance", "sb-screener", "sb-events",
];

fn open_scene(app: &mut mlc_app::ChartApp, scene: &str) {
    match scene {
        "chart-settings" => app.open_settings(),
        "user-settings" => app.open_user_settings(),
        "chart-browser" => app.open_chart_browser(),
        // The rename has nothing to rename on a fresh profile (only the default
        // preset): fall back to "Save as", the same modal in its other mode.
        "preset-rename" => {
            app.open_preset_rename();
            if app.panel_app.active_modal != mlc_core::modal::ChartOpenModal::PresetNameInput {
                app.open_preset_save_as();
            }
        }
        s if s.starts_with("sb-") => {
            app.sidebar_state.toggle_right_panel(mlc_app::ChartApp::str_to_panel(&s[3..]));
            app.sidebar_data_dirty = true;
        }
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
            if std::env::var_os("MLC_REF_FW").is_some() {
                let px = render_hosted(h.app, w, hgt);
                let path = out.join(format!("fw-{scene}-{w}x{hgt}.png"));
                save_png(&path, w, hgt, &px).expect("png");
                println!("{}", path.display());
            }
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
