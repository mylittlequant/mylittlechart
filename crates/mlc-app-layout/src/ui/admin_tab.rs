//! State, pure logic and view models of the settings modal's **Admin** tab.
//!
//! The tab exists for an account holding the `admin` role and drives the
//! server's `/api/staff/*` routes: gift-code batches (Coupons), paid days and
//! lifetime grants by nick (Grants), and an exact-nick account lookup (User).
//! The server is the authority — it answers everyone else with the same 403 —
//! so nothing here is a security boundary; hiding the tab and bounding the
//! forms only keeps an honest admin from wasting one of their 30 writes a
//! minute on an input the server would refuse.
//!
//! Split of work, the same mailbox contract the rest of the settings modal
//! follows: this module holds plain state and one-shot flags
//! ([`AdminState::pending`] out, [`AdminOutcome`] in); the shell does the
//! fetches (`mlc-app-shell-web`'s `cloud_session`); `mlc-app` wires clicks and
//! the seven text fields ([`AdminField`]); `layout::modals::
//! user_settings_admin` draws what is here.
//!
//! Gift codes are secrets shown once: [`AdminMinted`] never prints them in
//! `Debug`, and [`AdminState::on_close`] drops them.

use mlc_core::i18n::{AdminKey, Language};

use crate::ui::scroll_state::ScrollState;

// =============================================================================
// Server bounds (mirrors `routes/staff.rs` and `routes/admin.rs`)
// =============================================================================

/// Days a gift code may grant.
pub const MIN_GIFT_DAYS: i64 = 1;
/// Days a gift code may grant.
pub const MAX_GIFT_DAYS: i64 = 365;
/// Codes one batch may hold.
pub const MIN_GIFT_COUNT: i64 = 1;
/// Codes one batch may hold.
pub const MAX_GIFT_COUNT: i64 = 200;
/// Paid days one grant may add.
pub const MIN_PAID_DAYS: i64 = 1;
/// Paid days one grant may add.
pub const MAX_PAID_DAYS: i64 = 3650;
/// Longest note, in characters (batch note and lifetime-grant reason).
pub const MAX_NOTE_CHARS: usize = 200;
/// Longest nick the server accepts in a request.
pub const MAX_NICK_CHARS: usize = 64;

// =============================================================================
// Text fields
// =============================================================================

/// Field id of the mint form's days box.
pub const FIELD_MINT_DAYS: &str = "admin_mint_days";
/// Field id of the mint form's count box.
pub const FIELD_MINT_COUNT: &str = "admin_mint_count";
/// Field id of the mint form's note box.
pub const FIELD_MINT_NOTE: &str = "admin_mint_note";
/// Field id of the Grants section's nick box.
pub const FIELD_GRANT_NICK: &str = "admin_grant_nick";
/// Field id of the Grants section's paid-days box.
pub const FIELD_GRANT_DAYS: &str = "admin_grant_days";
/// Field id of the Grants section's lifetime-reason box.
pub const FIELD_GRANT_NOTE: &str = "admin_grant_note";
/// Field id of the User section's nick box.
pub const FIELD_USER_NICK: &str = "admin_user_nick";

/// One of the tab's seven text inputs. The text itself lives in the
/// `TextFieldStore` (`mlc-app`); this names the field and its bounds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdminField {
    MintDays,
    MintCount,
    MintNote,
    GrantNick,
    GrantDays,
    GrantNote,
    UserNick,
}

impl AdminField {
    /// Every field, in registration order.
    pub const ALL: [AdminField; 7] = [
        AdminField::MintDays,
        AdminField::MintCount,
        AdminField::MintNote,
        AdminField::GrantNick,
        AdminField::GrantDays,
        AdminField::GrantNote,
        AdminField::UserNick,
    ];

    /// The `TextFieldStore` id.
    pub const fn id(self) -> &'static str {
        match self {
            Self::MintDays => FIELD_MINT_DAYS,
            Self::MintCount => FIELD_MINT_COUNT,
            Self::MintNote => FIELD_MINT_NOTE,
            Self::GrantNick => FIELD_GRANT_NICK,
            Self::GrantDays => FIELD_GRANT_DAYS,
            Self::GrantNote => FIELD_GRANT_NOTE,
            Self::UserNick => FIELD_USER_NICK,
        }
    }

    /// Inverse of [`Self::id`].
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| f.id() == id)
    }

    /// The store's own length cap. Deliberately above the server bound for the
    /// free-text fields, so an over-long paste stays editable and reads the
    /// "at most N characters" message instead of being cut silently.
    pub const fn max_len(self) -> usize {
        match self {
            Self::MintDays | Self::GrantDays => 6,
            Self::MintCount => 4,
            Self::MintNote | Self::GrantNote => 400,
            Self::GrantNick | Self::UserNick => 96,
        }
    }

    /// Key the renderer publishes the field's rect under (content items and
    /// hit id — one string, like the Account tab's redeem box), which the app
    /// reads back to stamp the store's geometry.
    pub fn content_key(self) -> String {
        AdminAction::Field(self).hit_id()
    }
}

// =============================================================================
// Sections and actions
// =============================================================================

/// The tab's three sections, switched by a strip above the body.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum AdminSection {
    #[default]
    Coupons,
    Grants,
    User,
}

impl AdminSection {
    /// Every section, in strip order.
    pub const ALL: [AdminSection; 3] = [AdminSection::Coupons, AdminSection::Grants, AdminSection::User];

    /// Stable id used in hit ids.
    pub fn id(self) -> &'static str {
        match self {
            Self::Coupons => "coupons",
            Self::Grants => "grants",
            Self::User => "user",
        }
    }

    /// Inverse of [`Self::id`].
    pub fn from_id(id: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.id() == id)
    }

    /// Strip label.
    pub fn label(self, lang: Language) -> &'static str {
        match self {
            Self::Coupons => AdminKey::SectionCoupons.get(lang),
            Self::Grants => AdminKey::SectionGrants.get(lang),
            Self::User => AdminKey::SectionUser.get(lang),
        }
    }
}

/// The two lifetime grants an admin can add or revoke.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LifetimeKind {
    Paid,
    Anon,
}

impl LifetimeKind {
    /// The server's wire name.
    pub fn wire(self) -> &'static str {
        match self {
            Self::Paid => "lifetime_paid",
            Self::Anon => "lifetime_anon",
        }
    }

    /// Inverse of [`Self::wire`].
    pub fn from_wire(wire: &str) -> Option<Self> {
        match wire {
            "lifetime_paid" => Some(Self::Paid),
            "lifetime_anon" => Some(Self::Anon),
            _ => None,
        }
    }

    /// Short id used in hit ids.
    fn tag(self) -> &'static str {
        match self {
            Self::Paid => "paid",
            Self::Anon => "anon",
        }
    }

    fn from_tag(tag: &str) -> Option<Self> {
        match tag {
            "paid" => Some(Self::Paid),
            "anon" => Some(Self::Anon),
            _ => None,
        }
    }

    /// The grant's name inside a sentence.
    pub fn name(self, lang: Language) -> &'static str {
        match self {
            Self::Paid => AdminKey::KindLifetimePaid.get(lang),
            Self::Anon => AdminKey::KindLifetimeAnon.get(lang),
        }
    }
}

/// Everything clickable on the tab. [`Self::hit_id`] is what the renderer
/// registers (after the `user_settings:` prefix) and [`Self::parse`] is what
/// the click router reads back — one definition for both sides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminAction {
    Section(AdminSection),
    /// Click into a text field (claimed on mouse-down by the store).
    Field(AdminField),
    Mint,
    CopyAll,
    CopyOne(usize),
    DismissCodes,
    RefreshBatches,
    RevokeBatch(String),
    ConfirmYes,
    ConfirmNo,
    PaidDays,
    LifetimeGrant(LifetimeKind),
    LifetimeRevoke(LifetimeKind),
    Lookup,
}

impl AdminAction {
    /// The widget id, including the `user_settings:` prefix every settings
    /// modal item carries.
    pub fn hit_id(&self) -> String {
        let tail = match self {
            Self::Section(s) => format!("section:{}", s.id()),
            Self::Field(f) => format!("field:{}", f.id()),
            Self::Mint => "mint".to_string(),
            Self::CopyAll => "copy_all".to_string(),
            Self::CopyOne(i) => format!("copy:{i}"),
            Self::DismissCodes => "done".to_string(),
            Self::RefreshBatches => "refresh".to_string(),
            Self::RevokeBatch(id) => format!("revoke:{id}"),
            Self::ConfirmYes => "yes".to_string(),
            Self::ConfirmNo => "no".to_string(),
            Self::PaidDays => "paid_days".to_string(),
            Self::LifetimeGrant(k) => format!("life_grant:{}", k.tag()),
            Self::LifetimeRevoke(k) => format!("life_revoke:{}", k.tag()),
            Self::Lookup => "lookup".to_string(),
        };
        format!("user_settings:admin:{tail}")
    }

    /// Parse a widget id (with or without the `user_settings:` prefix).
    /// `None` for anything that is not one of this tab's ids.
    pub fn parse(widget_id: &str) -> Option<Self> {
        let rest = widget_id.strip_prefix("user_settings:").unwrap_or(widget_id);
        let tail = rest.strip_prefix("admin:")?;
        match tail {
            "mint" => Some(Self::Mint),
            "copy_all" => Some(Self::CopyAll),
            "done" => Some(Self::DismissCodes),
            "refresh" => Some(Self::RefreshBatches),
            "yes" => Some(Self::ConfirmYes),
            "no" => Some(Self::ConfirmNo),
            "paid_days" => Some(Self::PaidDays),
            "lookup" => Some(Self::Lookup),
            _ => {
                let (head, arg) = tail.split_once(':')?;
                match head {
                    "section" => AdminSection::from_id(arg).map(Self::Section),
                    "field" => AdminField::from_id(arg).map(Self::Field),
                    "copy" => arg.parse().ok().map(Self::CopyOne),
                    "revoke" if !arg.is_empty() => Some(Self::RevokeBatch(arg.to_string())),
                    "life_grant" => LifetimeKind::from_tag(arg).map(Self::LifetimeGrant),
                    "life_revoke" => LifetimeKind::from_tag(arg).map(Self::LifetimeRevoke),
                    _ => None,
                }
            }
        }
    }
}

// =============================================================================
// Form validation
// =============================================================================

/// Why a form was refused before any request went out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FormError {
    DaysRange { min: i64, max: i64 },
    CountRange { min: i64, max: i64 },
    NoteTooLong { max: usize },
    NoteBadChars,
    NoteRequired { max: usize },
    NickRequired,
    NickTooLong { max: usize },
}

impl FormError {
    /// The sentence to print.
    pub fn message(&self, lang: Language) -> String {
        match self {
            Self::DaysRange { min, max } => Fill::new(AdminKey::ErrDaysRange.get(lang))
                .arg("min", min)
                .arg("max", max)
                .done(),
            Self::CountRange { min, max } => Fill::new(AdminKey::ErrCountRange.get(lang))
                .arg("min", min)
                .arg("max", max)
                .done(),
            Self::NoteTooLong { max } => Fill::new(AdminKey::ErrNoteTooLong.get(lang)).arg("max", max).done(),
            Self::NoteBadChars => AdminKey::ErrNoteBadChars.get(lang).to_string(),
            Self::NoteRequired { max } => Fill::new(AdminKey::ErrNoteRequired.get(lang)).arg("max", max).done(),
            Self::NickRequired => AdminKey::ErrNickRequired.get(lang).to_string(),
            Self::NickTooLong { max } => Fill::new(AdminKey::ErrNickTooLong.get(lang)).arg("max", max).done(),
        }
    }
}

/// A whole number inside `min..=max`, or `None`.
fn parse_bounded(raw: &str, min: i64, max: i64) -> Option<i64> {
    raw.trim().parse::<i64>().ok().filter(|n| (min..=max).contains(n))
}

/// Days of a gift code, 1..=365.
pub fn parse_gift_days(raw: &str) -> Result<i64, FormError> {
    parse_bounded(raw, MIN_GIFT_DAYS, MAX_GIFT_DAYS)
        .ok_or(FormError::DaysRange { min: MIN_GIFT_DAYS, max: MAX_GIFT_DAYS })
}

/// Codes in a batch, 1..=200.
pub fn parse_gift_count(raw: &str) -> Result<i64, FormError> {
    parse_bounded(raw, MIN_GIFT_COUNT, MAX_GIFT_COUNT)
        .ok_or(FormError::CountRange { min: MIN_GIFT_COUNT, max: MAX_GIFT_COUNT })
}

/// Paid days of a grant, 1..=3650.
pub fn parse_paid_days(raw: &str) -> Result<i64, FormError> {
    parse_bounded(raw, MIN_PAID_DAYS, MAX_PAID_DAYS)
        .ok_or(FormError::DaysRange { min: MIN_PAID_DAYS, max: MAX_PAID_DAYS })
}

/// A note, trimmed. `required` makes an empty one an error (the lifetime
/// reason); the batch note is optional. At most [`MAX_NOTE_CHARS`] characters
/// and no control characters — the server's rule.
pub fn clean_note(raw: &str, required: bool) -> Result<String, FormError> {
    let note = raw.trim();
    if note.is_empty() {
        return if required {
            Err(FormError::NoteRequired { max: MAX_NOTE_CHARS })
        } else {
            Ok(String::new())
        };
    }
    if note.chars().count() > MAX_NOTE_CHARS {
        return Err(FormError::NoteTooLong { max: MAX_NOTE_CHARS });
    }
    if note.chars().any(char::is_control) {
        return Err(FormError::NoteBadChars);
    }
    Ok(note.to_string())
}

/// An exact nick, trimmed, non-empty, at most [`MAX_NICK_CHARS`] characters.
pub fn clean_nick(raw: &str) -> Result<String, FormError> {
    let nick = raw.trim();
    if nick.is_empty() {
        return Err(FormError::NickRequired);
    }
    if nick.chars().count() > MAX_NICK_CHARS {
        return Err(FormError::NickTooLong { max: MAX_NICK_CHARS });
    }
    Ok(nick.to_string())
}

/// Replace `{name}` placeholders in a translated sentence, in one pass — a
/// substituted value that itself contains `{other}` is never re-expanded. An
/// unknown placeholder is left as written.
pub fn fill(template: &str, pairs: &[(&str, &str)]) -> String {
    let mut out = String::with_capacity(template.len());
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let tail = &rest[open + 1..];
        match tail.find('}') {
            Some(close) => {
                let key = &tail[..close];
                match pairs.iter().find(|(k, _)| *k == key) {
                    Some((_, value)) => out.push_str(value),
                    None => {
                        out.push('{');
                        out.push_str(key);
                        out.push('}');
                    }
                }
                rest = &tail[close + 1..];
            }
            None => {
                out.push('{');
                rest = tail;
            }
        }
    }
    out.push_str(rest);
    out
}

/// Builder over [`fill`] that owns its values, so call sites pass numbers,
/// `&String`s and `&str`s side by side without converting each one first.
pub struct Fill<'a> {
    template: &'a str,
    pairs: Vec<(&'a str, String)>,
}

impl<'a> Fill<'a> {
    /// Start filling `template`.
    pub fn new(template: &'a str) -> Self {
        Self { template, pairs: Vec::new() }
    }

    /// Bind `{key}` to `value`.
    pub fn arg(mut self, key: &'a str, value: impl ToString) -> Self {
        self.pairs.push((key, value.to_string()));
        self
    }

    /// The finished sentence.
    pub fn done(self) -> String {
        let pairs: Vec<(&str, &str)> = self.pairs.iter().map(|(k, v)| (*k, v.as_str())).collect();
        fill(self.template, &pairs)
    }
}

// =============================================================================
// Server data, as the tab holds it
// =============================================================================

/// A freshly minted batch. The codes are shown once and dropped on close.
#[derive(Clone, PartialEq, Eq)]
pub struct AdminMinted {
    pub batch_id: String,
    pub days: i64,
    pub count: i64,
    pub note: String,
    pub codes: Vec<String>,
}

impl std::fmt::Debug for AdminMinted {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AdminMinted")
            .field("batch_id", &self.batch_id)
            .field("days", &self.days)
            .field("count", &self.count)
            .field("note", &self.note)
            .field("codes", &format_args!("<{} redacted>", self.codes.len()))
            .finish()
    }
}

/// One row of the batches table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminBatchRow {
    pub batch_id: String,
    pub note: String,
    pub days: i64,
    pub count: i64,
    pub redeemed: i64,
    pub revoked: i64,
    pub created_at: String,
    pub created_by: Option<String>,
}

impl AdminBatchRow {
    /// Whether the batch still holds unredeemed codes: `redeemed + revoked`
    /// short of `count`.
    pub fn is_revocable(&self) -> bool {
        self.redeemed + self.revoked < self.count
    }
}

/// One lifetime-grant ledger row of the user card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminLedgerRow {
    pub grant_id: i64,
    pub grant: String,
    pub source: String,
    pub source_ref: String,
    pub created_at: String,
    pub revoked_at: Option<String>,
}

/// The User section's card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminUserCard {
    pub nick: String,
    pub user_id: i64,
    pub paid_until: Option<String>,
    pub lifetime_paid: bool,
    pub lifetime_anon: bool,
    pub ledger: Vec<AdminLedgerRow>,
    pub quant_tier: Option<String>,
    pub quant_active_until: Option<String>,
    pub roles: Vec<String>,
}

/// Result of a lifetime grant: created now, or already in force.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AdminLifetimeDone {
    pub nick: String,
    pub kind: LifetimeKind,
    pub created: bool,
}

// =============================================================================
// Failures
// =============================================================================

/// What went wrong with a request, classified by the shell from the transport
/// error (`mlc_cloud::staff_error`) so this crate needs no transport types.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminFailure {
    /// 403 — not an admin.
    Forbidden,
    /// 404 — unknown nick or batch.
    NotFound,
    /// 429 — the 30-writes-a-minute limit; `Retry-After` when sent.
    RateLimited { retry_after_secs: Option<u64> },
    /// 400 — the server's sentence about the input.
    BadRequest { detail: String },
    /// 401 — the session is no longer valid.
    SignedOut,
    /// The request never completed.
    Network { detail: String },
    /// Any other failure.
    Server { detail: String },
}

impl AdminFailure {
    /// The sentence to print, in `lang`.
    pub fn message(&self, lang: Language) -> String {
        match self {
            Self::Forbidden => AdminKey::FailForbidden.get(lang).to_string(),
            Self::NotFound => AdminKey::FailNotFound.get(lang).to_string(),
            Self::RateLimited { retry_after_secs: Some(secs) } => {
                Fill::new(AdminKey::FailRateLimited.get(lang)).arg("secs", secs).done()
            }
            Self::RateLimited { retry_after_secs: None } => AdminKey::FailRateLimitedSoon.get(lang).to_string(),
            Self::BadRequest { detail } => Fill::new(AdminKey::FailBadRequest.get(lang)).arg("detail", detail).done(),
            Self::SignedOut => AdminKey::FailSignedOut.get(lang).to_string(),
            Self::Network { detail } => Fill::new(AdminKey::FailNetwork.get(lang)).arg("detail", detail).done(),
            Self::Server { detail } => Fill::new(AdminKey::FailServer.get(lang)).arg("detail", detail).done(),
        }
    }
}

// =============================================================================
// Requests in, outcomes back
// =============================================================================

/// A request the shell should perform, taken from [`AdminState::pending`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminRequest {
    Mint { days: i64, count: i64, note: String },
    LoadBatches,
    RevokeBatch { batch_id: String },
    PaidDays { nick: String, days: i64 },
    LifetimeGrant { nick: String, kind: LifetimeKind, note: String },
    LifetimeRevoke { nick: String, kind: LifetimeKind },
    LookupUser { nick: String },
}

/// What the shell hands back when a request settles.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminOutcome {
    Minted(Result<AdminMinted, AdminFailure>),
    Batches(Result<Vec<AdminBatchRow>, AdminFailure>),
    /// Unredeemed codes revoked.
    BatchRevoked(Result<u64, AdminFailure>),
    /// `(nick, paid_until)`.
    PaidDays(Result<(String, String), AdminFailure>),
    LifetimeGranted(Result<AdminLifetimeDone, AdminFailure>),
    /// `(nick, kind, revoked)`.
    LifetimeRevoked(Result<(String, LifetimeKind, u64), AdminFailure>),
    User(Result<AdminUserCard, AdminFailure>),
}

/// An action armed and waiting for the confirm click.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminConfirm {
    RevokeBatch { batch_id: String },
    PaidDays { nick: String, days: i64 },
    LifetimeGrant { nick: String, kind: LifetimeKind, note: String },
    LifetimeRevoke { nick: String, kind: LifetimeKind },
}

impl AdminConfirm {
    /// The question above the Confirm / Cancel buttons.
    pub fn prompt(&self, lang: Language) -> String {
        match self {
            Self::RevokeBatch { batch_id } => {
                Fill::new(AdminKey::ConfirmRevokeBatch.get(lang)).arg("batch", batch_id).done()
            }
            Self::PaidDays { nick, days } => Fill::new(AdminKey::ConfirmPaidDays.get(lang))
                .arg("days", days)
                .arg("nick", nick)
                .done(),
            Self::LifetimeGrant { nick, kind, .. } => Fill::new(AdminKey::ConfirmLifetimeGrant.get(lang))
                .arg("kind", kind.name(lang))
                .arg("nick", nick)
                .done(),
            Self::LifetimeRevoke { nick, kind } => Fill::new(AdminKey::ConfirmLifetimeRevoke.get(lang))
                .arg("kind", kind.name(lang))
                .arg("nick", nick)
                .done(),
        }
    }

    /// The request this confirmation stands for.
    pub fn into_request(self) -> AdminRequest {
        match self {
            Self::RevokeBatch { batch_id } => AdminRequest::RevokeBatch { batch_id },
            Self::PaidDays { nick, days } => AdminRequest::PaidDays { nick, days },
            Self::LifetimeGrant { nick, kind, note } => AdminRequest::LifetimeGrant { nick, kind, note },
            Self::LifetimeRevoke { nick, kind } => AdminRequest::LifetimeRevoke { nick, kind },
        }
    }
}

/// What a finished request says, kept as data and worded at draw time so a
/// language switch re-words it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminSuccess {
    Minted { count: i64, days: i64 },
    BatchRevoked { revoked: u64 },
    PaidUntil { nick: String, until: String },
    LifetimeGranted { nick: String, kind: LifetimeKind },
    LifetimeExisting { nick: String, kind: LifetimeKind },
    LifetimeRevoked { nick: String, kind: LifetimeKind, revoked: u64 },
    Copied,
    Discarded,
}

/// The status line under the active section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AdminNotice {
    Ok(AdminSuccess),
    Failed(AdminFailure),
    Form(FormError),
}

impl AdminNotice {
    /// The sentence to print.
    pub fn text(&self, lang: Language) -> String {
        match self {
            Self::Form(e) => e.message(lang),
            Self::Failed(f) => f.message(lang),
            Self::Ok(ok) => match ok {
                AdminSuccess::Minted { count, days } => Fill::new(AdminKey::ResultMinted.get(lang))
                    .arg("count", count)
                    .arg("days", days)
                    .done(),
                AdminSuccess::BatchRevoked { revoked } => {
                    Fill::new(AdminKey::ResultBatchRevoked.get(lang)).arg("n", revoked).done()
                }
                AdminSuccess::PaidUntil { nick, until } => Fill::new(AdminKey::ResultPaidUntil.get(lang))
                    .arg("nick", nick)
                    .arg("until", short_date(until))
                    .done(),
                AdminSuccess::LifetimeGranted { nick, kind } => Fill::new(AdminKey::ResultLifetimeGranted.get(lang))
                    .arg("nick", nick)
                    .arg("kind", kind.name(lang))
                    .done(),
                AdminSuccess::LifetimeExisting { nick, kind } => Fill::new(AdminKey::ResultLifetimeExisting.get(lang))
                    .arg("nick", nick)
                    .arg("kind", kind.name(lang))
                    .done(),
                AdminSuccess::LifetimeRevoked { nick, kind, revoked } => {
                    Fill::new(AdminKey::ResultLifetimeRevoked.get(lang))
                        .arg("nick", nick)
                        .arg("n", revoked)
                        .arg("kind", kind.name(lang))
                        .done()
                }
                AdminSuccess::Copied => AdminKey::ResultCopied.get(lang).to_string(),
                AdminSuccess::Discarded => AdminKey::ResultDiscarded.get(lang).to_string(),
            },
        }
    }

    /// Whether the line reads as a problem (danger colour) or a result.
    pub fn is_error(&self) -> bool {
        !matches!(self, Self::Ok(_))
    }
}

// =============================================================================
// State
// =============================================================================

/// State of the Admin tab. Lives in `UserSettingsState::admin`.
#[derive(Debug, Clone, Default)]
pub struct AdminState {
    /// The shell can serve this tab's requests (the web shell sets it every
    /// frame). The native shell has no `/api/staff/*` transport yet, so the
    /// tab stays hidden there instead of showing buttons that do nothing.
    pub served: bool,
    /// Active section.
    pub section: AdminSection,
    /// Scroll of the section body.
    pub scroll: ScrollState,
    /// The freshly minted codes. Shown once; dropped by [`Self::on_close`].
    pub minted: Option<AdminMinted>,
    /// The batches table; `None` until the first fetch lands.
    pub batches: Option<Vec<AdminBatchRow>>,
    /// The User section's card.
    pub user: Option<AdminUserCard>,
    /// Status line under the active section.
    pub notice: Option<AdminNotice>,
    /// An armed action waiting for Confirm.
    pub confirm: Option<AdminConfirm>,
    /// One-shot mailbox: the request the shell should perform next.
    pub pending: Option<AdminRequest>,
    /// A request is queued or in flight; buttons draw dead meanwhile.
    pub busy: bool,
    /// The first batches load of this visit has been issued (a failed load is
    /// retried by Refresh, not in a loop).
    batches_requested: bool,
    /// The modal closed while a request was in flight; its outcome is dropped.
    discard_inflight: bool,
}

impl AdminState {
    /// Switch section. Clears the status line and any armed action — both
    /// belong to what was on screen.
    pub fn set_section(&mut self, section: AdminSection) {
        if self.section != section {
            self.section = section;
            self.notice = None;
            self.confirm = None;
            self.scroll = ScrollState::new();
        }
    }

    /// Queue `request` unless one is already queued or in flight, or the shell
    /// cannot serve it. Returns whether it was queued.
    fn issue(&mut self, request: AdminRequest) -> bool {
        if self.busy || !self.served {
            return false;
        }
        self.pending = Some(request);
        self.busy = true;
        true
    }

    /// Load the batches table when the Coupons section is on screen and has
    /// never been asked for this visit. `on_screen` is "the Admin tab is the
    /// visible tab of an open modal" — known only to the caller.
    pub fn auto_load(&mut self, on_screen: bool) {
        if on_screen && self.section == AdminSection::Coupons && !self.batches_requested && self.batches.is_none() {
            self.refresh_batches();
        }
    }

    /// Reload the batches table.
    pub fn refresh_batches(&mut self) {
        if self.issue(AdminRequest::LoadBatches) {
            self.batches_requested = true;
        }
    }

    /// Validate the mint form and queue the request.
    pub fn submit_mint(&mut self, days_raw: &str, count_raw: &str, note_raw: &str) {
        let parsed = (|| {
            let days = parse_gift_days(days_raw)?;
            let count = parse_gift_count(count_raw)?;
            let note = clean_note(note_raw, false)?;
            Ok::<_, FormError>((days, count, note))
        })();
        match parsed {
            Ok((days, count, note)) => {
                self.notice = None;
                self.confirm = None;
                self.issue(AdminRequest::Mint { days, count, note });
            }
            Err(e) => self.notice = Some(AdminNotice::Form(e)),
        }
    }

    /// Arm the revoke of one batch.
    pub fn arm_revoke_batch(&mut self, batch_id: &str) {
        // A batch with nothing left to revoke never arms (its button is off).
        let known_spent = self
            .batches
            .as_deref()
            .and_then(|rows| rows.iter().find(|r| r.batch_id == batch_id))
            .is_some_and(|r| !r.is_revocable());
        if known_spent {
            return;
        }
        self.notice = None;
        self.confirm = Some(AdminConfirm::RevokeBatch { batch_id: batch_id.to_string() });
    }

    /// Validate the paid-days form and arm it.
    pub fn arm_paid_days(&mut self, nick_raw: &str, days_raw: &str) {
        let parsed = clean_nick(nick_raw).and_then(|nick| Ok((nick, parse_paid_days(days_raw)?)));
        match parsed {
            Ok((nick, days)) => {
                self.notice = None;
                self.confirm = Some(AdminConfirm::PaidDays { nick, days });
            }
            Err(e) => self.notice = Some(AdminNotice::Form(e)),
        }
    }

    /// Validate the lifetime-grant form (nick and a required reason) and arm it.
    pub fn arm_lifetime_grant(&mut self, nick_raw: &str, kind: LifetimeKind, note_raw: &str) {
        let parsed = clean_nick(nick_raw).and_then(|nick| Ok((nick, clean_note(note_raw, true)?)));
        match parsed {
            Ok((nick, note)) => {
                self.notice = None;
                self.confirm = Some(AdminConfirm::LifetimeGrant { nick, kind, note });
            }
            Err(e) => self.notice = Some(AdminNotice::Form(e)),
        }
    }

    /// Validate the nick and arm a lifetime revoke.
    pub fn arm_lifetime_revoke(&mut self, nick_raw: &str, kind: LifetimeKind) {
        match clean_nick(nick_raw) {
            Ok(nick) => {
                self.notice = None;
                self.confirm = Some(AdminConfirm::LifetimeRevoke { nick, kind });
            }
            Err(e) => self.notice = Some(AdminNotice::Form(e)),
        }
    }

    /// The Confirm click: the armed action becomes a request. The armed action
    /// is cleared either way, so a second click cannot send it twice.
    pub fn confirm_yes(&mut self) {
        if let Some(confirm) = self.confirm.take() {
            self.issue(confirm.into_request());
        }
    }

    /// The Cancel click.
    pub fn confirm_no(&mut self) {
        self.confirm = None;
    }

    /// Validate the lookup nick and queue the request.
    pub fn submit_lookup(&mut self, nick_raw: &str) {
        match clean_nick(nick_raw) {
            Ok(nick) => {
                self.notice = None;
                self.issue(AdminRequest::LookupUser { nick });
            }
            Err(e) => self.notice = Some(AdminNotice::Form(e)),
        }
    }

    /// Drop the freshly minted codes ("Done").
    pub fn dismiss_minted(&mut self) {
        self.minted = None;
    }

    /// Every fresh code, one per line, for "Copy all".
    pub fn copy_all_text(&self) -> Option<String> {
        self.minted.as_ref().map(|m| m.codes.join("\n")).filter(|s| !s.is_empty())
    }

    /// One fresh code, for its own Copy button.
    pub fn copy_one_text(&self, index: usize) -> Option<String> {
        self.minted.as_ref().and_then(|m| m.codes.get(index)).cloned()
    }

    /// Record that a copy went to the clipboard.
    pub fn note_copied(&mut self) {
        self.notice = Some(AdminNotice::Ok(AdminSuccess::Copied));
    }

    /// The shell drains the mailbox. The request stays `busy` until
    /// [`Self::apply`] settles it.
    pub fn take_request(&mut self) -> Option<AdminRequest> {
        self.pending.take()
    }

    /// Settle a finished request.
    pub fn apply(&mut self, outcome: AdminOutcome) {
        self.busy = false;
        let refresh = matches!(outcome, AdminOutcome::Minted(Ok(_)) | AdminOutcome::BatchRevoked(Ok(_)));
        if std::mem::take(&mut self.discard_inflight) {
            // The window closed while this ran. A mint's codes must not
            // outlive the close, so they are dropped and the admin is told
            // where to look; everything else is simply not wanted any more.
            if matches!(outcome, AdminOutcome::Minted(Ok(_))) {
                self.notice = Some(AdminNotice::Ok(AdminSuccess::Discarded));
            }
            if refresh {
                self.issue(AdminRequest::LoadBatches);
            }
            return;
        }
        match outcome {
            AdminOutcome::Minted(Ok(minted)) => {
                self.notice = Some(AdminNotice::Ok(AdminSuccess::Minted { count: minted.count, days: minted.days }));
                self.minted = Some(minted);
            }
            AdminOutcome::Batches(Ok(rows)) => self.batches = Some(rows),
            AdminOutcome::BatchRevoked(Ok(revoked)) => {
                self.notice = Some(AdminNotice::Ok(AdminSuccess::BatchRevoked { revoked }));
            }
            AdminOutcome::PaidDays(Ok((nick, until))) => {
                self.notice = Some(AdminNotice::Ok(AdminSuccess::PaidUntil { nick, until }));
            }
            AdminOutcome::LifetimeGranted(Ok(done)) => {
                let AdminLifetimeDone { nick, kind, created } = done;
                self.notice = Some(AdminNotice::Ok(if created {
                    AdminSuccess::LifetimeGranted { nick, kind }
                } else {
                    AdminSuccess::LifetimeExisting { nick, kind }
                }));
            }
            AdminOutcome::LifetimeRevoked(Ok((nick, kind, revoked))) => {
                self.notice = Some(AdminNotice::Ok(AdminSuccess::LifetimeRevoked { nick, kind, revoked }));
            }
            AdminOutcome::User(Ok(card)) => {
                self.notice = None;
                self.user = Some(card);
            }
            AdminOutcome::User(Err(failure)) => {
                self.user = None;
                self.notice = Some(AdminNotice::Failed(failure));
            }
            AdminOutcome::Minted(Err(f))
            | AdminOutcome::Batches(Err(f))
            | AdminOutcome::BatchRevoked(Err(f))
            | AdminOutcome::PaidDays(Err(f))
            | AdminOutcome::LifetimeGranted(Err(f))
            | AdminOutcome::LifetimeRevoked(Err(f)) => self.notice = Some(AdminNotice::Failed(f)),
        }
        if refresh {
            self.issue(AdminRequest::LoadBatches);
        }
    }

    /// The modal closed: drop everything a later visit must not inherit —
    /// the minted codes above all, but also the account card and armed
    /// actions. A request already in flight keeps the tab busy until it
    /// settles, and its outcome is then dropped ([`Self::apply`]).
    pub fn on_close(&mut self) {
        if self.pending.take().is_some() {
            // Never handed to the shell: cancelled outright.
            self.busy = false;
        }
        let inflight = self.busy;
        *self = Self { served: self.served, busy: self.busy, discard_inflight: inflight, ..Self::default() };
    }

    /// Sign-out (or loss of the role): everything goes, including the
    /// in-flight marker, since the session the request ran under is gone.
    pub fn reset(&mut self) {
        *self = Self { served: self.served, ..Self::default() };
    }
}

// =============================================================================
// View models (what the renderer draws; tested without a render context)
// =============================================================================

/// The date part of an RFC 3339 timestamp; `—` for an empty one.
pub fn short_date(ts: &str) -> String {
    if ts.is_empty() {
        return "\u{2014}".to_string();
    }
    ts.get(..10).unwrap_or(ts).to_string()
}

/// Column headers of the batches table, in draw order.
pub fn batch_headers(lang: Language) -> [&'static str; 6] {
    [
        AdminKey::LabelNote.get(lang),
        AdminKey::LabelDays.get(lang),
        AdminKey::ColRedeemed.get(lang),
        AdminKey::ColRevoked.get(lang),
        AdminKey::ColCreated.get(lang),
        AdminKey::ColBy.get(lang),
    ]
}

/// One batches-table row, formatted for display.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BatchLine {
    pub batch_id: String,
    pub note: String,
    pub days: String,
    /// `redeemed/count`.
    pub redeemed: String,
    pub revoked: String,
    pub created: String,
    pub by: String,
    /// Whether any code of the batch is still unredeemed and unrevoked, i.e.
    /// whether Revoke has something to do.
    pub revocable: bool,
}

/// The batches table body. A batch with no note shows its id in the note
/// column so the row is never blank; a missing creator reads `—`.
pub fn batch_lines(rows: &[AdminBatchRow]) -> Vec<BatchLine> {
    rows.iter()
        .map(|r| BatchLine {
            batch_id: r.batch_id.clone(),
            note: if r.note.is_empty() { r.batch_id.clone() } else { r.note.clone() },
            days: r.days.to_string(),
            redeemed: format!("{}/{}", r.redeemed, r.count),
            revoked: r.revoked.to_string(),
            created: short_date(&r.created_at),
            revocable: r.is_revocable(),
            by: r.created_by.clone().unwrap_or_else(|| "\u{2014}".to_string()),
        })
        .collect()
}

/// A labelled value of the user card.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardField {
    pub label: &'static str,
    pub value: String,
}

fn yes_no(flag: bool, lang: Language) -> String {
    let word = if flag { AdminKey::ValueYes.get(lang) } else { AdminKey::ValueNo.get(lang) };
    word.to_string()
}

fn or_dash(value: Option<&str>) -> String {
    match value {
        Some(v) if !v.is_empty() => v.to_string(),
        _ => "\u{2014}".to_string(),
    }
}

/// The user card's top block, in draw order.
pub fn user_card_fields(card: &AdminUserCard, lang: Language) -> Vec<CardField> {
    let roles = if card.roles.is_empty() { "\u{2014}".to_string() } else { card.roles.join(", ") };
    vec![
        CardField { label: AdminKey::LabelNick.get(lang), value: card.nick.clone() },
        CardField { label: AdminKey::CardUserId.get(lang), value: card.user_id.to_string() },
        CardField {
            label: AdminKey::CardPaidUntil.get(lang),
            value: card.paid_until.as_deref().map(short_date).unwrap_or_else(|| "\u{2014}".to_string()),
        },
        CardField { label: AdminKey::CardLifetimePaid.get(lang), value: yes_no(card.lifetime_paid, lang) },
        CardField { label: AdminKey::CardLifetimeAnon.get(lang), value: yes_no(card.lifetime_anon, lang) },
        CardField { label: AdminKey::CardQuantTier.get(lang), value: or_dash(card.quant_tier.as_deref()) },
        CardField {
            label: AdminKey::CardQuantUntil.get(lang),
            value: card.quant_active_until.as_deref().map(short_date).unwrap_or_else(|| "\u{2014}".to_string()),
        },
        CardField { label: AdminKey::CardRoles.get(lang), value: roles },
    ]
}

/// One row of the user card's lifetime-grant ledger.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LedgerLine {
    pub grant: String,
    pub source: String,
    /// The ledger's reference (`admin id:note` or an invoice id).
    pub reference: String,
    pub created: String,
    /// `—` while the grant is in force.
    pub revoked: String,
}

/// The ledger rows, formatted. Grant kinds the client knows are worded in
/// `lang`; an unknown wire value is shown as sent.
pub fn ledger_lines(card: &AdminUserCard, lang: Language) -> Vec<LedgerLine> {
    card.ledger
        .iter()
        .map(|g| LedgerLine {
            grant: match LifetimeKind::from_wire(&g.grant) {
                Some(kind) => kind.name(lang).to_string(),
                None => g.grant.clone(),
            },
            source: g.source.clone(),
            reference: g.source_ref.clone(),
            created: short_date(&g.created_at),
            revoked: g.revoked_at.as_deref().map(short_date).unwrap_or_else(|| "\u{2014}".to_string()),
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const EN: Language = Language::En;

    fn served() -> AdminState {
        AdminState { served: true, ..AdminState::default() }
    }

    fn minted(codes: &[&str]) -> AdminMinted {
        AdminMinted {
            batch_id: "gb_1".to_string(),
            days: 30,
            count: codes.len() as i64,
            note: "beta".to_string(),
            codes: codes.iter().map(|c| c.to_string()).collect(),
        }
    }

    fn batch(id: &str, note: &str, by: Option<&str>) -> AdminBatchRow {
        AdminBatchRow {
            batch_id: id.to_string(),
            note: note.to_string(),
            days: 30,
            count: 3,
            redeemed: 1,
            revoked: 1,
            created_at: "2026-09-29T10:00:00+00:00".to_string(),
            created_by: by.map(str::to_string),
        }
    }

    #[test]
    fn a_batch_with_no_unredeemed_codes_is_not_revocable_and_never_arms() {
        let spent = |redeemed: i64, revoked: i64| AdminBatchRow { redeemed, revoked, ..batch("gb_9", "n", None) };
        assert!(spent(1, 1).is_revocable(), "one code left");
        assert!(!spent(3, 0).is_revocable(), "all redeemed");
        assert!(!spent(0, 3).is_revocable(), "all revoked");
        assert!(!spent(1, 2).is_revocable(), "redeemed + revoked == count");
        assert!(!batch_lines(&[spent(1, 2)])[0].revocable);

        let mut st = served();
        st.batches = Some(vec![spent(1, 2)]);
        st.arm_revoke_batch("gb_9");
        assert!(st.confirm.is_none(), "a spent batch does not arm");
    }

    fn card() -> AdminUserCard {
        AdminUserCard {
            nick: "alice".to_string(),
            user_id: 42,
            paid_until: Some("2026-11-01T00:00:00+00:00".to_string()),
            lifetime_paid: true,
            lifetime_anon: false,
            ledger: vec![
                AdminLedgerRow {
                    grant_id: 12,
                    grant: "lifetime_paid".to_string(),
                    source: "admin".to_string(),
                    source_ref: "7:early supporter".to_string(),
                    created_at: "2026-09-01T00:00:00+00:00".to_string(),
                    revoked_at: None,
                },
                AdminLedgerRow {
                    grant_id: 9,
                    grant: "lifetime_anon".to_string(),
                    source: "invoice".to_string(),
                    source_ref: "inv_9".to_string(),
                    created_at: "2026-08-01T00:00:00+00:00".to_string(),
                    revoked_at: Some("2026-08-20T00:00:00+00:00".to_string()),
                },
            ],
            quant_tier: None,
            quant_active_until: None,
            roles: vec!["moderator".to_string()],
        }
    }

    // ── mint form bounds ────────────────────────────────────────────────

    #[test]
    fn mint_days_are_bounded_client_side() {
        assert_eq!(parse_gift_days("1"), Ok(1));
        assert_eq!(parse_gift_days(" 365 "), Ok(365));
        for bad in ["0", "366", "-3", "", "abc", "1.5", "99999999999999999999"] {
            assert_eq!(
                parse_gift_days(bad),
                Err(FormError::DaysRange { min: 1, max: 365 }),
                "days {bad:?} must be refused"
            );
        }
    }

    #[test]
    fn mint_count_is_bounded_client_side() {
        assert_eq!(parse_gift_count("1"), Ok(1));
        assert_eq!(parse_gift_count("200"), Ok(200));
        for bad in ["0", "201", "-1", "", "ten"] {
            assert_eq!(
                parse_gift_count(bad),
                Err(FormError::CountRange { min: 1, max: 200 }),
                "count {bad:?} must be refused"
            );
        }
    }

    #[test]
    fn mint_note_is_optional_bounded_and_printable() {
        assert_eq!(clean_note("", false), Ok(String::new()));
        assert_eq!(clean_note("  beta wave  ", false), Ok("beta wave".to_string()));
        assert_eq!(clean_note(&"x".repeat(200), false), Ok("x".repeat(200)));
        assert_eq!(clean_note(&"x".repeat(201), false), Err(FormError::NoteTooLong { max: 200 }));
        assert_eq!(clean_note("a\u{0007}b", false), Err(FormError::NoteBadChars));
        assert_eq!(clean_note("line\nbreak", false), Err(FormError::NoteBadChars));
        // Characters, not bytes: 200 multi-byte characters still fit.
        assert_eq!(clean_note(&"\u{044F}".repeat(200), false), Ok("\u{044F}".repeat(200)));
    }

    #[test]
    fn an_out_of_bounds_mint_form_queues_nothing() {
        let mut st = served();
        st.submit_mint("366", "5", "");
        assert!(st.pending.is_none() && !st.busy);
        assert_eq!(st.notice, Some(AdminNotice::Form(FormError::DaysRange { min: 1, max: 365 })));
        st.submit_mint("30", "201", "");
        assert!(st.pending.is_none());
        assert_eq!(st.notice, Some(AdminNotice::Form(FormError::CountRange { min: 1, max: 200 })));
        st.submit_mint("30", "5", &"n".repeat(201));
        assert!(st.pending.is_none());
        assert_eq!(st.notice, Some(AdminNotice::Form(FormError::NoteTooLong { max: 200 })));
    }

    #[test]
    fn a_valid_mint_form_queues_one_request_and_goes_busy() {
        let mut st = served();
        st.submit_mint(" 30 ", "10", " beta ");
        assert_eq!(st.pending, Some(AdminRequest::Mint { days: 30, count: 10, note: "beta".to_string() }));
        assert!(st.busy);
        // A second click while busy queues nothing.
        st.pending = None;
        st.submit_mint("7", "1", "");
        assert!(st.pending.is_none());
    }

    #[test]
    fn nothing_is_queued_when_the_shell_cannot_serve() {
        let mut st = AdminState::default();
        st.submit_mint("30", "10", "");
        assert!(st.pending.is_none() && !st.busy);
    }

    // ── codes are shown once ────────────────────────────────────────────

    #[test]
    fn minted_codes_are_kept_for_display_then_dropped_on_close() {
        let mut st = served();
        st.busy = true;
        st.apply(AdminOutcome::Minted(Ok(minted(&["AAAAAAAAAA", "BBBBBBBBBB"]))));
        let shown = st.minted.as_ref().expect("codes are shown after a mint");
        assert_eq!(shown.codes.len(), 2);
        assert_eq!(st.copy_all_text().as_deref(), Some("AAAAAAAAAA\nBBBBBBBBBB"));
        assert_eq!(st.copy_one_text(1).as_deref(), Some("BBBBBBBBBB"));
        assert_eq!(st.copy_one_text(2), None);

        st.on_close();
        assert!(st.minted.is_none(), "closing must drop the codes");
        assert_eq!(st.copy_all_text(), None);
        assert!(st.served, "the shell's capability survives a close");
    }

    #[test]
    fn done_drops_the_codes_without_closing() {
        let mut st = served();
        st.apply(AdminOutcome::Minted(Ok(minted(&["AAAAAAAAAA"]))));
        st.dismiss_minted();
        assert!(st.minted.is_none());
    }

    #[test]
    fn a_mint_that_lands_after_close_is_not_kept() {
        let mut st = served();
        assert!(st.issue(AdminRequest::Mint { days: 30, count: 1, note: String::new() }));
        // The shell took the request and it is in flight when the window closes.
        assert!(st.take_request().is_some());
        st.on_close();
        assert!(st.busy, "the request is still in flight");
        st.apply(AdminOutcome::Minted(Ok(minted(&["ZZZZZZZZZZ"]))));
        assert!(st.minted.is_none(), "codes that outlived the window must not be stored");
        assert_eq!(st.notice, Some(AdminNotice::Ok(AdminSuccess::Discarded)));
        // The batch list is refreshed so the new batch is visible.
        assert_eq!(st.pending, Some(AdminRequest::LoadBatches));
    }

    #[test]
    fn a_request_never_handed_to_the_shell_is_cancelled_by_close() {
        let mut st = served();
        st.submit_mint("30", "1", "");
        assert!(st.busy && st.pending.is_some());
        st.on_close();
        assert!(!st.busy && st.pending.is_none());
    }

    #[test]
    fn minted_debug_never_prints_a_code() {
        let printed = format!("{:?}", minted(&["SECRETCODE"]));
        assert!(!printed.contains("SECRETCODE"), "{printed}");
        let mut st = served();
        st.minted = Some(minted(&["SECRETCODE"]));
        assert!(!format!("{st:?}").contains("SECRETCODE"));
    }

    #[test]
    fn a_successful_mint_and_a_revoke_refresh_the_table() {
        let mut st = served();
        st.busy = true;
        st.apply(AdminOutcome::Minted(Ok(minted(&["A"]))));
        assert_eq!(st.pending, Some(AdminRequest::LoadBatches));
        assert!(st.busy);
        st.pending = None;
        st.busy = true;
        st.apply(AdminOutcome::BatchRevoked(Ok(2)));
        assert_eq!(st.pending, Some(AdminRequest::LoadBatches));
        assert_eq!(st.notice, Some(AdminNotice::Ok(AdminSuccess::BatchRevoked { revoked: 2 })));
    }

    // ── batches table ───────────────────────────────────────────────────

    #[test]
    fn the_batches_table_renders_one_line_per_batch() {
        let rows = vec![batch("gb_2", "beta wave", Some("alice")), batch("gb_1", "", None)];
        let lines = batch_lines(&rows);
        assert_eq!(lines.len(), 2);
        assert_eq!(lines[0].note, "beta wave");
        assert_eq!(lines[0].days, "30");
        assert_eq!(lines[0].redeemed, "1/3");
        assert_eq!(lines[0].revoked, "1");
        assert!(lines[0].revocable);
        assert_eq!(lines[0].created, "2026-09-29");
        assert_eq!(lines[0].by, "alice");
        // No note: the id stands in. No creator: a dash.
        assert_eq!(lines[1].note, "gb_1");
        assert_eq!(lines[1].by, "\u{2014}");
    }

    #[test]
    fn the_batches_table_has_localized_headers() {
        let en = batch_headers(EN);
        assert_eq!(en, ["Note", "Days", "Redeemed", "Revoked", "Created", "By"]);
        let ru = batch_headers(Language::Ru);
        assert_ne!(ru, en);
        assert!(ru.iter().all(|h| !h.is_empty()));
    }

    #[test]
    fn the_first_load_is_issued_once_per_visit() {
        let mut st = served();
        st.auto_load(false);
        assert!(st.pending.is_none(), "nothing loads while the tab is not on screen");
        st.auto_load(true);
        assert_eq!(st.take_request(), Some(AdminRequest::LoadBatches));
        st.apply(AdminOutcome::Batches(Err(AdminFailure::Network { detail: "offline".to_string() })));
        st.auto_load(true);
        assert!(st.pending.is_none(), "a failed load is retried by Refresh, not in a loop");
        st.refresh_batches();
        assert_eq!(st.pending, Some(AdminRequest::LoadBatches));
    }

    #[test]
    fn a_batches_answer_fills_the_table() {
        let mut st = served();
        st.busy = true;
        st.apply(AdminOutcome::Batches(Ok(vec![batch("gb_1", "x", None)])));
        assert_eq!(st.batches.as_ref().map(Vec::len), Some(1));
        assert!(!st.busy);
    }

    // ── confirmation ────────────────────────────────────────────────────

    #[test]
    fn revoke_asks_for_confirmation_before_anything_is_sent() {
        let mut st = served();
        st.arm_revoke_batch("gb_2");
        assert_eq!(st.confirm, Some(AdminConfirm::RevokeBatch { batch_id: "gb_2".to_string() }));
        assert!(st.pending.is_none() && !st.busy, "arming sends nothing");
        let prompt = st.confirm.as_ref().expect("armed").prompt(EN);
        assert!(prompt.contains("gb_2"), "{prompt}");
        st.confirm_yes();
        assert_eq!(st.pending, Some(AdminRequest::RevokeBatch { batch_id: "gb_2".to_string() }));
        assert!(st.confirm.is_none());
    }

    #[test]
    fn cancel_disarms_and_sends_nothing() {
        let mut st = served();
        st.arm_revoke_batch("gb_2");
        st.confirm_no();
        assert!(st.confirm.is_none());
        st.confirm_yes();
        assert!(st.pending.is_none());
    }

    #[test]
    fn every_grant_action_asks_for_confirmation() {
        let mut st = served();
        st.arm_paid_days(" alice ", "30");
        assert_eq!(st.confirm, Some(AdminConfirm::PaidDays { nick: "alice".to_string(), days: 30 }));
        assert!(st.pending.is_none());
        st.arm_lifetime_grant("alice", LifetimeKind::Paid, " early supporter ");
        assert_eq!(
            st.confirm,
            Some(AdminConfirm::LifetimeGrant {
                nick: "alice".to_string(),
                kind: LifetimeKind::Paid,
                note: "early supporter".to_string()
            })
        );
        st.arm_lifetime_revoke("alice", LifetimeKind::Anon);
        assert_eq!(
            st.confirm,
            Some(AdminConfirm::LifetimeRevoke { nick: "alice".to_string(), kind: LifetimeKind::Anon })
        );
        assert!(st.pending.is_none(), "arming never sends");
        st.confirm_yes();
        assert_eq!(
            st.pending,
            Some(AdminRequest::LifetimeRevoke { nick: "alice".to_string(), kind: LifetimeKind::Anon })
        );
    }

    #[test]
    fn grant_forms_refuse_bad_input_without_arming() {
        let mut st = served();
        st.arm_paid_days("", "30");
        assert_eq!(st.notice, Some(AdminNotice::Form(FormError::NickRequired)));
        assert!(st.confirm.is_none());
        st.arm_paid_days("alice", "3651");
        assert_eq!(st.notice, Some(AdminNotice::Form(FormError::DaysRange { min: 1, max: 3650 })));
        st.arm_lifetime_grant("alice", LifetimeKind::Anon, "   ");
        assert_eq!(st.notice, Some(AdminNotice::Form(FormError::NoteRequired { max: 200 })));
        st.arm_lifetime_grant(&"n".repeat(65), LifetimeKind::Anon, "why");
        assert_eq!(st.notice, Some(AdminNotice::Form(FormError::NickTooLong { max: 64 })));
        assert!(st.confirm.is_none());
    }

    #[test]
    fn switching_section_drops_the_armed_action_and_the_status_line() {
        let mut st = served();
        st.arm_paid_days("alice", "5");
        st.set_section(AdminSection::User);
        assert!(st.confirm.is_none() && st.notice.is_none());
    }

    #[test]
    fn confirm_prompts_name_the_target() {
        let paid = AdminConfirm::PaidDays { nick: "alice".to_string(), days: 30 }.prompt(EN);
        assert!(paid.contains("alice") && paid.contains("30"), "{paid}");
        let life = AdminConfirm::LifetimeGrant {
            nick: "bob".to_string(),
            kind: LifetimeKind::Anon,
            note: "x".to_string(),
        }
        .prompt(EN);
        assert!(life.contains("bob") && life.contains("lifetime anonymous"), "{life}");
    }

    // ── results ─────────────────────────────────────────────────────────

    #[test]
    fn server_results_are_shown_as_status_lines() {
        let mut st = served();
        st.busy = true;
        st.apply(AdminOutcome::PaidDays(Ok(("alice".to_string(), "2026-11-01T00:00:00+00:00".to_string()))));
        let line = st.notice.as_ref().expect("notice").text(EN);
        assert!(line.contains("alice") && line.contains("2026-11-01"), "{line}");
        assert!(!st.notice.as_ref().expect("notice").is_error());

        st.apply(AdminOutcome::LifetimeGranted(Ok(AdminLifetimeDone {
            nick: "alice".to_string(),
            kind: LifetimeKind::Paid,
            created: true,
        })));
        assert!(st.notice.as_ref().expect("notice").text(EN).contains("granted"));
        st.apply(AdminOutcome::LifetimeGranted(Ok(AdminLifetimeDone {
            nick: "alice".to_string(),
            kind: LifetimeKind::Paid,
            created: false,
        })));
        assert!(st.notice.as_ref().expect("notice").text(EN).contains("already holds"));
        st.apply(AdminOutcome::LifetimeRevoked(Ok(("alice".to_string(), LifetimeKind::Paid, 1))));
        assert!(st.notice.as_ref().expect("notice").text(EN).contains('1'));
    }

    // ── user card ───────────────────────────────────────────────────────

    #[test]
    fn the_user_card_renders_a_sample() {
        let fields = user_card_fields(&card(), EN);
        let get = |label: &str| fields.iter().find(|f| f.label == label).map(|f| f.value.clone());
        assert_eq!(get("Nick").as_deref(), Some("alice"));
        assert_eq!(get("User id").as_deref(), Some("42"));
        assert_eq!(get("Paid until").as_deref(), Some("2026-11-01"));
        assert_eq!(get("Lifetime paid").as_deref(), Some("yes"));
        assert_eq!(get("Lifetime anonymous").as_deref(), Some("no"));
        assert_eq!(get("Quant tier").as_deref(), Some("\u{2014}"));
        assert_eq!(get("Roles").as_deref(), Some("moderator"));

        let ledger = ledger_lines(&card(), EN);
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger[0].grant, "lifetime paid");
        assert_eq!(ledger[0].source, "admin");
        assert_eq!(ledger[0].reference, "7:early supporter");
        assert_eq!(ledger[0].created, "2026-09-01");
        assert_eq!(ledger[0].revoked, "\u{2014}");
        assert_eq!(ledger[1].revoked, "2026-08-20");
    }

    #[test]
    fn a_bare_account_card_renders_dashes() {
        let bare = AdminUserCard {
            nick: "bob".to_string(),
            user_id: 7,
            paid_until: None,
            lifetime_paid: false,
            lifetime_anon: false,
            ledger: Vec::new(),
            quant_tier: Some("pro".to_string()),
            quant_active_until: Some("2026-12-31T00:00:00+00:00".to_string()),
            roles: Vec::new(),
        };
        let fields = user_card_fields(&bare, EN);
        let get = |label: &str| fields.iter().find(|f| f.label == label).map(|f| f.value.clone());
        assert_eq!(get("Paid until").as_deref(), Some("\u{2014}"));
        assert_eq!(get("Roles").as_deref(), Some("\u{2014}"));
        assert_eq!(get("Quant tier").as_deref(), Some("pro"));
        assert_eq!(get("Quant active until").as_deref(), Some("2026-12-31"));
        assert!(ledger_lines(&bare, EN).is_empty());
    }

    #[test]
    fn a_user_lookup_fills_and_a_miss_clears_the_card() {
        let mut st = served();
        st.busy = true;
        st.apply(AdminOutcome::User(Ok(card())));
        assert_eq!(st.user.as_ref().map(|c| c.nick.as_str()), Some("alice"));
        assert!(st.notice.is_none());
        st.busy = true;
        st.apply(AdminOutcome::User(Err(AdminFailure::NotFound)));
        assert!(st.user.is_none(), "a stale card must not stay under a 404");
        assert_eq!(st.notice, Some(AdminNotice::Failed(AdminFailure::NotFound)));
    }

    #[test]
    fn a_lookup_needs_a_nick() {
        let mut st = served();
        st.submit_lookup("  ");
        assert_eq!(st.notice, Some(AdminNotice::Form(FormError::NickRequired)));
        assert!(st.pending.is_none());
        st.submit_lookup(" alice ");
        assert_eq!(st.pending, Some(AdminRequest::LookupUser { nick: "alice".to_string() }));
    }

    // ── error codes to messages ─────────────────────────────────────────

    #[test]
    fn error_codes_map_to_distinct_messages() {
        let forbidden = AdminFailure::Forbidden.message(EN);
        let not_found = AdminFailure::NotFound.message(EN);
        let limited = AdminFailure::RateLimited { retry_after_secs: Some(60) }.message(EN);
        let limited_soon = AdminFailure::RateLimited { retry_after_secs: None }.message(EN);
        let bad = AdminFailure::BadRequest { detail: "days must be between 1 and 365".to_string() }.message(EN);
        assert!(forbidden.contains("admin role"), "403: {forbidden}");
        assert!(not_found.contains("no such user or batch"), "404: {not_found}");
        assert!(limited.contains("60s"), "429 with Retry-After: {limited}");
        assert!(limited_soon.contains("shortly") && !limited_soon.contains('{'), "429 bare: {limited_soon}");
        assert!(bad.contains("days must be between 1 and 365"), "400 keeps the server sentence: {bad}");
        let all = [&forbidden, &not_found, &limited, &limited_soon, &bad];
        for (i, a) in all.iter().enumerate() {
            for b in all.iter().skip(i + 1) {
                assert_ne!(a, b);
            }
        }
    }

    #[test]
    fn error_messages_are_localized_and_leave_no_placeholder() {
        let failures = [
            AdminFailure::Forbidden,
            AdminFailure::NotFound,
            AdminFailure::RateLimited { retry_after_secs: Some(12) },
            AdminFailure::RateLimited { retry_after_secs: None },
            AdminFailure::BadRequest { detail: "d".to_string() },
            AdminFailure::SignedOut,
            AdminFailure::Network { detail: "d".to_string() },
            AdminFailure::Server { detail: "d".to_string() },
        ];
        for failure in &failures {
            let en = failure.message(EN);
            let ru = failure.message(Language::Ru);
            assert_ne!(en, ru, "{failure:?} has no Russian wording");
            for &lang in Language::all() {
                let text = failure.message(lang);
                assert!(!text.is_empty() && !text.contains('{') && !text.contains('}'), "{failure:?}/{lang:?}: {text}");
            }
        }
    }

    #[test]
    fn a_failed_request_lands_in_the_status_line() {
        let mut st = served();
        st.busy = true;
        st.apply(AdminOutcome::Minted(Err(AdminFailure::Forbidden)));
        assert_eq!(st.notice, Some(AdminNotice::Failed(AdminFailure::Forbidden)));
        assert!(st.notice.as_ref().expect("notice").is_error());
        assert!(!st.busy && st.minted.is_none());
    }

    // ── helpers ─────────────────────────────────────────────────────────

    #[test]
    fn fill_replaces_in_one_pass() {
        assert_eq!(fill("a {x} b {y}", &[("x", "1"), ("y", "2")]), "a 1 b 2");
        // A value that looks like a placeholder is not expanded again.
        assert_eq!(fill("{a}", &[("a", "{b}"), ("b", "no")]), "{b}");
        assert_eq!(fill("keep {unknown}", &[("x", "1")]), "keep {unknown}");
        assert_eq!(fill("open { brace", &[]), "open { brace");
        assert_eq!(fill("\u{043F}\u{0440} {x} \u{2014}", &[("x", "\u{4E2D}")]), "\u{043F}\u{0440} \u{4E2D} \u{2014}");
    }

    #[test]
    fn every_translation_keeps_the_english_placeholder_set() {
        // The table generator already pins this; the same check here guards a
        // hand edit of one column.
        fn placeholders(s: &str) -> Vec<String> {
            let mut out = Vec::new();
            let mut rest = s;
            while let Some(open) = rest.find('{') {
                let tail = &rest[open + 1..];
                let Some(close) = tail.find('}') else { break };
                out.push(tail[..close].to_string());
                rest = &tail[close + 1..];
            }
            out.sort();
            out
        }
        let keys = [
            AdminKey::PlaceholderNote,
            AdminKey::ConfirmRevokeBatch,
            AdminKey::ConfirmPaidDays,
            AdminKey::ConfirmLifetimeGrant,
            AdminKey::ConfirmLifetimeRevoke,
            AdminKey::ResultMinted,
            AdminKey::ResultBatchRevoked,
            AdminKey::ResultPaidUntil,
            AdminKey::ResultLifetimeGranted,
            AdminKey::ResultLifetimeExisting,
            AdminKey::ResultLifetimeRevoked,
            AdminKey::ErrDaysRange,
            AdminKey::ErrCountRange,
            AdminKey::ErrNoteTooLong,
            AdminKey::ErrNoteRequired,
            AdminKey::ErrNickTooLong,
            AdminKey::FailRateLimited,
            AdminKey::FailBadRequest,
            AdminKey::FailNetwork,
            AdminKey::FailServer,
        ];
        for key in keys {
            let want = placeholders(key.get(EN));
            assert!(!want.is_empty(), "{key:?} carries no placeholder");
            for &lang in Language::all() {
                assert_eq!(placeholders(key.get(lang)), want, "{key:?}/{lang:?}");
            }
        }
    }

    // ── hit ids ─────────────────────────────────────────────────────────

    #[test]
    fn hit_ids_round_trip() {
        let actions = [
            AdminAction::Section(AdminSection::Coupons),
            AdminAction::Section(AdminSection::Grants),
            AdminAction::Section(AdminSection::User),
            AdminAction::Field(AdminField::MintDays),
            AdminAction::Field(AdminField::UserNick),
            AdminAction::Mint,
            AdminAction::CopyAll,
            AdminAction::CopyOne(3),
            AdminAction::DismissCodes,
            AdminAction::RefreshBatches,
            AdminAction::RevokeBatch("gb_abc123".to_string()),
            AdminAction::ConfirmYes,
            AdminAction::ConfirmNo,
            AdminAction::PaidDays,
            AdminAction::LifetimeGrant(LifetimeKind::Paid),
            AdminAction::LifetimeGrant(LifetimeKind::Anon),
            AdminAction::LifetimeRevoke(LifetimeKind::Paid),
            AdminAction::LifetimeRevoke(LifetimeKind::Anon),
            AdminAction::Lookup,
        ];
        for action in actions {
            let id = action.hit_id();
            assert!(id.starts_with("user_settings:admin:"), "{id}");
            assert_eq!(AdminAction::parse(&id), Some(action.clone()), "{id}");
            let bare = id.trim_start_matches("user_settings:");
            assert_eq!(AdminAction::parse(bare), Some(action), "{bare}");
        }
        assert_eq!(AdminAction::parse("user_settings:shareadm:pin:x"), None);
        assert_eq!(AdminAction::parse("user_settings:admin:nonsense"), None);
        assert_eq!(AdminAction::parse("user_settings:admin:revoke:"), None);
    }

    #[test]
    fn field_ids_round_trip_and_stay_distinct() {
        for f in AdminField::ALL {
            assert_eq!(AdminField::from_id(f.id()), Some(f));
        }
        let mut ids: Vec<&str> = AdminField::ALL.iter().map(|f| f.id()).collect();
        ids.sort_unstable();
        ids.dedup();
        assert_eq!(ids.len(), AdminField::ALL.len());
        assert_eq!(AdminField::from_id("friends_nick"), None);
    }
}
