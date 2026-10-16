//! Load workspace-root `build.local.env` into this crate's rustc env.
//!
//! Absent file, or a key whose value is empty: emit nothing. Do not invent
//! an origin or a public key.

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
        let key = key.trim();
        let value = unquote(raw_value.trim());
        if value.is_empty() {
            continue;
        }
        match key {
            "MLC_ORIGIN" => {
                let origin = value.trim_end_matches('/');
                if !origin.is_empty() {
                    println!("cargo:rustc-env=MLC_ORIGIN={origin}");
                }
            }
            "MLC_UPDATE_PUBLIC_KEY" => {
                if is_hex_64(value) {
                    println!("cargo:rustc-env=MLC_UPDATE_PUBLIC_KEY={value}");
                }
            }
            _ => {}
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

fn is_hex_64(value: &str) -> bool {
    let bytes = value.as_bytes();
    bytes.len() == 64 && bytes.iter().all(|b| b.is_ascii_hexdigit())
}
