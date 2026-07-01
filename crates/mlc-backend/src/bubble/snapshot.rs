use mlc_core::ids::SyncMemberId;
use mlc_core::preset::snapshots::{DataBubbleSnapshot, MemberOverrideSnapshot};
use mlc_core::drawing::PrimitiveSnapshot;

use super::data_bubble::DataBubble;

impl DataBubble {
    /// Capture a snapshot from this bubble.
    ///
    /// `synced_panels` is the global BubbleStore.synced_panels set (panel ids
    /// flagged "follow active chart"). We persist only those panel ids that
    /// are also members of THIS bubble so that on restore each bubble recovers
    /// its own follow-list.
    pub fn to_snapshot(&self, synced_panels: &[SyncMemberId]) -> DataBubbleSnapshot {
        let primitives: Vec<PrimitiveSnapshot> = self
            .primitives
            .iter()
            .map(|p| PrimitiveSnapshot::from_primitive(p.as_ref()))
            .collect();

        // Separate chart and panel members
        let mut members: Vec<u64> = Vec::new();
        let mut panel_members: Vec<u64> = Vec::new();
        for member in &self.members {
            match member {
                SyncMemberId::Chart(id) => members.push(*id),
                SyncMemberId::Panel(id) => panel_members.push(*id),
            }
        }

        // Cross-reference synced_panels with this group's panel members so
        // we save only the panels that BELONG to this group.
        let synced_panel_ids: Vec<u64> = synced_panels
            .iter()
            .filter_map(|m| match m {
                SyncMemberId::Panel(id) if self.members.contains(m) => Some(*id),
                _ => None,
            })
            .collect();

        // Serialize per-member overrides
        let member_overrides: Vec<MemberOverrideSnapshot> = self
            .member_overrides
            .iter()
            .map(|(member, ovr)| {
                let (kind, id) = match member {
                    SyncMemberId::Chart(id) => (0u8, *id),
                    SyncMemberId::Panel(id) => (1u8, *id),
                };
                MemberOverrideSnapshot {
                    kind,
                    id,
                    sync_symbol: ovr.sync_symbol,
                    sync_crosshair: ovr.sync_crosshair,
                }
            })
            .collect();

        // Serialize per-tool last-used style from the shared store.
        let last_used_style: std::collections::HashMap<String, serde_json::Value> = self
            .last_used_style
            .read()
            .map(|store| {
                store
                    .iter()
                    .filter_map(|(k, v)| serde_json::to_value(v).ok().map(|val| (k.clone(), val)))
                    .collect()
            })
            .unwrap_or_default();

        DataBubbleSnapshot {
            id: self.id.0,
            color: self.color,
            symbol: self.address.symbol.clone(),
            exchange: self.address.exchange.clone(),
            account_type: self.address.account_type.as_label().to_string(),
            timeframe: self.timeframe.clone(),
            sync_flags: self.sync_flags.clone(),
            scale_mode: self.scale_mode,
            indicator_configs: self.indicator_configs.clone(),
            members,
            panel_members,
            member_overrides,
            synced_panel_ids,
            primitives,
            command_history: Some(self.command_history.clone()),
            auto_created: self.auto_created,
            last_used_style,
            alerts: self.alerts.clone(),
        }
    }
}
