//! Telegram Bot API client.
//!
//! Provides functions to verify tokens, discover chat IDs via getUpdates,
//! send text messages, and send photo messages (chart screenshots).

use crate::DeliveryEvent;

const API_BASE: &str = "https://api.telegram.org/bot";

/// Verify a bot token is valid by calling getMe.
/// Returns the bot's username on success.
pub async fn verify_token(
    client: &reqwest::Client,
    bot_token: &str,
) -> Result<String, String> {
    let url = format!("{}{}/getMe", API_BASE, bot_token);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    if body["ok"].as_bool() == Some(true) {
        let username = body["result"]["username"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();
        Ok(username)
    } else {
        let desc = body["description"]
            .as_str()
            .unwrap_or("Unknown error");
        Err(desc.to_string())
    }
}

/// Fetch pending updates to discover which chat IDs have messaged the bot.
///
/// Returns a deduplicated list of `(chat_id, display_name, username)` triples.
/// `username` is formatted as `@handle` when present, or empty string if absent.
/// Callers should prompt the user to send `/start` to the bot first.
pub async fn get_updates(
    client: &reqwest::Client,
    bot_token: &str,
) -> Result<Vec<(String, String, String)>, String> {
    let url = format!("{}{}/getUpdates?limit=100&timeout=0", API_BASE, bot_token);
    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| e.to_string())?;
    let body: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    let mut results = Vec::new();

    if let Some(updates) = body["result"].as_array() {
        for update in updates {
            if let Some(msg) = update.get("message") {
                let chat_id = msg["chat"]["id"].as_i64().unwrap_or(0).to_string();
                let first_name = msg["chat"]["first_name"]
                    .as_str()
                    .unwrap_or("")
                    .to_string();
                let username = msg["chat"]["username"]
                    .as_str()
                    .map(|u| format!("@{}", u))
                    .unwrap_or_default();
                let display_name = if !first_name.is_empty() {
                    first_name
                } else if !username.is_empty() {
                    username.clone()
                } else {
                    "unknown".to_string()
                };

                if !chat_id.is_empty() && chat_id != "0" {
                    results.push((chat_id, display_name, username));
                }
            }
        }
    }

    // Deduplicate by chat_id, keeping first occurrence.
    results.sort_by(|a, b| a.0.cmp(&b.0));
    results.dedup_by(|a, b| a.0 == b.0);

    Ok(results)
}

/// Send a plain text message (HTML parse mode).
pub async fn send_message(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    text: &str,
) -> Result<(), String> {
    let url = format!("{}{}/sendMessage", API_BASE, bot_token);
    let body = serde_json::json!({
        "chat_id": chat_id,
        "text": text,
        "parse_mode": "HTML",
    });

    let resp = client
        .post(&url)
        .json(&body)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    if result["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        let desc = result["description"]
            .as_str()
            .unwrap_or("Send failed");
        Err(desc.to_string())
    }
}

/// Send a PNG photo (chart screenshot) with an HTML caption.
///
/// Native only — the reqwest wasm backend (fetch) does not support
/// `reqwest::multipart::Form`. Wasm screenshot delivery is a Phase 5
/// follow-up (hand-rolled FormData via web-sys).
#[cfg(not(target_arch = "wasm32"))]
pub async fn send_photo(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    png_bytes: Vec<u8>,
    caption: &str,
) -> Result<(), String> {
    let url = format!("{}{}/sendPhoto", API_BASE, bot_token);

    let part = reqwest::multipart::Part::bytes(png_bytes)
        .file_name("chart.png")
        .mime_str("image/png")
        .map_err(|e| e.to_string())?;

    let form = reqwest::multipart::Form::new()
        .text("chat_id", chat_id.to_string())
        .text("caption", caption.to_string())
        .text("parse_mode", "HTML")
        .part("photo", part);

    let resp = client
        .post(&url)
        .multipart(form)
        .send()
        .await
        .map_err(|e| e.to_string())?;

    let result: serde_json::Value = resp.json().await.map_err(|e| e.to_string())?;

    if result["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        let desc = result["description"]
            .as_str()
            .unwrap_or("Send photo failed");
        Err(desc.to_string())
    }
}

/// Send a PNG photo via hand-rolled FormData + web-sys fetch.
///
/// reqwest's wasm backend does not expose multipart; we go directly to the
/// browser's native FormData + fetch APIs. The browser sets the
/// `multipart/form-data` boundary automatically — we must NOT set Content-Type.
#[cfg(target_arch = "wasm32")]
pub async fn send_photo(
    _client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
    png_bytes: Vec<u8>,
    caption: &str,
) -> Result<(), String> {
    use js_sys::{Array, Uint8Array};
    use wasm_bindgen::JsCast;
    use wasm_bindgen_futures::JsFuture;
    use web_sys::{Blob, BlobPropertyBag, FormData, Request, RequestInit, Response};

    // 1. Wrap PNG bytes in a JS Uint8Array.
    let uint8 = Uint8Array::from(png_bytes.as_slice());
    let arr = Array::new();
    arr.push(&uint8);

    // 2. Build a Blob with MIME type image/png.
    let opts = BlobPropertyBag::new();
    opts.set_type("image/png");
    let blob = Blob::new_with_u8_array_sequence_and_options(&arr, &opts)
        .map_err(|e| format!("{e:?}"))?;

    // 3. Build FormData.
    let form = FormData::new().map_err(|e| format!("{e:?}"))?;
    form.append_with_str("chat_id", chat_id)
        .map_err(|e| format!("{e:?}"))?;
    form.append_with_str("caption", caption)
        .map_err(|e| format!("{e:?}"))?;
    form.append_with_str("parse_mode", "HTML")
        .map_err(|e| format!("{e:?}"))?;
    form.append_with_blob_and_filename("photo", &blob, "chart.png")
        .map_err(|e| format!("{e:?}"))?;

    // 4. Build fetch Request. Do NOT set Content-Type — the browser injects
    //    the multipart/form-data boundary header automatically.
    let url = format!("https://api.telegram.org/bot{bot_token}/sendPhoto");
    let init = RequestInit::new();
    init.set_method("POST");
    init.set_body(&form.into());
    let request = Request::new_with_str_and_init(&url, &init)
        .map_err(|e| format!("{e:?}"))?;

    // 5. Dispatch via the window global fetch.
    let window = web_sys::window().ok_or("no window")?;
    let resp_val = JsFuture::from(window.fetch_with_request(&request))
        .await
        .map_err(|e| format!("{e:?}"))?;
    let resp: Response = resp_val.dyn_into().map_err(|e| format!("{e:?}"))?;

    // 6. Parse response as text, then deserialise via serde_json.
    //    Using text() avoids adding serde-wasm-bindgen as a dependency.
    let text_val = JsFuture::from(resp.text().map_err(|e| format!("{e:?}"))?)
        .await
        .map_err(|e| format!("{e:?}"))?;
    let text: String = text_val.as_string().ok_or("response.text() is not a string")?;
    let body: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| e.to_string())?;

    if body["ok"].as_bool() == Some(true) {
        Ok(())
    } else {
        Err(body["description"]
            .as_str()
            .unwrap_or("sendPhoto failed")
            .to_string())
    }
}

/// Send a test message to verify that bot token + chat ID are working.
pub async fn send_test(
    client: &reqwest::Client,
    bot_token: &str,
    chat_id: &str,
) -> Result<(), String> {
    send_message(
        client,
        bot_token,
        chat_id,
        "✅ <b>mylittlechart Alert Bot connected!</b>\n\nYou will receive trading alerts here.",
    )
    .await
}

/// Format a [`DeliveryEvent`] as an HTML string suitable for Telegram.
pub fn format_alert_message(event: &DeliveryEvent) -> String {
    let ts_secs = event.timestamp / 1000;
    let secs_in_day = ts_secs % 86400;
    let hours = secs_in_day / 3600;
    let minutes = (secs_in_day % 3600) / 60;
    let seconds = secs_in_day % 60;

    format!(
        "🔔 <b>Alert: {}</b>\n\n\
         Symbol: <code>{}</code>\n\
         Price: <code>{:.8}</code>\n\
         {}\n\n\
         ⏰ {:02}:{:02}:{:02} UTC",
        html_escape(&event.alert_name),
        html_escape(&event.symbol),
        event.price,
        html_escape(&event.message),
        hours,
        minutes,
        seconds,
    )
}

/// Escape HTML special characters for Telegram's HTML parse mode.
fn html_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}
