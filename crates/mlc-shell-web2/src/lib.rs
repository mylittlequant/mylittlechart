//! mylittlechart in the browser: [`mlc_chart_view::ChartViewApp`] on the canvas
//! `#mlc-canvas` through uzor-framework's web host. The UI language follows the
//! browser (Russian by default, English otherwise). With `live` the chart loads
//! Binance BTC/USDT through the data bridge; without it, demo candles.

#[cfg(target_arch = "wasm32")]
mod web {
    use mlc_chart_view::{i18n, ChartViewApp, CANVAS_ID};
    use wasm_bindgen::prelude::*;

    fn now_s() -> i64 {
        (js_sys::Date::now() / 1000.0) as i64
    }

    #[wasm_bindgen(start)]
    pub fn start() {
        console_error_panic_hook::set_once();
        let locale = web_sys::window().and_then(|w| w.navigator().language()).unwrap_or_default();
        i18n::set_lang(i18n::lang_for_locale(&locale));
        #[cfg(feature = "live")]
        let app = ChartViewApp::live(now_s());
        #[cfg(not(feature = "live"))]
        let app = ChartViewApp::demo(now_s());
        #[cfg(feature = "dev-control")]
        let opts = uzor_framework::host::WebOptions::default().dev_control(uzor_framework::host::WebDevControl::new("mylittlechart").alias("__mlcDev"));
        #[cfg(not(feature = "dev-control"))]
        let opts = uzor_framework::host::WebOptions::default();
        let _ = uzor_framework::host::run_web_with(CANVAS_ID, app, ChartViewApp::runtime_config(), opts);
    }
}
