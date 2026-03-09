//! Alert notification delivery for zengeld-terminal.
//!
//! Dispatches alert events through multiple transports concurrently:
//! - **Toast**: in-app popup (rendered by the chart UI layer)
//! - **Telegram**: Bot API text/photo messages
//! - **Webhook**: generic HTTP POST
//!
//! # Usage
//!
//! ```rust,no_run
//! use alert_delivery::{AlertDelivery, DeliveryEvent, NotificationSettings};
//!
//! #[tokio::main]
//! async fn main() {
//!     let settings = NotificationSettings::default();
//!     let (delivery, mut toasts) = AlertDelivery::new(settings);
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

// ── Settings ─────────────────────────────────────────────────────────────────

/// Global notification settings, typically persisted in UserProfile.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct NotificationSettings {
    /// Show a toast popup inside the app.
    #[serde(default = "default_true")]
    pub toast_enabled: bool,

    /// Play a sound on alert (handled by the UI layer; delivery crate only
    /// propagates the flag).
    #[serde(default)]
    pub sound_enabled: bool,

    /// Telegram bot delivery settings.
    #[serde(default)]
    pub telegram: TelegramSettings,

    /// Generic HTTP webhook settings.
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

/// Telegram Bot API delivery settings.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct TelegramSettings {
    /// Whether Telegram delivery is active.
    #[serde(default)]
    pub enabled: bool,

    /// Bot token obtained from @BotFather.
    #[serde(default)]
    pub bot_token: String,

    /// List of Telegram users who receive alerts.
    #[serde(default)]
    pub subscribers: Vec<TelegramSubscriber>,

    /// Whether to attach a chart screenshot to alert messages.
    #[serde(default)]
    pub send_screenshots: bool,

    /// Legacy field — migrated to subscribers on load.
    #[serde(default, skip_serializing)]
    chat_id: String,
}

impl TelegramSettings {
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

/// Generic HTTP webhook delivery settings.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct WebhookSettings {
    /// Whether webhook delivery is active.
    #[serde(default)]
    pub enabled: bool,

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
    UpdateSettings(NotificationSettings),
}

impl AlertDelivery {
    /// Create a new delivery engine.
    ///
    /// Returns `(engine, toast_rx)`. The caller must hold `toast_rx` and poll
    /// it (e.g. in the UI event loop) to receive [`ToastNotification`] items.
    pub fn new(
        settings: NotificationSettings,
    ) -> (Self, mpsc::UnboundedReceiver<ToastNotification>) {
        let (cmd_tx, cmd_rx) = mpsc::unbounded_channel();
        let (toast_tx, toast_rx) = mpsc::unbounded_channel();

        let http_client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(15))
            .build()
            .unwrap_or_default();

        let worker = DeliveryWorker {
            cmd_rx,
            toast_tx,
            settings,
            http_client,
        };

        tokio::spawn(worker.run());

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
    /// Takes effect for the next event delivered.
    pub fn update_settings(&self, settings: NotificationSettings) {
        let _ = self.tx.send(DeliveryCommand::UpdateSettings(settings));
    }
}

// ── Background worker ─────────────────────────────────────────────────────────

struct DeliveryWorker {
    cmd_rx: mpsc::UnboundedReceiver<DeliveryCommand>,
    toast_tx: mpsc::UnboundedSender<ToastNotification>,
    settings: NotificationSettings,
    http_client: reqwest::Client,
}

impl DeliveryWorker {
    async fn run(mut self) {
        while let Some(cmd) = self.cmd_rx.recv().await {
            match cmd {
                DeliveryCommand::Deliver(event) => {
                    self.dispatch(event).await;
                }
                DeliveryCommand::UpdateSettings(s) => {
                    eprintln!(
                        "[alert-delivery] Settings updated: tg_enabled={}, token_len={}, subscribers={}",
                        s.telegram.enabled,
                        s.telegram.bot_token.len(),
                        s.telegram.subscribers.len(),
                    );
                    self.settings = s;
                }
            }
        }
    }

    async fn dispatch(&self, event: DeliveryEvent) {
        let tg = &self.settings.telegram;
        let active_ids = tg.active_chat_ids();
        eprintln!(
            "[alert-delivery] dispatch: alert='{}' tg_enabled={} token_len={} subscribers={}",
            event.alert_name,
            tg.enabled,
            tg.bot_token.len(),
            active_ids.len(),
        );

        // Toast — synchronous channel send, no I/O.
        if self.settings.toast_enabled {
            let _ = self.toast_tx.send(ToastNotification {
                title: format!("Alert: {}", event.alert_name),
                message: event.message.clone(),
                timestamp: event.timestamp,
                duration_ms: 5_000,
            });
        }

        // Telegram — send to all active subscribers.
        if tg.enabled && !tg.bot_token.is_empty() && !active_ids.is_empty() {
            let text = telegram::format_alert_message(&event);

            for chat_id in &active_ids {
                if tg.send_screenshots {
                    if let Some(ref png) = event.screenshot {
                        if let Err(e) = telegram::send_photo(
                            &self.http_client,
                            &tg.bot_token,
                            chat_id,
                            png.clone(),
                            &text,
                        )
                        .await
                        {
                            eprintln!("[alert-delivery] Telegram sendPhoto error ({chat_id}): {e}");
                        }
                    } else {
                        // No screenshot available; fall back to text message.
                        if let Err(e) = telegram::send_message(
                            &self.http_client,
                            &tg.bot_token,
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
                    &tg.bot_token,
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
        if self.settings.webhook.enabled && !self.settings.webhook.url.is_empty() {
            if let Err(e) = webhook::send_webhook(
                &self.http_client,
                &self.settings.webhook.url,
                &event,
            )
            .await
            {
                eprintln!("[alert-delivery] Webhook error: {e}");
            }
        }
    }
}
