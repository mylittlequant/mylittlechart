//! Chrome types — all platform-neutral (no winit, no RenderContext).

use crate::tooltip::TooltipState;

/// Controls which chrome buttons are rendered / hit-tested.
///
/// - `Native` — full desktop chrome: tabs, (+), menu, close_window,
///   minimize, maximize, close, new_window.
/// - `Web` — browser-safe subset: tabs, (+), menu only. Window-management
///   buttons (minimize / maximize / close / new_window) have no meaning in a
///   browser tab and are suppressed.
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum ChromeFlavor {
    Native,
    Web,
}

impl Default for ChromeFlavor {
    fn default() -> Self {
        ChromeFlavor::Native
    }
}

/// Height of the chrome strip in logical pixels.
pub const CHROME_HEIGHT: f64 = 32.0;

/// Items available in the chrome right-click context menu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeMenuAction {
    CloseWindow,
    DeleteWindow,
}

/// State for the chrome strip's right-click context menu.
pub struct ChromeContextMenu {
    pub open: bool,
    /// Position where the menu was opened (logical px, relative to window).
    pub x: f64,
    pub y: f64,
    /// Which item is currently hovered (-1 = none).
    pub hovered_index: i32,
}

impl ChromeContextMenu {
    pub fn new() -> Self {
        Self { open: false, x: 0.0, y: 0.0, hovered_index: -1 }
    }

    pub fn open_at(&mut self, x: f64, y: f64) {
        self.open = true;
        self.x = x;
        self.y = y;
        self.hovered_index = -1;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.hovered_index = -1;
    }
}

impl Default for ChromeContextMenu {
    fn default() -> Self {
        Self::new()
    }
}

/// Theme colours for the chrome strip, synced from the chart theme each frame.
pub struct ChromeColors {
    pub background: String,
    pub icon_normal: String,
    pub icon_hover: String,
    pub button_hover: String,
    pub close_hover: String,
    pub separator: String,
    pub tab_accent: String,
    /// Primary-CTA fill ("Sign in") — the theme accent.
    pub cta_bg: String,
    /// CTA fill under hover — the theme accent_hover.
    pub cta_bg_hover: String,
    pub tooltip_bg: String,
    pub tooltip_text: String,
}

impl Default for ChromeColors {
    fn default() -> Self {
        Self {
            background:   "#131722".into(),
            icon_normal:  "#a6adc8".into(),
            icon_hover:   "#cdd6f4".into(),
            button_hover: "#1f2937".into(),
            close_hover:  "#e81123".into(),
            separator:    "#313244".into(),
            tab_accent:   "#3b82f6".into(),
            cta_bg:       "#3b82f6".into(),
            cta_bg_hover: "#2563eb".into(),
            tooltip_bg:   "#323232".into(),
            tooltip_text: "#ffffff".into(),
        }
    }
}

/// A single tab in the chrome strip.
#[derive(Clone)]
pub struct Tab {
    /// Preset id (used to fire LoadPreset event).
    pub id: String,
    /// Display name shown on the tab.
    pub name: String,
    /// Whether this is the currently active tab.
    pub active: bool,
}

/// Live tab drag&drop reorder state (chrome strip).
#[derive(Debug, Clone, Copy)]
pub struct TabDrag {
    /// Index of the tab being dragged.
    pub index: usize,
    /// Window x where the press landed.
    pub press_x: f64,
    /// Horizontal grab offset: press_x minus the tab's left edge at press.
    pub grab_dx: f64,
    /// Latest cursor x.
    pub cur_x: f64,
    /// False until the cursor moves past the threshold — before that the
    /// gesture is still a plain click/double-click.
    pub active: bool,
}

/// The region of the window chrome that was hit by a pointer event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChromeHit {
    None,
    Caption,
    MinimizeButton,
    MaximizeButton,
    CloseButton,
    CloseWindowButton,
    MascotButton,
    MenuButton,
    /// Cloud sign-in button (Web flavor, server builds only).
    LoginButton,
    /// Cloud "What's new" button (Web flavor, cloud builds).
    NewsButton,
    /// Cloud social/project links button (Web flavor, cloud builds).
    LinksButton,
    /// Cloud desktop-download button (Web flavor, cloud builds).
    DownloadButton,
    Tab(usize),
    TabClose(usize),
    /// Hover-revealed rename pencil inside a tab (left of the close ×).
    TabRename(usize),
    /// "⋯" overflow button left of "+" — visible only when the tab strip
    /// overflows; hover/click opens the tab-list dropdown.
    TabOverflow,
    NewTabButton,
    NewWindowButton,
    ResizeTop,
    ResizeBottom,
    ResizeLeft,
    ResizeRight,
    ResizeTopLeft,
    ResizeTopRight,
    ResizeBottomLeft,
    ResizeBottomRight,
}

/// True when `hit` lands on a control that a modal's backdrop veil covers,
/// and which therefore must neither light up nor act while that modal is
/// open.
///
/// The complement is deliberate, not an oversight. `render_backdrop_dim`
/// leaves [`SYSTEM_BUTTONS_WIDTH`](crate::chrome::SYSTEM_BUTTONS_WIDTH)
/// unpainted on native — minimise, maximise, close stay live under a modal
/// (owner's ruling 2026-08-31) — and window management is not a control at
/// all: dragging the window by its caption or pulling a resize edge must
/// keep working, or a modal turns the window into a brick.
///
/// One list, used by BOTH the hover suppression and the click suppression in
/// `dispatch.rs`. They were separate once and the click half was simply
/// missing, so a veiled button did not highlight but still fired.
pub fn is_veiled_hit(hit: ChromeHit) -> bool {
    matches!(
        hit,
        ChromeHit::MascotButton
            | ChromeHit::MenuButton
            | ChromeHit::LoginButton
            | ChromeHit::NewsButton
            | ChromeHit::LinksButton
            | ChromeHit::DownloadButton
            | ChromeHit::Tab(_)
            | ChromeHit::TabClose(_)
            | ChromeHit::TabRename(_)
            | ChromeHit::TabOverflow
            | ChromeHit::NewTabButton
            | ChromeHit::NewWindowButton
            | ChromeHit::CloseWindowButton
    )
}

/// Mutable rendering state for the chrome strip.
pub struct ChromeState {
    pub hovered: ChromeHit,
    /// Display name of the signed-in account, or `None` when nobody is.
    ///
    /// Drives one button: `None` draws "Sign in", `Some(name)` draws the
    /// account. The chrome does not own auth — the shell pushes this from
    /// whatever it already watches (`mlc_updater`'s `auth_rx` on native) —
    /// so there is one source of truth and the strip just reflects it.
    pub account_label: Option<String>,
    /// Which provider that account signed in with (`"github"`, …), when the
    /// shell knows.
    ///
    /// Travels beside `account_label` because it comes from the same place
    /// and is needed at the same moment: the account overlay prints "via
    /// {provider}", and the click that opens it is handled here. Without it
    /// that line read "via " with nothing after it on native, which is what
    /// happens when a screen is handed half a fact.
    pub account_provider: Option<String>,
    /// True while an update has been installed and is waiting for a restart.
    ///
    /// The updater reaches `RestartPending` and the overlay says so — but only
    /// while the overlay is open, and closing it leaves no trace. The app keeps
    /// running the old binary quite happily, so the one person who needs to
    /// know that a restart is now worth something has no way to find out.
    /// While this is set the cloud-download button becomes a restart glyph in
    /// the theme's accent: the control that would have offered the update
    /// becomes the notice that it is waiting.
    pub update_restart_pending: bool,
    pub is_maximized: bool,
    pub title: String,
    pub colors: ChromeColors,
    pub tabs: Vec<Tab>,
    /// Pre-computed tab widths (updated each frame via `update_tab_widths`).
    pub tab_widths: Vec<f64>,
    /// Horizontal scroll offset of the tab strip (px). When tabs don't fit
    /// even at MIN_TAB_WIDTH the strip scrolls (wheel over the chrome);
    /// the "⋯" overflow button next to "+" opens the full tab list.
    pub tab_scroll: f64,
    /// Whether the tab-list dropdown (from the "⋯" overflow button) is open.
    pub tab_list_open: bool,
    /// Hovered row in the tab-list dropdown (-1 = none).
    pub tab_list_hovered: i32,
    /// Live tab drag&drop reorder state (armed on tab press, activates past
    /// a 4 px threshold, applies on release).
    pub tab_drag: Option<TabDrag>,
    /// Right-click context menu state.
    pub context_menu: ChromeContextMenu,
    /// Index of the tab being renamed inline (double-click flow), if any.
    /// While set, that tab renders as a text field (live text lives in the
    /// shared `TextFieldStore` under `mlc_app::text_input::CHROME_TAB_RENAME`)
    /// and its pencil / close × are neither drawn nor hit-tested.
    pub renaming_tab: Option<usize>,
    /// Tooltip state for chrome button hover tooltips.
    pub tooltip: TooltipState,
    /// Which buttons to render / hit-test. Default = `Native`.
    pub flavor: ChromeFlavor,
}

impl ChromeState {
    pub fn new(title: impl Into<String>) -> Self {
        Self {
            hovered:      ChromeHit::None,
            account_label: None,
            account_provider: None,
            update_restart_pending: false,
            is_maximized: false,
            title:        title.into(),
            colors:       ChromeColors::default(),
            tabs:         Vec::new(),
            tab_widths:   Vec::new(),
            context_menu: ChromeContextMenu::new(),
            renaming_tab: None,
            tab_scroll:   0.0,
            tab_list_open: false,
            tab_list_hovered: -1,
            tab_drag:     None,
            tooltip:      TooltipState::new(),
            flavor:       ChromeFlavor::default(),
        }
    }
}
