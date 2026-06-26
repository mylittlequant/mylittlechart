//! Tag — app-level orchestrator binding one `DataBubble` to one or more
//! `UiContainer` slots, plus the per-tag identity fields (color, members,
//! sticky styles).  Lives in `mlc-app` because the orchestration is
//! shared-app-core concern, NOT shell-specific.  Headless never
//! instantiates `Tag`.
//!
//! Phase 1 (this commit): empty scaffold.  No callsite migration; the
//! existing fields stay on `DataBubble` until Phase 2 lifts them across.
//!
//! See `docs/mlc/plans/tag-bubble-uicontainer-cutover-2026-06-26.md`.

pub mod store;
pub mod tag;
pub mod view;

pub use store::TagStore;
pub use tag::{Tag, TagId};
pub use view::build_group_view;
