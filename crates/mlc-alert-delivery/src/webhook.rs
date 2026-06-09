//! Generic HTTP webhook delivery.
//!
//! Posts a JSON payload to the configured URL whenever an alert fires.

use crate::DeliveryEvent;
use serde::Serialize;

/// JSON payload sent to the webhook endpoint.
#[derive(Serialize)]
struct WebhookPayload<'a> {
    alert_name: &'a str,
    symbol: &'a str,
    message: &'a str,
    price: f64,
    timestamp: u64,
}

/// POST a [`DeliveryEvent`] as JSON to `url`.
///
/// Returns `Err` if the HTTP request itself fails; non-2xx responses are
/// currently treated as success to avoid blocking the delivery pipeline.
pub async fn send_webhook(
    client: &reqwest::Client,
    url: &str,
    event: &DeliveryEvent,
) -> Result<(), String> {
    let payload = WebhookPayload {
        alert_name: &event.alert_name,
        symbol: &event.symbol,
        message: &event.message,
        price: event.price,
        timestamp: event.timestamp,
    };

    client
        .post(url)
        .json(&payload)
        .send()
        .await
        .map_err(|e| format!("Webhook request failed: {}", e))?;

    Ok(())
}
