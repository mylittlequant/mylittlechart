/// Returns platform identifier for update URLs: "win64", "linux64", "macos-arm64", "macos-x64"
pub fn current_platform() -> &'static str {
    match (std::env::consts::OS, std::env::consts::ARCH) {
        ("windows", _) => "win64",
        ("linux", _) => "linux64",
        ("macos", "aarch64") => "macos-arm64",
        ("macos", _) => "macos-x64",
        _ => "unknown",
    }
}
