//! `mlc-fw`: mylittlechart on uzor-framework in a desktop window (phase 1 of the
//! migration, docs/uzor-framework-migration.md). The old desktop app
//! (`mlc-app-shell-vello`) keeps building next to it.
//!
//! Russian UI unless `LC_ALL` / `LC_MESSAGES` / `LANG` name another language;
//! `MLC_WINDOW=WxH` sizes the window; with `live`, the chart symbols load from the
//! exchanges through mlc's data bridge (demo bars when they do not answer).
//! `--dev` / `MLC_DEV=1` (feature `dev-control`) serves the automation port.

#[cfg(not(target_arch = "wasm32"))]
fn main() -> Result<(), uzor_framework::FrameworkError> {
    use mlc_chart_view::i18n;
    use mlc_desktop::DesktopApp;

    let tag = ["LC_ALL", "LC_MESSAGES", "LANG"].iter().find_map(|k| std::env::var(k).ok().filter(|v| !v.is_empty())).unwrap_or_default();
    i18n::set_lang(i18n::lang_for_locale(&tag));
    let (w, h) = std::env::var("MLC_WINDOW").ok().and_then(|s| parse_size(&s)).unwrap_or((1280, 800));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    let app = make_app(now);
    #[allow(unused_mut)]
    let mut native = uzor_framework::host::native::NativeOptions::default();
    #[cfg(feature = "dev-control")]
    {
        native.dev_control = uzor_framework::host::native::DevControlConfig::from_args_env("mylittlechart", "mlc", "MLC_DEV");
    }
    uzor_framework::host::native::run_native(app, DesktopApp::runtime_config_sized(w, h), native)
}

#[cfg(all(not(target_arch = "wasm32"), not(feature = "live")))]
fn make_app(now: i64) -> mlc_desktop::DesktopApp {
    mlc_desktop::DesktopApp::demo(now)
}

#[cfg(all(not(target_arch = "wasm32"), feature = "live"))]
fn make_app(now: i64) -> mlc_desktop::DesktopApp {
    use mlc_chart_view::source::{DemoSource, Source};
    // Binance BTC/USDT is live; the other symbols show demo bars until the data
    // bridge serves any symbol (phase 2, client core).
    let sources: mlc_desktop::SourceFactory = Box::new(|s: &mlc_desktop::screen::Sym, end: i64| {
        if s.symbol == "BTC/USDT" && s.venue == "Binance" {
            (Box::new(mlc_chart_view::live::LiveSource::new()) as Box<dyn Source>, true)
        } else {
            (Box::new(DemoSource::for_symbol(&s.symbol, &s.venue, end)) as Box<dyn Source>, false)
        }
    });
    mlc_desktop::DesktopApp::new(mlc_desktop::watchlist::Watchlist::default_symbols(), sources, now)
}

/// `1280x800` → `(1280, 800)`; at least 640×400.
fn parse_size(s: &str) -> Option<(u32, u32)> {
    let (w, h) = s.trim().split_once(['x', 'X', '×'])?;
    let (w, h) = (w.trim().parse::<u32>().ok()?, h.trim().parse::<u32>().ok()?);
    Some((w.max(640), h.max(400)))
}

#[cfg(target_arch = "wasm32")]
fn main() {}

#[cfg(test)]
mod tests {
    #[test]
    fn window_size_from_the_environment() {
        assert_eq!(super::parse_size("1440x900"), Some((1440, 900)));
        assert_eq!(super::parse_size(" 800 X 500 "), Some((800, 500)));
        assert_eq!(super::parse_size("100x100"), Some((640, 400)));
        assert_eq!(super::parse_size("big"), None);
    }
}
