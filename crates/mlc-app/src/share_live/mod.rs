//! The live socket of a share stream, as a state machine without I/O.
//!
//! `ChartApp` decides everything about the socket: when to fetch a ticket,
//! when to open the socket, what to send, how long to wait after a failure,
//! and what the camcorder button shows. The shell is only the hands: it
//! executes the [`LiveCommand`]s [`ChartApp::live_link_step`] returns and
//! reports back what happened ([`ChartApp::live_link_ticket`],
//! [`ChartApp::apply_edge_frame`], [`ChartApp::live_link_closed`]).
//!
//! Wire contract (server `share_hub`):
//! - `GET /api/share/{token}/stream-open` answers a single-use ticket, so
//!   every (re)connect fetches a new one.
//! - The socket is `/api/share-live/{token}?ticket=<ticket>`. Its first
//!   frame is `{"t":"welcome","publish":bool}`. Only a `publish` socket may
//!   send; a viewer that sends anything is closed with 1008.
//! - Close codes: 1008 policy or rate limit, 1009 frame too big, 1013 queue
//!   overflow or full room, 4003 rights changed (reconnect and read the new
//!   `publish`), 1001 server shutdown or idle drop.

use mlc_core::state::toolbar_state::ShareStreamButtonState;

use crate::ChartApp;

/// Wait before the first reconnect attempt after a failure.
pub const RECONNECT_FIRST_MS: f64 = 1_000.0;
/// The wait doubles per consecutive failure up to this ceiling.
pub const RECONNECT_MAX_MS: f64 = 30_000.0;
/// Minimum wait after a policy close (1008, 1009): our own frames caused it,
/// so retrying at once would repeat it.
pub const POLICY_WAIT_MS: f64 = 5_000.0;
/// Consecutive failures before the button shows the error state. The first
/// one is routine: a fresh Live click can reach stream-open before its PATCH.
pub const ERROR_AFTER_FAILURES: u32 = 2;
/// A ticket fetch that has not answered by then counts as a failure.
pub const TICKET_TIMEOUT_MS: f64 = 20_000.0;
/// A socket that has not sent its welcome by then counts as a failure.
pub const WELCOME_TIMEOUT_MS: f64 = 15_000.0;

/// Close code: a viewer sent a frame, or the rate limit was exceeded.
pub const CLOSE_POLICY: u16 = 1008;
/// Close code: a frame over 64 KiB.
pub const CLOSE_TOO_BIG: u16 = 1009;
/// Close code: the caller's rights changed; reconnect with a new ticket.
pub const CLOSE_RIGHTS_CHANGED: u16 = 4003;

/// Frames a publisher sends before it leaves, so viewers release the
/// viewport and the crosshair and see the stream end.
const STOP_FRAMES: [&str; 3] = [
    "{\"t\":\"c\",\"off\":true}",
    "{\"t\":\"v\",\"off\":true}",
    "{\"t\":\"end\"}",
];

/// Wait before reconnect attempt number `failures` (1 = first failure).
/// 1 s, 2 s, 4 s, ... capped at 30 s.
pub fn reconnect_delay_ms(failures: u32) -> f64 {
    let doublings = failures.saturating_sub(1).min(16);
    (RECONNECT_FIRST_MS * f64::from(1u32 << doublings)).min(RECONNECT_MAX_MS)
}

/// One thing the shell has to do for the live socket.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LiveCommand {
    /// `GET /api/share/{token}/stream-open`; answer with
    /// [`ChartApp::live_link_ticket`].
    FetchTicket { token: String },
    /// Open `/api/share-live/{token}?ticket=<ticket>` on the page's origin.
    /// Report the outcome through `apply_edge_frame` and
    /// [`ChartApp::live_link_closed`].
    Connect { token: String, ticket: String },
    /// Send these text frames on the open socket, in order.
    Send(Vec<String>),
    /// Close the socket and drop everything that belongs to it.
    Close,
}

#[derive(Clone, Debug, PartialEq)]
enum Phase {
    /// Nothing in flight.
    Idle,
    /// The ticket fetch was issued.
    AwaitTicket { since_ms: f64 },
    /// The ticket is here; the next step issues the connect.
    TicketReady(String),
    /// The socket was requested; waiting for its welcome frame.
    Connecting { since_ms: f64 },
    /// The welcome arrived. `publish` is the server's word.
    Open { publish: bool },
    /// The last attempt failed; the next one is not due before `until_ms`.
    Backoff { until_ms: f64 },
}

/// Link state for the live socket of the active tab.
#[derive(Clone, Debug)]
pub struct LiveLink {
    /// Share token the link is for. `None` while no tab wants the socket.
    token: Option<String>,
    phase: Phase,
    /// Consecutive failed attempts since the last welcome.
    failures: u32,
    /// A 4003 was already answered with an immediate reconnect and no
    /// welcome has arrived since. A second one waits like any failure.
    rights_retry: bool,
    /// The policy close of this failure streak was already logged.
    policy_logged: bool,
    /// Last `now_ms` the shell stepped the link with.
    clock_ms: f64,
}

impl Default for LiveLink {
    fn default() -> Self {
        Self {
            token: None,
            phase: Phase::Idle,
            failures: 0,
            rights_retry: false,
            policy_logged: false,
            clock_ms: 0.0,
        }
    }
}

impl LiveLink {
    /// Last time the shell stepped the link, on the shell's clock.
    pub fn clock_ms(&self) -> f64 {
        self.clock_ms
    }

    /// The welcome said this socket may send.
    pub fn is_publishing(&self) -> bool {
        matches!(self.phase, Phase::Open { publish: true })
    }

    /// The welcome arrived and the socket is up.
    pub fn is_open(&self) -> bool {
        matches!(self.phase, Phase::Open { .. })
    }

    /// Consecutive failed attempts since the last welcome.
    pub fn failures(&self) -> u32 {
        self.failures
    }

    fn has_socket(&self) -> bool {
        matches!(self.phase, Phase::Connecting { .. } | Phase::Open { .. })
    }

    fn fail(&mut self, now_ms: f64, at_least_ms: f64) {
        self.failures = self.failures.saturating_add(1);
        let wait = reconnect_delay_ms(self.failures).max(at_least_ms);
        self.phase = Phase::Backoff { until_ms: now_ms + wait };
    }

    /// Drop the link to Idle. A publisher says goodbye first.
    fn teardown(&mut self, cmds: &mut Vec<LiveCommand>) {
        if self.is_publishing() {
            cmds.push(LiveCommand::Send(STOP_FRAMES.iter().map(|f| (*f).to_string()).collect()));
        }
        if self.has_socket() {
            cmds.push(LiveCommand::Close);
        }
        self.phase = Phase::Idle;
        self.failures = 0;
        self.rights_retry = false;
        self.policy_logged = false;
    }
}

impl ChartApp {
    /// Whether the open socket may send. Only the server's welcome sets it.
    pub fn live_link_publishing(&self) -> bool {
        self.live_link.is_publishing()
    }

    /// One frame of the socket state machine. Call it every frame with the
    /// shell's clock; execute the returned commands in order.
    pub fn live_link_step(&mut self, now_ms: f64) -> Vec<LiveCommand> {
        self.live_link.clock_ms = now_ms;
        let want = self.edge_live_token();
        let mut cmds = Vec::new();
        if self.live_link.token != want {
            // A viewer that loses its socket must not stay pinned to the
            // author's viewport and cursor: nothing would ever release it.
            let watching = self.live_link.has_socket() && !self.live_link.is_publishing();
            self.live_link.teardown(&mut cmds);
            if watching {
                self.release_remote_follow();
            }
            self.live_link.token = want;
        }
        if let Some(token) = self.live_link.token.clone() {
            self.advance_live_link(&token, now_ms, &mut cmds);
        }
        self.panel_app.toolbar_state.share_stream = self.share_stream_button_state();
        cmds
    }

    fn advance_live_link(&mut self, token: &str, now_ms: f64, cmds: &mut Vec<LiveCommand>) {
        match self.live_link.phase.clone() {
            Phase::Idle => {
                self.live_link.phase = Phase::AwaitTicket { since_ms: now_ms };
                cmds.push(LiveCommand::FetchTicket { token: token.to_string() });
            }
            Phase::AwaitTicket { since_ms } if now_ms - since_ms > TICKET_TIMEOUT_MS => {
                self.live_link.fail(now_ms, 0.0);
            }
            Phase::TicketReady(ticket) => {
                self.live_link.phase = Phase::Connecting { since_ms: now_ms };
                cmds.push(LiveCommand::Connect { token: token.to_string(), ticket });
            }
            Phase::Connecting { since_ms } if now_ms - since_ms > WELCOME_TIMEOUT_MS => {
                cmds.push(LiveCommand::Close);
                self.live_link.fail(now_ms, 0.0);
            }
            Phase::Backoff { until_ms } if now_ms >= until_ms => {
                self.live_link.phase = Phase::AwaitTicket { since_ms: now_ms };
                cmds.push(LiveCommand::FetchTicket { token: token.to_string() });
            }
            Phase::Open { publish: true } => {
                if let Some((_, frames)) = self.take_edge_frames() {
                    cmds.push(LiveCommand::Send(frames));
                }
            }
            _ => {}
        }
    }

    /// The stream-open fetch for `token` came back. A ticket is single use,
    /// so every reconnect goes through here again.
    pub fn live_link_ticket(&mut self, token: &str, result: Result<String, String>, now_ms: f64) {
        let link = &mut self.live_link;
        if link.token.as_deref() != Some(token) || !matches!(link.phase, Phase::AwaitTicket { .. }) {
            return;
        }
        match result {
            Ok(ticket) if !ticket.is_empty() => link.phase = Phase::TicketReady(ticket),
            Ok(_) => link.fail(now_ms, 0.0),
            Err(message) => {
                if link.failures == 0 {
                    log::warn!("[Share] stream-open failed: {message}");
                }
                link.fail(now_ms, 0.0);
            }
        }
    }

    /// The socket closed, or never opened, with this close code (1006 when
    /// the browser reports no code).
    pub fn live_link_closed(&mut self, code: u16, now_ms: f64) {
        if !self.live_link.has_socket() {
            return;
        }
        let watching = !self.live_link.is_publishing();
        let link = &mut self.live_link;
        match code {
            CLOSE_RIGHTS_CHANGED if !link.rights_retry => {
                link.rights_retry = true;
                link.phase = Phase::Idle;
            }
            CLOSE_POLICY | CLOSE_TOO_BIG => {
                if !link.policy_logged {
                    log::warn!("[Share] live socket closed by the server, code {code}");
                    link.policy_logged = true;
                }
                link.fail(now_ms, POLICY_WAIT_MS);
            }
            _ => link.fail(now_ms, 0.0),
        }
        if watching {
            self.release_remote_follow();
        }
    }

    /// The server's first frame. It settles whether this socket publishes.
    pub(crate) fn live_link_welcome(&mut self, publish: bool) {
        if !self.live_link.has_socket() {
            return;
        }
        self.live_link.phase = Phase::Open { publish };
        self.live_link.failures = 0;
        self.live_link.rights_retry = false;
        self.live_link.policy_logged = false;
        if publish {
            if let Some(follow) = self.share_follow_target() {
                if let Some(stream) = self.share_streams.get_mut(&follow.preset_id) {
                    stream.forget_sent();
                }
            }
        }
    }

    /// What the camcorder button shows for the active tab.
    pub fn share_stream_button_state(&self) -> ShareStreamButtonState {
        let Some(want) = self.edge_live_token() else {
            return ShareStreamButtonState::Off;
        };
        let link = &self.live_link;
        if link.token.as_deref() != Some(want.as_str()) {
            return ShareStreamButtonState::Connecting;
        }
        match link.phase {
            Phase::Open { publish: true } => ShareStreamButtonState::Live,
            Phase::Open { publish: false } => ShareStreamButtonState::Viewing,
            _ if link.failures >= ERROR_AFTER_FAILURES => ShareStreamButtonState::Error,
            _ => ShareStreamButtonState::Connecting,
        }
    }

    /// Tooltip of the camcorder button for the active tab.
    pub fn share_stream_tooltip(&self) -> &'static str {
        use mlc_core::i18n::{t_share_viewer, t_toolbar, ShareViewerKey, ToolbarTooltipKey};
        match self.share_stream_button_state() {
            ShareStreamButtonState::Off => t_toolbar(ToolbarTooltipKey::ShareStream),
            ShareStreamButtonState::Connecting => t_share_viewer(ShareViewerKey::StreamStateConnecting),
            ShareStreamButtonState::Live => t_share_viewer(ShareViewerKey::StreamStateLive),
            ShareStreamButtonState::Viewing => t_share_viewer(ShareViewerKey::StreamStateViewing),
            ShareStreamButtonState::Error => t_share_viewer(ShareViewerKey::StreamStateError),
        }
    }
}

#[cfg(test)]
mod audit_tests;
#[cfg(test)]
mod tests;
