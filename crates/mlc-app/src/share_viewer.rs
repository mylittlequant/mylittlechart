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
    /// Author publish generation last applied to this tab. `0` if unknown.
    pub revision: u64,
}

impl SharedChartMeta {
    /// Whether `right` is in the effective set — e.g. `has_right("clone")`
    /// gates the chrome's "Clone" button.
    pub fn has_right(&self, right: &str) -> bool {
        self.rights.iter().any(|r| r == right)
    }
}

impl ChartApp {
    /// Flip the clone right on every open tab bound to `token`.
    /// Does not touch the preset. A policy change must not reload the chart.
    pub fn set_share_clone_allowed(&mut self, token: &str, allowed: bool) {
        for meta in self.shared_chart_meta.values_mut() {
            if meta.token != token {
                continue;
            }
            meta.rights.retain(|r| r != "clone");
            if allowed {
                meta.rights.push("clone".to_string());
            }
        }
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

/// One boot-time re-fetch request queued by
/// [`ChartApp::install_shared_tab_placeholders`] — see
/// [`crate::ChartApp`]'s `pending_reopen_shares` field doc. The shell
/// drains these via [`ChartApp::take_pending_reopen_shares`], re-fetches
/// `token` through whatever transport it already uses for a fresh
/// `open_shared_preset`, and reports the outcome back via
/// [`ChartApp::fill_shared_placeholder`] / [`ChartApp::share_reopen_gone`] /
/// [`ChartApp::share_reopen_transient`], naming `placeholder_id` so the
/// outcome lands on the right tab.
#[derive(Debug, Clone)]
pub struct PendingReopenShare {
    pub token: String,
    pub placeholder_id: PresetId,
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
    /// Fork the shared tab that is already open. Copies symbols, primitive
    /// points, indicators and layout into a new writable tab named
    /// `{title} — {owner} (clone)`. Does not fetch the preset again and
    /// does not keep bar series — those reload from the exchange when the
    /// new tab materialises. Returns false when `token` is not an open
    /// shared tab or the caller has no clone right.
    pub fn clone_open_share_locally(&mut self, token: &str) -> bool {
        let Some(id) = self.share_tab_id_for_token(token) else {
            return false;
        };
        let Some(meta) = self.shared_chart_meta(&id).cloned() else {
            return false;
        };
        if !meta.has_right("clone") {
            return false;
        }
        if self.panel_app.active_preset_id == id {
            let name = self
                .core
                .presets()
                .get(&id)
                .map(|p| p.name.clone())
                .unwrap_or_default();
            self.collect_state_into_preset(&id, &name);
        }
        {
            let mut presets = self.core.presets_mut();
            if let Some(preset) = presets.get_mut(&id) {
                for window in &mut preset.windows {
                    window.bars.clear();
                    window.command_history = None;
                    window.stashed_command_history = None;
                }
            }
        }
        let Some(mut preset) = self.core.presets().get(&id).cloned() else {
            return false;
        };
        preset.remap_ids_for_import();
        let new_id = fresh_preset_id("preset");
        preset.id = new_id.clone();
        preset.name = format!("{} — {} (clone)", meta.title, meta.owner_label);
        self.core.presets_mut().insert(new_id.clone(), preset);
        self.load_preset(&new_id);
        true
    }

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

    // =========================================================================
    // Boot-time reopen (shared-tab-reload-anchor arc, 2026-09-23)
    // =========================================================================
    //
    // A shared tab's frozen preset is deliberately never written to disk (see
    // this module's own header doc), so a plain `open_tabs.retain(|id|
    // presets.contains_key(id))` on boot silently drops its id and reflows
    // every tab to its right — owner report: "tabs change places after a
    // reload if one of them was imported from another user." The methods
    // below re-anchor the tab's SLOT instead: a placeholder preset goes in
    // at the saved index (so the tab strip shows something instead of
    // nothing), the shell re-fetches the token through the same transport a
    // fresh share-open already uses, and the outcome lands back on that
    // exact placeholder id — never a fresh append.

    /// Mint a fresh, bubble-less placeholder preset for a shared tab whose
    /// real content isn't on hand yet, insert it under a fresh id (so the
    /// tab strip's `presets.get(id)` lookup shows something instead of
    /// silently vanishing), mark it read-only exactly like a fully-opened
    /// share, and queue `token` for the shell to re-fetch via
    /// [`Self::take_pending_reopen_shares`]. Returns the placeholder's id.
    fn queue_shared_tab_placeholder(&mut self, token: &str) -> PresetId {
        let id = fresh_preset_id("shared");
        let mut placeholder = ChartPreset::new("Loading shared chart…".to_string());
        placeholder.id = id.clone();
        self.readonly_preset_ids.insert(id.clone());
        self.core.presets_mut().insert(id.clone(), placeholder);
        self.pending_share_tokens.insert(id.clone(), token.to_string());
        self.pending_reopen_shares.push(PendingReopenShare {
            token: token.to_string(),
            placeholder_id: id.clone(),
        });
        id
    }

    /// Re-anchor every entry of `shared` into `open_tabs` at its saved
    /// index, replacing whatever the caller's own dead-id filter already
    /// dropped. `open_tabs` must already be filtered down to ids that
    /// survive that filter (`presets.contains_key`) BEFORE this runs — see
    /// [`reinsert_at_saved_indices`]'s own doc for why that ordering
    /// matters and is proven separately.
    pub(crate) fn install_shared_tab_placeholders(
        &mut self,
        shared: &[mlc_core::user_profile::profile::PersistedSharedTab],
    ) -> Option<PresetId> {
        let mut active: Option<PresetId> = None;
        let entries: Vec<(usize, PresetId)> = shared
            .iter()
            .map(|entry| {
                let id = self.queue_shared_tab_placeholder(&entry.token);
                if entry.was_active {
                    active = Some(id.clone());
                }
                (entry.index, id)
            })
            .collect();
        reinsert_at_saved_indices(&mut self.panel_app.open_tabs, &entries);
        active
    }

    /// Id of the tab already bound to `token`, if this window has one.
    /// The token lives on the tab (`shared_chart_meta` once filled,
    /// `pending_share_tokens` while it is still the boot placeholder).
    pub fn share_tab_id_for_token(&self, token: &str) -> Option<String> {
        self.shared_chart_meta
            .iter()
            .find(|(_, meta)| meta.token == token)
            .map(|(id, _)| id.clone())
            .or_else(|| {
                self.pending_share_tokens
                    .iter()
                    .find(|(_, t)| t.as_str() == token)
                    .map(|(id, _)| id.clone())
            })
    }

    /// One-shot drain of the boot-time reopen queue — see
    /// `ChartApp::pending_reopen_shares`'s own doc.
    pub fn take_pending_reopen_shares(&mut self) -> Vec<PendingReopenShare> {
        std::mem::take(&mut self.pending_reopen_shares)
    }

    /// Fill a placeholder installed by [`Self::install_shared_tab_placeholders`]
    /// with the real, fetched preset. Same restore shape `open_shared_preset`
    /// takes (version check, id remap, read-only marking, meta) but keeps
    /// the placeholder's OWN id — already anchored in `open_tabs` at the
    /// right slot — instead of minting a new one, so the tab strip needs no
    /// further reconciliation. Materializes the chart live only when
    /// `placeholder_id` is this window's currently active tab (the tab the
    /// user is staring at, which would otherwise sit on empty placeholder
    /// content); otherwise the ordinary click-to-load cold path picks up
    /// the now-real preset the next time the tab is clicked, exactly like
    /// any other closed background tab.
    pub fn fill_shared_placeholder(
        &mut self,
        placeholder_id: &str,
        mut meta: SharedChartMeta,
        mut preset: ChartPreset,
    ) -> Result<(), String> {
        if preset.version != PRESET_VERSION {
            return Err(format!(
                "shared preset version {} unsupported (this build is on {})",
                preset.version, PRESET_VERSION,
            ));
        }
        preset.remap_ids_for_import();
        preset.id = placeholder_id.to_string();
        preset.name = format!("{} — {}", meta.title, meta.owner_label);

        // A boot-time re-fetch (`UpdaterCommand::ReopenShare`) doesn't
        // round-trip the token through its outcome (it would be redundant —
        // the shell sent it in the request) — recover it from
        // `pending_share_tokens`, which `queue_shared_tab_placeholder` set
        // at boot and nothing else has touched since. A fresh
        // `open_shared_preset` never reaches this method, so `meta.token`
        // (set by the caller there) is the only other source and is left
        // untouched when there is no pending entry.
        if let Some(token) = self.pending_share_tokens.remove(placeholder_id) {
            meta.token = token;
        }

        self.readonly_preset_ids.insert(placeholder_id.to_string());
        self.shared_chart_meta.insert(placeholder_id.to_string(), meta);
        self.core.presets_mut().insert(placeholder_id.to_string(), preset);
        if self.panel_app.active_preset_id == placeholder_id {
            self.load_preset(placeholder_id);
        }
        Ok(())
    }

    /// Drop a placeholder tab from `open_tabs`/`presets`/
    /// `readonly_preset_ids`, falling back the active tab exactly like an
    /// ordinary `CloseTab` when it was the one showing. Shared by
    /// [`Self::share_reopen_gone`] and [`Self::share_reopen_transient`] —
    /// see their own docs for what happens to the token afterward.
    fn remove_shared_placeholder_tab(&mut self, placeholder_id: &str) {
        self.panel_app.open_tabs.retain(|t| t != placeholder_id);
        self.readonly_preset_ids.remove(placeholder_id);
        self.shared_chart_meta.remove(placeholder_id);
        self.pending_share_tokens.remove(placeholder_id);
        self.core.presets_mut().remove(placeholder_id);
        if self.panel_app.active_preset_id == placeholder_id {
            if let Some(next) = self.panel_app.open_tabs.last().cloned() {
                self.process_chart_out_event(mlc_core::events::ChartOutEvent::LoadPreset { id: next });
            } else {
                self.panel_app.active_preset_id = "__default__".to_string();
            }
        }
        self.persist_profile();
    }

    /// The re-fetch for `placeholder_id` came back GONE — 404/410/403
    /// (unknown, expired, or revoked token, or access withdrawn — the
    /// server bundles all three into one bare 404 by design, see
    /// `mlc-updater/src/share.rs::error_message`'s own doc). Drops the
    /// placeholder for good: no reorder of the remaining tabs beyond
    /// closing that one slot, one toast through the existing system-event/
    /// toast path, and the entry never comes back in `open_shared_tabs` on
    /// the next save (it is no longer in `open_tabs` for
    /// `collect_open_shared_tabs` to find).
    pub fn share_reopen_gone(&mut self, placeholder_id: &str) {
        self.remove_shared_placeholder_tab(placeholder_id);
        self.push_system_event(
            SystemEventKind::Share,
            mlc_core::i18n::t_share_viewer(mlc_core::i18n::ShareViewerKey::SharedChartGoneTitle),
            mlc_core::i18n::t_share_viewer(mlc_core::i18n::ShareViewerKey::SharedChartGoneBody),
            true,
        );
    }

    /// The re-fetch for `placeholder_id` failed TRANSIENTLY — offline or a
    /// 5xx, not a verdict on the share itself. Drops the placeholder for
    /// the REST OF THIS SESSION (no in-session retry loop) but keeps
    /// `token` + the tab's current index in `deferred_share_reopens`, which
    /// `collect_open_shared_tabs` folds into the next save's
    /// `open_shared_tabs` regardless of `open_tabs` membership — so the
    /// NEXT boot's `install_shared_tab_placeholders` tries again. No toast:
    /// this is not "gone," just "couldn't reach it right now."
    pub fn share_reopen_transient(&mut self, placeholder_id: &str, token: &str) {
        if let Some(index) = self.panel_app.open_tabs.iter().position(|t| t == placeholder_id) {
            self.deferred_share_reopens.push(mlc_core::user_profile::profile::PersistedSharedTab {
                token: token.to_string(),
                window_id: self.window_id.clone(),
                index,
                was_active: false,
            });
        }
        self.remove_shared_placeholder_tab(placeholder_id);
    }

    /// Whether `preset_id` is currently open as a tab in this window — used
    /// by shells to route a re-fetch outcome to the window that actually
    /// holds the matching placeholder (presets live in the shared
    /// `BackendCore`, but `open_tabs`/`readonly_preset_ids` are per-window
    /// `ChartApp` fields).
    pub fn has_open_tab(&self, preset_id: &str) -> bool {
        self.panel_app.open_tabs.iter().any(|t| t == preset_id)
    }

    /// Build the persisted list of currently-open shared (read-only) tabs
    /// — used by both `build_user_profile` (legacy top-level mirror) and
    /// `build_window_state` (per-window) so a reload can re-anchor them
    /// instead of silently dropping their ids (item 1). Includes both
    /// fully-resolved shared tabs (`shared_chart_meta`) and still-pending
    /// placeholders queued this session but not yet fetched
    /// (`pending_share_tokens` — their token isn't in `shared_chart_meta`
    /// yet, so nothing else remembers it), plus carries forward
    /// `deferred_share_reopens` — placeholders this session gave up on
    /// after a transient re-fetch failure, kept for the NEXT boot to retry
    /// even though their placeholder tab is already gone from `open_tabs`
    /// (item 2).
    pub(crate) fn collect_open_shared_tabs(
        &self,
    ) -> Vec<mlc_core::user_profile::profile::PersistedSharedTab> {
        let mut out: Vec<mlc_core::user_profile::profile::PersistedSharedTab> = self
            .panel_app
            .open_tabs
            .iter()
            .enumerate()
            .filter(|(_, id)| self.is_readonly_tab(id))
            .filter_map(|(index, id)| {
                let token = self
                    .shared_chart_meta
                    .get(id)
                    .map(|meta| meta.token.clone())
                    .or_else(|| self.pending_share_tokens.get(id).cloned())?;
                Some(mlc_core::user_profile::profile::PersistedSharedTab {
                    token,
                    window_id: self.window_id.clone(),
                    index,
                    was_active: id == &self.panel_app.active_preset_id,
                })
            })
            .collect();
        out.extend(self.deferred_share_reopens.iter().cloned());
        out
    }
}

/// Re-insert `entries` (each an ORIGINAL, pre-drop `(index, id)` pair) into
/// `open_tabs` in ascending-index order, reproducing the exact order the
/// list had before every entry's stale id was dropped by a dead-id
/// `retain`. Each `Vec::insert` shifts only the entries after its own
/// index, which have not been processed yet (they're still ahead in
/// `entries`, sorted) — so processing low-to-high undoes the retain's
/// left-shift exactly, even for several shared tabs at once. Split out of
/// [`ChartApp::install_shared_tab_placeholders`] as a free, `ChartApp`-free
/// function so this ordering invariant is unit-testable on its own (see
/// `tests` below) without constructing a `ChartApp`/`BackendCore`.
fn reinsert_at_saved_indices(open_tabs: &mut Vec<PresetId>, entries: &[(usize, PresetId)]) {
    let mut ordered: Vec<&(usize, PresetId)> = entries.iter().collect();
    ordered.sort_by_key(|(index, _)| *index);
    for (index, id) in ordered {
        let at = (*index).min(open_tabs.len());
        open_tabs.insert(at, id.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(text: &str) -> String {
        text.to_string()
    }

    #[test]
    fn reinsert_single_slot_restores_original_order() {
        // Saved order was [A, B, shared, C] (shared@index 2); the dead-id
        // retain already dropped it, leaving [A, B, C].
        let mut open_tabs = vec![s("A"), s("B"), s("C")];
        reinsert_at_saved_indices(&mut open_tabs, &[(2, s("ph1"))]);
        assert_eq!(open_tabs, vec![s("A"), s("B"), s("ph1"), s("C")]);
    }

    #[test]
    fn reinsert_multiple_slots_restores_original_order() {
        // Saved order was [A, B, shared1, C, shared2, D]
        // (shared1@2, shared2@4); retain dropped both, leaving [A, B, C, D].
        let mut open_tabs = vec![s("A"), s("B"), s("C"), s("D")];
        reinsert_at_saved_indices(
            &mut open_tabs,
            &[(4, s("ph2")), (2, s("ph1"))], // deliberately out of order
        );
        assert_eq!(
            open_tabs,
            vec![s("A"), s("B"), s("ph1"), s("C"), s("ph2"), s("D")]
        );
    }

    #[test]
    fn reinsert_adjacent_slots_keep_their_own_relative_order() {
        // Two shared tabs saved back-to-back at indices 1 and 2.
        let mut open_tabs = vec![s("A"), s("D")];
        reinsert_at_saved_indices(&mut open_tabs, &[(1, s("ph1")), (2, s("ph2"))]);
        assert_eq!(open_tabs, vec![s("A"), s("ph1"), s("ph2"), s("D")]);
    }

    #[test]
    fn reinsert_index_past_end_clamps_to_append() {
        // A saved index beyond the current (shorter) list length — e.g. the
        // shared tab was last in a strip whose other tabs also went away —
        // appends instead of panicking.
        let mut open_tabs = vec![s("A")];
        reinsert_at_saved_indices(&mut open_tabs, &[(9, s("ph1"))]);
        assert_eq!(open_tabs, vec![s("A"), s("ph1")]);
    }

    #[test]
    fn reinsert_empty_entries_leaves_open_tabs_untouched() {
        let mut open_tabs = vec![s("A"), s("B")];
        reinsert_at_saved_indices(&mut open_tabs, &[]);
        assert_eq!(open_tabs, vec![s("A"), s("B")]);
    }
}
