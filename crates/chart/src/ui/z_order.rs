//! Centralized Z-order layer definitions for InputCoordinator.
//!
//! All UI layers and their z-order values are defined here.
//! Higher values render on top and receive input events first.

use uzor::input::coordinator::{InputCoordinator, LayerId};

/// Named rendering layers with centralized z-order values.
///
/// Ordered bottom-to-top:
/// - z=1: Base UI (panel headers, toolbars, sidebars)
/// - z=2: Popups (dropdowns, submenus, indicator overlay)
/// - z=3: Modals (search, settings, clock popup) — block lower layers
/// - z=5: Context menu (above modals)
/// - z=6: Color pickers (topmost interactive)
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum ZLayer {
    // z=1 — base UI
    PanelHeaders,
    Toolbar,
    Sidebar,

    // z=2 — popups
    Dropdown,
    Submenu,
    IndicatorOverlay,

    // z=3 — modals
    Modal,
    ClockPopup,

    // z=4 — modal overlays (modal dialogs stacked above other modals)
    ModalOverlay,

    // z=4 — floating windows (non-modal)
    FloatingWindow,

    // z=5 — context menu
    ContextMenu,

    // z=6 — color pickers
    ColorPicker,
}

impl ZLayer {
    /// Z-order value for InputCoordinator. Higher = on top.
    pub const fn z_order(self) -> u32 {
        match self {
            Self::PanelHeaders | Self::Toolbar | Self::Sidebar => 1,
            Self::Dropdown | Self::Submenu | Self::IndicatorOverlay => 2,
            Self::Modal | Self::ClockPopup => 3,
            Self::ModalOverlay | Self::FloatingWindow => 4,
            Self::ContextMenu => 5,
            Self::ColorPicker => 6,
        }
    }

    /// Whether this layer blocks events to lower layers.
    pub const fn is_modal(self) -> bool {
        matches!(self, Self::Modal | Self::ClockPopup | Self::ModalOverlay | Self::ColorPicker)
    }

    /// Default LayerId string for this layer.
    pub const fn layer_id(self) -> &'static str {
        match self {
            Self::PanelHeaders => "panel_headers",
            Self::Toolbar => "toolbar",
            Self::Sidebar => "sidebar",
            Self::Dropdown => "dropdown",
            Self::Submenu => "submenu",
            Self::IndicatorOverlay => "indicator_overlay",
            Self::Modal => "modal",
            Self::ClockPopup => "clock_popup",
            Self::ModalOverlay => "modal_overlay",
            Self::FloatingWindow => "floating_window",
            Self::ContextMenu => "context_menu",
            Self::ColorPicker => "color_picker",
        }
    }

    /// Push this layer onto InputCoordinator with its default name.
    pub fn push(self, coord: &mut InputCoordinator) -> LayerId {
        let id = LayerId::new(self.layer_id());
        coord.push_layer(id.clone(), self.z_order(), self.is_modal());
        id
    }

    /// Push with a custom layer name (keeps z-order and modal flag from the enum).
    pub fn push_named(self, coord: &mut InputCoordinator, name: &str) -> LayerId {
        let id = LayerId::new(name);
        coord.push_layer(id.clone(), self.z_order(), self.is_modal());
        id
    }
}
