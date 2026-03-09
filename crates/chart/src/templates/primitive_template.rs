//! Template for drawing primitive style settings.
//!
//! A [`PrimitiveTemplate`] captures the *style* of an existing primitive (color,
//! width, line style, text formatting, Fibonacci levels, etc.) so that the same
//! look can be applied to new primitives of the same type.  Positional data and
//! content (text) are intentionally excluded — the user fills those in at
//! draw time.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::drawing::primitives_v2::{
    config::FibLevelConfig,
    // PrimitiveColor, PrimitiveText, TextAlign, and LineStyle are re-exported
    // from the primitives_v2 module (types sub-module is private).
    PrimitiveColor, PrimitiveText, TextAlign, LineStyle,
    PrimitiveData,
};
use crate::preset::preset::unix_timestamp_parts;

// =============================================================================
// PrimitiveTemplate
// =============================================================================

/// Saved style configuration for a drawing primitive.
///
/// Contains everything needed to reproduce a primitive's *appearance* but not
/// its geometry or text content.  Apply to an existing primitive with
/// [`PrimitiveTemplate::apply_to_primitive_data`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrimitiveTemplate {
    /// Unique identifier generated at creation time.
    /// Format: `"ptmpl_{unix_secs}_{nanos_suffix}"`.
    pub id: String,
    /// User-visible display name for this template.
    pub name: String,
    /// Primitive type this template applies to (e.g. `"trend_line"`, `"fib_retracement"`).
    pub type_id: String,

    // ---- Core style --------------------------------------------------------
    /// Stroke and optional fill color.
    pub color: PrimitiveColor,
    /// Line width in pixels.
    pub width: f64,
    /// Line dash style.
    pub style: LineStyle,

    // ---- Text formatting (content is not saved — user provides it) ---------
    /// Font size in pixels.
    #[serde(default)]
    pub text_font_size: Option<f64>,
    /// Text color override (hex string).  `None` means inherit stroke color.
    #[serde(default)]
    pub text_color: Option<String>,
    /// Bold text.
    #[serde(default)]
    pub text_bold: Option<bool>,
    /// Italic text.
    #[serde(default)]
    pub text_italic: Option<bool>,
    /// Horizontal alignment string (`"start"`, `"center"`, `"end"`).
    #[serde(default)]
    pub text_h_align: Option<String>,
    /// Vertical alignment string (`"start"`, `"center"`, `"end"`).
    #[serde(default)]
    pub text_v_align: Option<String>,

    // ---- Level configuration (Fibonacci, Gann, etc.) -----------------------
    /// Per-level configuration for multi-level primitives.
    #[serde(default)]
    pub level_configs: Vec<FibLevelConfig>,

    // ---- Type-specific numeric style properties ----------------------------
    /// Catch-all map for primitive-specific style values (e.g. `"fill_opacity"`,
    /// `"extend_mode"`).  Values are plain `f64` to keep the schema simple.
    #[serde(default)]
    pub style_properties: HashMap<String, f64>,
}

impl PrimitiveTemplate {
    // ---- Constructors ------------------------------------------------------

    /// Create a template with sensible defaults for the given `type_id`.
    pub fn new_with_defaults(type_id: &str, name: &str) -> Self {
        let (secs, nanos) = unix_timestamp_parts();
        Self {
            id: format!("ptmpl_{}_{}", secs, nanos),
            name: name.to_string(),
            type_id: type_id.to_string(),
            color: PrimitiveColor::default(),
            width: 1.0,
            style: LineStyle::default(),
            text_font_size: None,
            text_color: None,
            text_bold: None,
            text_italic: None,
            text_h_align: None,
            text_v_align: None,
            level_configs: Vec::new(),
            style_properties: HashMap::new(),
        }
    }

    /// Extract style fields from an existing [`PrimitiveData`] into a new template.
    ///
    /// The template name is taken from `name`.  Positional data and text content
    /// are not copied.
    pub fn from_primitive_data(data: &PrimitiveData, name: &str) -> Self {
        let (secs, nanos) = unix_timestamp_parts();

        let (text_font_size, text_color, text_bold, text_italic, text_h_align, text_v_align) =
            if let Some(text) = &data.text {
                (
                    Some(text.font_size),
                    text.color.clone(),
                    Some(text.bold),
                    Some(text.italic),
                    Some(text.h_align.as_str().to_string()),
                    Some(text.v_align.as_str().to_string()),
                )
            } else {
                (None, None, None, None, None, None)
            };

        Self {
            id: format!("ptmpl_{}_{}", secs, nanos),
            name: name.to_string(),
            type_id: data.type_id.clone(),
            color: data.color.clone(),
            width: data.width,
            style: data.style.clone(),
            text_font_size,
            text_color,
            text_bold,
            text_italic,
            text_h_align,
            text_v_align,
            // Level configs and style_properties are not available on
            // PrimitiveData directly (they live in the concrete primitive
            // impl), so we start with empty collections.
            level_configs: Vec::new(),
            style_properties: HashMap::new(),
        }
    }

    // ---- Application -------------------------------------------------------

    /// Apply the template's style to an existing [`PrimitiveData`].
    ///
    /// Only style fields are written; positional data, visibility, lock state,
    /// sync mode, and text content are left untouched.
    pub fn apply_to_primitive_data(&self, data: &mut PrimitiveData) {
        data.color = self.color.clone();
        data.width = self.width;
        data.style = self.style.clone();

        // Apply text formatting if the primitive already has a text block.
        if let Some(text) = &mut data.text {
            apply_text_template(text, self);
        }
    }

    /// Returns `true` if this template stores level configuration.
    pub fn has_levels(&self) -> bool {
        !self.level_configs.is_empty()
    }
}

// =============================================================================
// Helpers
// =============================================================================

/// Write template text fields onto an existing [`PrimitiveText`].
fn apply_text_template(text: &mut PrimitiveText, tmpl: &PrimitiveTemplate) {
    if let Some(size) = tmpl.text_font_size {
        text.font_size = size;
    }
    if tmpl.text_color.is_some() {
        text.color = tmpl.text_color.clone();
    }
    if let Some(bold) = tmpl.text_bold {
        text.bold = bold;
    }
    if let Some(italic) = tmpl.text_italic {
        text.italic = italic;
    }
    if let Some(ref align) = tmpl.text_h_align {
        text.h_align = TextAlign::from_str(align);
    }
    if let Some(ref align) = tmpl.text_v_align {
        text.v_align = TextAlign::from_str(align);
    }
}
