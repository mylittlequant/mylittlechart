//! Build attestation headers — injected into all HTTP requests to protected endpoints.
//!
//! The attestation values are embedded at compile time in the **binary crate**
//! (`chart-app-vello`) by its `build.rs`, then passed into [`crate::start`] as
//! a [`BuildAttestation`] struct.  The updater library crate itself carries no
//! compile-time constants; it receives the values at runtime from the binary.
//!
//! Dev builds (no `RELEASE_SIGNING_KEY` set) produce an empty attestation string.
//! In that case [`attestation_headers`] returns an empty `Vec` — no headers are
//! added, and the server applies its grace-period policy.

use crate::state::BuildAttestation;

/// Build the four `X-Build-*` HTTP headers from a [`BuildAttestation`].
///
/// Returns an empty `Vec` when the attestation signature is empty (dev/unsigned
/// builds), so callers can unconditionally append the result to any request.
pub fn attestation_headers(attest: &BuildAttestation) -> Vec<(String, String)> {
    if attest.attestation.is_empty() {
        return Vec::new();
    }
    vec![
        ("X-Build-Attestation".to_string(), attest.attestation.clone()),
        ("X-Build-Version".to_string(), attest.version.clone()),
        ("X-Build-Platform".to_string(), attest.platform.clone()),
        ("X-Build-Timestamp".to_string(), attest.timestamp.clone()),
    ]
}

/// Attach build attestation headers to a [`reqwest::RequestBuilder`].
///
/// This is the single call-site helper used by [`crate::cloud_sync`] and
/// [`crate::key_sync`].  If attestation is empty the builder is returned
/// unchanged.
pub fn with_attestation(
    builder: reqwest::RequestBuilder,
    attest: &BuildAttestation,
) -> reqwest::RequestBuilder {
    let headers = attestation_headers(attest);
    headers.into_iter().fold(builder, |b, (k, v)| b.header(k, v))
}
