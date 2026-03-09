//! Default chart input handler implementation
//!
//! This module provides a platform-agnostic implementation of the
//! `ChartInputHandler` trait.
//!
//! # Overview
//!
//! The handler processes `ChartInputAction` events and produces
//! `ChartOutputAction` commands that tell the chart what to do.
//!
//! # Architecture
//!
//! ```text
//! User Input (mouse, keyboard, touch)
//!        |
//!        v
//! Platform Adapter (application-specific)
//!        |
//!        v
//! ChartInputAction (semantic action)
//!        |
//!        v
//! DefaultChartInputHandler (this module)
//!        |
//!        v
//! ChartOutputAction (commands)
//!        |
//!        v
//! Chart State Update
//! ```
//!
//! # Example
//!
//! ```ignore
//! use zengeld_chart::input::{
//!     DefaultChartInputHandler, ChartInputAction, ChartOutputAction,
//!     ChartHitTester, InputHandlerConfig,
//! };
//!
//! let mut handler = DefaultChartInputHandler::new();
//!
//! let action = ChartInputAction::Pan { delta_x: 10.0, delta_y: 0.0 };
//! let outputs = handler.handle_action(action, &my_hit_tester);
//!
//! for output in outputs {
//!     match output {
//!         ChartOutputAction::Repaint => chart.request_repaint(),
//!         ChartOutputAction::UpdateCursor(cursor) => set_cursor(cursor),
//!         _ => {}
//!     }
//! }
//! ```

use super::traits::{ChartHitTester, HitResult};
use super::super::events::{ChartInputAction, DragMode, MouseButton};
use super::super::objects::CursorStyle;
use crate::drawing::ControlPointType;

// =============================================================================
// Configuration
// =============================================================================

/// Configuration for the input handler.
///
/// Controls various thresholds and factors used during input processing.
#[derive(Debug, Clone)]
pub struct InputHandlerConfig {
    /// Minimum pixel distance to distinguish drag from click.
    ///
    /// Default: 5.0 pixels
    pub drag_threshold: f64,

    /// Friction coefficient for kinetic scrolling.
    ///
    /// Higher values mean faster deceleration.
    /// Default: 0.95
    pub kinetic_friction: f64,

    /// Minimum velocity to start kinetic scrolling.
    ///
    /// Default: 5.0 pixels/ms
    pub kinetic_min_velocity: f64,

    /// Zoom factor per scroll wheel step.
    ///
    /// Default: 0.05 (5% per step)
    pub scroll_zoom_factor: f64,

    /// Price scale drag sensitivity.
    ///
    /// Default: 0.005
    pub price_scale_drag_factor: f64,

    /// Time scale drag sensitivity.
    ///
    /// Default: 0.003
    pub time_scale_drag_factor: f64,
}

impl Default for InputHandlerConfig {
    fn default() -> Self {
        Self {
            drag_threshold: 5.0,
            kinetic_friction: 0.95,
            kinetic_min_velocity: 5.0,
            scroll_zoom_factor: 0.05,
            price_scale_drag_factor: 0.005,
            time_scale_drag_factor: 0.003,
        }
    }
}

impl InputHandlerConfig {
    /// Create configuration with custom drag threshold.
    pub fn with_drag_threshold(mut self, threshold: f64) -> Self {
        self.drag_threshold = threshold;
        self
    }

    /// Create configuration with custom scroll zoom factor.
    pub fn with_scroll_zoom_factor(mut self, factor: f64) -> Self {
        self.scroll_zoom_factor = factor;
        self
    }
}

// =============================================================================
// Input State
// =============================================================================

/// Tracks the current state of input handling.
///
/// This struct maintains all state needed to process multi-step interactions
/// like drags, kinetic scrolling, and primitive manipulation.
#[derive(Debug, Clone, Default)]
pub struct ChartInputState {
    /// Current drag mode.
    pub drag_mode: DragMode,

    /// Drag start position in screen coordinates.
    pub drag_start: Option<(f64, f64)>,

    /// Viewport view_start at drag start (for undo).
    pub drag_start_view: Option<f64>,

    /// Bar spacing at drag start (for time scale zoom).
    pub drag_start_spacing: Option<f64>,

    /// Price scale minimum at drag start (for undo).
    pub drag_start_price_min: Option<f64>,

    /// Price scale maximum at drag start (for undo).
    pub drag_start_price_max: Option<f64>,

    /// Last drag position for velocity calculation.
    pub last_drag_pos: Option<(f64, f64)>,

    /// Timestamp of last drag position (milliseconds).
    pub last_drag_time: Option<f64>,

    /// Current kinetic velocity (pixels per millisecond).
    pub kinetic_velocity: (f64, f64),

    /// Whether kinetic scrolling is active.
    pub kinetic_active: bool,

    /// Constrain proportions (e.g., Ctrl key held for square/circle).
    pub constrain_proportions: bool,

    /// Currently selected primitive index.
    pub selected_primitive: Option<usize>,

    /// Index of primitive being dragged.
    pub dragging_primitive_idx: Option<usize>,

    /// Control point being dragged (if any).
    pub dragging_control_point: Option<ControlPointType>,

    /// Pane ID when dragging a primitive in a sub-pane.
    pub dragging_pane_id: Option<u64>,

    /// Initial primitive points for undo.
    pub drag_primitive_initial_points: Option<Vec<(f64, f64)>>,
}

impl ChartInputState {
    /// Create a new input state.
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset all drag-related state.
    pub fn reset_drag(&mut self) {
        self.drag_mode = DragMode::None;
        self.drag_start = None;
        self.last_drag_pos = None;
        self.last_drag_time = None;
        self.dragging_primitive_idx = None;
        self.dragging_control_point = None;
        self.dragging_pane_id = None;
        self.drag_primitive_initial_points = None;
    }

    /// Reset kinetic scrolling state.
    pub fn reset_kinetic(&mut self) {
        self.kinetic_velocity = (0.0, 0.0);
        self.kinetic_active = false;
    }

    /// Check if currently dragging.
    #[inline]
    pub fn is_dragging(&self) -> bool {
        self.drag_mode.is_dragging()
    }

    /// Save viewport state for undo.
    pub fn save_viewport_state(&mut self, view_start: f64, bar_spacing: f64, price_min: f64, price_max: f64) {
        self.drag_start_view = Some(view_start);
        self.drag_start_spacing = Some(bar_spacing);
        self.drag_start_price_min = Some(price_min);
        self.drag_start_price_max = Some(price_max);
    }

    /// Clear saved viewport state.
    pub fn clear_viewport_state(&mut self) {
        self.drag_start_view = None;
        self.drag_start_spacing = None;
        self.drag_start_price_min = None;
        self.drag_start_price_max = None;
    }
}

// =============================================================================
// Output Actions
// =============================================================================

/// Actions produced by the input handler.
///
/// These tell the chart what operations to perform in response to user input.
/// The platform-specific code is responsible for executing these actions.
#[derive(Debug, Clone, PartialEq, Default)]
pub enum ChartOutputAction {
    /// Request a repaint of the chart.
    Repaint,

    /// Open a context menu at the specified position.
    OpenContextMenu {
        /// X coordinate in screen pixels.
        x: f64,
        /// Y coordinate in screen pixels.
        y: f64,
        /// ID of primitive under cursor (if any).
        primitive_id: Option<u64>,
    },

    /// Start kinetic scrolling animation.
    StartKinetic {
        /// Horizontal velocity in pixels per millisecond.
        velocity_x: f64,
        /// Vertical velocity in pixels per millisecond.
        velocity_y: f64,
    },

    /// Stop kinetic scrolling.
    StopKinetic,

    /// Update the cursor style.
    UpdateCursor(CursorStyle),

    /// Show a tooltip at the specified position.
    ShowTooltip {
        /// X coordinate in screen pixels.
        x: f64,
        /// Y coordinate in screen pixels.
        y: f64,
        /// Tooltip text content.
        text: String,
    },

    /// Hide the current tooltip.
    HideTooltip,

    /// Record an undo action.
    RecordUndo(UndoAction),

    /// Update crosshair position.
    UpdateCrosshair {
        /// X coordinate in screen pixels.
        x: f64,
        /// Y coordinate in screen pixels.
        y: f64,
        /// Whether crosshair should be visible.
        visible: bool,
    },

    /// Hide the crosshair.
    HideCrosshair,

    /// Pan the viewport.
    Pan {
        /// Bar delta (positive = show earlier data).
        bar_delta: f64,
        /// Price delta (positive = show higher prices).
        price_delta: f64,
    },

    /// Zoom the viewport.
    Zoom {
        /// Center X in screen pixels.
        center_x: f64,
        /// Center Y in screen pixels.
        center_y: f64,
        /// Horizontal zoom factor.
        factor_x: f64,
        /// Vertical zoom factor.
        factor_y: f64,
    },

    /// Select a primitive.
    SelectPrimitive {
        /// Primitive ID to select (None to deselect).
        id: Option<u64>,
    },

    /// Start dragging a primitive.
    StartPrimitiveDrag {
        /// Primitive ID.
        id: u64,
        /// Start X in data coordinates.
        bar: f64,
        /// Start Y in data coordinates.
        price: f64,
    },

    /// Start dragging a control point.
    StartControlPointDrag {
        /// Primitive ID.
        primitive_id: u64,
        /// Control point type.
        control_point: ControlPointType,
        /// Start bar position.
        bar: f64,
        /// Start price position.
        price: f64,
    },

    /// Update primitive drag position.
    UpdatePrimitiveDrag {
        /// Current bar position.
        bar: f64,
        /// Current price position.
        price: f64,
    },

    /// End primitive drag.
    EndPrimitiveDrag,

    /// Reset price scale to auto mode.
    ResetPriceScale,

    /// Reset time scale (fit all data).
    ResetTimeScale,

    /// Toggle price scale mode (lin/log/percent).
    TogglePriceScaleMode,

    /// Finish a multipoint drawing.
    FinishMultipointDrawing,

    /// Handle a drawing click.
    DrawingClick {
        /// Bar position.
        bar: f64,
        /// Price position.
        price: f64,
        /// Pane ID (None for main chart).
        pane_id: Option<u64>,
    },

    /// No action needed.
    #[default]
    None,
}

impl ChartOutputAction {
    /// Check if this is a no-op action.
    #[inline]
    pub fn is_none(&self) -> bool {
        matches!(self, ChartOutputAction::None)
    }

    /// Check if this action requires a repaint.
    #[inline]
    pub fn needs_repaint(&self) -> bool {
        !matches!(
            self,
            ChartOutputAction::None
                | ChartOutputAction::UpdateCursor(_)
                | ChartOutputAction::HideTooltip
        )
    }
}

// =============================================================================
// Undo Actions
// =============================================================================

/// Undo action types that can be recorded.
#[derive(Debug, Clone, PartialEq)]
pub enum UndoAction {
    /// Viewport was changed (pan/zoom).
    ViewportChange {
        /// Previous view start.
        old_view_start: f64,
        /// Previous bar spacing.
        old_bar_spacing: f64,
        /// Previous price min.
        old_price_min: f64,
        /// Previous price max.
        old_price_max: f64,
        /// New view start.
        new_view_start: f64,
        /// New bar spacing.
        new_bar_spacing: f64,
        /// New price min.
        new_price_min: f64,
        /// New price max.
        new_price_max: f64,
    },

    /// Primitive was moved.
    PrimitiveMoved {
        /// Primitive index.
        index: usize,
        /// Previous points.
        old_points: Vec<(f64, f64)>,
        /// New points.
        new_points: Vec<(f64, f64)>,
    },

    /// Primitive was created.
    PrimitiveCreated {
        /// Primitive index.
        index: usize,
    },

    /// Primitive was deleted.
    PrimitiveDeleted {
        /// Primitive index.
        index: usize,
        /// Serialized primitive data for restoration.
        data: String,
    },
}

// =============================================================================
// Default Input Handler
// =============================================================================

/// Default implementation of the chart input handler.
///
/// This handler processes semantic input actions and produces output actions
/// that the chart should execute. It maintains state for multi-step interactions
/// like drags and kinetic scrolling.
///
/// # Usage
///
/// ```ignore
/// let mut handler = DefaultChartInputHandler::new();
///
/// // Process an action
/// let outputs = handler.handle_action(action, &hit_tester);
///
/// // Execute output actions
/// for output in outputs {
///     match output {
///         ChartOutputAction::Repaint => chart.repaint(),
///         // ... handle other outputs
///     }
/// }
/// ```
#[derive(Debug, Clone)]
pub struct DefaultChartInputHandler {
    /// Current input state.
    pub state: ChartInputState,
    /// Handler configuration.
    pub config: InputHandlerConfig,
}

impl Default for DefaultChartInputHandler {
    fn default() -> Self {
        Self::new()
    }
}

impl DefaultChartInputHandler {
    /// Create a new input handler with default configuration.
    pub fn new() -> Self {
        Self {
            state: ChartInputState::new(),
            config: InputHandlerConfig::default(),
        }
    }

    /// Create a new input handler with custom configuration.
    pub fn with_config(config: InputHandlerConfig) -> Self {
        Self {
            state: ChartInputState::new(),
            config,
        }
    }

    /// Get a reference to the current state.
    pub fn state(&self) -> &ChartInputState {
        &self.state
    }

    /// Get a mutable reference to the current state.
    pub fn state_mut(&mut self) -> &mut ChartInputState {
        &mut self.state
    }

    /// Process an input action and return output actions.
    ///
    /// This is the main entry point for processing input. It examines the
    /// action type and current state to determine what outputs to produce.
    pub fn process_action(
        &mut self,
        action: ChartInputAction,
        hit_tester: &dyn ChartHitTester,
    ) -> Vec<ChartOutputAction> {
        match action {
            ChartInputAction::Pan { delta_x, delta_y } => {
                self.handle_pan(delta_x, delta_y)
            }
            ChartInputAction::Zoom { center_x, center_y, factor_x, factor_y } => {
                self.handle_zoom(center_x, center_y, factor_x, factor_y)
            }
            ChartInputAction::DragStart { mode, x, y } => {
                self.handle_drag_start(mode, x, y, hit_tester)
            }
            ChartInputAction::DragMove { mode, x, y, delta_x, delta_y } => {
                self.handle_drag_move(mode, x, y, delta_x, delta_y, hit_tester)
            }
            ChartInputAction::DragEnd { mode, x, y } => {
                self.handle_drag_end(mode, x, y)
            }
            ChartInputAction::Click { x, y, button } => {
                self.handle_click(x, y, button, hit_tester)
            }
            ChartInputAction::DoubleClick { x, y } => {
                self.handle_double_click(x, y, hit_tester)
            }
            ChartInputAction::ContextMenu { x, y } => {
                self.handle_context_menu(x, y, hit_tester)
            }
            ChartInputAction::CrosshairMove { x, y } => {
                self.handle_crosshair_move(x, y, hit_tester)
            }
            ChartInputAction::CrosshairHide => {
                vec![ChartOutputAction::HideCrosshair]
            }
            ChartInputAction::KeyPress { key, modifiers } => {
                self.handle_key_press(key, modifiers)
            }
            ChartInputAction::Scroll { x, y, delta_x, delta_y } => {
                self.handle_scroll(x, y, delta_x, delta_y, hit_tester)
            }
            ChartInputAction::None => {
                vec![]
            }
        }
    }

    // =========================================================================
    // Action Handlers
    // =========================================================================

    /// Handle pan action.
    fn handle_pan(&mut self, delta_x: f64, delta_y: f64) -> Vec<ChartOutputAction> {
        vec![
            ChartOutputAction::Pan {
                bar_delta: delta_x,
                price_delta: delta_y,
            },
            ChartOutputAction::Repaint,
        ]
    }

    /// Handle zoom action.
    fn handle_zoom(
        &mut self,
        center_x: f64,
        center_y: f64,
        factor_x: f64,
        factor_y: f64,
    ) -> Vec<ChartOutputAction> {
        vec![
            ChartOutputAction::Zoom {
                center_x,
                center_y,
                factor_x,
                factor_y,
            },
            ChartOutputAction::Repaint,
        ]
    }

    /// Handle drag start action.
    fn handle_drag_start(
        &mut self,
        mode: DragMode,
        x: f64,
        y: f64,
        hit_tester: &dyn ChartHitTester,
    ) -> Vec<ChartOutputAction> {
        // Save drag start state
        self.state.drag_mode = mode;
        self.state.drag_start = Some((x, y));
        self.state.last_drag_pos = Some((x, y));
        self.state.last_drag_time = None; // Will be set by platform

        // Stop any kinetic scrolling
        self.state.reset_kinetic();

        let mut outputs = vec![ChartOutputAction::StopKinetic];

        // Determine drag mode from hit test if not specified
        let effective_mode = if mode == DragMode::None {
            self.determine_drag_mode(x, y, hit_tester)
        } else {
            mode
        };

        self.state.drag_mode = effective_mode;

        // Set cursor based on drag mode
        let cursor = match effective_mode {
            DragMode::Chart | DragMode::SubPaneChart { .. } => CursorStyle::Grabbing,
            DragMode::PriceScale | DragMode::SubPanePriceScale { .. } => CursorStyle::NsResize,
            DragMode::TimeScale => CursorStyle::EwResize,
            DragMode::Primitive { .. } | DragMode::ControlPoint { .. } => CursorStyle::Move,
            DragMode::PaneSeparator { .. } => CursorStyle::NsResize,
            DragMode::Selection => CursorStyle::Crosshair,
            DragMode::None => CursorStyle::Default,
        };

        outputs.push(ChartOutputAction::UpdateCursor(cursor));
        outputs
    }

    /// Determine drag mode based on hit test.
    fn determine_drag_mode(&self, x: f64, y: f64, hit_tester: &dyn ChartHitTester) -> DragMode {
        let hit = hit_tester.hit_test(x, y);

        match hit {
            HitResult::Chart => DragMode::Chart,
            HitResult::PriceScale => DragMode::PriceScale,
            HitResult::TimeScale => DragMode::TimeScale,
            HitResult::SubPaneChart { pane_index } => DragMode::SubPaneChart { pane_index },
            HitResult::SubPanePriceScale { pane_index } => DragMode::SubPanePriceScale { pane_index },
            HitResult::PaneSeparator { pane_index } => DragMode::PaneSeparator { pane_index },
            HitResult::Primitive { id } => DragMode::Primitive { id },
            HitResult::ControlPoint { primitive_id, point_index } => {
                DragMode::ControlPoint { primitive_id, point_index }
            }
            HitResult::ScaleCorner | HitResult::Toolbar | HitResult::None => DragMode::None,
        }
    }

    /// Handle drag move action.
    fn handle_drag_move(
        &mut self,
        _mode: DragMode,
        x: f64,
        y: f64,
        delta_x: f64,
        delta_y: f64,
        hit_tester: &dyn ChartHitTester,
    ) -> Vec<ChartOutputAction> {
        let mut outputs = Vec::new();

        // Get chart bounds for clamping crosshair
        let (chart_x, _chart_y, chart_w, _chart_h) = hit_tester.chart_rect();

        // Update last position
        self.state.last_drag_pos = Some((x, y));

        match self.state.drag_mode {
            DragMode::Chart => {
                // Pan the chart
                outputs.push(ChartOutputAction::Pan {
                    bar_delta: delta_x,
                    price_delta: delta_y,
                });

                // Update crosshair if allowed - clamp X to chart bounds
                // Y clamping is handled by the app based on auto_scale mode
                if self.state.drag_mode.allows_crosshair_update() {
                    let clamped_x = x.clamp(chart_x, chart_x + chart_w);
                    outputs.push(ChartOutputAction::UpdateCrosshair {
                        x: clamped_x,
                        y,
                        visible: true,
                    });
                }
            }

            DragMode::PriceScale => {
                // Zoom price scale (convert delta to zoom factor)
                let factor = 1.0 + delta_y * self.config.price_scale_drag_factor;
                outputs.push(ChartOutputAction::Zoom {
                    center_x: x,
                    center_y: y,
                    factor_x: 1.0,
                    factor_y: factor,
                });
            }

            DragMode::TimeScale => {
                // Zoom time scale
                let factor = 1.0 - delta_x * self.config.time_scale_drag_factor;
                outputs.push(ChartOutputAction::Zoom {
                    center_x: x,
                    center_y: y,
                    factor_x: factor,
                    factor_y: 1.0,
                });
            }

            DragMode::Primitive { .. } | DragMode::ControlPoint { .. } => {
                // Update primitive drag
                outputs.push(ChartOutputAction::UpdatePrimitiveDrag {
                    bar: x,   // Platform will convert to data coordinates
                    price: y,
                });

                // Crosshair follows during primitive drag - clamp X only
                // Y clamping is handled by the app based on auto_scale mode
                let clamped_x = x.clamp(chart_x, chart_x + chart_w);
                outputs.push(ChartOutputAction::UpdateCrosshair {
                    x: clamped_x,
                    y,
                    visible: true,
                });
            }

            DragMode::SubPaneChart { pane_index: _ } => {
                // Pan sub-pane
                outputs.push(ChartOutputAction::Pan {
                    bar_delta: delta_x,
                    price_delta: delta_y,
                });

                // Crosshair follows during sub-pane drag - clamp X only
                // Y clamping is handled by the app based on auto_scale mode
                let clamped_x = x.clamp(chart_x, chart_x + chart_w);
                outputs.push(ChartOutputAction::UpdateCrosshair {
                    x: clamped_x,
                    y,
                    visible: true,
                });
            }

            DragMode::SubPanePriceScale { pane_index: _ } => {
                // Zoom sub-pane price scale
                let factor = 1.0 + delta_y * self.config.price_scale_drag_factor;
                outputs.push(ChartOutputAction::Zoom {
                    center_x: x,
                    center_y: y,
                    factor_x: 1.0,
                    factor_y: factor,
                });
            }

            DragMode::PaneSeparator { pane_index: _ } => {
                // Pane resize is handled by platform
            }

            DragMode::Selection => {
                // Selection rectangle is handled by platform
            }

            DragMode::None => {}
        }

        outputs.push(ChartOutputAction::Repaint);
        outputs
    }

    /// Handle drag end action.
    fn handle_drag_end(&mut self, _mode: DragMode, _x: f64, _y: f64) -> Vec<ChartOutputAction> {
        let mut outputs = Vec::new();

        // Check for kinetic scrolling
        if self.state.drag_mode == DragMode::Chart {
            let (vx, vy) = self.state.kinetic_velocity;
            if vx.abs() > self.config.kinetic_min_velocity
                || vy.abs() > self.config.kinetic_min_velocity
            {
                outputs.push(ChartOutputAction::StartKinetic {
                    velocity_x: vx,
                    velocity_y: vy,
                });
            }
        }

        // End primitive drag if applicable
        if self.state.drag_mode.is_primitive_drag() {
            outputs.push(ChartOutputAction::EndPrimitiveDrag);
        }

        // Reset drag state
        self.state.reset_drag();

        // Reset cursor
        outputs.push(ChartOutputAction::UpdateCursor(CursorStyle::Default));
        outputs.push(ChartOutputAction::Repaint);

        outputs
    }

    /// Handle click action.
    fn handle_click(
        &mut self,
        x: f64,
        y: f64,
        button: MouseButton,
        hit_tester: &dyn ChartHitTester,
    ) -> Vec<ChartOutputAction> {
        let mut outputs = Vec::new();
        let hit = hit_tester.hit_test(x, y);

        match button {
            MouseButton::Left => {
                match hit {
                    HitResult::Primitive { id } => {
                        // Select the primitive
                        outputs.push(ChartOutputAction::SelectPrimitive { id: Some(id) });
                    }
                    HitResult::Chart | HitResult::SubPaneChart { .. } => {
                        // Deselect any primitive
                        outputs.push(ChartOutputAction::SelectPrimitive { id: None });
                    }
                    _ => {}
                }
            }
            MouseButton::Right => {
                // Right click on price scale toggles mode, otherwise shows context menu
                match hit {
                    HitResult::PriceScale | HitResult::SubPanePriceScale { .. } => {
                        outputs.push(ChartOutputAction::TogglePriceScaleMode);
                    }
                    _ => {
                        let primitive_id = hit.primitive_id();
                        outputs.push(ChartOutputAction::OpenContextMenu {
                            x,
                            y,
                            primitive_id,
                        });
                    }
                }
            }
            MouseButton::Middle => {
                // Middle click could be used for other actions
            }
        }

        outputs.push(ChartOutputAction::Repaint);
        outputs
    }

    /// Handle double-click action.
    fn handle_double_click(
        &mut self,
        x: f64,
        y: f64,
        hit_tester: &dyn ChartHitTester,
    ) -> Vec<ChartOutputAction> {
        let mut outputs = Vec::new();
        let hit = hit_tester.hit_test(x, y);

        match hit {
            HitResult::PriceScale | HitResult::SubPanePriceScale { .. } => {
                // Reset price scale to auto
                outputs.push(ChartOutputAction::ResetPriceScale);
            }
            HitResult::TimeScale => {
                // Fit all data
                outputs.push(ChartOutputAction::ResetTimeScale);
            }
            HitResult::Chart | HitResult::SubPaneChart { .. } => {
                // Finish multipoint drawing if in progress
                outputs.push(ChartOutputAction::FinishMultipointDrawing);
            }
            _ => {}
        }

        outputs.push(ChartOutputAction::Repaint);
        outputs
    }

    /// Handle context menu request.
    fn handle_context_menu(
        &mut self,
        x: f64,
        y: f64,
        hit_tester: &dyn ChartHitTester,
    ) -> Vec<ChartOutputAction> {
        let hit = hit_tester.hit_test(x, y);

        match hit {
            HitResult::PriceScale | HitResult::SubPanePriceScale { .. } => {
                // Toggle price scale mode
                vec![
                    ChartOutputAction::TogglePriceScaleMode,
                    ChartOutputAction::Repaint,
                ]
            }
            _ => {
                // Show context menu
                vec![ChartOutputAction::OpenContextMenu {
                    x,
                    y,
                    primitive_id: hit.primitive_id(),
                }]
            }
        }
    }

    /// Handle crosshair move.
    fn handle_crosshair_move(
        &mut self,
        x: f64,
        y: f64,
        hit_tester: &dyn ChartHitTester,
    ) -> Vec<ChartOutputAction> {
        let mut outputs = Vec::new();

        // Only update crosshair if not in a blocking drag mode
        if self.state.drag_mode.allows_crosshair_update() {
            let (cx, cy, cw, ch) = hit_tester.chart_rect();
            let visible = x >= cx && x < cx + cw && y >= cy && y < cy + ch;

            outputs.push(ChartOutputAction::UpdateCrosshair { x, y, visible });

            // Update cursor based on hover
            let hit = hit_tester.hit_test(x, y);
            let cursor = match hit {
                HitResult::PriceScale | HitResult::SubPanePriceScale { .. } => {
                    CursorStyle::NsResize
                }
                HitResult::TimeScale => CursorStyle::EwResize,
                HitResult::PaneSeparator { .. } => CursorStyle::NsResize,
                HitResult::Primitive { .. } | HitResult::ControlPoint { .. } => CursorStyle::Grab,
                HitResult::Chart | HitResult::SubPaneChart { .. } => CursorStyle::Crosshair,
                _ => CursorStyle::Default,
            };

            outputs.push(ChartOutputAction::UpdateCursor(cursor));
            outputs.push(ChartOutputAction::Repaint);
        }

        outputs
    }

    /// Handle key press.
    fn handle_key_press(
        &mut self,
        key: super::super::events::KeyCode,
        modifiers: super::super::events::Modifiers,
    ) -> Vec<ChartOutputAction> {
        use super::super::events::KeyCode;

        // Update constrain proportions state
        self.state.constrain_proportions = modifiers.ctrl || modifiers.meta;

        match key {
            KeyCode::Escape => {
                // Cancel current operation
                if self.state.is_dragging() {
                    self.state.reset_drag();
                    vec![
                        ChartOutputAction::EndPrimitiveDrag,
                        ChartOutputAction::UpdateCursor(CursorStyle::Default),
                        ChartOutputAction::Repaint,
                    ]
                } else {
                    // Deselect primitive
                    vec![
                        ChartOutputAction::SelectPrimitive { id: None },
                        ChartOutputAction::Repaint,
                    ]
                }
            }
            KeyCode::Delete | KeyCode::Backspace => {
                // Delete selected primitive (handled by platform)
                vec![ChartOutputAction::Repaint]
            }
            _ => vec![],
        }
    }

    /// Handle scroll wheel.
    fn handle_scroll(
        &mut self,
        x: f64,
        y: f64,
        _delta_x: f64,
        delta_y: f64,
        hit_tester: &dyn ChartHitTester,
    ) -> Vec<ChartOutputAction> {
        let hit = hit_tester.hit_test(x, y);

        // Calculate zoom factor from scroll delta
        let factor = if delta_y > 0.0 {
            1.0 + self.config.scroll_zoom_factor
        } else {
            1.0 - self.config.scroll_zoom_factor
        };

        match hit {
            HitResult::PriceScale | HitResult::SubPanePriceScale { .. } => {
                // Zoom price scale — invert factor so scroll-up expands range
                let inv_factor = 1.0 / factor;
                vec![
                    ChartOutputAction::Zoom {
                        center_x: x,
                        center_y: y,
                        factor_x: 1.0,
                        factor_y: inv_factor,
                    },
                    ChartOutputAction::Repaint,
                ]
            }
            _ => {
                // Zoom time scale (horizontal)
                vec![
                    ChartOutputAction::Zoom {
                        center_x: x,
                        center_y: y,
                        factor_x: factor,
                        factor_y: 1.0,
                    },
                    ChartOutputAction::Repaint,
                ]
            }
        }
    }

    /// Update kinetic velocity from drag movement.
    ///
    /// Call this from the platform code during drag with timestamps.
    pub fn update_kinetic_velocity(&mut self, dx: f64, dy: f64, dt_ms: f64) {
        if dt_ms > 0.0 {
            // Simple velocity calculation (pixels per millisecond)
            let vx = dx / dt_ms;
            let vy = dy / dt_ms;

            // Apply some smoothing
            self.state.kinetic_velocity = (
                self.state.kinetic_velocity.0 * 0.5 + vx * 0.5,
                self.state.kinetic_velocity.1 * 0.5 + vy * 0.5,
            );
        }
    }

    /// Update kinetic scrolling state.
    ///
    /// Call this from the animation loop. Returns the pan delta if kinetic
    /// scrolling is active, or None if it has stopped.
    pub fn update_kinetic(&mut self, dt_ms: f64) -> Option<(f64, f64)> {
        if !self.state.kinetic_active {
            return None;
        }

        let (vx, vy) = self.state.kinetic_velocity;

        // Check if velocity is below threshold
        if vx.abs() < self.config.kinetic_min_velocity
            && vy.abs() < self.config.kinetic_min_velocity
        {
            self.state.reset_kinetic();
            return None;
        }

        // Calculate pan delta
        let delta_x = vx * dt_ms;
        let delta_y = vy * dt_ms;

        // Apply friction
        self.state.kinetic_velocity = (
            vx * self.config.kinetic_friction,
            vy * self.config.kinetic_friction,
        );

        Some((delta_x, delta_y))
    }

    /// Start kinetic scrolling with the given velocity.
    pub fn start_kinetic(&mut self, velocity_x: f64, velocity_y: f64) {
        self.state.kinetic_velocity = (velocity_x, velocity_y);
        self.state.kinetic_active = true;
    }

    /// Stop kinetic scrolling.
    pub fn stop_kinetic(&mut self) {
        self.state.reset_kinetic();
    }
}

// =============================================================================
// Trait Implementation
// =============================================================================

impl super::ChartInputHandler for DefaultChartInputHandler {
    fn handle_action(&mut self, action: ChartInputAction, hit_tester: &dyn ChartHitTester) {
        // Process the action and discard outputs
        // (This is for simple use cases where outputs aren't needed)
        let _ = self.process_action(action, hit_tester);
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    /// Mock hit tester for testing.
    struct MockHitTester {
        result: HitResult,
        chart_rect: (f64, f64, f64, f64),
    }

    impl MockHitTester {
        fn new(result: HitResult) -> Self {
            Self {
                result,
                chart_rect: (0.0, 0.0, 800.0, 600.0),
            }
        }
    }

    impl ChartHitTester for MockHitTester {
        fn hit_test(&self, _x: f64, _y: f64) -> HitResult {
            self.result
        }

        fn chart_rect(&self) -> (f64, f64, f64, f64) {
            self.chart_rect
        }

        fn price_scale_rect(&self) -> Option<(f64, f64, f64, f64)> {
            Some((800.0, 0.0, 80.0, 600.0))
        }

        fn time_scale_rect(&self) -> Option<(f64, f64, f64, f64)> {
            Some((0.0, 600.0, 800.0, 30.0))
        }
    }

    #[test]
    fn test_input_handler_creation() {
        let handler = DefaultChartInputHandler::new();
        assert!(!handler.state.is_dragging());
        assert_eq!(handler.config.drag_threshold, 5.0);
    }

    #[test]
    fn test_input_handler_with_config() {
        let config = InputHandlerConfig::default().with_drag_threshold(10.0);
        let handler = DefaultChartInputHandler::with_config(config);
        assert_eq!(handler.config.drag_threshold, 10.0);
    }

    #[test]
    fn test_handle_pan() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::Chart);

        let outputs = handler.process_action(
            ChartInputAction::Pan {
                delta_x: 10.0,
                delta_y: 5.0,
            },
            &hit_tester,
        );

        assert!(!outputs.is_empty());
        assert!(outputs.iter().any(|o| matches!(o, ChartOutputAction::Pan { .. })));
    }

    #[test]
    fn test_handle_zoom() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::Chart);

        let outputs = handler.process_action(
            ChartInputAction::Zoom {
                center_x: 400.0,
                center_y: 300.0,
                factor_x: 1.1,
                factor_y: 1.0,
            },
            &hit_tester,
        );

        assert!(!outputs.is_empty());
        assert!(outputs.iter().any(|o| matches!(o, ChartOutputAction::Zoom { .. })));
    }

    #[test]
    fn test_handle_drag_start() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::Chart);

        let outputs = handler.process_action(
            ChartInputAction::DragStart {
                mode: DragMode::Chart,
                x: 100.0,
                y: 100.0,
            },
            &hit_tester,
        );

        assert!(handler.state.is_dragging());
        assert_eq!(handler.state.drag_mode, DragMode::Chart);
        assert!(outputs.iter().any(|o| matches!(o, ChartOutputAction::StopKinetic)));
    }

    #[test]
    fn test_handle_drag_end_kinetic() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::Chart);

        // Start drag
        handler.process_action(
            ChartInputAction::DragStart {
                mode: DragMode::Chart,
                x: 100.0,
                y: 100.0,
            },
            &hit_tester,
        );

        // Set high velocity
        handler.state.kinetic_velocity = (10.0, 0.0);

        // End drag
        let outputs = handler.process_action(
            ChartInputAction::DragEnd {
                mode: DragMode::Chart,
                x: 200.0,
                y: 100.0,
            },
            &hit_tester,
        );

        assert!(!handler.state.is_dragging());
        assert!(outputs.iter().any(|o| matches!(o, ChartOutputAction::StartKinetic { .. })));
    }

    #[test]
    fn test_handle_double_click_price_scale() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::PriceScale);

        let outputs = handler.process_action(
            ChartInputAction::DoubleClick { x: 850.0, y: 300.0 },
            &hit_tester,
        );

        assert!(outputs.iter().any(|o| matches!(o, ChartOutputAction::ResetPriceScale)));
    }

    #[test]
    fn test_handle_double_click_time_scale() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::TimeScale);

        let outputs = handler.process_action(
            ChartInputAction::DoubleClick { x: 400.0, y: 620.0 },
            &hit_tester,
        );

        assert!(outputs.iter().any(|o| matches!(o, ChartOutputAction::ResetTimeScale)));
    }

    #[test]
    fn test_handle_click_primitive() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::Primitive { id: 42 });

        let outputs = handler.process_action(
            ChartInputAction::Click {
                x: 200.0,
                y: 200.0,
                button: MouseButton::Left,
            },
            &hit_tester,
        );

        assert!(outputs.iter().any(|o| matches!(
            o,
            ChartOutputAction::SelectPrimitive { id: Some(42) }
        )));
    }

    #[test]
    fn test_handle_scroll_chart() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::Chart);

        let outputs = handler.process_action(
            ChartInputAction::Scroll {
                x: 400.0,
                y: 300.0,
                delta_x: 0.0,
                delta_y: 10.0,
            },
            &hit_tester,
        );

        // Should zoom horizontally
        assert!(outputs.iter().any(|o| matches!(
            o,
            ChartOutputAction::Zoom { factor_x, factor_y, .. }
            if *factor_x != 1.0 && *factor_y == 1.0
        )));
    }

    #[test]
    fn test_handle_scroll_price_scale() {
        let mut handler = DefaultChartInputHandler::new();
        let hit_tester = MockHitTester::new(HitResult::PriceScale);

        let outputs = handler.process_action(
            ChartInputAction::Scroll {
                x: 850.0,
                y: 300.0,
                delta_x: 0.0,
                delta_y: 10.0,
            },
            &hit_tester,
        );

        // Should zoom vertically
        assert!(outputs.iter().any(|o| matches!(
            o,
            ChartOutputAction::Zoom { factor_x, factor_y, .. }
            if *factor_x == 1.0 && *factor_y != 1.0
        )));
    }

    #[test]
    fn test_kinetic_update() {
        let mut handler = DefaultChartInputHandler::new();

        // Start kinetic with velocity
        handler.start_kinetic(10.0, 5.0);
        assert!(handler.state.kinetic_active);

        // Update should return delta
        let delta = handler.update_kinetic(16.0);
        assert!(delta.is_some());

        let (dx, dy) = delta.unwrap();
        assert!(dx > 0.0);
        assert!(dy > 0.0);

        // Velocity should decrease due to friction
        assert!(handler.state.kinetic_velocity.0 < 10.0);
    }

    #[test]
    fn test_kinetic_stops_below_threshold() {
        let mut handler = DefaultChartInputHandler::new();

        // Start with low velocity
        handler.start_kinetic(1.0, 1.0);

        // Should stop immediately
        let delta = handler.update_kinetic(16.0);
        assert!(delta.is_none());
        assert!(!handler.state.kinetic_active);
    }

    #[test]
    fn test_chart_input_state_reset() {
        let mut state = ChartInputState::new();

        state.drag_mode = DragMode::Chart;
        state.drag_start = Some((100.0, 100.0));
        state.kinetic_active = true;
        state.kinetic_velocity = (10.0, 5.0);

        state.reset_drag();
        assert_eq!(state.drag_mode, DragMode::None);
        assert!(state.drag_start.is_none());

        state.reset_kinetic();
        assert!(!state.kinetic_active);
        assert_eq!(state.kinetic_velocity, (0.0, 0.0));
    }

    #[test]
    fn test_output_action_needs_repaint() {
        assert!(ChartOutputAction::Repaint.needs_repaint());
        assert!(ChartOutputAction::Pan { bar_delta: 1.0, price_delta: 0.0 }.needs_repaint());
        assert!(!ChartOutputAction::None.needs_repaint());
        assert!(!ChartOutputAction::UpdateCursor(CursorStyle::Default).needs_repaint());
        assert!(!ChartOutputAction::HideTooltip.needs_repaint());
    }

    #[test]
    fn test_config_builder_pattern() {
        let config = InputHandlerConfig::default()
            .with_drag_threshold(10.0)
            .with_scroll_zoom_factor(0.1);

        assert_eq!(config.drag_threshold, 10.0);
        assert_eq!(config.scroll_zoom_factor, 0.1);
    }

    #[test]
    fn test_default_output_action() {
        let action: ChartOutputAction = Default::default();
        assert!(action.is_none());
    }
}
