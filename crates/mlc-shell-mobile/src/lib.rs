//! mylittlechart on a phone: Android enters through `android_main` (GameActivity),
//! iOS through `mlc_mobile_run()` from the Swift host; both run
//! [`mlc_chart_view::ChartViewApp`] through uzor-window-mobile. Russian UI unless the
//! host passes another locale in `MLC_LOCALE`.

#[cfg(not(target_arch = "wasm32"))]
fn make() -> (mlc_chart_view::ChartViewApp, uzor_framework::RuntimeConfig) {
    use mlc_chart_view::{i18n, ChartViewApp};
    let locale = std::env::var("MLC_LOCALE").unwrap_or_default();
    i18n::set_lang(i18n::lang_for_locale(&locale));
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs() as i64).unwrap_or(0);
    #[cfg(feature = "live")]
    let app = ChartViewApp::live(now);
    #[cfg(not(feature = "live"))]
    let app = ChartViewApp::demo(now);
    (app, ChartViewApp::runtime_config())
}

#[cfg(not(target_arch = "wasm32"))]
uzor_window_mobile::entry!(mlc_mobile_run, make);

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    #[test]
    fn one_phone_window() {
        // `make()` itself would open the network source; the window config is the shell's part.
        let cfg = mlc_chart_view::ChartViewApp::runtime_config();
        assert_eq!(cfg.windows.len(), 1);
        let _ = super::make as fn() -> _;
    }
}
