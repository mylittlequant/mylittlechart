//! Opaque identifiers — plain wrappers over `String`, never parsed (no
//! localpart/server splitting, no validation, no assumed structure). The
//! provider that implements [`crate::MessengerProvider`] over a real
//! protocol owns whatever internal shape its own ids carry; this crate only
//! ever stores and compares them as-is (coordinator ruling 2026-09-24: room/
//! event/user ids are opaque strings, no parsing).

use std::fmt;

macro_rules! opaque_string_id {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Clone, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(pub String);

        impl $name {
            pub fn new(value: impl Into<String>) -> Self {
                Self(value.into())
            }

            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(value: String) -> Self {
                Self(value)
            }
        }

        impl From<&str> for $name {
            fn from(value: &str) -> Self {
                Self(value.to_string())
            }
        }
    };
}

opaque_string_id!(
    RoomId,
    "A room's own opaque id — a Matrix room id when the provider is \
     Matrix-backed, or whatever stand-in id today's DM adapter assigns (see \
     `mlc-app-layout`'s DM `MessengerProvider` adapter)."
);
opaque_string_id!(
    EventId,
    "One timeline event's own opaque id — a Matrix event id when the \
     provider is Matrix-backed."
);
opaque_string_id!(
    UserId,
    "A user's own opaque id — a Matrix user id (`@user:server`) when the \
     provider is Matrix-backed, or a plain account id/nick string \
     otherwise. Never parsed by this crate."
);
opaque_string_id!(
    TagName,
    "A room tag's own name — Matrix `m.tag` naming (`m.favourite`, \
     `u.<custom>`, …) when the provider is Matrix-backed. A custom \
     folder's identity IS its own tag name — see [`crate::Folder`]'s own \
     doc for why folders are tags, not a separate store."
);
opaque_string_id!(
    PaginationToken,
    "An opaque back-pagination cursor for one room's timeline — never \
     constructed or inspected by the UI, only round-tripped back into \
     [`crate::MessengerProvider`] queries."
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_ids_round_trip_through_string_without_parsing() {
        let room = RoomId::from("!abc123:example.org");
        assert_eq!(room.as_str(), "!abc123:example.org");
        assert_eq!(room.to_string(), "!abc123:example.org");
        assert_eq!(RoomId::new("x"), RoomId::from("x".to_string()));
    }
}
