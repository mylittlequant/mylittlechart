//! Registry of primitive-settings modal design variants.
//!
//! A "variant" is not a fork of the lab — it is a single function with
//! exactly the signature `render_primitive_settings_modal` already has.
//! The baseline variant IS that real `mlc-app-layout` function, registered
//! unchanged; a second design is a new function with the identical
//! signature (same inputs: state / primitives / themes / coordinator / templates,
//! same output: `PrimitiveSettingsResult`) added to [`VARIANTS`].
//!
//! Keeping the signature identical to the real modal builder is what keeps
//! this a scaffold rather than a rewrite: any variant that reuses the
//! `"prim_settings"` widget-id namespace (recommended — same control ids,
//! different geometry/colours) gets every dispatch behaviour in
//! `crate::dispatch` for free, because that module is written against those
//! ids, not against `render_primitive_settings_modal` itself.

use mlc_app_layout::layout::modals::primitive_settings::render_primitive_settings_modal;
use mlc_app_layout::layout::render_frame::PrimitiveSettingsResult;
use mlc_app_layout::layout::toolbar_state::ToolbarState;
use mlc_app_layout::layout::FrameTheme;
use mlc_app_layout::ui::modal_settings::PrimitiveSettingsState;
use mlc_app_layout::ui::toolbar_theme::ToolbarTheme;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::render::RenderContext;
use mlc_core::templates::PrimitiveTemplate;
use uzor::input::InputCoordinator;

/// Exactly `render_primitive_settings_modal`'s own signature. A new variant
/// author writes one function matching this type and adds it to
/// [`VARIANTS`] — nothing else in the lab needs to change.
pub type RenderFn = fn(
    &mut dyn RenderContext,
    f64,
    f64,
    f64,
    &PrimitiveSettingsState,
    &[Box<dyn Primitive>],
    &FrameTheme,
    &ToolbarTheme,
    &ToolbarState,
    u64,
    &mut InputCoordinator,
    &[PrimitiveTemplate],
) -> PrimitiveSettingsResult;

/// One named design.
pub struct Variant {
    /// Shown on the lab bar's variant chips and in the debug readout.
    pub name: &'static str,
    pub render: RenderFn,
}

/// The registry. `"baseline"` is the current shipped design, unmodified.
pub const VARIANTS: &[Variant] = &[Variant { name: "baseline", render: render_primitive_settings_modal }];

/// The variant at `idx`, falling back to the first (baseline) entry for an
/// out-of-range index rather than panicking — `active_variant` is a plain
/// `usize` the lab bar writes to, and `VARIANTS` is non-empty by
/// construction.
pub fn get(idx: usize) -> &'static Variant {
    VARIANTS.get(idx).unwrap_or(&VARIANTS[0])
}
