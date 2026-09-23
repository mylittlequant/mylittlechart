//! View-model contract for the messenger UI (Community modal → Messages
//! tab, rewrite piece UI-1) — plain, Matrix-shaped types plus a
//! synchronous [`MessengerProvider`] trait. Built per the owner's
//! Matrix-basis ruling (`docs/mlc/plans/community-messenger-arc-
//! 2026-09-24.md` §7.2/§7.3): the UI is built now against today's 1:1 DM
//! data behind this trait; the `messenger-proto` team implements it over
//! the real Matrix-shaped protocol later, in its own worktree, without the
//! UI changing shape.
//!
//! Hard rules this crate holds itself to, since two separate teams share
//! it as a contract:
//! - no `uzor`, no rendering, no network, no async — a plain data crate,
//!   wasm-compatible by construction (pure `std`, zero external deps);
//! - ids ([`RoomId`]/[`EventId`]/[`UserId`]/[`TagName`]/[`PaginationToken`])
//!   are OPAQUE `String` newtypes, never parsed;
//! - relations (edits, redactions, reactions, reply previews) arrive
//!   ALREADY AGGREGATED onto a [`TimelineItem`] — the UI must never
//!   re-aggregate raw relation events, and this crate has no type for one;
//! - folders ARE tags ([`Folder::id`] is a [`TagName`]), not a separate
//!   store;
//! - every mutation is one [`MessengerProvider::enqueue`] call into a
//!   mailbox the provider drains on its own schedule — the UI never
//!   awaits, and this trait has no `async fn`.

mod folder;
mod ids;
mod member;
mod misc;
mod provider;
mod room;
mod timeline;

pub use folder::{Folder, FolderExclude, FolderIcon, FolderInclude};
pub use ids::{EventId, PaginationToken, RoomId, TagName, UserId};
pub use member::{initials_from_name, Member, MemberRole, RankBadge};
pub use misc::{
    BackPagination, Capabilities, CreateRoomRequest, MessengerIntent, PowerLevel, RoomVisibility,
    Timestamp, VaultState,
};
pub use provider::MessengerProvider;
pub use room::{LastEventPreview, PreviewContent, RoomKind, RoomSummary, Tag};
pub use timeline::{
    MembershipChange, MembershipEvent, MessageContent, ReactionSummary, ReplyPreview, SendState,
    TimelineItem, UtdReason,
};
