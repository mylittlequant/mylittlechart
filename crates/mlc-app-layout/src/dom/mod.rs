//! uzor DOM trees for the surfaces a uzor-framework host takes over from the
//! app's own paint (the in-place migration's cloud arc, item 15).
//!
//! Same contract as the shell's `dom::chart_settings` / `dom::user_settings`
//! modules, lived here so the layout crate owns its own DOM shape: the app
//! still lays the surface out, registers its hit zones and takes its input
//! (its paint goes nowhere, `mlc_app::paint_off::PaintOff`); these trees own
//! the chrome (dim veil, dialog frame, header, footer) as `domkit::dialog`
//! boxes and hand the legacy body back as embed paint slots
//! ([`uzor::ui::dom::El::slot`] — no widget id, so the slot swallows no
//! clicks: body input falls through to the app's live zones).
//!
//! Geometry is never re-derived: [`geometry`] runs the real
//! `begin_modal_shell` over a paint-nothing context and reads the rects it
//! settled on, so the DOM frame and the app frame are the same arithmetic
//! by construction.

#[cfg(feature = "cloud")]
pub mod cloud_overlay;
#[cfg(feature = "cloud")]
pub mod community;

#[cfg(feature = "cloud")]
mod shared;

#[cfg(feature = "cloud")]
pub(crate) use shared::{fixed, frame_border, geometry, hairline, SlotChartCtx};
