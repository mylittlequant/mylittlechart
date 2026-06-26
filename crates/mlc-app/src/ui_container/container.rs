//! `UiContainer` — composite docking space tied to one `Tag`.
//!
//! Phase 1 stub: id + tag_id + placement + inline `ChartView`.  No
//! enum, no store.  Fields beyond `ChartView` (sub-pane layout, show
//! flags, drawings) move in here cluster-by-cluster in Phase 4.

use serde::{Deserialize, Serialize};

use mlc_core::tag_manager::BubbleId;

use super::chart::ChartView;

/// Stable identifier for a `UiContainer`.  Independent of `TagId` —
/// a single Tag may have N containers in the future (today: N=1).
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UiContainerId(pub u64);

static NEXT_UI_CONTAINER_ID: std::sync::atomic::AtomicU64 =
    std::sync::atomic::AtomicU64::new(1);

impl UiContainerId {
    pub fn generate() -> Self {
        Self(NEXT_UI_CONTAINER_ID.fetch_add(1, std::sync::atomic::Ordering::SeqCst))
    }

    pub fn bump_past(raw: u64) {
        let next = raw + 1;
        NEXT_UI_CONTAINER_ID.fetch_max(next, std::sync::atomic::Ordering::SeqCst);
    }
}

/// Where the container is mounted in the outer docking model.
///
/// `leaf_id` is the shell-agnostic docking leaf identifier (a `u64`),
/// NOT a winit window or DOM canvas handle.  Shells map leaf_id → their
/// native surface in their own code.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Placement {
    Canvas { leaf_id: u64 },
    Sidebar { slot: usize, leaf_id: u64 },
    Detached,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UiContainer {
    pub id: UiContainerId,
    pub tag_id: BubbleId,
    pub placement: Placement,
    pub chart: ChartView,
}

impl UiContainer {
    pub fn new(tag_id: BubbleId, placement: Placement) -> Self {
        Self {
            id: UiContainerId::generate(),
            tag_id,
            placement,
            chart: ChartView::default(),
        }
    }
}
