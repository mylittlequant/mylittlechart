use std::time::{SystemTime, UNIX_EPOCH};

fn main() {
    // Build timestamp — always set so env!("BUILD_TIMESTAMP") is always valid.
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();
    println!("cargo:rustc-env=BUILD_TIMESTAMP={}", timestamp);

    // Platform — prefer the injected CI value, fall back to Cargo's target OS.
    let platform = std::env::var("BUILD_PLATFORM")
        .unwrap_or_else(|_| {
            std::env::var("CARGO_CFG_TARGET_OS").unwrap_or_else(|_| "unknown".to_string())
        });
    println!("cargo:rustc-env=BUILD_PLATFORM={}", platform);

    // Build attestation — only when RELEASE_SIGNING_KEY is available (CI builds).
    match std::env::var("RELEASE_SIGNING_KEY") {
        Ok(key_b64) if !key_b64.is_empty() => {
            // Canonical message: BUILD_ATTESTATION_V1:{version}:{timestamp}:{platform}
            let version = std::env::var("CARGO_PKG_VERSION").unwrap();
            let message = format!("BUILD_ATTESTATION_V1:{}:{}:{}", version, timestamp, platform);

            match sign_attestation(&key_b64, message.as_bytes()) {
                Ok(sig_b64) => {
                    println!("cargo:rustc-env=BUILD_ATTESTATION={}", sig_b64);
                    eprintln!("build.rs: Build attestation signed for v{}", version);
                }
                Err(e) => {
                    eprintln!("build.rs: WARNING: Failed to sign attestation: {}", e);
                    println!("cargo:rustc-env=BUILD_ATTESTATION=");
                }
            }
        }
        _ => {
            // Dev build — no signing key available, produce empty attestation.
            println!("cargo:rustc-env=BUILD_ATTESTATION=");
        }
    }

    // Ensure cargo reruns this script if the env vars change.
    println!("cargo:rerun-if-env-changed=RELEASE_SIGNING_KEY");
    println!("cargo:rerun-if-env-changed=BUILD_PLATFORM");

    // Windows executable metadata (icon, product name, company, description).
    #[cfg(target_os = "windows")]
    {
        let version = std::env::var("CARGO_PKG_VERSION").unwrap_or_else(|_| "0.0.0".into());
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/mascot/icon.ico");
        res.set("ProductName",      "My Little Chart");
        res.set("CompanyName",      "chart.example");
        res.set("FileDescription",  "My Little Chart — Trading Terminal");
        res.set("LegalCopyright",   "Copyright © 2026 chart.example");
        res.set("OriginalFilename", "mylittlechart.exe");
        res.set("ProductVersion",   &version);
        res.set("FileVersion",      &version);
        res.compile().expect("winresource: failed to compile .rc");
    }
}

fn sign_attestation(key_b64: &str, message: &[u8]) -> Result<String, Box<dyn std::error::Error>> {
    use base64::Engine;
    use base64::engine::general_purpose::STANDARD;
    use ed25519_dalek::{Signer, SigningKey};

    let key_bytes = STANDARD.decode(key_b64)?;
    if key_bytes.len() != 32 {
        return Err(format!("Expected 32-byte signing key, got {} bytes", key_bytes.len()).into());
    }
    let mut key_arr = [0u8; 32];
    key_arr.copy_from_slice(&key_bytes);

    let signing_key = SigningKey::from_bytes(&key_arr);
    let signature = signing_key.sign(message);

    Ok(STANDARD.encode(signature.to_bytes()))
}
