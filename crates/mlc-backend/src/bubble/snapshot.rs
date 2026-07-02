use mlc_core::ids::SyncMemberId;
use mlc_core::preset::snapshots::{DataBubbleSnapshot, MemberOverrideSnapshot};
use mlc_core::drawing::PrimitiveSnapshot;
use mlc_core::tag::Tag;

use super::data_bubble::DataBubble;

/// Capture a snapshot of `bubble` joined with its paired `tag`.
///
/// Domain fields that moved to `Tag` in the Step 6 doctrine cutover
/// (`color`, `auto_created`, `members`, `panel_members`,
/// `member_overrides`) are read from `tag`, not `bubble` — `DataBubbleSnapshot`'s
/// on-disk JSON shape is unchanged, only the in-memory source of these
/// fields moved.
///
/// `synced_panels` is the global `TagManager.synced_panels` set (panel ids
/// flagged "follow active chart"). We persist only those panel ids that are
/// also members of THIS tag so that on restore each bubble recovers its own
/// follow-list.
pub fn to_snapshot(bubble: &DataBubble, tag: &Tag, synced_panels: &[SyncMemberId]) -> DataBubbleSnapshot {
    let primitives: Vec<PrimitiveSnapshot> = bubble
        .primitives
        .iter()
        .map(|p| PrimitiveSnapshot::from_primitive(p.as_ref()))
        .collect();

    // Separate chart and panel members.
    let mut members: Vec<u64> = Vec::new();
    let mut panel_members: Vec<u64> = Vec::new();
    for member in &tag.members {
        match member {
            SyncMemberId::Chart(id) => members.push(*id),
            SyncMemberId::Panel(id) => panel_members.push(*id),
        }
    }

    // Cross-reference synced_panels with this tag's panel members so we
    // save only the panels that BELONG to this tag.
    let synced_panel_ids: Vec<u64> = synced_panels
        .iter()
        .filter_map(|m| match m {
            SyncMemberId::Panel(id) if tag.members.contains(m) => Some(*id),
            _ => None,
        })
        .collect();

    // Serialize per-member overrides.
    let member_overrides: Vec<MemberOverrideSnapshot> = tag
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
    let last_used_style: std::collections::HashMap<String, serde_json::Value> = bubble
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
        id: bubble.id.0,
        color: tag.color,
        symbol: bubble.address.symbol.clone(),
        exchange: bubble.address.exchange.clone(),
        account_type: bubble.address.account_type.as_label().to_string(),
        timeframe: bubble.timeframe.clone(),
        sync_flags: bubble.sync_flags.clone(),
        scale_mode: bubble.scale_mode,
        indicator_configs: bubble.indicator_configs.clone(),
        members,
        panel_members,
        member_overrides,
        synced_panel_ids,
        primitives,
        command_history: Some(bubble.command_history.clone()),
        auto_created: tag.auto_created,
        last_used_style,
        alerts: bubble.alerts.clone(),
    }
}

#[cfg(test)]
mod json_compat_tests {
    //! Gate tests for the Step 6 doctrine cutover: `DataBubbleSnapshot`'s
    //! on-disk JSON shape is unchanged (tag fields — `color`, `auto_created`,
    //! `members`, `panel_members`, `member_overrides` — still live on the
    //! flat snapshot struct), but the in-memory split moved: `DataBubble`
    //! no longer owns these fields, `Tag` does.
    //!
    //! (1) proves a legacy (pre-Step-6) preset JSON — where these fields
    //! were sourced from `DataBubble` — still deserializes and reconstructs
    //! correctly into `BubbleStore` (registry) + `TagManager` (tag fields).
    //! (2) proves the new split round-trips: `to_snapshot` (backend) reads
    //! tag fields from `Tag`, and restoring that snapshot rebuilds the same
    //! `Tag`.

    use super::*;
    use crate::bubble::BubbleStore;
    use crate::tag::TagManager;
    use mlc_core::ids::BubbleId;

    /// Literal fixture: one legacy `DataBubbleSnapshot` JSON entry exactly as
    /// a pre-Step-6 preset file would contain it — a colored, 2-member,
    /// non-auto-created tag with one Chart member override.
    const LEGACY_SNAPSHOT_JSON: &str = r#"{
        "id": 42,
        "color": [0.8, 0.2, 0.1, 1.0],
        "symbol": "BTCUSDT",
        "exchange": "binance",
        "account_type": "S",
        "timeframe": { "name": "1H", "ms": 3600000 },
        "sync_flags": {
            "sync_crosshair": true,
            "sync_viewport": true,
            "sync_symbol": true,
            "sync_timeframe": true,
            "sync_drawings": true,
            "sync_indicators": true
        },
        "indicator_configs": [],
        "members": [1, 2],
        "panel_members": [9],
        "member_overrides": [
            { "kind": 0, "id": 1, "sync_symbol": false, "sync_crosshair": null }
        ],
        "synced_panel_ids": [9],
        "primitives": [],
        "auto_created": false,
        "alerts": []
    }"#;

    #[test]
    fn legacy_snapshot_restores_into_bubble_store_plus_tag_manager() {
        let snap: DataBubbleSnapshot =
            serde_json::from_str(LEGACY_SNAPSHOT_JSON).expect("legacy snapshot JSON must deserialize");
        let snaps = vec![snap];

        let mut bubbles = BubbleStore::new();
        let restored_bids = bubbles.restore_from_snapshots(&snaps);
        assert_eq!(restored_bids, vec![BubbleId(42)]);

        let mut tags = TagManager::new();
        let panel_syncs = tags.restore_from_snapshots(&snaps);

        // Domain fields landed on Tag, not DataBubble.
        let tag = tags.get(BubbleId(42)).expect("tag must exist after restore");
        assert_eq!(tag.color, [0.8, 0.2, 0.1, 1.0]);
        assert!(!tag.auto_created);
        assert!(tag.members.contains(&SyncMemberId::Chart(1)));
        assert!(tag.members.contains(&SyncMemberId::Chart(2)));
        assert!(tag.members.contains(&SyncMemberId::Panel(9)));
        assert_eq!(tag.members.len(), 3);
        let ovr = tag.member_overrides.get(&SyncMemberId::Chart(1)).expect("override");
        assert_eq!(ovr.sync_symbol, Some(false));
        assert_eq!(ovr.sync_crosshair, None);

        // Routing table rebuilt.
        assert_eq!(tags.tag_for_member(SyncMemberId::Chart(1)), Some(BubbleId(42)));
        assert_eq!(tags.tag_for_member(SyncMemberId::Panel(9)), Some(BubbleId(42)));

        // Synced-panel restore hook fired for the one synced panel member.
        assert_eq!(panel_syncs, vec![(SyncMemberId::Panel(9), BubbleId(42))]);
        assert!(tags.is_synced(SyncMemberId::Panel(9)));

        // DataBubble itself carries none of the tag fields any more — the
        // registry-side symbol/exchange/timeframe still round-trip.
        assert_eq!(bubbles.symbol(BubbleId(42)), Some("BTCUSDT"));
        assert_eq!(bubbles.exchange(BubbleId(42)), Some("binance"));
    }

    #[test]
    fn round_trips_current_split_format() {
        let bid = BubbleId(7);
        let mut bubbles = BubbleStore::new();
        let created_bid = bubbles.create_group("ETHUSDT".to_string(), mlc_core::state::Timeframe::m1());
        // Re-key to a known id for the assertions below.
        mlc_core::ids::BubbleId::bump_past(bid.0);

        let mut tags = TagManager::new();
        let mut tag = Tag::new(created_bid, [0.1, 0.2, 0.3, 1.0]);
        tag.members.insert(SyncMemberId::Chart(5));
        tags.insert(tag);
        tags.mark_synced(SyncMemberId::Panel(11));

        let bubble = bubbles.bubble(created_bid).expect("bubble exists");
        let tag_ref = tags.get(created_bid).expect("tag exists");
        let synced = tags.synced_panels_snapshot();
        let snap = to_snapshot(bubble, tag_ref, &synced);

        assert_eq!(snap.color, [0.1, 0.2, 0.3, 1.0]);
        assert!(!snap.auto_created);
        assert_eq!(snap.members, vec![5]);
        assert_eq!(snap.symbol, "ETHUSDT");

        // Round-trip through JSON.
        let json = serde_json::to_string(&snap).expect("serialize");
        let back: DataBubbleSnapshot = serde_json::from_str(&json).expect("deserialize");

        let mut bubbles2 = BubbleStore::new();
        bubbles2.restore_from_snapshots(std::slice::from_ref(&back));
        let mut tags2 = TagManager::new();
        tags2.restore_from_snapshots(std::slice::from_ref(&back));

        let restored_tag = tags2.get(created_bid).expect("restored tag");
        assert_eq!(restored_tag.color, [0.1, 0.2, 0.3, 1.0]);
        assert!(restored_tag.members.contains(&SyncMemberId::Chart(5)));
    }
}
