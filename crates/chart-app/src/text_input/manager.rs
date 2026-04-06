//! Central text-input manager — owns text/cursor/selection for all fields.

use std::collections::HashMap;

use crate::input::KeyPress;

use super::types::{FieldAction, FieldConfig, FieldId, FieldState, InputCapability};

// =============================================================================
// TextInputManager
// =============================================================================

/// Central text-input manager.
///
/// Lives on `ChartApp` as a plain field (not behind Arc or RefCell) and is
/// mutably accessed from input handlers. Renderers access it via a shared
/// reference to call `update_field`.
///
/// # Focus model
/// Only one field is focused at a time. Focusing a new field clears the
/// selection anchor on the previously focused field but does NOT clear its
/// text — text is owned by the manager and persists across focus changes.
pub struct TextInputManager {
    /// All registered fields keyed by their `FieldId`.
    fields: HashMap<FieldId, FieldState>,
    /// Currently focused field, if any.
    focused: Option<FieldId>,
    /// Field currently being drag-selected (set on drag_start, cleared on drag_end).
    drag_field: Option<FieldId>,
    /// Monotonically increasing frame counter. Incremented by `begin_frame`.
    current_frame: u64,
    /// Shared blink epoch: milliseconds timestamp reset when any cursor moves.
    blink_reset_time: u64,
}

impl TextInputManager {
    // =========================================================================
    // Lifecycle
    // =========================================================================

    /// Create the manager with all 19 fields pre-registered.
    /// Called once in `ChartApp::new`.
    pub fn new() -> Self {
        let mut fields = HashMap::new();

        let register = |fields: &mut HashMap<FieldId, FieldState>, id: FieldId, cfg: FieldConfig| {
            fields.insert(id, FieldState::new(cfg));
        };

        register(&mut fields, FieldId::HexColor,              FieldConfig::hex_color());
        register(&mut fields, FieldId::E2ePassphrase,         FieldConfig::password());
        register(&mut fields, FieldId::WizardPassphrase,      FieldConfig::password());
        register(&mut fields, FieldId::NewProfileName,        FieldConfig::text());
        register(&mut fields, FieldId::ConfirmPassphrase,     FieldConfig::password());
        register(&mut fields, FieldId::RecoveryKeyInput,      FieldConfig::text());
        register(&mut fields, FieldId::RecoveryKeyDisplay,    FieldConfig::read_only_display());
        register(&mut fields, FieldId::NewPassphrase,         FieldConfig::password());
        register(&mut fields, FieldId::ProfileRename,         FieldConfig::text());
        register(&mut fields, FieldId::ChartBrowserSearch,    FieldConfig::search());
        register(&mut fields, FieldId::WatchlistSearch,       FieldConfig::search());
        register(&mut fields, FieldId::WatchlistGroupName,    FieldConfig::text());
        register(&mut fields, FieldId::PresetName,            FieldConfig::text());
        register(&mut fields, FieldId::PrimitiveTemplateName, FieldConfig::text());
        register(&mut fields, FieldId::IndicatorTemplateName, FieldConfig::text());
        register(&mut fields, FieldId::CompareTemplateName,   FieldConfig::text());
        register(&mut fields, FieldId::ChartTemplateName,     FieldConfig::text());
        register(&mut fields, FieldId::NewKeyLabel,           FieldConfig::keyboard_only());
        register(&mut fields, FieldId::SymbolSearch,          FieldConfig::search());
        register(&mut fields, FieldId::AgentPty,              FieldConfig::raw());
        register(&mut fields, FieldId::AgentChat,             FieldConfig::text());

        Self {
            fields,
            focused: None,
            drag_field: None,
            current_frame: 0,
            blink_reset_time: 0,
        }
    }

    /// Must be called at the START of every render frame before any
    /// `update_field` calls. Advances the frame counter so stale
    /// geometry is automatically expired.
    pub fn begin_frame(&mut self) {
        self.current_frame = self.current_frame.wrapping_add(1);
    }

    // =========================================================================
    // Registration (called from renderer each frame)
    // =========================================================================

    /// Register or refresh the screen geometry of a field for this frame.
    ///
    /// Called by the renderer immediately after `draw_input` returns.
    /// `char_positions` is the list of char boundary X positions.
    /// Fields NOT updated this frame have `last_frame < current_frame` and
    /// the manager ignores mouse events targeting them.
    pub fn update_field(
        &mut self,
        id: FieldId,
        rect: (f64, f64, f64, f64),
        char_positions: Vec<f64>,
    ) {
        if let Some(state) = self.fields.get_mut(&id) {
            state.last_rect = Some(rect);
            state.last_char_positions = char_positions;
            state.last_frame = self.current_frame;
        }
    }

    /// Query the current text of a field.
    pub fn text(&self, id: FieldId) -> &str {
        self.fields.get(&id).map(|s| s.text.as_str()).unwrap_or("")
    }

    /// Query cursor position.
    pub fn cursor(&self, id: FieldId) -> usize {
        self.fields.get(&id).map(|s| s.cursor).unwrap_or(0)
    }

    /// Query selection range as `(lo, hi)` if a non-empty selection exists.
    pub fn selection_range(&self, id: FieldId) -> Option<(usize, usize)> {
        self.fields.get(&id)?.selection_range()
    }

    /// Query selection anchor (lo side of the selection).
    pub fn selection_start(&self, id: FieldId) -> Option<usize> {
        self.selection_range(id).map(|(lo, _)| lo)
    }

    /// Whether the field is currently focused.
    pub fn is_focused(&self, id: FieldId) -> bool {
        self.focused == Some(id)
    }

    /// Whether the cursor should be visible right now (500 ms blink).
    pub fn cursor_visible(&self, now_ms: u64) -> bool {
        let elapsed = now_ms.wrapping_sub(self.blink_reset_time);
        (elapsed / 500).is_multiple_of(2)
    }

    // =========================================================================
    // Focus management
    // =========================================================================

    /// Focus a field. Clears the selection anchor of the previously focused
    /// field. If `id` is already focused, this is a no-op.
    /// Returns `true` if focus changed.
    pub fn focus(&mut self, id: FieldId) -> bool {
        if self.focused == Some(id) {
            return false;
        }
        // Clear selection on the previously focused field.
        if let Some(prev) = self.focused {
            if let Some(state) = self.fields.get_mut(&prev) {
                state.selection_start = None;
            }
        }
        self.focused = Some(id);
        self.reset_blink();
        true
    }

    /// Remove focus from the currently focused field.
    pub fn blur(&mut self) {
        if let Some(id) = self.focused {
            if let Some(state) = self.fields.get_mut(&id) {
                state.selection_start = None;
            }
        }
        self.focused = None;
        self.drag_field = None;
    }

    /// Return currently focused field id.
    pub fn focused(&self) -> Option<FieldId> {
        self.focused
    }

    /// Set text programmatically. Positions cursor at end. Does NOT require focus.
    pub fn set_text(&mut self, id: FieldId, text: &str) {
        if let Some(state) = self.fields.get_mut(&id) {
            state.text = text.to_string();
            state.cursor = state.char_count();
            state.selection_start = None;
        }
    }

    /// Clear text and reset cursor/selection. Does NOT require focus.
    pub fn clear(&mut self, id: FieldId) {
        if let Some(state) = self.fields.get_mut(&id) {
            state.text.clear();
            state.cursor = 0;
            state.selection_start = None;
        }
    }

    /// Snapshot `text` into `original_text` so `Cancel` can revert to it.
    /// Call this when the field enters edit mode (modal opens, click-to-edit).
    pub fn begin_edit(&mut self, id: FieldId) {
        if let Some(state) = self.fields.get_mut(&id) {
            state.original_text = state.text.clone();
        }
    }

    // =========================================================================
    // Input dispatch
    // =========================================================================

    /// Handle a printable character (from `ChartApp::on_char_input`).
    pub fn on_char(&mut self, ch: char) -> FieldAction {
        let id = match self.focused {
            Some(id) => id,
            None => return FieldAction::None,
        };

        // Guard: Raw fields bypass all text-editing logic and return raw bytes.
        if self.fields.get(&id).map(|s| s.config.capability == InputCapability::Raw).unwrap_or(false) {
            return FieldAction::RawInput(raw_char_to_bytes(ch));
        }

        let state = match self.fields.get_mut(&id) {
            Some(s) => s,
            None => return FieldAction::None,
        };

        // Guard: Mouse-only or read-only fields reject all char input.
        if state.config.capability == InputCapability::Mouse || state.config.read_only {
            return FieldAction::None;
        }

        match ch {
            '\r' | '\n' => {
                let text = state.text.clone();
                FieldAction::Commit(text)
            }
            '\x1b' => {
                // Escape: revert to original_text.
                let original = state.original_text.clone();
                state.text = original;
                state.cursor = state.char_count();
                state.selection_start = None;
                self.reset_blink();
                FieldAction::Cancel
            }
            '\x08' => {
                // Backspace: delete selection or char before cursor.
                if state.selection_range().is_some() {
                    state.delete_selection();
                } else if state.cursor > 0 {
                    let byte_pos = state.char_to_byte(state.cursor - 1);
                    let byte_end = state.char_to_byte(state.cursor);
                    state.text.drain(byte_pos..byte_end);
                    state.cursor -= 1;
                }
                self.reset_blink();
                let text = self.fields[&id].text.clone();
                FieldAction::TextChanged(text)
            }
            c if c.is_control() => FieldAction::None,
            c => {
                // Check char filter.
                if let Some(filter) = state.config.char_filter {
                    if !filter(c) {
                        return FieldAction::None;
                    }
                }
                // Check max_len (applies to text after deleting any selection).
                let text_len_after_delete = if state.selection_range().is_some() {
                    // Selection will be replaced.
                    let (lo, hi) = state.selection_range().unwrap();
                    state.char_count() - (hi - lo)
                } else {
                    state.char_count()
                };
                if let Some(max) = state.config.max_len {
                    if text_len_after_delete >= max {
                        return FieldAction::None;
                    }
                }
                // Delete selection first, then insert.
                if state.selection_range().is_some() {
                    state.delete_selection();
                }
                let byte_pos = state.char_to_byte(state.cursor);
                state.text.insert(byte_pos, c);
                state.cursor += 1;
                self.reset_blink();
                let text = self.fields[&id].text.clone();
                FieldAction::TextChanged(text)
            }
        }
    }

    /// Handle a named key press (from `ChartApp::on_key_press`).
    pub fn on_key(&mut self, key: KeyPress) -> FieldAction {
        let id = match self.focused {
            Some(id) => id,
            None => return FieldAction::None,
        };

        // Guard: Raw fields bypass all text-editing logic and return raw bytes.
        if self.fields.get(&id).map(|s| s.config.capability == InputCapability::Raw).unwrap_or(false) {
            if let Some(bytes) = key_to_pty_bytes(&key) {
                return FieldAction::RawInput(bytes);
            }
            return FieldAction::None;
        }

        let state = match self.fields.get_mut(&id) {
            Some(s) => s,
            None => return FieldAction::None,
        };

        // Mouse-only or read-only: only Copy and SelectAll pass through.
        let restricted = state.config.capability == InputCapability::Mouse || state.config.read_only;
        if restricted {
            match &key {
                KeyPress::Copy | KeyPress::SelectAll => {}
                _ => return FieldAction::None,
            }
        }

        let consumed = Self::apply_key_to(state, key);
        if consumed {
            self.reset_blink();
        }
        FieldAction::None
    }

    /// Begin a mouse drag on the field whose `last_rect` contains `(x, y)`.
    pub fn on_drag_start(&mut self, x: f64, y: f64) {
        // Find the field that was updated this frame and contains the point.
        let mut hit_id: Option<FieldId> = None;
        for (id, state) in &self.fields {
            // Keyboard-only fields ignore mouse.
            if state.config.capability == InputCapability::Keyboard {
                continue;
            }
            let frame_lag = self.current_frame.wrapping_sub(state.last_frame);
            // Accept geometry from this frame or the previous one — input events
            // arrive after begin_frame() but before update_field() re-stamps the
            // geometry, so last_frame is typically current_frame - 1.
            if frame_lag > 1 {
                continue;
            }
            if let Some((rx, ry, rw, rh)) = state.last_rect {
                if x >= rx && x <= rx + rw && y >= ry && y <= ry + rh {
                    hit_id = Some(*id);
                    break;
                }
            }
        }

        let id = match hit_id {
            Some(id) => id,
            None => return,
        };

        // Focus the field (clears other selections).
        self.focus(id);

        let state = match self.fields.get_mut(&id) {
            Some(s) => s,
            None => return,
        };

        // Position cursor at click point and start selection anchor.
        let cursor = Self::cursor_from_x(&state.last_char_positions, x);
        state.cursor = cursor;
        state.selection_start = Some(cursor);
        self.drag_field = Some(id);
        self.reset_blink();
    }

    /// Update the drag-selection cursor as the mouse moves.
    pub fn on_drag_move(&mut self, x: f64) {
        let id = match self.drag_field {
            Some(id) => id,
            None => return,
        };

        let state = match self.fields.get_mut(&id) {
            Some(s) => s,
            None => return,
        };

        let new_cursor = Self::cursor_from_x(&state.last_char_positions, x);
        state.cursor = new_cursor;
        // Keep the selection_start anchor from drag_start; cursor moves.
        // Do NOT clear selection_start here even if cursor == anchor —
        // the user may drag back. Degenerate check is in on_drag_end.
    }

    /// End the drag-selection. Clears `drag_field`.
    pub fn on_drag_end(&mut self) {
        if let Some(id) = self.drag_field {
            if let Some(state) = self.fields.get_mut(&id) {
                // Clear degenerate (zero-width) selections.
                if state.selection_start == Some(state.cursor) {
                    state.selection_start = None;
                }
            }
        }
        self.drag_field = None;
    }

    /// Return the selected text of the currently focused field (or drag field)
    /// for placement on the system clipboard.
    pub fn copy_selection(&self) -> Option<String> {
        let id = self.focused.or(self.drag_field)?;
        let state = self.fields.get(&id)?;
        let (lo, hi) = state.selection_range()?;
        let byte_lo = state.char_to_byte(lo);
        let byte_hi = state.char_to_byte(hi);
        Some(state.text[byte_lo..byte_hi].to_string())
    }

    // =========================================================================
    // Private helpers
    // =========================================================================

    /// Apply a key event to a given `FieldState`. Returns true if consumed.
    fn apply_key_to(state: &mut FieldState, key: KeyPress) -> bool {
        let char_count = state.char_count();

        match key {
            // ── Delete (forward) ────────────────────────────────────────────
            KeyPress::Delete => {
                if state.selection_range().is_some() {
                    state.delete_selection();
                } else if state.cursor < char_count {
                    let byte_idx = state.char_to_byte(state.cursor);
                    state.text.remove(byte_idx);
                }
                true
            }
            // ── Plain movement — collapses any active selection ──────────────
            KeyPress::ArrowLeft => {
                if state.selection_range().is_some() {
                    let (lo, _) = state.selection_range().unwrap();
                    state.cursor = lo;
                    state.selection_start = None;
                } else {
                    state.cursor = state.cursor.saturating_sub(1);
                }
                true
            }
            KeyPress::ArrowRight => {
                if state.selection_range().is_some() {
                    let (_, hi) = state.selection_range().unwrap();
                    state.cursor = hi;
                    state.selection_start = None;
                } else if state.cursor < char_count {
                    state.cursor += 1;
                }
                true
            }
            KeyPress::Home => {
                state.cursor = 0;
                state.selection_start = None;
                true
            }
            KeyPress::End => {
                state.cursor = char_count;
                state.selection_start = None;
                true
            }
            // ── Select-all (Ctrl+A) ──────────────────────────────────────────
            KeyPress::SelectAll => {
                state.selection_start = Some(0);
                state.cursor = char_count;
                true
            }
            // ── Shift movement — extends/creates selection ───────────────────
            KeyPress::ShiftLeft => {
                if state.selection_start.is_none() {
                    state.selection_start = Some(state.cursor);
                }
                state.cursor = state.cursor.saturating_sub(1);
                if state.selection_start == Some(state.cursor) {
                    state.selection_start = None;
                }
                true
            }
            KeyPress::ShiftRight => {
                if state.selection_start.is_none() {
                    state.selection_start = Some(state.cursor);
                }
                if state.cursor < char_count {
                    state.cursor += 1;
                }
                if state.selection_start == Some(state.cursor) {
                    state.selection_start = None;
                }
                true
            }
            KeyPress::ShiftHome => {
                if state.selection_start.is_none() {
                    state.selection_start = Some(state.cursor);
                }
                state.cursor = 0;
                if state.selection_start == Some(state.cursor) {
                    state.selection_start = None;
                }
                true
            }
            KeyPress::ShiftEnd => {
                if state.selection_start.is_none() {
                    state.selection_start = Some(state.cursor);
                }
                state.cursor = char_count;
                if state.selection_start == Some(state.cursor) {
                    state.selection_start = None;
                }
                true
            }
            // ── Copy (Ctrl+C) — handled externally ──────────────────────────
            KeyPress::Copy => false,
            // ── Paste (Ctrl+V) ───────────────────────────────────────────────
            KeyPress::Paste(ref text) => {
                if state.config.read_only {
                    return false;
                }
                if state.selection_range().is_some() {
                    state.delete_selection();
                }
                for ch in text.chars() {
                    if ch.is_control() {
                        continue;
                    }
                    if let Some(filter) = state.config.char_filter {
                        if !filter(ch) {
                            continue;
                        }
                    }
                    if let Some(max) = state.config.max_len {
                        if state.char_count() >= max {
                            break;
                        }
                    }
                    let byte_pos = state.char_to_byte(state.cursor);
                    state.text.insert(byte_pos, ch);
                    state.cursor += 1;
                }
                true
            }
            // ── Undo/Redo — not consumed by text fields ──────────────────────
            KeyPress::Undo | KeyPress::Redo => false,
        }
    }

    /// Compute cursor index from x position using char boundary X positions.
    ///
    /// Same logic as the existing `cursor_from_char_positions` helper.
    fn cursor_from_x(positions: &[f64], x: f64) -> usize {
        if positions.is_empty() {
            return 0;
        }
        // positions[i] = left edge of char i; positions.last() = right edge of last char.
        let char_count = positions.len().saturating_sub(1);
        for i in 0..char_count {
            let left = positions[i];
            let right = positions[i + 1];
            let mid = (left + right) * 0.5;
            if x < mid {
                return i;
            }
        }
        char_count
    }

    /// Reset the blink timer to "cursor visible" state.
    fn reset_blink(&mut self) {
        // We use a fake epoch: we don't have access to real time here.
        // The actual reset time is set to 0 and the caller should ensure
        // `cursor_visible(now_ms)` is called with the real timestamp.
        // In practice, `blink_reset_time` is set from the platform clock
        // when a cursor movement occurs — so we store the last frame's timestamp.
        // For now, set to a sentinel that `cursor_visible` will treat as "just reset":
        // the caller will override this with the real timestamp if needed.
        // Since we don't have access to SystemTime here, we mark the need to reset
        // by setting blink_reset_time to a value that makes cursor visible.
        // The platform runner calls cursor_visible(now_ms) — if blink_reset_time
        // is close to now_ms, elapsed ≈ 0, so (0/500)%2==0 → visible.
        // We set it to u64::MAX so elapsed = now.wrapping_sub(u64::MAX) ≈ now+1,
        // which for reasonable now_ms values gives a large elapsed that could be
        // odd phase. Instead: we expose set_blink_time() for the platform to call.
        // For internal use, mark needs_blink_reset = true. But since we don't have
        // that field, we just do nothing here and let the platform call set_blink_time.
        // Actually: set to 0 means elapsed = now_ms, which for large now_ms gives
        // (now_ms / 500) % 2 — could be either phase. Not ideal.
        //
        // Simplest correct approach: set blink_reset_time = 0 and have the platform
        // runner call `set_blink_time(now_ms)` after any input event. But since we
        // don't want to add that complexity now, we set blink_reset_time to u64::MAX
        // so that wrapping_sub produces 0..1 for any reasonable now_ms:
        // elapsed = now_ms.wrapping_sub(u64::MAX) = now_ms + 1
        // which is still not 0. The cleanest solution is to have the caller
        // pass now_ms to reset_blink. Since that requires API changes to on_char etc,
        // we instead expose a separate method:
        self.blink_reset_time = 0; // will be overridden by set_blink_time()
    }

    /// Set the blink reset time from the platform clock.
    ///
    /// Call this whenever `on_char`, `on_key`, `on_drag_start`, or `focus`
    /// returns a non-None action, to reset the cursor blink phase.
    pub fn set_blink_time(&mut self, now_ms: u64) {
        self.blink_reset_time = now_ms;
    }
}

impl Default for TextInputManager {
    fn default() -> Self {
        Self::new()
    }
}

// =============================================================================
// PTY byte helpers
// =============================================================================

/// Encode a printable (or special control) character as PTY bytes.
fn raw_char_to_bytes(ch: char) -> Vec<u8> {
    if ch == '\r' || ch == '\n' {
        return vec![b'\r'];
    }
    if ch == '\x08' {
        return vec![0x7f];
    }
    if ch == '\x1b' {
        return vec![0x1b];
    }
    if (ch as u32) < 0x20 {
        return vec![ch as u8];
    }
    let mut buf = [0u8; 4];
    let s = ch.encode_utf8(&mut buf);
    s.as_bytes().to_vec()
}

/// Map named key presses to their ANSI escape sequences for PTY forwarding.
/// Returns `None` for keys that have no PTY representation.
fn key_to_pty_bytes(key: &KeyPress) -> Option<Vec<u8>> {
    match key {
        KeyPress::ArrowLeft => Some(b"\x1b[D".to_vec()),
        KeyPress::ArrowRight => Some(b"\x1b[C".to_vec()),
        KeyPress::Home => Some(b"\x1b[H".to_vec()),
        KeyPress::End => Some(b"\x1b[F".to_vec()),
        KeyPress::Delete => Some(b"\x1b[3~".to_vec()),
        _ => None,
    }
}

// =============================================================================
// Unit tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    fn make_manager() -> TextInputManager {
        TextInputManager::new()
    }

    #[test]
    fn test_hex_filter_rejects_non_hex_chars() {
        let mut m = make_manager();
        m.focus(FieldId::HexColor);
        m.set_text(FieldId::HexColor, "#ff0000");
        m.begin_edit(FieldId::HexColor);
        // 'Z' is not a hex digit and not '#'
        let action = m.on_char('Z');
        assert_eq!(action, FieldAction::None);
        assert_eq!(m.text(FieldId::HexColor), "#ff0000");
    }

    #[test]
    fn test_hex_max_len() {
        let mut m = make_manager();
        m.focus(FieldId::HexColor);
        m.set_text(FieldId::HexColor, "#rrggbbaa"); // 9 chars at max
        // Replace with valid 9-char hex
        m.set_text(FieldId::HexColor, "#aabbccdd");
        m.begin_edit(FieldId::HexColor);
        // Inserting a 10th char should be a no-op
        let action = m.on_char('e');
        assert_eq!(action, FieldAction::None);
        assert_eq!(m.text(FieldId::HexColor).chars().count(), 9);
    }

    #[test]
    fn test_read_only_blocks_char() {
        let mut m = make_manager();
        m.focus(FieldId::RecoveryKeyDisplay);
        m.set_text(FieldId::RecoveryKeyDisplay, "ABC");
        let action = m.on_char('X');
        assert_eq!(action, FieldAction::None);
        assert_eq!(m.text(FieldId::RecoveryKeyDisplay), "ABC");
    }

    #[test]
    fn test_read_only_allows_select_all() {
        let mut m = make_manager();
        m.focus(FieldId::RecoveryKeyDisplay);
        m.set_text(FieldId::RecoveryKeyDisplay, "hello");
        let action = m.on_key(KeyPress::SelectAll);
        assert_eq!(action, FieldAction::None); // no special action, but selection applied
        // Selection should cover the whole text
        assert_eq!(m.selection_range(FieldId::RecoveryKeyDisplay), Some((0, 5)));
    }

    #[test]
    fn test_keyboard_only_ignores_drag() {
        let mut m = make_manager();
        m.focus(FieldId::NewKeyLabel);
        m.set_text(FieldId::NewKeyLabel, "my-key");
        m.begin_frame();
        // Register a rect for it
        m.update_field(FieldId::NewKeyLabel, (10.0, 10.0, 100.0, 30.0), vec![10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0]);
        m.on_drag_start(15.0, 15.0); // inside the rect
        // Focus should stay on NewKeyLabel (drag was ignored because Keyboard-only)
        // drag_field should be None since Keyboard-only fields skip drag
        assert!(m.drag_field.is_none());
    }

    #[test]
    fn test_focus_clears_other_selections() {
        let mut m = make_manager();
        m.focus(FieldId::HexColor);
        m.set_text(FieldId::HexColor, "#ff0000");
        // Manually set a selection anchor on HexColor
        if let Some(s) = m.fields.get_mut(&FieldId::HexColor) {
            s.selection_start = Some(0);
            s.cursor = 3;
        }
        // Now focus a different field
        m.focus(FieldId::ChartBrowserSearch);
        // HexColor's selection_start should be cleared
        assert!(m.fields[&FieldId::HexColor].selection_start.is_none());
    }

    #[test]
    fn test_begin_edit_cancel_reverts() {
        let mut m = make_manager();
        m.focus(FieldId::HexColor);
        m.set_text(FieldId::HexColor, "#ff0000");
        m.begin_edit(FieldId::HexColor);
        // Type a valid hex char
        m.on_char('a');
        assert_ne!(m.text(FieldId::HexColor), "#ff0000");
        // Escape to cancel
        let action = m.on_char('\x1b');
        assert_eq!(action, FieldAction::Cancel);
        assert_eq!(m.text(FieldId::HexColor), "#ff0000");
    }

    #[test]
    fn test_blink_visibility() {
        let m = TextInputManager::new();
        // blink_reset_time = 0, now_ms = 0 → elapsed = 0 → (0/500)%2 = 0 → visible
        assert!(m.cursor_visible(0));
        // now_ms = 600 → elapsed = 600 → (600/500)%2 = 1 → not visible
        assert!(!m.cursor_visible(600));
        // now_ms = 1100 → elapsed = 1100 → (1100/500)%2 = (2)%2 = 0 → visible
        assert!(m.cursor_visible(1100));
    }

    #[test]
    fn test_drag_updates_cursor_not_anchor() {
        let mut m = make_manager();
        m.focus(FieldId::HexColor);
        m.set_text(FieldId::HexColor, "#aabbcc");
        m.begin_frame();
        // positions: left edges of chars 0..7 + right edge of char 6
        let positions: Vec<f64> = (0..=7).map(|i| i as f64 * 10.0).collect(); // [0,10,20,30,40,50,60,70]
        m.update_field(FieldId::HexColor, (0.0, 0.0, 70.0, 20.0), positions);
        // drag_start at x=35 → between char 3 (30) and char 4 (40), mid=35 → cursor=3 OR 4
        // Let's use x=25 → between char 2 (mid=25) → cursor=2 or 3
        m.on_drag_start(25.0, 5.0);
        let anchor = m.fields[&FieldId::HexColor].selection_start;
        let cursor_after_start = m.cursor(FieldId::HexColor);
        assert!(anchor.is_some());
        assert_eq!(anchor.unwrap(), cursor_after_start);
        // drag_move to x=55 → between char 5 (50) and 6 (60), mid=55 → cursor=5 or 6
        m.on_drag_move(55.0);
        let cursor_after_move = m.cursor(FieldId::HexColor);
        assert_ne!(cursor_after_move, cursor_after_start);
        assert_eq!(m.fields[&FieldId::HexColor].selection_start, anchor);
    }

    #[test]
    fn test_stale_geometry_ignores_drag() {
        let mut m = make_manager();
        m.focus(FieldId::HexColor);
        m.set_text(FieldId::HexColor, "#ff0000");
        // Register in frame 0
        m.update_field(FieldId::HexColor, (0.0, 0.0, 70.0, 20.0), vec![0.0, 10.0, 20.0, 30.0, 40.0, 50.0, 60.0, 70.0]);
        // Advance frame without updating
        m.begin_frame();
        // Now drag_start — field not updated this frame, should be no-op
        m.on_drag_start(5.0, 5.0);
        assert!(m.drag_field.is_none());
    }

    #[test]
    fn test_utf8_cursor_arithmetic() {
        let mut m = make_manager();
        m.focus(FieldId::ChartBrowserSearch);
        m.begin_edit(FieldId::ChartBrowserSearch);
        // Insert Cyrillic chars (2-byte UTF-8 each)
        m.on_char('А'); // Cyrillic A
        m.on_char('Б');
        m.on_char('В');
        assert_eq!(m.text(FieldId::ChartBrowserSearch).chars().count(), 3);
        assert_eq!(m.cursor(FieldId::ChartBrowserSearch), 3);
        // Backspace removes one char
        m.on_char('\x08');
        assert_eq!(m.text(FieldId::ChartBrowserSearch).chars().count(), 2);
        assert_eq!(m.cursor(FieldId::ChartBrowserSearch), 2);
    }

    #[test]
    fn test_paste_respects_filter() {
        let mut m = make_manager();
        m.focus(FieldId::HexColor);
        m.set_text(FieldId::HexColor, "");
        m.begin_edit(FieldId::HexColor);
        // Paste "##ZZGG" — only ## should survive (# and hex digits allowed, Z/G rejected)
        // Actually '#' is allowed and 'G' is not a hex digit (a-f only), 'Z' not.
        m.on_key(KeyPress::Paste("##ZZGG".to_string()));
        let text = m.text(FieldId::HexColor).to_string();
        // '#' is accepted, '#' second would make 2 chars, then Z,Z,G,G rejected
        for ch in text.chars() {
            assert!(ch == '#' || ch.is_ascii_hexdigit(), "unexpected char '{}'", ch);
        }
    }
}
