//! Approved-design concept data — see [`spec`]'s own module doc — plus the
//! renderer that draws it ([`render`]), the state it draws against
//! ([`state`]), and the click dispatch that mutates that state
//! ([`click`]).

/// Click dispatch for [`render::render_concept_modal`]'s own interactive
/// rows — see the module's own doc.
pub mod click;
pub mod render;
pub mod spec;
pub mod state;

/// Scoped-region id the concept modal draws into — pushed, sized to the
/// chart pane's own rect, every frame by `crate::lib` (same convention
/// `objtree_modal`/`crate::app_modals::REGION_ID` use — see those own
/// docs for why a fresh push every frame is required).
pub const REGION_ID: &str = "concept_modal_region";
