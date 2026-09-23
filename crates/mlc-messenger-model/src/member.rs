//! Room members — role/power-level, ladder rank badge, and the pure
//! `initials_from_name` helper every avatar-drawing adapter uses to fill a
//! fallback initials string before a real avatar image exists.

use crate::ids::UserId;
use crate::misc::{PowerLevel, Timestamp};

/// Coarse role a UI draws a badge/permission decision from — derived from a
/// room's own power-level scheme via [`MemberRole::from_power_level`] for
/// `Owner`/`Admin`/`Member`. `Subscriber` has no power-level representation
/// at all (a public channel's read-only follower may hold no membership
/// event in the room whatsoever) — the provider sets it explicitly.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberRole {
    Owner,
    Admin,
    Member,
    Subscriber,
}

impl MemberRole {
    /// Pure mapping from a raw power level to the coarse role a group/
    /// channel member list draws — never returns `Subscriber` (see this
    /// type's own doc). Thresholds match the Matrix convention: `100` =
    /// room-creator/admin tier, `50` = moderator, anything else = plain
    /// member.
    pub fn from_power_level(pl: PowerLevel) -> Self {
        if pl.0 >= 100 {
            MemberRole::Owner
        } else if pl.0 >= 50 {
            MemberRole::Admin
        } else {
            MemberRole::Member
        }
    }

    /// Whether this role can moderate the room (remove members, pin/delete
    /// others' messages, edit room settings) — `Owner`/`Admin` only.
    pub fn can_moderate(self) -> bool {
        matches!(self, MemberRole::Owner | MemberRole::Admin)
    }
}

/// A site-wide ladder rank badge shown next to a name (`PAID`/`TRUSTED`/
/// `MOD`/`ADMIN` in the approved concept) — orthogonal to a member's own
/// per-room [`MemberRole`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RankBadge {
    Paid,
    Trusted,
    Moderator,
    Admin,
}

/// One room member, as the UI needs to draw them (member list, mention
/// autocomplete, the New Group dialog's people picker).
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    pub user_id: UserId,
    pub display_name: String,
    pub initials: String,
    pub rank: Option<RankBadge>,
    pub role: MemberRole,
    pub power_level: PowerLevel,
    pub online: bool,
    pub last_active: Option<Timestamp>,
    /// `false` disables the "add to a group" picker row with the "no
    /// messaging key yet — can join once they open Messages" explanation
    /// the concept's New Group dialog shows for a keyless candidate.
    pub has_messaging_key: bool,
}

/// Compute a short "avatar initials" fallback from a display name — pure,
/// used by every adapter that fills [`crate::RoomSummary::initials`]/
/// [`Member::initials`] before a real avatar image exists. Up to two
/// initials: the first letter of the first two whitespace-separated words,
/// or the first two characters of a single word; `"?"` for an empty name.
pub fn initials_from_name(name: &str) -> String {
    let mut words = name.split_whitespace();
    match (words.next(), words.next()) {
        (Some(a), Some(b)) => match (a.chars().next(), b.chars().next()) {
            (Some(fa), Some(fb)) => [fa, fb].iter().collect::<String>().to_uppercase(),
            _ => "?".to_string(),
        },
        (Some(a), None) => {
            let two: String = a.chars().take(2).collect();
            if two.is_empty() {
                "?".to_string()
            } else {
                two.to_uppercase()
            }
        }
        _ => "?".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_power_level_matches_matrix_thresholds() {
        assert_eq!(MemberRole::from_power_level(PowerLevel(100)), MemberRole::Owner);
        assert_eq!(MemberRole::from_power_level(PowerLevel(75)), MemberRole::Admin);
        assert_eq!(MemberRole::from_power_level(PowerLevel(50)), MemberRole::Admin);
        assert_eq!(MemberRole::from_power_level(PowerLevel(49)), MemberRole::Member);
        assert_eq!(MemberRole::from_power_level(PowerLevel(0)), MemberRole::Member);
        assert_eq!(MemberRole::from_power_level(PowerLevel(-5)), MemberRole::Member);
    }

    #[test]
    fn can_moderate_is_owner_and_admin_only() {
        assert!(MemberRole::Owner.can_moderate());
        assert!(MemberRole::Admin.can_moderate());
        assert!(!MemberRole::Member.can_moderate());
        assert!(!MemberRole::Subscriber.can_moderate());
    }

    #[test]
    fn initials_take_first_letter_of_first_two_words() {
        assert_eq!(initials_from_name("Trading Desk"), "TD");
        assert_eq!(initials_from_name("  zeng3ld   trader "), "ZT");
    }

    #[test]
    fn initials_fall_back_to_two_chars_of_a_single_word() {
        assert_eq!(initials_from_name("moderator"), "MO");
        assert_eq!(initials_from_name("Z"), "Z");
    }

    #[test]
    fn initials_of_an_empty_name_is_a_question_mark() {
        assert_eq!(initials_from_name(""), "?");
        assert_eq!(initials_from_name("   "), "?");
    }
}
