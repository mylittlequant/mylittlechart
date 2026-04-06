//! Types for the centralized text-input manager.

// =============================================================================
// InputCapability
// =============================================================================

/// Which interaction classes a field supports.
///
/// The manager enforces these as hard guards; callers do not need to check.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum InputCapability {
    /// Full keyboard + mouse: typing, arrow/home/end navigation,
    /// Shift-extend selection, Ctrl+A/C/V, click-to-position, drag-select.
    Both,
    /// Keyboard only: typing, navigation, Ctrl+A/C/V.
    /// Mouse clicks and drags are ignored by the manager.
    Keyboard,
    /// Mouse only: click-to-position, drag-select, Ctrl+C/A.
    /// Character input and key navigation are silently ignored.
    Mouse,
    /// Raw PTY pass-through: all input is forwarded as raw bytes without
    /// any text-editing semantics. Used for terminal/PTY fields.
    Raw,
}

// =============================================================================
// FieldId
// =============================================================================

/// Strongly typed field identifier. All known fields are listed here so the
/// compiler catches typos. New fields require adding a variant — intentional.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FieldId {
    // Color picker (5 instances share one ID; manager routes by active picker)
    HexColor,

    // Profile manager / welcome wizard
    E2ePassphrase,
    WizardPassphrase,
    NewProfileName,
    ConfirmPassphrase,
    /// Keyboard entry during UseRecoveryKey flow
    RecoveryKeyInput,
    /// Read-only display (Mouse only, copy)
    RecoveryKeyDisplay,
    NewPassphrase,
    ProfileRename,

    // Chart browser / watchlist
    ChartBrowserSearch,
    WatchlistSearch,
    WatchlistGroupName,

    // Preset / template name inputs
    PresetName,
    PrimitiveTemplateName,
    IndicatorTemplateName,
    CompareTemplateName,
    ChartTemplateName,

    // Settings
    NewKeyLabel,

    // Symbol search overlay
    SymbolSearch,

    // Agent PTY / chat fields
    /// Raw PTY input for the agent terminal pane.
    AgentPty,
    /// Text chat input for the agent chat pane.
    AgentChat,
}

// =============================================================================
// FieldConfig
// =============================================================================

/// Static configuration for a single text field. Created once at
/// registration time; not updated per-frame.
#[derive(Clone, Debug)]
pub struct FieldConfig {
    /// Interaction class.
    pub capability: InputCapability,
    /// Optional character-level filter. Return `true` to accept the char.
    /// `None` means accept everything except control characters.
    pub char_filter: Option<fn(char) -> bool>,
    /// Maximum character count. `None` = unlimited.
    pub max_len: Option<usize>,
    /// Mask display (password dots). Does not affect stored text.
    pub masked: bool,
    /// Whether the field is read-only (blocks all mutations but keeps
    /// Mouse capability for select+copy). Implies capability >= Mouse.
    pub read_only: bool,
}

impl FieldConfig {
    /// Plain text field: Both capability, no filter, no limit, unmasked.
    pub fn text() -> Self {
        Self {
            capability: InputCapability::Both,
            char_filter: None,
            max_len: None,
            masked: false,
            read_only: false,
        }
    }

    /// Password field: Both capability, no filter, no limit, masked.
    pub fn password() -> Self {
        Self {
            capability: InputCapability::Both,
            char_filter: None,
            max_len: None,
            masked: true,
            read_only: false,
        }
    }

    /// Hex color field: Both, accepts `#` and hex digits only, max 9 chars.
    pub fn hex_color() -> Self {
        Self {
            capability: InputCapability::Both,
            char_filter: Some(|c| c == '#' || c.is_ascii_hexdigit()),
            max_len: Some(9),
            masked: false,
            read_only: false,
        }
    }

    /// Search field: Both, no filter, no limit.
    pub fn search() -> Self {
        Self {
            capability: InputCapability::Both,
            char_filter: None,
            max_len: None,
            masked: false,
            read_only: false,
        }
    }

    /// Read-only display field: Mouse capability, read-only.
    pub fn read_only_display() -> Self {
        Self {
            capability: InputCapability::Mouse,
            char_filter: None,
            max_len: None,
            masked: false,
            read_only: true,
        }
    }

    /// Keyboard-only field: no drag-select, no mouse positioning.
    pub fn keyboard_only() -> Self {
        Self {
            capability: InputCapability::Keyboard,
            char_filter: None,
            max_len: None,
            masked: false,
            read_only: false,
        }
    }

    /// Raw PTY field: all input is forwarded as raw bytes, no text-editing
    /// semantics apply. Used for terminal/PTY panes.
    pub fn raw() -> Self {
        Self {
            capability: InputCapability::Raw,
            char_filter: None,
            max_len: None,
            masked: false,
            read_only: false,
        }
    }
}

// =============================================================================
// FieldAction
// =============================================================================

/// Returned by every mutating call so the caller can react without
/// coupling the manager to application logic.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FieldAction {
    /// Nothing special — text/cursor changed in-place.
    None,
    /// Enter was pressed. Contains the committed text value.
    Commit(String),
    /// Escape was pressed. Manager reverted to `original_text`.
    Cancel,
    /// Text content changed (useful for live-filter fields like search).
    TextChanged(String),
    /// Raw PTY bytes to forward to the terminal process.
    /// Returned when the focused field has `InputCapability::Raw`.
    RawInput(Vec<u8>),
}

// =============================================================================
// FieldState
// =============================================================================

/// Runtime state for one registered field.
///
/// Stored inside `TextInputManager`; not pub outside the crate.
#[derive(Clone, Debug)]
pub(crate) struct FieldState {
    /// Text content.
    pub text: String,
    /// Text value at the moment `begin_edit` was called (for Cancel revert).
    pub original_text: String,
    /// Cursor position (char index, not byte).
    pub cursor: usize,
    /// Selection anchor (char index). `None` = no selection.
    pub selection_start: Option<usize>,
    /// Field geometry from the most-recent `update_field` call this frame.
    /// Stored as (x, y, w, h) to avoid importing uzor types into this module.
    pub last_rect: Option<(f64, f64, f64, f64)>,
    /// Pre-computed char boundary X positions from `draw_input` result.
    pub last_char_positions: Vec<f64>,
    /// Frame counter at which `update_field` was last called.
    pub last_frame: u64,
    /// Static config (immutable after registration).
    pub config: FieldConfig,
}

impl FieldState {
    pub(crate) fn new(config: FieldConfig) -> Self {
        Self {
            text: String::new(),
            original_text: String::new(),
            cursor: 0,
            selection_start: None,
            last_rect: None,
            last_char_positions: Vec::new(),
            last_frame: 0,
            config,
        }
    }

    /// Return `(lo, hi)` selection bounds if a non-empty selection exists.
    pub(crate) fn selection_range(&self) -> Option<(usize, usize)> {
        let anchor = self.selection_start?;
        let cursor = self.cursor;
        if anchor == cursor {
            return None;
        }
        let lo = anchor.min(cursor);
        let hi = anchor.max(cursor);
        Some((lo, hi))
    }

    /// Delete the selected text and collapse the selection.
    pub(crate) fn delete_selection(&mut self) {
        if let Some((lo, hi)) = self.selection_range() {
            let byte_lo = self.char_to_byte(lo);
            let byte_hi = self.char_to_byte(hi);
            self.text.drain(byte_lo..byte_hi);
            self.cursor = lo;
            self.selection_start = None;
        }
    }

    /// Convert a char index to a byte index.
    pub(crate) fn char_to_byte(&self, char_idx: usize) -> usize {
        self.text
            .char_indices()
            .nth(char_idx)
            .map(|(b, _)| b)
            .unwrap_or(self.text.len())
    }

    /// Return the char count (not byte count) of `text`.
    pub(crate) fn char_count(&self) -> usize {
        self.text.chars().count()
    }
}
