pub mod types;
pub mod modal;
pub mod input;
pub mod slider;
pub mod popup;
pub mod color_picker;
pub mod button;
pub mod radio_group;

pub use types::{WidgetState, WidgetTheme};
pub use button::{ButtonConfig, ButtonResult, draw_button};
pub use modal::{
    ModalSize, ModalConfig, ModalTheme, ModalResult, ModalTab,
    draw_modal_backdrop, draw_modal_frame, draw_close_icon,
    draw_modal_tabs, render_modal_frame_only, render_modal,
};
pub use input::{InputConfig, InputType, InputResult, draw_input, draw_input_cursor, input_position_to_cursor, cursor_from_char_positions};
pub use slider::{
    // Configuration
    SliderConfig,

    // Rendering functions
    render_single_slider, render_dual_slider,

    // Results
    SingleSliderResult, DualSliderResult, SliderTrackInfo,

    // Drag state
    SliderDragState, DualSliderHandle,

    // Input handling
    SliderInputHandler,

    // Editing state passed to render_single_slider
    SliderEditingInfo,

    // Utilities
    value_to_position, position_to_value,
};
pub use popup::{
    draw_popup, popup_hit_test, PopupConfig, PopupResult, PopupTheme,
};
pub use radio_group::{RadioGroupResult, RadioOption, draw_radio_group};
pub use color_picker::{
    draw_color_picker_l1, color_picker_l1_hit_test,
    draw_color_picker_l2, color_picker_l2_hit_test,
    ColorPickerL1Result, ColorPickerL1HitResult,
    ColorPickerL2Result, ColorPickerL2HitResult,
    // Re-exports from color_picker_state (via color_picker module)
    ColorPickerL1Config, ColorPickerL2Config,
    ColorPickerL2Area, ColorPickerLevel, ColorPickerState,
    HsvColor, hsv_to_rgb, rgb_to_hsv, apply_opacity_to_hex,
    STANDARD_PALETTE, MAX_CUSTOM_COLORS,
};
