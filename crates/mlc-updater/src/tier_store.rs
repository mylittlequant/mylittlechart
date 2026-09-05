//! Persists the last APPLIED entitlements answer (paid tier), keyed by the
//! identity it was answered for.
//!
//! Read back by `mlc-app-shell-vello/src/main.rs` on the first entitlements
//! answer of a NEW session (`prev_entitlements == None`) so a tier change
//! that landed while the app was closed — a gift code redeemed via the
//! API, a subscription that expired — still produces a Tier system event
//! once, instead of silently becoming the new baseline. Mirrors the web
//! shell's `cloud_session.rs::TIER_KEY` (commit fe955099).
//!
//! File lives next to the auth token (`token_store::token_path`'s app data
//! dir) as its own file, so a token clear and a tier clear are two
//! independent writes — see [`clear_tier`]'s own doc for why both fire
//! together.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Last entitlements answer this device applied, keyed by the user it was
/// answered for — a stored record for a DIFFERENT user (sign-out, then a
/// different sign-in) must never be read back as this session's baseline.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StoredTier {
    pub user_id: i64,
    pub paid: bool,
    #[serde(default)]
    pub paid_until: Option<String>,
}

fn tier_path() -> Option<PathBuf> {
    Some(mlc_core::app_data_dir().join("tier.json"))
}

/// Load the last applied entitlements answer, if any is stored.
pub fn load_tier() -> Option<StoredTier> {
    let path = tier_path()?;
    let data = std::fs::read_to_string(&path).ok()?;
    serde_json::from_str(&data).ok()
}

/// Persist `tier` as the new baseline. Best-effort: a write failure changes
/// nothing else about the caller's flow (same tolerance `token_store`'s own
/// writes get from every call site here).
pub fn save_tier(tier: &StoredTier) {
    if let Some(path) = tier_path() {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(tier) {
            let _ = std::fs::write(&path, json);
        }
    }
}

/// Delete the stored record. Called everywhere `token_store::clear_token`
/// is called (sign-out, a rejected/expired session) so a later sign-in —
/// same identity or a different one — never reads a stale baseline back.
pub fn clear_tier() {
    if let Some(path) = tier_path() {
        let _ = std::fs::remove_file(&path);
    }
}
