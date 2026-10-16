//! Load workspace-root `build.local.env` so `MLC_ORIGIN` is visible to
//! `option_env!` in this crate (invite links). Empty or absent: emit nothing.

use std::fs;
use std::path::PathBuf;

fn main() {
    let manifest_dir = std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default();
    let env_path = PathBuf::from(&manifest_dir).join("../../build.local.env");
    println!("cargo:rerun-if-changed=../../build.local.env");

    let Ok(text) = fs::read_to_string(&env_path) else {
        return;
    };

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, raw_value)) = line.split_once('=') else {
            continue;
        };
        if key.trim() != "MLC_ORIGIN" {
            continue;
        }
        let value = unquote(raw_value.trim());
        let origin = value.trim_end_matches('/');
        if !origin.is_empty() {
            println!("cargo:rustc-env=MLC_ORIGIN={origin}");
        }
    }
}

fn unquote(value: &str) -> &str {
    value
        .strip_prefix('"')
        .and_then(|s| s.strip_suffix('"'))
        .or_else(|| value.strip_prefix('\'').and_then(|s| s.strip_suffix('\'')))
        .unwrap_or(value)
}
