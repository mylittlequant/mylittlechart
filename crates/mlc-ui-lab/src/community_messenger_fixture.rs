//! A hand-built [`MessengerProvider`] fixture reproducing messenger concept
//! v1's own screen "1 · Группа" — folders, the chat list (Trading desk /
//! zeng3ld / BTC levels / moderator / MLC Announcements / Scalpers / Saved
//! messages / kvant_42), and the open Trading desk group's own timeline
//! (a pinned rules message, a grouped kvant_42 pair, a shared-chart-card
//! message with reactions, an own reply quoting kvant_42, and an `@admin`
//! mention from moderator) — feeding the REAL `messenger_overlay.rs`
//! renderer exactly the way a Matrix-shaped provider eventually will.
//!
//! Every room/timeline fact here is invented ONCE, in this file, rather
//! than scattered across `community.rs`'s own fixture builders (those stay
//! `DmState`-shaped, for the pre-existing `messages-chat`/`messages-locked`
//! 1:1 states) — this crate's own top doc already explains why the
//! Community lab page hosts the REAL production renderer against hand-built
//! state; this is that same idea, one level deeper into the messenger
//! model.
//!
//! `rooms()` intentionally holds exactly the 8 rooms the concept's own
//! chat-list panel shows — not the concept's own illustrative rail badge
//! counts ("All chats 18", "Channels 13", …), which describe a much larger
//! account this fixture never builds the other ~10 rooms for. Fabricating
//! ten more channels nobody ever sees would be inventing data with no
//! textual content behind it; the rail's own badges are derived HONESTLY
//! from these 8 real rooms instead (`messenger_overlay.rs`'s own
//! `rail_rows`), so they read smaller than the concept's own screenshot —
//! a documented, deliberate gap, not an oversight.

use mlc_messenger_model::{
    BackPagination, Capabilities, EventId, Folder, FolderExclude, FolderIcon, FolderInclude, Member,
    MemberRole, MessageContent, MessengerIntent, MessengerProvider, PowerLevel, PreviewContent,
    RankBadge, ReactionSummary, ReplyPreview, RoomId, RoomKind, RoomSummary, SendState,
    SharedChartCard, Tag, TagName, Timestamp, TimelineItem, UserId, VaultState,
};

const MINUTE_MS: f64 = 60_000.0;
const HOUR_MS: f64 = 3_600_000.0;
const DAY_MS: f64 = 86_400_000.0;

const ME: &str = "me";

fn ts(now_ms: f64, offset_ms: f64) -> Timestamp {
    Timestamp((now_ms - offset_ms) as i64)
}

fn room(id: &str) -> RoomId {
    RoomId::from(id)
}

fn user(id: &str) -> UserId {
    UserId::from(id)
}

fn event(id: &str) -> EventId {
    EventId::from(id)
}

/// The messenger concept v1 screen 1 fixture — see this module's own top
/// doc. `now_ms` is `js_sys::Date::now()` at boot, same "every timestamp is
/// relative to it" idiom `community.rs`'s own `ts_before` already uses, so
/// day separators/row time labels always read sensibly regardless of which
/// real day this page is opened on.
pub struct MessengerFixture {
    now_ms: f64,
}

impl MessengerFixture {
    pub fn new(now_ms: f64) -> Self {
        Self { now_ms }
    }

    fn rooms_data(&self) -> Vec<RoomSummary> {
        let now = self.now_ms;
        vec![
            RoomSummary {
                id: room("1"),
                kind: RoomKind::Group,
                name: "Trading desk".to_string(),
                initials: "TD".to_string(),
                is_encrypted: true,
                is_public: false,
                notification_count: 2,
                highlight_count: 0,
                last_event: Some(mlc_messenger_model::LastEventPreview {
                    sender: user("moderator"),
                    content: PreviewContent::Text("@admin закрепил правила — глянь, пожалуйста".to_string()),
                    ts: ts(now, 28.0 * MINUTE_MS),
                }),
                muted: false,
                pinned: true,
                archived: false,
                tags: vec![Tag { name: TagName::from("u.trading-desk"), order: 1.0 }],
                typing_user_ids: vec![user("zeng3ld")],
                peer_last_active: None,
                my_role: MemberRole::Member,
                my_power_level: PowerLevel(0),
                can_post: true,
            },
            RoomSummary {
                id: room("2"),
                kind: RoomKind::Direct,
                name: "zeng3ld".to_string(),
                initials: "Z".to_string(),
                is_encrypted: true,
                is_public: false,
                notification_count: 0,
                highlight_count: 0,
                last_event: Some(mlc_messenger_model::LastEventPreview {
                    sender: user("zeng3ld"),
                    content: PreviewContent::Text("новый ключ, переставил винду".to_string()),
                    ts: ts(now, 10.0 * MINUTE_MS),
                }),
                muted: false,
                pinned: true,
                archived: false,
                tags: Vec::new(),
                typing_user_ids: Vec::new(),
                peer_last_active: Some(ts(now, 2.0 * MINUTE_MS)),
                my_role: MemberRole::Member,
                my_power_level: PowerLevel(0),
                can_post: true,
            },
            RoomSummary {
                id: room("3"),
                kind: RoomKind::Channel,
                name: "BTC levels".to_string(),
                initials: "BL".to_string(),
                is_encrypted: true,
                is_public: false,
                notification_count: 12,
                highlight_count: 0,
                last_event: Some(mlc_messenger_model::LastEventPreview {
                    sender: user("kvant_42"),
                    content: PreviewContent::Text("Weekly levels: 62 800 / 66 400".to_string()),
                    ts: ts(now, 50.0 * MINUTE_MS),
                }),
                muted: true,
                pinned: false,
                archived: false,
                tags: Vec::new(),
                typing_user_ids: Vec::new(),
                peer_last_active: None,
                my_role: MemberRole::Subscriber,
                my_power_level: PowerLevel(0),
                can_post: false,
            },
            RoomSummary {
                id: room("4"),
                kind: RoomKind::Direct,
                name: "moderator".to_string(),
                initials: "M".to_string(),
                is_encrypted: true,
                is_public: false,
                notification_count: 2,
                highlight_count: 0,
                last_event: Some(mlc_messenger_model::LastEventPreview {
                    sender: user("moderator"),
                    content: PreviewContent::Text("закрепил тред с правилами".to_string()),
                    ts: ts(now, 4.0 * DAY_MS + 3.0 * HOUR_MS),
                }),
                muted: false,
                pinned: false,
                archived: false,
                tags: Vec::new(),
                typing_user_ids: Vec::new(),
                peer_last_active: None,
                my_role: MemberRole::Member,
                my_power_level: PowerLevel(0),
                can_post: true,
            },
            RoomSummary {
                id: room("5"),
                kind: RoomKind::Channel,
                name: "MLC Announcements".to_string(),
                initials: "M".to_string(),
                is_encrypted: true,
                is_public: true,
                notification_count: 1,
                highlight_count: 0,
                last_event: Some(mlc_messenger_model::LastEventPreview {
                    sender: user("mlc_team"),
                    content: PreviewContent::Text("0.5.614 — the forum, new icons".to_string()),
                    ts: ts(now, 3.0 * DAY_MS + 5.0 * HOUR_MS),
                }),
                muted: false,
                pinned: false,
                archived: false,
                tags: Vec::new(),
                typing_user_ids: Vec::new(),
                peer_last_active: None,
                my_role: MemberRole::Subscriber,
                my_power_level: PowerLevel(0),
                can_post: false,
            },
            RoomSummary {
                id: room("6"),
                kind: RoomKind::Group,
                name: "Scalpers".to_string(),
                initials: "SC".to_string(),
                is_encrypted: true,
                is_public: false,
                notification_count: 1,
                highlight_count: 1,
                last_event: Some(mlc_messenger_model::LastEventPreview {
                    sender: user("riverflow"),
                    content: PreviewContent::Text("ок, завтра гляну уровни".to_string()),
                    ts: ts(now, 2.0 * DAY_MS + 6.0 * HOUR_MS),
                }),
                muted: false,
                pinned: false,
                archived: false,
                tags: Vec::new(),
                typing_user_ids: Vec::new(),
                peer_last_active: None,
                my_role: MemberRole::Member,
                my_power_level: PowerLevel(0),
                can_post: true,
            },
            RoomSummary {
                id: room("7"),
                kind: RoomKind::Direct,
                name: "Saved messages".to_string(),
                initials: String::new(),
                is_encrypted: true,
                is_public: false,
                notification_count: 0,
                highlight_count: 0,
                last_event: Some(mlc_messenger_model::LastEventPreview {
                    sender: user(ME),
                    content: PreviewContent::Text("BTCUSDT · 4H — SMC scan".to_string()),
                    ts: ts(now, 13.0 * DAY_MS),
                }),
                muted: false,
                pinned: false,
                archived: false,
                tags: Vec::new(),
                typing_user_ids: Vec::new(),
                peer_last_active: None,
                my_role: MemberRole::Member,
                my_power_level: PowerLevel(0),
                can_post: true,
            },
            RoomSummary {
                id: room("8"),
                kind: RoomKind::Direct,
                name: "kvant_42".to_string(),
                initials: "K".to_string(),
                is_encrypted: true,
                is_public: false,
                notification_count: 0,
                highlight_count: 0,
                last_event: Some(mlc_messenger_model::LastEventPreview {
                    sender: user(ME),
                    content: PreviewContent::Text("спасибо, пресет работает".to_string()),
                    ts: ts(now, 12.0 * DAY_MS + 4.0 * HOUR_MS),
                }),
                muted: false,
                pinned: false,
                archived: false,
                tags: Vec::new(),
                typing_user_ids: Vec::new(),
                peer_last_active: None,
                my_role: MemberRole::Member,
                my_power_level: PowerLevel(0),
                can_post: true,
            },
        ]
    }

    /// Trading desk's own held timeline — oldest first (trait contract):
    /// a pinned rules message from a few days back, a grouped kvant_42
    /// pair, zeng3ld's shared-chart-card message with reactions, an own
    /// reply (edited) quoting kvant_42, and moderator's `@admin` mention.
    fn trading_desk_timeline(&self) -> Vec<TimelineItem> {
        let now = self.now_ms;
        vec![
            TimelineItem {
                event_id: event("e0"),
                sender: user("moderator"),
                sender_rank: Some(RankBadge::Moderator),
                ts: ts(now, 4.0 * DAY_MS + 2.0 * HOUR_MS),
                content: MessageContent::Text("Правила: сетап — только с графиком, без сигналов «в лоб»".to_string()),
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: true,
                shared_chart: None,
                mentions_me: false,
                forwarded: None,
            },
            TimelineItem {
                event_id: event("e1"),
                sender: user("kvant_42"),
                sender_rank: Some(RankBadge::Paid),
                ts: ts(now, 42.0 * MINUTE_MS),
                content: MessageContent::Text("BTC держит 64к?".to_string()),
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: None,
                mentions_me: false,
                forwarded: None,
            },
            TimelineItem {
                event_id: event("e2"),
                sender: user("kvant_42"),
                sender_rank: Some(RankBadge::Paid),
                ts: ts(now, 41.0 * MINUTE_MS),
                // `$BTCUSDT` — the messenger UI-2b cashtag pill
                // (`messenger_overlay.rs`'s own `text_tokens`): a `$`
                // immediately followed by an uppercase letter renders as a
                // clickable ticker pill (concept `.sym`), never a dollar
                // amount (those start with a digit).
                content: MessageContent::Text("на 4h ордерблок $BTCUSDT 64 150–64 420".to_string()),
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: None,
                mentions_me: false,
                forwarded: None,
            },
            TimelineItem {
                event_id: event("e3"),
                sender: user("zeng3ld"),
                sender_rank: Some(RankBadge::Paid),
                ts: ts(now, 36.0 * MINUTE_MS),
                content: MessageContent::Text("вот скан, нижний блок совпадает с его уровнем".to_string()),
                reply_to: None,
                edited: false,
                reactions: vec![
                    ReactionSummary { key: "👍".to_string(), count: 2, by_me: false },
                    ReactionSummary { key: "🔥".to_string(), count: 1, by_me: true },
                ],
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: Some(SharedChartCard {
                    symbol: "BTCUSDT".to_string(),
                    timeframe: "4H".to_string(),
                    venue: "BINANCE".to_string(),
                    note: "SMC scan · 2 order blocks · shared chart".to_string(),
                    allow_forward: true,
                }),
                mentions_me: false,
                forwarded: None,
            },
            TimelineItem {
                event_id: event("e4"),
                sender: user(ME),
                sender_rank: None,
                ts: ts(now, 31.0 * MINUTE_MS),
                content: MessageContent::Text("если закроем 4h ниже 64 150 — шорт до FVG".to_string()),
                reply_to: Some(ReplyPreview {
                    event_id: event("e2"),
                    sender: user("kvant_42"),
                    preview: "на 4h ордерблок $BTCUSDT 64 150–64 420".to_string(),
                }),
                edited: true,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: vec![user("zeng3ld")],
                pinned: false,
                shared_chart: None,
                mentions_me: false,
                forwarded: None,
            },
            TimelineItem {
                event_id: event("e5"),
                sender: user("moderator"),
                sender_rank: Some(RankBadge::Moderator),
                ts: ts(now, 28.0 * MINUTE_MS),
                content: MessageContent::Text("@admin закрепил правила — глянь, пожалуйста".to_string()),
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: None,
                mentions_me: true,
                forwarded: None,
            },
        ]
    }

    fn trading_desk_members(&self) -> Vec<Member> {
        vec![
            Member {
                user_id: user(ME),
                display_name: "You".to_string(),
                initials: "Y".to_string(),
                rank: None,
                role: MemberRole::Member,
                power_level: PowerLevel(0),
                online: true,
                last_active: None,
                has_messaging_key: true,
            },
            Member {
                user_id: user("kvant_42"),
                display_name: "kvant_42".to_string(),
                initials: "K".to_string(),
                rank: Some(RankBadge::Paid),
                role: MemberRole::Member,
                power_level: PowerLevel(0),
                online: false,
                last_active: None,
                has_messaging_key: true,
            },
            Member {
                user_id: user("zeng3ld"),
                display_name: "zeng3ld".to_string(),
                initials: "Z".to_string(),
                rank: Some(RankBadge::Paid),
                role: MemberRole::Member,
                power_level: PowerLevel(0),
                online: true,
                last_active: None,
                has_messaging_key: true,
            },
            Member {
                user_id: user("moderator"),
                display_name: "moderator".to_string(),
                initials: "M".to_string(),
                rank: Some(RankBadge::Moderator),
                role: MemberRole::Admin,
                power_level: PowerLevel(50),
                online: false,
                last_active: None,
                has_messaging_key: true,
            },
        ]
    }

    /// A bare 1:1 member pair for a Direct room `peer_id`/`peer_name` —
    /// only [`Member::rank`] matters to the chat-list row's own rank chip
    /// (`moderator`'s `MOD` chip); every other Direct room in this fixture
    /// has no rank chip in the concept, so its peer's `rank` stays `None`.
    fn direct_members(peer_id: &str, peer_name: &str, peer_rank: Option<RankBadge>) -> Vec<Member> {
        vec![
            Member {
                user_id: user(ME),
                display_name: "You".to_string(),
                initials: "Y".to_string(),
                rank: None,
                role: MemberRole::Member,
                power_level: PowerLevel(0),
                online: true,
                last_active: None,
                has_messaging_key: true,
            },
            Member {
                user_id: user(peer_id),
                display_name: peer_name.to_string(),
                initials: mlc_messenger_model::initials_from_name(peer_name),
                rank: peer_rank,
                role: MemberRole::Member,
                power_level: PowerLevel(0),
                online: false,
                last_active: None,
                has_messaging_key: true,
            },
        ]
    }

    /// "BTC levels" channel's own post feed (concept screen "2 · Канал") —
    /// a Monday post with reactions/read receipts and a today post, plus
    /// ONE forwarded post (this fixture's own addition, not in the concept
    /// screenshot — the concept never shows a forward inside a channel;
    /// added here only so the forward attribution rule
    /// ([`mlc_messenger_model::ForwardedFrom`]) has something to grade).
    fn btc_levels_timeline(&self) -> Vec<TimelineItem> {
        let now = self.now_ms;
        vec![
            TimelineItem {
                event_id: event("c0"),
                sender: user("kvant_42"),
                sender_rank: None,
                ts: ts(now, DAY_MS + HOUR_MS + 47.0 * MINUTE_MS),
                content: MessageContent::Text(
                    "Weekly levels. Support 62 800 (weekly OB), resistance 66 400 (last month's high). \
                     Invalidation — a daily close under 61 900."
                        .to_string(),
                ),
                reply_to: None,
                edited: false,
                reactions: vec![
                    ReactionSummary { key: "👍".to_string(), count: 41, by_me: false },
                    ReactionSummary { key: "🔥".to_string(), count: 12, by_me: true },
                    ReactionSummary { key: "🤔".to_string(), count: 3, by_me: false },
                ],
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: None,
                mentions_me: false,
                forwarded: None,
            },
            TimelineItem {
                event_id: event("c1"),
                sender: user("kvant_42"),
                sender_rank: None,
                ts: ts(now, 50.0 * MINUTE_MS),
                content: MessageContent::Text(
                    "Price is back inside the weekly range; no new trade until one of the edges is tested."
                        .to_string(),
                ),
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: None,
                mentions_me: false,
                forwarded: None,
            },
            TimelineItem {
                event_id: event("c2"),
                sender: user("kvant_42"),
                sender_rank: None,
                ts: ts(now, 12.0 * MINUTE_MS),
                content: MessageContent::Text("$BTCUSDT holding the weekly OB — no change to the plan above.".to_string()),
                reply_to: None,
                edited: false,
                reactions: Vec::new(),
                redacted: false,
                send_state: SendState::Sent,
                read_by: Vec::new(),
                pinned: false,
                shared_chart: None,
                mentions_me: false,
                forwarded: Some(mlc_messenger_model::ForwardedFrom::Channel { room_name: "ETH flows".to_string() }),
            },
        ]
    }

    fn btc_levels_members(&self) -> Vec<Member> {
        vec![Member {
            user_id: user("kvant_42"),
            display_name: "kvant_42".to_string(),
            initials: "K".to_string(),
            rank: Some(RankBadge::Paid),
            role: MemberRole::Admin,
            power_level: PowerLevel(100),
            online: false,
            last_active: None,
            has_messaging_key: true,
        }]
    }

    fn folders_data(&self) -> Vec<Folder> {
        vec![Folder {
            id: Some(TagName::from("u.trading-desk")),
            name: "Trading desk".to_string(),
            icon: FolderIcon::Candles,
            include: FolderInclude::default(),
            exclude: FolderExclude::default(),
            order: 1.0,
        }]
    }
}

impl MessengerProvider for MessengerFixture {
    fn change_counter(&self) -> u64 {
        1
    }

    fn capabilities(&self) -> Capabilities {
        Capabilities {
            groups: true,
            channels: true,
            folders: true,
            reactions: true,
            edits: true,
            replies: true,
            typing: true,
            receipts: true,
        }
    }

    fn vault_state(&self) -> VaultState {
        VaultState::Ready
    }

    fn last_error(&self) -> Option<String> {
        None
    }

    fn rooms(&self) -> Vec<RoomSummary> {
        self.rooms_data()
    }

    fn room(&self, room_id: &RoomId) -> Option<RoomSummary> {
        self.rooms_data().into_iter().find(|r| &r.id == room_id)
    }

    fn timeline(&self, room_id: &RoomId) -> Vec<TimelineItem> {
        match room_id.as_str() {
            "1" => self.trading_desk_timeline(),
            "3" => self.btc_levels_timeline(),
            _ => Vec::new(),
        }
    }

    fn back_pagination(&self, room_id: &RoomId) -> BackPagination {
        if room_id.as_str() == "1" {
            BackPagination { token: None, has_more: true }
        } else {
            BackPagination::default()
        }
    }

    fn members(&self, room_id: &RoomId) -> Vec<Member> {
        match room_id.as_str() {
            "1" => self.trading_desk_members(),
            "2" => Self::direct_members("zeng3ld", "zeng3ld", None),
            "3" => self.btc_levels_members(),
            "4" => Self::direct_members("moderator", "moderator", Some(RankBadge::Moderator)),
            "8" => Self::direct_members("kvant_42", "kvant_42", None),
            _ => Vec::new(),
        }
    }

    fn folders(&self) -> Vec<Folder> {
        self.folders_data()
    }

    /// Static fixture, no click-dispatch wiring this pass — see this
    /// module's own top doc; nothing this lab page draws ever calls this
    /// today (no DOM event polling), so recording an intent nobody reads
    /// would be dead state, not a real mailbox.
    fn enqueue(&mut self, _intent: MessengerIntent) {}
}
