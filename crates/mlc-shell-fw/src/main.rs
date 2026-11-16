//! `mylittlechart-fw`: the existing desktop app in a uzor-framework native window.

fn main() -> Result<(), uzor_framework::FrameworkError> {
    let symbol = std::env::var("MLC_SYMBOL").unwrap_or_else(|_| "BTCUSDT".into());
    let app = mlc_shell_fw::HostedApp::with_dom(mlc_app::ChartApp::new(&symbol));
    let native = uzor_framework::host::native::NativeOptions {
        dev_control: uzor_framework::host::native::DevControlConfig::from_args_env("mylittlechart-fw", "mlc-fw", "MLC_DEV"),
        ..Default::default()
    };
    uzor_framework::host::native::run_native(app, mlc_shell_fw::HostedApp::runtime_config(1280, 800), native)
}
