//! Rendering foundation module
//!
//! This module provides the core abstractions for platform-agnostic rendering:
//!
//! - `RenderContext` - Trait for low-level drawing operations
//! - `InputState` - Platform-agnostic input state capture
//! - `FrameResult` - Return type from rendering with actions/cursors
//!
//! # Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │  Primitives, Chart, Widgets                                     │
//! │  (use RenderContext trait for drawing)                          │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  RenderContext trait                                            │
//! │  (fill_rect, stroke_line, fill_text, etc.)                      │
//! ├─────────────────────────────────────────────────────────────────┤
//! │  Platform Implementations (provided by application)             │
//! └─────────────────────────────────────────────────────────────────┘
//! ```
//!
//! # Usage
//!
//! ```ignore
//! use zengeld_chart::render::{RenderContext, InputState, FrameResult};
//!
//! fn render_frame(
//!     ctx: &mut dyn RenderContext,
//!     input: &InputState,
//! ) -> FrameResult {
//!     ctx.set_fill_color("#1e222d");
//!     ctx.fill_rect(0.0, 0.0, ctx.chart_width(), ctx.chart_height());
//!
//!     if input.is_hovered(some_rect) {
//!         ctx.set_fill_color("#2a2e39");
//!         ctx.fill_rect(some_rect.x, some_rect.y, some_rect.w, some_rect.h);
//!     }
//!
//!     FrameResult::default()
//! }
//! ```

mod context;
mod input_state;
mod result;

// Re-export chart-specific RenderContext trait (extends uzor::render::RenderContext)
pub use context::RenderContext;

// Re-export base rendering types from uzor-render
pub use uzor::render::{
    TextAlign, TextBaseline,
    crisp, crisp_rect,
    RenderOp, RenderOps, execute_ops,
};

// Re-export chart-specific helpers
pub use context::{
    render_primitive_text, render_primitive_text_rotated,
    render_text_with_background, measure_primitive_text,
};

// Re-export SVG rendering from uzor so that `crate::engine::render::draw_svg_icon`
// and `crate::engine::render::draw_svg_multicolor` continue to resolve correctly
// for all modal/panel code that imports from this path.
pub use uzor::render::{draw_svg_icon, draw_svg_multicolor};

// Re-export input state and frame result
pub use input_state::{
    InputState, MouseButton, ModifierKeys,
    PointerState, DragState, Rect,
};

pub use result::{
    FrameResult, CursorIcon, RenderAction,
};
