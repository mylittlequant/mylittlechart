//! UiContainer — a composite frontend docking space owned by a `Tag`.
//! Holds an `inner_docking: DockState<...>` with 1..N panels inside.
//!
//! Two kinds will eventually live here: `Chart` (1 MainChart + N
//! indicator SubPane panels) and `Trading` (OrderEntry / PositionManager /
//! TradeLog / RiskCalc panels — future cutover).
//!
//! Phase 1 (this commit): empty scaffold.  No enum yet — only one
//! concrete kind exists (`ChartView`).  Wave 3.3 simplification:
//! `chart: ChartView` is an inline field on `UiContainer`, not behind
//! a `UiContainerKind` enum.  Enum re-introduces when `Trading` lands.
//!
//! Wave 3.3 simplification: no separate `UiContainerStore`.
//! Containers are inlined on `Tag.ui_container_ids` for now (N=1 typical;
//! store comes back if multi-container per Tag becomes real).
//!
//! Lives in `mlc-app` because the docking + visualisation half is
//! shared-app-core concern, NOT shell-specific.  All fields must
//! remain shell-agnostic and serde-clean — no winit / web_sys / wgpu
//! / vello types may be added.

pub mod chart;
pub mod container;

pub use chart::ChartView;
pub use container::{Placement, UiContainer, UiContainerId};
