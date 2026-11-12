//! `mylittlechart-fw`: the existing desktop app in a uzor-framework native window.

fn main() -> Result<(), uzor_framework::FrameworkError> {
    let symbol = std::env::var("MLC_SYMBOL").unwrap_or_else(|_| "BTCUSDT".into());
    let app = mlc_shell_fw::HostedApp::new(mlc_app::ChartApp::new(&symbol));
    uzor_framework::host::native::run_native_default(app, mlc_shell_fw::HostedApp::runtime_config(1280, 800))
}
