//! Visual theme for chart toolbar / modal chrome rendering.
//!
//! Extracted 2026-07-29 from the deleted `toolbar_render.rs` (its
//! `render_panel_toolbar` composer had zero callers — the live chart
//! toolbar composer is `toolbar_core::draw_toolbar_with_icons`). The
//! theme struct is consumed by every modal/overlay as the palette type.

/// Visual theme for toolbar rendering.
#[derive(Clone, Debug)]
pub struct ToolbarTheme {
    /// Toolbar background fill colour (CSS hex or rgba string).
    pub background: String,
    /// Dropdown / context menu background colour.
    ///
    /// This is a separate slot from `background` because dropdown menus
    /// often use a slightly different shade (e.g. slightly lighter/darker
    /// than the toolbar).  Modals that render inline dropdowns should use
    /// this field rather than `background`.
    pub dropdown_bg: String,
    /// Separator line colour.
    pub separator: String,
    /// Item background when hovered.
    pub item_bg_hover: String,
    /// Item background when active/selected.
    pub item_bg_active: String,
    /// Button background colour (idle state).
    pub button_bg: String,
    /// Button background colour on hover.
    pub button_bg_hover: String,
    /// Default icon / label colour.
    pub item_text: String,
    /// Muted icon / label colour (for secondary/disabled items).
    pub item_text_muted: String,
    /// Colour for hidden (invisible) items — item_text at 50% opacity.
    pub item_text_hidden: String,
    /// Icon / label colour on hover.
    pub item_text_hover: String,
    /// Icon / label colour when active.
    pub item_text_active: String,
    /// Accent colour for active indicator dots etc.
    pub accent: String,
    /// Accent colour on hover.
    pub accent_hover: String,
    /// Success colour (confirmations, positive states).
    pub success: String,
    /// Danger colour (errors, destructive actions, logout).
    pub danger: String,
    /// Warning colour (cautions, alerts).
    pub warning: String,
    /// When `true` the toolbar is rendered in a compact sidebar style
    /// (no background blur, narrower padding).
    pub sidebar_style: bool,
    /// Background colour for embedded terminal (PTY) panes.
    pub terminal_bg: String,
    /// Pane header background — focused state.
    pub pane_header_focused: String,
    /// Pane header background — hovered state.
    pub pane_header_hover: String,
    /// Pane header background — idle state.
    pub pane_header_idle: String,
    /// User chat bubble background.
    pub bubble_user_bg: String,
    /// User chat bubble border.
    pub bubble_user_border: String,
    /// User chat bubble text.
    pub bubble_user_text: String,
    /// Tool output bubble background.
    pub bubble_tool_bg: String,
    /// Tool output bubble text.
    pub bubble_tool_text: String,
    /// Text selection highlight color (used with alpha).
    pub selection: String,
    /// Danger button hover background (dark red).
    pub danger_hover_bg: String,
}

impl Default for ToolbarTheme {
    fn default() -> Self {
        Self {
            background: "#1e1e2e".into(),
            dropdown_bg: "#1e222d".into(),
            separator: "#313244".into(),
            item_bg_hover: "#45475a".into(),
            item_bg_active: "#585b70".into(),
            button_bg: "#1e222d".into(),
            button_bg_hover: "#2a2e39".into(),
            item_text: "#a6adc8".into(),
            item_text_muted: "#6c7086".into(),
            item_text_hidden: "#a6adc880".into(),
            item_text_hover: "#cdd6f4".into(),
            item_text_active: "#cdd6f4".into(),
            accent: "#3b82f6".into(),
            accent_hover: "#1e53e4".into(),
            success: "#26a69a".into(),
            danger: "#f23645".into(),
            warning: "#ff9800".into(),
            terminal_bg: "#131722".into(),
            pane_header_focused: "#1a1a2e".into(),
            pane_header_hover: "#171725".into(),
            pane_header_idle: "#13131c".into(),
            bubble_user_bg: "#1e1e3f".into(),
            bubble_user_border: "#6366f1".into(),
            bubble_user_text: "#e0e0ff".into(),
            bubble_tool_bg: "#0a0a14".into(),
            bubble_tool_text: "#a0a0b0".into(),
            selection: "#4a7bc8".into(),
            danger_hover_bg: "#7f1d1d".into(),
            sidebar_style: false,
        }
    }
}
