//! Cross-window shared application state.

/// Application-level shared state — single source of truth for data that is
/// shared across all windows (watchlist, connector preferences).
///
/// These fields were previously duplicated inside each `ChartApp`. Moving them
/// here means there is one authoritative copy; per-window copies are synced
/// from/to `AppState` each frame via `sync_app_state_to_window` /
/// `sync_app_state_from_window`.
pub(crate) struct AppState {
    /// Watchlist manager — all lists, groups, and symbols.
    pub(crate) watchlist_manager: mlc_app::WatchlistManager,
    /// Per-exchange enabled/disabled flag (keyed by `ExchangeId::as_str()`).
    pub(crate) connector_enabled: std::collections::HashMap<String, bool>,
    /// All chart presets loaded at startup (keyed by preset id).
    pub(crate) presets: std::collections::HashMap<String, mlc_core::preset::preset::ChartPreset>,
    /// Preset ids that have been modified but not yet persisted.
    pub(crate) preset_dirty_ids: std::collections::HashSet<String>,
    /// Settings snapshots — shared across all windows (last-used settings per category).
    pub(crate) snapshots: mlc_app::user_manager::manager::SettingsSnapshots,
    /// Template manager — single source of truth for all template types across windows.
    pub(crate) template_manager: mlc_core::templates::manager::TemplateManager,
    /// Active theme preset name (e.g. "dark", "light").
    /// Single source of truth — synced to all windows each frame.
    pub(crate) theme_preset: String,
    /// Device identity — read-only after startup, shared to avoid stale per-window copies.
    pub(crate) device_name: String,
    pub(crate) app_version: String,

    // ── Sync dirty flags ──────────────────────────────────────────────────────
    // Set to `true` when the corresponding data changes; reset after syncing to
    // all windows.  Prevents per-frame deep clones when nothing changed.
    /// Presets map changed — need to clone to all windows.
    pub(crate) presets_dirty: bool,
    /// Template manager changed — need to clone to all windows.
    pub(crate) templates_dirty: bool,
    /// Settings snapshots changed — need to clone to all windows.
    pub(crate) snapshots_dirty: bool,
    /// Watchlist manager changed — need to clone to all windows.
    pub(crate) watchlists_dirty: bool,
    /// Connector enabled map changed — need to clone to all windows.
    pub(crate) connectors_dirty: bool,

    // ── Performance settings ──────────────────────────────────────────────────

    /// Indicator recalculation mode — controls CPU/accuracy trade-off.
    /// Synced to every window's `indicator_manager.recalc_mode` each frame.
    pub(crate) recalc_mode: mlc_app::RecalcMode,

    /// User's preferred price scale mode (Auto / Focus / Manual).
    /// Applied as the default when windows load bars for the first time.
    pub(crate) scale_mode: mlc_core::ScaleMode,

    // ── Agent API server settings ────────────────────────────────────────────
    /// Whether the server is enabled.
    pub(crate) server_enabled: bool,
    /// Port the server listens on.
    pub(crate) server_port: u16,

    /// Encryption key for zero-trust storage. Derived from passphrase at startup.
    /// `None` during migration or when running without a passphrase (plaintext mode).
    pub(crate) vault_key: Option<mlc_vault::vault::VaultKey>,
}

impl AppState {
    /// Initialise from a loaded watchlist file and user profile.
    pub(crate) fn from_profile(
        profile: &mlc_core::UserProfile,
        presets: std::collections::HashMap<String, mlc_core::preset::preset::ChartPreset>,
        snapshots: mlc_app::user_manager::manager::SettingsSnapshots,
        template_manager: mlc_core::templates::manager::TemplateManager,
        vault_key: Option<mlc_vault::vault::VaultKey>,
    ) -> Self {
        let default_wl = || {
            mlc_app::WatchlistManager::new(vec![
                mlc_app::InstrumentAddress::new("binance", "BTCUSDT", mlc_core::instrument::AccountType::Spot),
                mlc_app::InstrumentAddress::new("binance", "ETHUSDT", mlc_core::instrument::AccountType::Spot),
                mlc_app::InstrumentAddress::new("binance", "SOLUSDT", mlc_core::instrument::AccountType::Spot),
                mlc_app::InstrumentAddress::new("binance", "BNBUSDT", mlc_core::instrument::AccountType::Spot),
                mlc_app::InstrumentAddress::new("bybit", "BTCUSDT", mlc_core::instrument::AccountType::Spot),
                mlc_app::InstrumentAddress::new("okx", "BTCUSDT", mlc_core::instrument::AccountType::Spot),
            ])
        };

        let watchlist_manager = {
            let watchlists_path = mlc_core::user_profile::storage::watchlists_path();
            if watchlists_path.exists() {
                // Watchlists are always plaintext — pass None regardless of vault key.
                mlc_core::load_json::<mlc_app::WatchlistManager>(&watchlists_path, None)
                    .unwrap_or_else(|e| {
                        eprintln!("[AppState] Failed to load watchlists: {}", e);
                        default_wl()
                    })
            } else {
                default_wl()
            }
        };

        // Parse recalc_mode from DeviceSettings (authoritative) with profile as fallback.
        let recalc_mode = {
            let ds = mlc_core::user_profile::DeviceSettings::load();
            let src = if ds.recalc_mode == "per_frame" && !profile.recalc_mode.is_empty() {
                // DeviceSettings has the default value — use the profile value if it
                // carries a non-default mode so that existing saves are honoured.
                &profile.recalc_mode
            } else {
                &ds.recalc_mode
            };
            match src.as_str() {
                "PerTick" => mlc_app::RecalcMode::PerTick,
                "PerBar"  => mlc_app::RecalcMode::PerBar,
                _         => mlc_app::RecalcMode::PerFrame,
            }
        };

        // Parse scale_mode from the profile string.
        let scale_mode = match profile.scale_mode.as_str() {
            "Focus"  => mlc_core::ScaleMode::Focus,
            "Manual" => mlc_core::ScaleMode::Manual,
            _        => mlc_core::ScaleMode::Auto, // default / "Auto"
        };

        Self {
            watchlist_manager,
            connector_enabled: profile.connector_enabled.clone(),
            presets,
            preset_dirty_ids: std::collections::HashSet::new(),
            snapshots,
            template_manager,
            theme_preset: profile.active_theme.clone(),
            device_name: profile.device_name.clone(),
            app_version: profile.app_version.clone(),
            // Start dirty so the first frame syncs everything to all windows.
            presets_dirty: true,
            templates_dirty: true,
            snapshots_dirty: true,
            watchlists_dirty: true,
            connectors_dirty: true,
            recalc_mode,
            scale_mode,
            server_enabled: profile.server_enabled,
            server_port: profile.server_port,
            vault_key,
        }
    }
}
