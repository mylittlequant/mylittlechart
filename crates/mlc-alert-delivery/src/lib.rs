//! Alert notification delivery for zengeld-terminal.
//!
//! Dispatches alert events through multiple transports concurrently:
//! - **Toast**: in-app popup (rendered by the chart UI layer)
//! - **Telegram**: Bot API text/photo messages
//! - **Webhook**: generic HTTP POST
//!
//! # Preferences vs. secrets
//!
//! Settings for this crate are split along the vault classification rule
//! (`docs/mlc/plans/identity-vault-and-shells-2026-08-31.md` §6): a field
//! belongs in the vault if disclosing it lets someone act as the user, or if
//! it concerns third parties who have no reason to reach the server —
//! everything else is plaintext profile content.
//!
//! - [`NotificationSettings`] — PREFERENCES: enabled flags, `toast_enabled`,
//!   `sound_enabled`, `send_screenshots`. Plaintext, synced, always readable
//!   without unlocking the vault.
//! - [`NotificationSecrets`] — SECRETS: the Telegram bot token (lets anyone
//!   send as the bot), the subscriber chat-id list (third parties' data),
//!   and the webhook URL (may embed an auth token). Vault tier only.
//!
//! [`AlertDelivery`] takes both halves at construction and on every update —
//! the caller recombines them explicitly at the call site, which is also the
//! one place that has to decide what to do when the vault is locked (pass an
//! empty/default [`NotificationSecrets`]; delivery already no-ops on an empty
//! bot token or URL).
//!
//! # Usage
//!
//! ```rust,no_run
//! use mlc_alert_delivery::{AlertDelivery, DeliveryEvent, NotificationSecrets, NotificationSettings};
//!
//! #[tokio::main]
//! async fn main() {
//!     let prefs = NotificationSettings::default();
//!     let secrets = NotificationSecrets::default(); // vault locked — no secrets yet
//!     let (delivery, mut toasts) = AlertDelivery::new(prefs, secrets);
//!
//!     delivery.deliver(DeliveryEvent {
//!         alert_name: "Price Cross".into(),
//!         symbol: "BTCUSDT".into(),
//!         message: "Price crossed above 50000".into(),
//!         price: 50001.5,
//!         timestamp: 1_700_000_000_000,
//!         screenshot: None,
//!     });
//!
//!     if let Some(toast) = toasts.recv().await {
//!         println!("Toast: {}", toast.title);
//!     }
//! }
//! ```

use tokio::sync::mpsc;

pub mod telegram;
pub mod toast;
pub mod webhook;

// ── Preferences (plaintext profile tier) ────────────────────────────────────

/// Notification delivery PREFERENCES, typically persisted in the plaintext
/// `UserProfile`.
///
/// Every field here is safe to read without unlocking the vault: disclosing
/// an `enabled` flag or `send_screenshots`/`toast_enabled`/`sound_enabled`
/// does not let anyone act as the user or reach a third party.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotificationSettings {
    /// Show a toast popup inside the app.
    #[serde(default = "default_true")]
    pub toast_enabled: bool,

    /// Play a sound on alert (handled by the UI layer; delivery crate only
    /// propagates the flag).
    #[serde(default)]
    pub sound_enabled: bool,

    /// Telegram bot delivery preferences.
    #[serde(default)]
    pub telegram: TelegramSettings,

    /// Generic HTTP webhook delivery preferences.
    #[serde(default)]
    pub webhook: WebhookSettings,
}

impl Default for NotificationSettings {
    fn default() -> Self {
        Self {
            toast_enabled: true,
            sound_enabled: false,
            telegram: TelegramSettings::default(),
            webhook: WebhookSettings::default(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// Telegram delivery PREFERENCES — no secret in here.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TelegramSettings {
    /// Whether Telegram delivery is active.
    #[serde(default)]
    pub enabled: bool,

    /// Whether to attach a chart screenshot to alert messages.
    #[serde(default)]
    pub send_screenshots: bool,
}

/// Generic HTTP webhook delivery PREFERENCES — no secret in here (the URL,
/// which may embed an auth token, lives in [`WebhookSecrets`]).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WebhookSettings {
    /// Whether webhook delivery is active.
    #[serde(default)]
    pub enabled: bool,
}

// ── Secrets (vault tier) ─────────────────────────────────────────────────────

/// Notification delivery SECRETS — vault tier, encrypted at rest, absent
/// while the vault is locked.
///
/// Everything here either lets someone act as the user (the bot token, a
/// webhook URL that may embed its own auth token) or concerns third parties
/// who have no reason to reach the server (subscriber chat ids).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct NotificationSecrets {
    /// Telegram bot token + subscriber list.
    #[serde(default)]
    pub telegram: TelegramSecrets,

    /// Webhook URL.
    #[serde(default)]
    pub webhook: WebhookSecrets,
}

/// A Telegram user who will receive alert notifications.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TelegramSubscriber {
    /// Telegram chat ID (numeric, as string).
    pub chat_id: String,
    /// Display name (first_name from Telegram).
    pub display_name: String,
    /// @username handle (empty if not set by user).
    pub username: String,
    /// Whether this subscriber is active (receives alerts).
    pub active: bool,
}

/// Telegram Bot API SECRETS: the bot token and the subscriber list (other
/// people's chat ids — PII with no reason to reach the server ungated).
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TelegramSecrets {
    /// Bot token obtained from @BotFather.
    #[serde(default)]
    pub bot_token: String,

    /// List of Telegram users who receive alerts.
    #[serde(default)]
    pub subscribers: Vec<TelegramSubscriber>,

    /// Legacy field — migrated to subscribers on load.
    #[serde(default, skip_serializing)]
    chat_id: String,
}

impl TelegramSecrets {
    /// Migrate legacy single chat_id to subscribers list.
    /// Call after deserializing.
    pub fn migrate_legacy(&mut self) {
        if !self.chat_id.is_empty()
            && !self.subscribers.iter().any(|s| s.chat_id == self.chat_id)
        {
            self.subscribers.push(TelegramSubscriber {
                chat_id: std::mem::take(&mut self.chat_id),
                display_name: "User".to_string(),
                username: String::new(),
                active: true,
            });
        }
    }

    /// Get all active subscriber chat IDs.
    pub fn active_chat_ids(&self) -> Vec<&str> {
        self.subscribers
            .iter()
            .filter(|s| s.active)
            .map(|s| s.chat_id.as_str())
            .collect()
    }
}

/// Webhook SECRETS: the URL itself, which may embed an auth token in its
/// path or query string.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WebhookSecrets {
    /// Full URL to POST the alert JSON payload to.
    #[serde(default)]
    pub url: String,
}

// ── Event types ───────────────────────────────────────────────────────────────

/// An alert event to be dispatched through enabled transports.
#[derive(Debug, Clone)]
pub struct DeliveryEvent {
    /// Human-readable alert name (e.g. "Price Cross Above MA").
    pub alert_name: String,
    /// Ticker symbol the alert fired on.
    pub symbol: String,
    /// Free-form description of what triggered the alert.
    pub message: String,
    /// Price at the moment the alert fired.
    pub price: f64,
    /// Unix timestamp in milliseconds.
    pub timestamp: u64,
    /// Optional PNG screenshot of the chart at the moment of the alert.
    pub screenshot: Option<Vec<u8>>,
}

/// A toast notification produced by [`AlertDelivery`] for the UI to render.
#[derive(Debug, Clone)]
pub struct ToastNotification {
    /// Short heading shown prominently.
    pub title: String,
    /// Body text shown below the title.
    pub message: String,
    /// Unix timestamp in milliseconds when the toast was created.
    pub timestamp: u64,
    /// How long to display the toast, in milliseconds.
    pub duration_ms: u64,
}

// ── Delivery engine ───────────────────────────────────────────────────────────

/// The alert delivery engine.
///
/// Spawns a background Tokio task that drains a command channel and dispatches
/// each [`DeliveryEvent`] through all enabled transports.
pub struct AlertDelivery {
    tx: mpsc::UnboundedSender<DeliveryCommand>,
}

enum DeliveryCommand {
    Deliver(DeliveryEvent),
    UpdateSettings(NotificationSettings, NotificationSecrets),
}

impl AlertDelivery {
    /// Create a new delivery engine.
    ///
    /// Takes both halves explicitly: `prefs` (plaintext, always available)
    /// and `secrets` (vault tier — pass [`NotificationSecrets::default`] when
    /// the vault is locked; delivery already no-ops on an empty bot token or
    /// webhook URL, so a locked vault just means Telegram/webhook delivery is
    /// silently skipped until the caller pushes real secrets via
    /// [`Self::update_settings`]).
    ///
    /// Returns `(engine, toast_rx)`. The caller must hold `toast_rx` and poll
    /// it (e.g. in the UI event loop) to receive [`ToastNotification`] items.
    pub fn new(
        prefs: NotificationSettings,
        secrets: NotificationSecrets,
    ) -> (Self, mpsc::UnboundedReceiver<ToastNotification>) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (toast_tx, toast_rx) = mpsc::unbounded_channel();

        // `.timeout()` is native-only on the reqwest builder; on wasm the
        // browser owns request lifetimes.
        let builder = reqwest::Client::builder();
        #[cfg(not(target_arch = "wasm32"))]
        let builder = builder.timeout(std::time::Duration::from_secs(15));
        let http_client = builder.build().unwrap_or_default();

        let worker = DeliveryWorker {
            cmd_rx,
            toast_tx,
            prefs,
            secrets,
            http_client,
        };

        #[cfg(not(target_arch = "wasm32"))]
        tokio::spawn(worker.run());
        #[cfg(target_arch = "wasm32")]
        wasm_bindgen_futures::spawn_local(worker.run());

        (Self { tx: cmd_tx }, toast_rx)
    }

    /// Queue an alert event for delivery through all enabled transports.
    ///
    /// Non-blocking; returns immediately. If the background worker has shut
    /// down the error is silently dropped.
    pub fn deliver(&self, event: DeliveryEvent) {
        let _ = self.tx.send(DeliveryCommand::Deliver(event));
    }

    /// Update the live notification settings without restarting the engine.
    ///
    /// Takes effect for the next event delivered. Same recombination contract
    /// as [`Self::new`] — the caller decides what `secrets` to pass when the
    /// vault is locked.
    pub fn update_settings(&self, prefs: NotificationSettings, secrets: NotificationSecrets) {
        let _ = self.tx.send(DeliveryCommand::UpdateSettings(prefs, secrets));
    }
}

// ── Background worker ─────────────────────────────────────────────────────────

struct DeliveryWorker {
    cmd_rx: mpsc::UnboundedReceiver<DeliveryCommand>,
    toast_tx: mpsc::UnboundedSender<ToastNotification>,
    prefs: NotificationSettings,
    secrets: NotificationSecrets,
    http_client: reqwest::Client,
}

impl DeliveryWorker {
    async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                DeliveryCommand::Deliver(event) => {
                    self.dispatch(event).await;
                }
                DeliveryCommand::UpdateSettings(prefs, secrets) => {
                    eprintln!(
                        "[alert-delivery] Settings updated: tg_enabled={}, token_len={}, subscribers={}",
                        prefs.telegram.enabled,
                        secrets.telegram.bot_token.len(),
                        secrets.telegram.subscribers.len(),
                    );
                    self.prefs = prefs;
                    self.secrets = secrets;
                }
            }
        }
    }

    async fn dispatch(&self, event: DeliveryEvent) {
        let tg_prefs = &self.prefs.telegram;
        let tg_secrets = &self.secrets.telegram;
        let active_ids = tg_secrets.active_chat_ids();
        eprintln!(
            "[alert-delivery] dispatch: alert='{}' tg_enabled={} token_len={} subscribers={}",
            event.alert_name,
            tg_prefs.enabled,
            tg_secrets.bot_token.len(),
            active_ids.len(),
        );

        // Toast — synchronous channel send, no I/O.
        if self.prefs.toast_enabled {
            let _ = self.toast_tx.send(ToastNotification {
                title: format!("Alert: {}", event.alert_name),
                message: event.message.clone(),
                timestamp: event.timestamp,
                duration_ms: 5_000,
            });
        }

        // Telegram — send to all active subscribers.
        if tg_prefs.enabled && !tg_secrets.bot_token.is_empty() && !active_ids.is_empty() {
            let text = telegram::format_alert_message(&event);

            for chat_id in &active_ids {
                if tg_prefs.send_screenshots {
                    if let Some(ref png) = event.screenshot {
                        if let Err(e) = telegram::send_photo(
                            &self.http_client,
                            &tg_secrets.bot_token,
                            chat_id,
                            png.clone(),
                            &text,
                        )
                        .await
                        {
                            eprintln!("[alert-delivery] Telegram sendPhoto error ({chat_id}): {e}");
                            // Fallback: deliver text-only so the subscriber
                            // always receives the alert.
                            if let Err(e2) = telegram::send_message(
                                &self.http_client,
                                &tg_secrets.bot_token,
                                chat_id,
                                &text,
                            )
                            .await
                            {
                                eprintln!("[alert-delivery] Telegram sendMessage fallback error ({chat_id}): {e2}");
                            }
                        }
                    } else {
                        // No screenshot available; fall back to text message.
                        if let Err(e) = telegram::send_message(
                            &self.http_client,
                            &tg_secrets.bot_token,
                            chat_id,
                            &text,
                        )
                        .await
                        {
                            eprintln!("[alert-delivery] Telegram sendMessage error ({chat_id}): {e}");
                        }
                    }
                } else if let Err(e) = telegram::send_message(
                    &self.http_client,
                    &tg_secrets.bot_token,
                    chat_id,
                    &text,
                )
                .await
                {
                    eprintln!("[alert-delivery] Telegram sendMessage error ({chat_id}): {e}");
                }
            }
        }

        // Webhook
        if self.prefs.webhook.enabled && !self.secrets.webhook.url.is_empty() {
            if let Err(e) = webhook::send_webhook(
                &self.http_client,
                &self.secrets.webhook.url,
                &event,
            )
            .await
            {
                eprintln!("[alert-delivery] Webhook error: {e}");
            }
        }
    }
}
