fn main() {
    // Windows executable metadata (icon, product name, company, description).
    #[cfg(target_os = "windows")]
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/mascot/icon.ico");
        res.set("ProductName",      "My Little Chart");
        res.set("CompanyName",      "chart.example");
        res.set("FileDescription",  "mylittlechart");
        res.set("LegalCopyright",   "Copyright © 2026 chart.example");
        res.set("OriginalFilename", "mylittlechart.exe");
        res.set("ProductVersion",   &version);
        res.set("FileVersion",      &version);
        res.compile().expect("winresource: failed to compile .rc");
    }
}
