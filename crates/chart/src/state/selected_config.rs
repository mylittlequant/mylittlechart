/// Configuration for selected primitive (for inline config toolbar)
#[derive(Debug, Clone, Default)]
pub struct SelectedPrimitiveConfig {
    /// Display name (e.g., "Линия тренда")
    pub name: String,
    /// Current stroke color (hex)
    pub color: String,
    /// Current line width
    pub width: f64,
    /// Current line style ("solid", "dashed", "dotted")
    pub style: String,
    /// Is primitive locked
    pub locked: bool,
    /// Text color (hex) - None if no text or uses stroke color
    pub text_color: Option<String>,
    /// Whether this primitive supports text
    pub supports_text: bool,
}
