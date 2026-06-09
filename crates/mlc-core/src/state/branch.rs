//! Branch-level docking payload types.
//!
//! `PanelLeaf` and `PanelLeafKind` have been replaced by the unified `Panel`
//! enum (K1.2).  This module now only contains `InstanceId`, which is still
//! used by `ChartContainerLeaf::IndicatorPanel` for indicator-manager keying.

use serde::{Deserialize, Serialize};

// =============================================================================
// Identifiers
// =============================================================================

/// Identity of one indicator instance.
///
/// Thin newtype over the indicator manager's `instance_id: u64` — the
/// canonical cross-subsystem key for an indicator that already
/// underlies sub-pane reconciliation, sync group propagation and
/// preset persistence.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct InstanceId(pub u64);

impl std::fmt::Display for InstanceId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl From<u64> for InstanceId {
    fn from(v: u64) -> Self {
        InstanceId(v)
    }
}

impl From<InstanceId> for u64 {
    fn from(v: InstanceId) -> Self {
        v.0
    }
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_id_round_trip_u64() {
        let id: InstanceId = 42_u64.into();
        let raw: u64 = id.into();
        assert_eq!(raw, 42);
    }
}
