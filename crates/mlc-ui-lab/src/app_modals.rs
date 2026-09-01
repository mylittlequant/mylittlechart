//! Application-level modals launched from the shell's "Modals" launcher
//! menu (wave 3) — the other 15 `mlc-app-layout` modal renderers besides
//! the primitive-settings modal (Object Tree, wave 2, `crate::shell::
//! object_tree` / `objtree_modal`). Same scheme as wave 2: own state (this
//! module), own scoped region ([`REGION_ID`], pushed full-screen every
//! frame by `crate::lib`, mirroring `objtree_modal`'s own doc for why),
//! own response prefix (`"app_modal:"`). At most one of these is open at a
//! time — [`AppModalsState::open`] is a single `Option<AppModalKind>`, not
//! 15 independent `is_open` flags — matching the wave's own constraint
//! ("одновременно открыта одна модалка").
//!
//! No data, no primitives beyond the Object Tree's own registered set —
//! every list these modals draw (watchlist rows, chart-browser presets,
//! trading container panels, ...) is a small honest fixture built at the
//! bottom of this file, not a stand-in for a real data plane.
//!
//! Dispatch depth is deliberately shallow: close (✕ / OK / Create),
//! Escape, and tab-strip switching are wired for every modal that has
//! them; per-field interaction (checkboxes, dropdowns, colour swatches
//! inside a modal's own body) is NOT — that is the wave 2 primitive-
//! settings modal's own job, this wave connects the REST of the app's
//! modal surface, not re-implements each one's business logic.
//!
//! `indicator_overlay` is the one exception to "one modal at a time": it
//! is not a modal at all (no backdrop, no cross/OK footer —
//! `mlc-app-layout`'s own `render_indicator_overlay` does not even take an
//! `InputCoordinator`, the caller registers its own hit zones). It is a
//! small persistent chart-corner toggle, drawn every frame independent of
//! `open`, exactly like the real app draws it — see
//! [`AppModalsState::draw_indicator_overlay`]'s own doc.

use std::collections::HashMap;

use mlc_app_layout::layout::modals::alert_settings::render_alert_settings_modal;
use mlc_app_layout::layout::modals::chart_browser::render_chart_browser;
use mlc_app_layout::layout::modals::chart_settings::{render_settings_modal, ChartSettingsData};
use mlc_app_layout::layout::modals::clock_popup::render_clock_popup;
use mlc_app_layout::layout::modals::compare_settings::render_compare_settings_modal;
use mlc_app_layout::layout::modals::context_menu::render_context_menu;
use mlc_app_layout::layout::modals::indicator_overlay::render_indicator_overlay;
use mlc_app_layout::layout::modals::indicator_settings::render_indicator_settings_modal;
use mlc_app_layout::layout::modals::panel_settings_popup::{
    render_panel_settings_popup, PanelSettingRow, PanelSettingsView,
};
use mlc_app_layout::layout::modals::preset_name_input::render_preset_name_input;
use mlc_app_layout::layout::modals::search_overlay::render_search_overlay;
use mlc_app_layout::layout::modals::tags_tabs_modal::render_tags_tabs_modal;
use mlc_app_layout::layout::modals::template_name_modal::render_template_name_modal;
use mlc_app_layout::layout::modals::user_settings::render_user_settings_modal;
use mlc_app_layout::layout::modals::watchlist_modal::{
    render_watchlist_modal, WatchlistEntry, WatchlistGroupInfo,
};
use mlc_app_layout::layout::modals::{
    render_trading_container_modal, TcMapPanel, TcModalSection, TcSetupData,
    TradingContainerModalState,
};
use mlc_app_layout::layout::render_ui::IndicatorOverlayInfo;
use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::context_menu::{ContextMenuItemState, ContextMenuState, ContextMenuTarget};
use mlc_app_layout::ui::dropdown::DropdownTheme;
use mlc_app_layout::ui::modal_settings::{
    AlertSettingsState, AlertSettingsTab, ChartBrowserState, ChartScreenArea, ChartSettingsState,
    ChartSettingsTab, CompareSettingsState, CompareSettingsTab, CredentialsState,
    IndicatorOverlayState, IndicatorSettingsState, IndicatorSettingsTab, OverlaySettingsState,
    PresetNameInputMode, PresetNameInputState, TagsTabsSidebar, TagsTabsState, UserSettingsState,
    UserSettingsTab, WatchlistModalState, WatchlistModalTab,
};
use mlc_app_layout::ui::modal_state::{ModalState, OpenModal};
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;

use mlc_core::i18n::{t_shell, ShellKey};
use mlc_core::indicator_source::SignalDisplayConfig;
use mlc_core::preset_view::PresetSummary;
use mlc_core::render::RenderContext;
use mlc_core::state::panel::Panel;
use mlc_core::theme::ThemeManager;
use mlc_core::types::BOTTOM_TOOLBAR_HEIGHT;

use uzor::input::{InputCoordinator, LayerId, Sense};
use uzor::layout::DockState;
use uzor::types::Rect;

/// Scoped-region id this feature's state draws into — pushed full-screen
/// every frame by `crate::lib` (same convention as `objtree_modal`).
pub const REGION_ID: &str = "app_modal";

/// `TextFieldStore` key for the symbol-search overlay's own input —
/// registered once in `crate::LabState::new`, same shape as
/// `PRIMITIVE_SETTINGS_FIELD`'s own per-region registration.
pub const SEARCH_FIELD: &str = "app_modal_symbol_search";
/// `TextFieldStore` key for the "Save Template As" overlay's own input.
pub const TEMPLATE_NAME_FIELD: &str = "app_modal_template_name";
/// `render_preset_name_input`'s own hard-coded `TextFieldStore` key (not a
/// parameter on that function — see its own source) — registered under
/// this same literal so the lab's registration and the render call agree.
pub const PRESET_NAME_FIELD: &str = "preset_name_input";
/// Widget-id namespace for the "Save Template As" overlay
/// (`render_template_name_modal`'s own `ns_prefix` parameter).
const TEMPLATE_NAME_NS: &str = "app_tmpl_name";

/// One of the sixteen application-level modals the launcher menu lists.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum AppModalKind {
    ChartSettings,
    IndicatorSettings,
    Watchlist,
    Alerts,
    UserSettings,
    TagsTabs,
    TradingContainer,
    Search,
    TemplateName,
    PresetName,
    ChartBrowser,
    CompareSettings,
    ContextMenu,
    Clock,
    PanelSettings,
    IndicatorOverlay,
}

impl AppModalKind {
    pub const ALL: [AppModalKind; 16] = [
        Self::ChartSettings,
        Self::IndicatorSettings,
        Self::Watchlist,
        Self::Alerts,
        Self::UserSettings,
        Self::TagsTabs,
        Self::TradingContainer,
        Self::Search,
        Self::TemplateName,
        Self::PresetName,
        Self::ChartBrowser,
        Self::CompareSettings,
        Self::ContextMenu,
        Self::Clock,
        Self::PanelSettings,
        Self::IndicatorOverlay,
    ];

    /// Stable string id — the launcher menu's own `"launcher_menu:app:{id}"`
    /// widget ids, and how `crate::lib` looks a kind back up on click.
    pub fn id(self) -> &'static str {
        match self {
            Self::ChartSettings => "chart_settings",
            Self::IndicatorSettings => "indicator_settings",
            Self::Watchlist => "watchlist",
            Self::Alerts => "alerts",
            Self::UserSettings => "user_settings",
            Self::TagsTabs => "tags_tabs",
            Self::TradingContainer => "trading_container",
            Self::Search => "search",
            Self::TemplateName => "template_name",
            Self::PresetName => "preset_name",
            Self::ChartBrowser => "chart_browser",
            Self::CompareSettings => "compare_settings",
            Self::ContextMenu => "context_menu",
            Self::Clock => "clock",
            Self::PanelSettings => "panel_settings",
            Self::IndicatorOverlay => "indicator_overlay",
        }
    }

    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.id() == id)
    }

    /// `ModalShellSpec::new(ns, ..)`'s own `ns` namespace for this modal's
    /// widget ids (`"{ns}:close"` / `"{ns}:ok"` / `"{ns}:create"` /
    /// `"{ns}:tab:{tab_id}"`) — read directly off each modal's own render
    /// function, not invented here. `None` for the three modals that build
    /// their own chrome by hand instead of going through the shared shell
    /// (`ContextMenu`, `Clock`, `PanelSettings` — each has its own close id
    /// handled in [`AppModalsState::apply_response`]) and for
    /// `IndicatorOverlay`, which has no shell at all.
    fn ns(self) -> Option<&'static str> {
        match self {
            Self::ChartSettings => Some("chart_settings"),
            Self::IndicatorSettings => Some("ind_settings"),
            Self::Watchlist => Some("wl_modal"),
            Self::Alerts => Some("alert_set"),
            Self::UserSettings => Some("user_settings"),
            Self::TagsTabs => Some("tags_tabs"),
            Self::TradingContainer => Some("tc_modal"),
            Self::Search => Some("modal_search"),
            Self::TemplateName => Some(TEMPLATE_NAME_NS),
            Self::PresetName => Some("preset_name_input"),
            Self::ChartBrowser => Some("chart_browser"),
            Self::CompareSettings => Some("cmp_settings"),
            Self::ContextMenu | Self::Clock | Self::PanelSettings | Self::IndicatorOverlay => None,
        }
    }

    /// Launcher-menu row label — user-facing, via `mlc_core::i18n`
    /// (`ShellKey`, not the modal's own internal title key: the launcher
    /// row and the modal's own header are two different UI surfaces that
    /// happen to name the same thing).
    pub fn label(self) -> &'static str {
        t_shell(match self {
            Self::ChartSettings => ShellKey::AppModalChartSettings,
            Self::IndicatorSettings => ShellKey::AppModalIndicatorSettings,
            Self::Watchlist => ShellKey::AppModalWatchlist,
            Self::Alerts => ShellKey::AppModalAlerts,
            Self::UserSettings => ShellKey::AppModalUserSettings,
            Self::TagsTabs => ShellKey::AppModalTagsTabs,
            Self::TradingContainer => ShellKey::AppModalTradingContainer,
            Self::Search => ShellKey::AppModalSearch,
            Self::TemplateName => ShellKey::AppModalTemplateName,
            Self::PresetName => ShellKey::AppModalPresetName,
            Self::ChartBrowser => ShellKey::AppModalChartBrowser,
            Self::CompareSettings => ShellKey::AppModalCompareSettings,
            Self::ContextMenu => ShellKey::AppModalContextMenu,
            Self::Clock => ShellKey::AppModalClock,
            Self::PanelSettings => ShellKey::AppModalPanelSettings,
            Self::IndicatorOverlay => ShellKey::AppModalIndicatorOverlay,
        })
    }

    /// Icon name (`mlc_core::state::icons::icon_svg` vocabulary).
    pub fn icon(self) -> &'static str {
        match self {
            Self::ChartSettings => "settings",
            Self::IndicatorSettings => "indicators",
            Self::Watchlist => "watchlist",
            Self::Alerts => "alert",
            Self::UserSettings => "user",
            Self::TagsTabs => "layers",
            Self::TradingContainer => "trading",
            Self::Search => "search",
            Self::TemplateName => "bookmark",
            Self::PresetName => "star",
            Self::ChartBrowser => "layout",
            Self::CompareSettings => "grid",
            Self::ContextMenu => "menu",
            Self::Clock => "clock",
            Self::PanelSettings => "layout_split_h",
            Self::IndicatorOverlay => "eye",
        }
    }
}

/// The clock popup has no dedicated state type upstream (its render fn
/// takes discrete `active_offset`/`use_24h`/`show_utc` params) — this is
/// this crate's own copy, the same role `ClockPopupState` plays inside
/// `mlc-core`'s real `ModalState` family.
#[derive(Clone, Debug)]
struct ClockState {
    active_offset: i32,
    use_24h: bool,
    show_utc: bool,
    hovered: Option<String>,
}

/// State for every application-level modal, plus which one (if any) is
/// open. See the module doc for the "one at a time" / "shallow dispatch"
/// scope decisions.
pub struct AppModalsState {
    pub open: Option<AppModalKind>,

    chart_settings: ChartSettingsState,
    chart_settings_data: ChartSettingsData,

    indicator_settings: IndicatorSettingsState,

    watchlist: WatchlistModalState,
    watchlist_items: Vec<WatchlistEntry>,
    watchlist_groups: Vec<WatchlistGroupInfo>,

    alerts: AlertSettingsState,

    user_settings: UserSettingsState,
    /// The Vault tab's own state — this lab has no vault-gated data plane
    /// to speak of, so a bare default (sealed, no keys) is honest here;
    /// it exists only so `render_user_settings_modal`'s Vault tab has
    /// something to draw.
    credentials: CredentialsState,

    tags_tabs: TagsTabsState,
    /// `render_tags_tabs_modal`'s own separate TABS-section state (the
    /// panel-tree manager it reuses verbatim) — a sibling parameter to
    /// `TagsTabsState`, not a field on it, in this version of the modal.
    tags_tabs_overlay: OverlaySettingsState,
    docking: DockState<Panel>,

    trading_container: TradingContainerModalState,
    trading_panels: Vec<TcMapPanel>,
    trading_setup: TcSetupData,

    search: ModalState,

    preset_name: PresetNameInputState,

    chart_browser: ChartBrowserState,
    chart_browser_presets: Vec<PresetSummary>,

    compare_settings: CompareSettingsState,

    context_menu: ContextMenuState,

    clock: ClockState,

    panel_settings_rows: Vec<PanelSettingRow>,

    /// Public: `crate::lib` reads `.is_open` for the launcher menu's own
    /// "already open" highlight and toggles it directly on a shell-button
    /// click (there is no dedicated `AppModalKind` gate for this one — see
    /// the module doc for why).
    pub indicator_overlay: IndicatorOverlayState,
    indicator_overlay_items: Vec<IndicatorOverlayInfo>,

    /// The currently-open modal's own outer frame rect, captured from
    /// whichever `*Result` [`Self::draw`] got back this frame (every one of
    /// the sixteen carries an outer rect, under one of a few field names —
    /// see each match arm below). `None` while nothing is open, or for one
    /// frame after `open_kind` until the next `draw` call runs. Read by the
    /// agent control plane (`crate::lab_metrics`) so external tooling can
    /// compare this lab's REAL measured modal geometry against `sim.js`'s
    /// own DOM-measured `__SIM.metrics()`.
    last_rect: Option<Rect>,
}

impl AppModalsState {
    pub fn new() -> Self {
        Self {
            open: None,
            chart_settings: ChartSettingsState::default(),
            chart_settings_data: ChartSettingsData::default(),
            indicator_settings: IndicatorSettingsState::default(),
            watchlist: WatchlistModalState::new(),
            watchlist_items: stub_watchlist_items(),
            watchlist_groups: stub_watchlist_groups(),
            alerts: AlertSettingsState::default(),
            user_settings: UserSettingsState::default(),
            credentials: CredentialsState::default(),
            tags_tabs: TagsTabsState::default(),
            tags_tabs_overlay: OverlaySettingsState::default(),
            docking: DockState::new(),
            trading_container: TradingContainerModalState::default(),
            trading_panels: stub_trading_panels(),
            trading_setup: TcSetupData::default(),
            search: ModalState::default(),
            preset_name: PresetNameInputState::default(),
            chart_browser: ChartBrowserState::default(),
            chart_browser_presets: stub_chart_browser_presets(),
            compare_settings: CompareSettingsState::default(),
            context_menu: ContextMenuState::default(),
            clock: ClockState { active_offset: 0, use_24h: true, show_utc: true, hovered: None },
            panel_settings_rows: stub_panel_settings_rows(),
            indicator_overlay: IndicatorOverlayState::default(),
            indicator_overlay_items: stub_indicator_overlay_items(),
            last_rect: None,
        }
    }

    /// Open one modal, replacing whatever else was open (see the module
    /// doc for the single-slot rule). `coord` is the lab's own
    /// [`REGION_ID`] child coordinator — some modals own a text field this
    /// crate must (re)seed/focus at open time, the same thing `ChartApp`'s
    /// own openers do on the real app's open path.
    pub fn open_kind(&mut self, kind: AppModalKind, coord: &mut InputCoordinator) {
        self.open = Some(kind);
        match kind {
            AppModalKind::Search => {
                self.search.current = OpenModal::SymbolSearch;
                self.search.position = None;
                coord.text_fields_mut().focus(uzor::WidgetId::from(SEARCH_FIELD));
            }
            AppModalKind::PresetName => {
                self.preset_name.mode = PresetNameInputMode::SaveAs;
                self.preset_name.position = None;
                let wid = uzor::WidgetId::from(PRESET_NAME_FIELD);
                coord.text_fields_mut().set_text(&wid, "New chart");
                coord.text_fields_mut().focus(wid);
            }
            AppModalKind::TemplateName => {
                // `render_template_name_modal` re-focuses this field every
                // frame itself (see its own doc) — only the seed text is
                // this crate's job.
                coord.text_fields_mut().set_text(&uzor::WidgetId::from(TEMPLATE_NAME_FIELD), "");
            }
            AppModalKind::ContextMenu => {
                self.context_menu.open(240.0, 240.0, ContextMenuTarget::None, stub_context_menu_items());
            }
            _ => {}
        }
    }

    /// Close whichever modal is open. A no-op if none is.
    pub fn close(&mut self) {
        self.open = None;
        self.last_rect = None;
    }

    pub fn is_open(&self) -> bool {
        self.open.is_some()
    }

    /// The open modal's own outer frame rect, as of the last [`Self::draw`]
    /// call — see [`Self::last_rect`]'s own doc.
    pub fn last_rect(&self) -> Option<Rect> {
        self.last_rect
    }

    /// Draw whichever modal is open into `coord` (the lab's [`REGION_ID`]
    /// child coordinator). A no-op when nothing is open.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        ctx: &mut dyn RenderContext,
        coord: &mut InputCoordinator,
        screen_w: f64,
        screen_h: f64,
        frame_theme: &FrameTheme,
        toolbar_theme: &ToolbarTheme,
        theme_manager: &ThemeManager,
        now_ms: u64,
    ) {
        let Some(kind) = self.open else { return };
        match kind {
            AppModalKind::ChartSettings => {
                let result = render_settings_modal(
                    ctx,
                    screen_w,
                    screen_h,
                    0.0,
                    0.0,
                    &self.chart_settings,
                    &self.chart_settings_data,
                    theme_manager,
                    frame_theme,
                    toolbar_theme,
                    now_ms,
                    coord,
                    &[],
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::IndicatorSettings => {
                let signal_display = SignalDisplayConfig::default();
                let params = [("Length".to_string(), "14".to_string())];
                let outputs = [("RSI".to_string(), toolbar_theme.accent.clone())];
                let result = render_indicator_settings_modal(
                    ctx,
                    screen_w,
                    screen_h,
                    0.0,
                    0.0,
                    &self.indicator_settings,
                    "RSI (stub)",
                    &params,
                    &outputs,
                    None,
                    false,
                    &signal_display,
                    None,
                    now_ms,
                    frame_theme,
                    toolbar_theme,
                    coord,
                    &[],
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::Watchlist => {
                let result = render_watchlist_modal(
                    ctx,
                    screen_w,
                    screen_h,
                    &self.watchlist,
                    &self.watchlist_items,
                    &self.watchlist_groups,
                    frame_theme,
                    toolbar_theme,
                    now_ms,
                    coord,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::Alerts => {
                let result = render_alert_settings_modal(
                    ctx, screen_w, screen_h, &self.alerts, frame_theme, toolbar_theme, coord,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::UserSettings => {
                let result = render_user_settings_modal(
                    ctx, screen_w, screen_h, &self.user_settings, &self.credentials, frame_theme, toolbar_theme, now_ms, coord,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::TagsTabs => {
                let leaf_color_tags = HashMap::new();
                let result = render_tags_tabs_modal(
                    ctx,
                    &self.tags_tabs,
                    &self.tags_tabs_overlay,
                    frame_theme,
                    toolbar_theme,
                    &self.docking,
                    false,
                    &[],
                    None,
                    &leaf_color_tags,
                    screen_w,
                    screen_h,
                    theme_manager,
                    screen_w,
                    screen_h,
                    coord,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::TradingContainer => {
                let result = render_trading_container_modal(
                    ctx,
                    &self.trading_container,
                    frame_theme,
                    toolbar_theme,
                    &self.trading_panels,
                    (screen_w * 0.4, screen_h * 0.4),
                    &self.trading_setup,
                    screen_w,
                    screen_h,
                    coord,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::Search => {
                let screen = ChartScreenArea::new(0.0, 0.0, screen_w, screen_h);
                let result = render_search_overlay(
                    ctx,
                    screen,
                    &self.search,
                    &[],
                    &[],
                    &[],
                    None,
                    frame_theme,
                    toolbar_theme,
                    now_ms,
                    coord,
                    SEARCH_FIELD,
                    false,
                    true,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::TemplateName => {
                let result = render_template_name_modal(
                    ctx, screen_w, screen_h, TEMPLATE_NAME_FIELD, TEMPLATE_NAME_NS, frame_theme, toolbar_theme, now_ms, coord,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::PresetName => {
                let result = render_preset_name_input(
                    ctx, screen_w, screen_h, &self.preset_name, frame_theme, toolbar_theme, now_ms, coord,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::ChartBrowser => {
                let result = render_chart_browser(
                    ctx,
                    screen_w,
                    screen_h,
                    &self.chart_browser,
                    &self.chart_browser_presets,
                    "1",
                    frame_theme,
                    toolbar_theme,
                    now_ms,
                    coord,
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::CompareSettings => {
                let result = render_compare_settings_modal(
                    ctx, screen_w, screen_h, &self.compare_settings, frame_theme, toolbar_theme, coord, &[],
                );
                self.last_rect = Some(result.modal_rect);
            }
            AppModalKind::ContextMenu => {
                let dropdown_theme = DropdownTheme::from_toolbar(toolbar_theme);
                let result = render_context_menu(ctx, &self.context_menu, &dropdown_theme, None, coord);
                self.last_rect = Some(result.menu_rect);
            }
            AppModalKind::Clock => {
                let bottom_toolbar_y = screen_h - BOTTOM_TOOLBAR_HEIGHT;
                let result = render_clock_popup(
                    ctx,
                    screen_w,
                    screen_h,
                    bottom_toolbar_y,
                    self.clock.active_offset,
                    self.clock.use_24h,
                    self.clock.show_utc,
                    self.clock.hovered.as_deref(),
                    frame_theme,
                    toolbar_theme,
                    coord,
                );
                self.last_rect = Some(result.popup_rect);
            }
            AppModalKind::PanelSettings => {
                let view = PanelSettingsView {
                    title: AppModalKind::PanelSettings.label().to_string(),
                    rows: self.panel_settings_rows.clone(),
                    as_modal: true,
                    anchor: (0.0, 0.0),
                };
                let result = render_panel_settings_popup(ctx, &view, frame_theme, toolbar_theme, screen_w, screen_h, coord);
                self.last_rect = Some(result.rect);
            }
            // Not a modal — see the module doc / `draw_indicator_overlay`.
            AppModalKind::IndicatorOverlay => {}
        }
    }

    /// Route one clicked widget response from the [`REGION_ID`] scoped
    /// region (id already stripped of the `"app_modal:"` prefix) into this
    /// state. Returns `true` if recognised. Only close/OK/Create, tab-strip
    /// switching, and the handful of hand-rolled controls the three
    /// shell-less modals (`ContextMenu`/`Clock`/`PanelSettings`) need to at
    /// least close — see the module doc for the depth cutoff.
    pub fn apply_response(&mut self, id: &str, clicked: bool) -> bool {
        let Some(kind) = self.open else { return false };
        if !clicked {
            return false;
        }

        if let Some(ns) = kind.ns() {
            if id == format!("{ns}:close") || id == format!("{ns}:ok") || id == format!("{ns}:create") {
                self.close();
                return true;
            }
            if let Some(tab_id) = id.strip_prefix(&format!("{ns}:tab:")) {
                self.set_tab(kind, tab_id);
                return true;
            }
        }

        match kind {
            AppModalKind::ContextMenu => {
                if id == "context_menu:bg" || id.starts_with("context_menu:item:") {
                    self.close();
                    return true;
                }
            }
            AppModalKind::Clock => {
                if id == "clock_popup:bg" {
                    self.close();
                    return true;
                }
                if let Some(off) = id.strip_prefix("clock_popup:tz:") {
                    if let Ok(v) = off.parse::<i32>() {
                        self.clock.active_offset = v;
                    }
                    return true;
                }
                if id == "clock_popup:clock:use_24h" {
                    self.clock.use_24h = !self.clock.use_24h;
                    return true;
                }
                if id == "clock_popup:show_utc" {
                    self.clock.show_utc = !self.clock.show_utc;
                    return true;
                }
            }
            AppModalKind::PanelSettings => {
                if id == "pset:close" {
                    self.close();
                    return true;
                }
                if let Some(row_id) = id.strip_prefix("pset:toggle:") {
                    for row in &mut self.panel_settings_rows {
                        if let PanelSettingRow::Toggle { id, on, .. } = row {
                            if id == row_id {
                                *on = !*on;
                            }
                        }
                    }
                    return true;
                }
                if let Some(rest) = id.strip_prefix("pset:opt:") {
                    if let Some((row_id, idx_str)) = rest.rsplit_once(':') {
                        if let Ok(idx) = idx_str.parse::<usize>() {
                            for row in &mut self.panel_settings_rows {
                                if let PanelSettingRow::Choice { id, options, active, .. } = row {
                                    if id == row_id && idx < options.len() {
                                        *active = idx;
                                    }
                                }
                            }
                        }
                    }
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    /// `pub(crate)`: also called directly by the agent control plane
    /// (`crate::lab_open_app`) so `open_type`/`open_app`-equivalents can
    /// switch tabs in one call, the same way `sim.js`'s own `__SIM.openType`/
    /// `__SIM.app` take an optional tab id.
    pub(crate) fn set_tab(&mut self, kind: AppModalKind, tab_id: &str) {
        match kind {
            AppModalKind::ChartSettings => {
                if let Some(t) = ChartSettingsTab::all().iter().find(|t| t.id() == tab_id) {
                    self.chart_settings.active_tab = *t;
                }
            }
            AppModalKind::IndicatorSettings => {
                if let Some(t) = IndicatorSettingsTab::all().iter().find(|t| t.id() == tab_id) {
                    self.indicator_settings.active_tab = *t;
                }
            }
            AppModalKind::UserSettings => {
                if let Some(t) = UserSettingsTab::all().iter().find(|t| t.id() == tab_id) {
                    self.user_settings.active_tab = *t;
                }
            }
            AppModalKind::CompareSettings => {
                if let Some(t) = CompareSettingsTab::all().iter().find(|t| t.id() == tab_id) {
                    self.compare_settings.active_tab = *t;
                }
            }
            AppModalKind::Alerts => {
                self.alerts.active_tab = match tab_id {
                    "settings" => AlertSettingsTab::Settings,
                    "notifications" => AlertSettingsTab::Notifications,
                    "list" => AlertSettingsTab::AlertsList,
                    _ => self.alerts.active_tab,
                };
            }
            AppModalKind::Watchlist => {
                self.watchlist.active_tab = match tab_id {
                    "overview" => WatchlistModalTab::Overview,
                    "groups" => WatchlistModalTab::Groups,
                    _ => self.watchlist.active_tab,
                };
            }
            AppModalKind::TagsTabs => {
                self.tags_tabs.sidebar = match tab_id {
                    "tabs" => TagsTabsSidebar::Tabs,
                    "tags" => TagsTabsSidebar::Tags,
                    _ => self.tags_tabs.sidebar,
                };
            }
            AppModalKind::TradingContainer => {
                self.trading_container.section = match tab_id {
                    "map" => TcModalSection::Map,
                    "setup" => TcModalSection::Setup,
                    _ => self.trading_container.section,
                };
            }
            _ => {}
        }
    }

    /// Every tab id `kind` declares, in strip order — mirrors [`Self::
    /// set_tab`]'s own match arm-for-arm (same file, kept next to it on
    /// purpose so the two cannot drift). Empty for the eight kinds with no
    /// tab strip at all. Read by the agent control plane (`crate::
    /// lab_snapshot`) — the equivalent of `sim.js`'s own DOM query
    /// (`.modal .tabs .tab`), except sourced from the declared tab enums
    /// rather than scraped off a render result, since five of these
    /// sixteen renderers do not return a `tab_rects` field at all.
    pub(crate) fn declared_tabs(kind: AppModalKind) -> Vec<&'static str> {
        match kind {
            AppModalKind::ChartSettings => ChartSettingsTab::all().iter().map(|t| t.id()).collect(),
            AppModalKind::IndicatorSettings => IndicatorSettingsTab::all().iter().map(|t| t.id()).collect(),
            AppModalKind::UserSettings => UserSettingsTab::all().iter().map(|t| t.id()).collect(),
            AppModalKind::CompareSettings => CompareSettingsTab::all().iter().map(|t| t.id()).collect(),
            AppModalKind::Alerts => vec!["settings", "notifications", "list"],
            AppModalKind::Watchlist => vec!["overview", "groups"],
            AppModalKind::TagsTabs => vec!["tabs", "tags"],
            AppModalKind::TradingContainer => vec!["map", "setup"],
            AppModalKind::Search
            | AppModalKind::TemplateName
            | AppModalKind::PresetName
            | AppModalKind::ChartBrowser
            | AppModalKind::ContextMenu
            | AppModalKind::Clock
            | AppModalKind::PanelSettings
            | AppModalKind::IndicatorOverlay => Vec::new(),
        }
    }

    /// The currently open modal's own active tab id, or `None` for a kind
    /// with no tab strip. Companion to [`Self::declared_tabs`] — same
    /// match shape, read side.
    pub(crate) fn active_tab_id(&self) -> Option<&'static str> {
        match self.open? {
            AppModalKind::ChartSettings => Some(self.chart_settings.active_tab.id()),
            AppModalKind::IndicatorSettings => Some(self.indicator_settings.active_tab.id()),
            AppModalKind::UserSettings => Some(self.user_settings.active_tab.id()),
            AppModalKind::CompareSettings => Some(self.compare_settings.active_tab.id()),
            AppModalKind::Alerts => Some(match self.alerts.active_tab {
                AlertSettingsTab::Settings => "settings",
                AlertSettingsTab::Notifications => "notifications",
                AlertSettingsTab::AlertsList => "list",
            }),
            // `Settings`/`Map` are real variants (`set_tab` cannot reach
            // them by id below, so they are absent from `declared_tabs`
            // too) but a read-side match must still be exhaustive.
            AppModalKind::Watchlist => Some(match self.watchlist.active_tab {
                WatchlistModalTab::Overview => "overview",
                WatchlistModalTab::Groups => "groups",
                WatchlistModalTab::Settings => "settings",
            }),
            AppModalKind::TagsTabs => Some(match self.tags_tabs.sidebar {
                TagsTabsSidebar::Tabs => "tabs",
                TagsTabsSidebar::Tags => "tags",
                TagsTabsSidebar::Map => "map",
            }),
            AppModalKind::TradingContainer => Some(match self.trading_container.section {
                TcModalSection::Map => "map",
                TcModalSection::Setup => "setup",
            }),
            AppModalKind::Search
            | AppModalKind::TemplateName
            | AppModalKind::PresetName
            | AppModalKind::ChartBrowser
            | AppModalKind::ContextMenu
            | AppModalKind::Clock
            | AppModalKind::PanelSettings
            | AppModalKind::IndicatorOverlay => None,
        }
    }

    /// Draw the persistent chart-corner indicator-overlay toggle — always
    /// on screen, independent of `open` (see the module doc for why this
    /// one is not a modal). `coord` is the MAIN coordinator, not a scoped
    /// child: `render_indicator_overlay` takes no `InputCoordinator` at
    /// all — the real app registers the rects it returns itself, and this
    /// crate does the same, on its own small always-on-top layer.
    pub fn draw_indicator_overlay(
        &mut self,
        ctx: &mut dyn RenderContext,
        coord: &mut InputCoordinator,
        pane_rect: Rect,
        frame_theme: &FrameTheme,
        toolbar_theme: &ToolbarTheme,
    ) {
        let result = render_indicator_overlay(
            ctx,
            &pane_rect,
            &self.indicator_overlay_items,
            &self.indicator_overlay,
            frame_theme,
            toolbar_theme,
        );
        let layer = LayerId::new("app_ind_overlay");
        coord.push_layer(layer.clone(), 2, false);
        if self.indicator_overlay.is_open {
            if let Some(r) = result.close_button_rect {
                coord.register_on_layer("ind_overlay:close", r, Sense::CLICK, &layer);
            }
        } else {
            coord.register_on_layer("ind_overlay:open", result.button_rect, Sense::CLICK, &layer);
        }
        coord.pop_layer(&layer);
    }

    /// Route a click on the indicator-overlay toggle. Returns `true` if
    /// recognised.
    pub fn apply_indicator_overlay_response(&mut self, id: &str) -> bool {
        match id {
            "ind_overlay:open" => {
                self.indicator_overlay.is_open = true;
                true
            }
            "ind_overlay:close" => {
                self.indicator_overlay.is_open = false;
                true
            }
            _ => false,
        }
    }
}

// =============================================================================
// Stub data — small, honest fixtures. No data plane exists in this crate;
// see the crate's own README / `nemo/CLAUDE.md` client-only-data doctrine.
// =============================================================================

fn stub_watchlist_items() -> Vec<WatchlistEntry> {
    vec![
        WatchlistEntry {
            symbol: "BTCUSDT".to_string(),
            exchange: "binance".to_string(),
            price: 65_100.0,
            change_pct: 1.24,
            change_abs: 812.4,
            high_24h: 65_900.0,
            low_24h: 63_800.0,
            volume_24h: 1_200_000_000.0,
            color_flag: String::new(),
            account_type: String::new(),
        },
        WatchlistEntry {
            symbol: "ETHUSDT".to_string(),
            exchange: "binance".to_string(),
            price: 3_420.55,
            change_pct: -0.42,
            change_abs: -14.3,
            high_24h: 3_480.0,
            low_24h: 3_380.0,
            volume_24h: 640_000_000.0,
            color_flag: String::new(),
            account_type: String::new(),
        },
    ]
}

fn stub_watchlist_groups() -> Vec<WatchlistGroupInfo> {
    vec![WatchlistGroupInfo {
        id: 1,
        name: "Default".to_string(),
        color: String::new(),
        symbol_count: 2,
        is_active: true,
    }]
}

fn stub_trading_panels() -> Vec<TcMapPanel> {
    vec![
        TcMapPanel::plain("dom", "DOM".to_string(), String::new(), (0.0, 0.0, 120.0, 200.0), false),
        TcMapPanel::plain("chart", "Chart".to_string(), String::new(), (120.0, 0.0, 240.0, 200.0), false),
        TcMapPanel::plain("positions", "Positions".to_string(), String::new(), (0.0, 200.0, 360.0, 80.0), false),
    ]
}

fn stub_chart_browser_presets() -> Vec<PresetSummary> {
    vec![PresetSummary {
        id: "1".to_string(),
        name: "Untitled 1".to_string(),
        created_at: 0,
        first_window: Some(("BTCUSDT".to_string(), "1H".to_string())),
    }]
}

fn stub_context_menu_items() -> Vec<ContextMenuItemState> {
    vec![
        ContextMenuItemState::action("settings", "Settings"),
        ContextMenuItemState::action("clone", "Clone"),
        ContextMenuItemState::separator(),
        ContextMenuItemState::danger("delete", "Delete"),
    ]
}

fn stub_panel_settings_rows() -> Vec<PanelSettingRow> {
    vec![
        PanelSettingRow::Header("Panel".to_string()),
        PanelSettingRow::Toggle { id: "show_header".to_string(), label: "Show header".to_string(), on: true },
        PanelSettingRow::Toggle { id: "pin".to_string(), label: "Pinned".to_string(), on: false },
        PanelSettingRow::Choice {
            id: "opens_as".to_string(),
            label: "Open as".to_string(),
            options: vec!["Dropdown".to_string(), "Modal".to_string()],
            active: 1,
        },
    ]
}

fn stub_indicator_overlay_items() -> Vec<IndicatorOverlayInfo> {
    vec![
        IndicatorOverlayInfo {
            id: 1,
            display_name: "RSI 14".to_string(),
            visible: true,
            is_compare: false,
            symbol: None,
            color: None,
        },
        IndicatorOverlayInfo {
            id: 2,
            display_name: "EMA 50".to_string(),
            visible: true,
            is_compare: false,
            symbol: None,
            color: None,
        },
    ]
}
