//! Native + web shared restore path for "open a chart share link" and
//! "clone a shared chart" — the one place both shells call so a read-only
//! tab (or a cloned one) behaves identically everywhere.
//!
//! Plan: `docs/mlc/plans/native-share-viewer-and-chart-rights-2026-09-06.md`
//! §1/§3/§5. What the web viewer restores today
//! (`mlc-app-shell-web/src/lib.rs`'s "Live-share viewer" boot block) is the
//! same `ChartApp::load_preset` path this module generalizes, so drawings,
//! indicators, panels, and chart type/interval all restore exactly as they
//! do for an ordinary preset — no separate viewer-restore logic to keep in
//! sync with the real one.
//!
//! `ChartApp` tracks read-only tabs BY PRESET ID, never a shell-global
//! flag: native holds several tabs open at once and only the ones opened
//! from a share link are read-only, while the rest of the app stays fully
//! editable.

use mlc_core::preset::preset::{unix_timestamp_parts, ChartPreset, PRESET_VERSION};
use mlc_core::SystemEventKind;

use crate::ChartApp;

/// A preset id. Plain `String` under the hood — matches
/// `mlc_core::preset::preset::ChartPreset::id` and every preset-keyed map in
/// `ProfileManager`/`BackendCore` — named here so share-viewer signatures
/// read as "a preset id", not "some string".
pub type PresetId = String;

/// Metadata describing an opened share link — carried alongside the frozen
/// [`ChartPreset`] so the tab chrome can label the tab with the owner and
/// gate the "Clone" action on `rights`, without a second round trip to the
/// server.
///
/// Constructed by the shell from the server's `GET /api/share/{token}`
/// response (rights v1, plan §2) — deliberately NOT
/// `mlc_cloud::models::SharedChartMeta` itself: `mlc-app` does not depend on
/// `mlc-cloud` (SWC dependency direction is types → engines → backend →
/// app → shells; network transport lives above app), so this is a local
/// mirror, same pattern `mlc_core::preset::preset::PersistedSymbolSource`
/// already uses to avoid a cross-crate dependency.
#[derive(Debug, Clone)]
pub struct SharedChartMeta {
    pub token: String,
    pub title: String,
    pub owner_label: String,
    /// `"live"` or `"screenshot"` — mirrors `SharePublicResponse`'s `kind` tag.
    pub kind: String,
    /// Effective rights on this link, e.g. `["read"]` or `["read", "clone"]`.
    pub rights: Vec<String>,
    /// RFC 3339 instant after which the share stops resolving.
    pub expires_at: String,
}

impl SharedChartMeta {
    /// Whether `right` is in the effective set — e.g. `has_right("clone")`
    /// gates the chrome's "Clone" button.
    pub fn has_right(&self, right: &str) -> bool {
        self.rights.iter().any(|r| r == right)
    }
}

/// One indicator instance a clone dropped because its `type_id` names a
/// paid/private vendor indicator the recipient holds no grant for (plan
/// §3). Local mirror of `mlc_cloud::models::StrippedIndicator` — see
/// [`SharedChartMeta`]'s doc for why `mlc-app` mirrors instead of adding a
/// dependency.
#[derive(Debug, Clone)]
pub struct StrippedIndicator {
    pub type_id: String,
    pub name: String,
}

/// Build a fresh, collision-safe preset id in the same shape
/// `ChartPreset::new` uses (`"{prefix}_{unix_secs}_{nanos_suffix}"`), with a
/// caller-chosen prefix so a shared-viewer tab (`"shared"`) and a cloned tab
/// (`"preset"`, indistinguishable from one the user made by hand) never
/// collide with each other, with an on-disk id, or with the id the preset
/// carried on the wire (which may already belong to a local preset).
fn fresh_preset_id(prefix: &str) -> String {
    let (created_at, suffix) = unix_timestamp_parts();
    format!("{prefix}_{created_at}_{suffix}")
}

impl ChartApp {
    /// Open a shared preset as a new, read-only tab.
    ///
    /// Inserts `preset` under a fresh id, marks that id read-only in
    /// `readonly_preset_ids`, stores `meta` for the tab chrome, and loads it
    /// via [`Self::load_preset`] — the SAME restore path a chrome tab click
    /// takes, so drawings, indicators, panels, chart type/interval and
    /// viewport-persist fields all restore exactly as they do for an
    /// ordinary preset (every hydration path still lands on
    /// `restore_scale_mode = Auto` per the viewport-SWC invariants; this
    /// path adds no exception).
    ///
    /// Errs on a preset-schema version this build cannot restore rather
    /// than materializing a partially-restored tab.
    pub fn open_shared_preset(
        &mut self,
        meta: SharedChartMeta,
        mut preset: ChartPreset,
    ) -> Result<PresetId, String> {
        if preset.version != PRESET_VERSION {
            return Err(format!(
                "shared preset version {} unsupported (this build is on {})",
                preset.version, PRESET_VERSION,
            ));
        }
        // The sharer's own process minted every window/bubble/primitive id
        // this preset carries from ITS OWN counters — remap them all to
        // fresh local ids before this preset ever touches live state, so a
        // numeric collision with the recipient's own ids can't silently
        // alias/overwrite them (owner report M5: "VOLUME drawn twice").
        // See `ChartPreset::remap_ids_for_import`'s own doc for the exact
        // mechanism and what is/isn't in scope.
        preset.remap_ids_for_import();
        let id = fresh_preset_id("shared");
        preset.id = id.clone();
        preset.name = format!("{} — {}", meta.title, meta.owner_label);

        self.readonly_preset_ids.insert(id.clone());
        self.shared_chart_meta.insert(id.clone(), meta);
        self.core.presets_mut().insert(id.clone(), preset);
        self.load_preset(&id);
        Ok(id)
    }

    /// Whether `preset_id` is a read-only shared tab — the check every
    /// mutation site (autosave, drawing/indicator edits, sync-push preset
    /// enumeration) runs before writing. Per-preset, not a shell-global
    /// flag: see the module doc.
    pub fn is_readonly_tab(&self, preset_id: &str) -> bool {
        self.readonly_preset_ids.contains(preset_id)
    }

    /// Shared-chart metadata for a tab, if it is one — the tab chrome reads
    /// this for the owner label and the "Clone" button.
    pub fn shared_chart_meta(&self, preset_id: &str) -> Option<&SharedChartMeta> {
        self.shared_chart_meta.get(preset_id)
    }

    /// Queue an "open this share" request. The shell drains it with
    /// [`Self::take_pending_open_share`], fetches the token's
    /// [`SharedChartMeta`] + frozen [`ChartPreset`] over HTTP (`ChartApp`
    /// does no network IO itself — SWC crate contract), and calls
    /// [`Self::open_shared_preset`] with the result.
    pub fn request_open_share(&mut self, token: &str) {
        self.pending_open_share = Some(token.to_string());
    }

    /// One-shot drain of a pending open-share request.
    pub fn take_pending_open_share(&mut self) -> Option<String> {
        self.pending_open_share.take()
    }

    /// Queue a "clone this share" request. The shell drains it with
    /// [`Self::take_pending_clone_share`], calls
    /// `POST /api/share/{token}/clone`, and hands the response to
    /// [`Self::apply_cloned_preset`].
    pub fn request_clone_share(&mut self, token: &str) {
        self.pending_clone_share = Some(token.to_string());
    }

    /// One-shot drain of a pending clone-share request.
    pub fn take_pending_clone_share(&mut self) -> Option<String> {
        self.pending_clone_share.take()
    }

    /// Apply a server-side clone as an ordinary, WRITABLE preset of the
    /// caller's own — never routed through [`Self::open_shared_preset`]: a
    /// clone is the recipient's own chart from the moment it lands, not a
    /// viewer session. Inserts under a fresh id, loads it, and records a
    /// [`SystemEventKind::Share`] event naming any indicator the server
    /// stripped for a missing license (plan §3), so the recipient sees what
    /// changed instead of silently getting fewer indicators than the
    /// original. Returns the new preset id.
    pub fn apply_cloned_preset(
        &mut self,
        mut preset: ChartPreset,
        stripped: Vec<StrippedIndicator>,
    ) -> PresetId {
        // Same id-collision hazard as `open_shared_preset` — see that
        // method's doc and `ChartPreset::remap_ids_for_import`'s own doc.
        preset.remap_ids_for_import();
        let id = fresh_preset_id("preset");
        preset.id = id.clone();
        self.core.presets_mut().insert(id.clone(), preset);
        self.load_preset(&id);

        let body = if stripped.is_empty() {
            "Cloned with every indicator intact.".to_string()
        } else {
            let names = stripped
                .iter()
                .map(|s| s.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            format!(
                "{} indicator(s) removed (no license): {}",
                stripped.len(),
                names,
            )
        };
        self.push_system_event(SystemEventKind::Share, "Chart cloned", body, true);

        id
    }
}
