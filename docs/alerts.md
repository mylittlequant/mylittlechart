# Alerts

Alerts fire when a condition is met on a chart and dispatch through one or more delivery channels.

## Trigger types

- **Price crossing** — price crosses above or below a specified level
- **Indicator crossing** — one indicator line crosses another, or crosses a level
- **Drawing primitive crossing** — price crosses a drawn line, level, or zone

## Delivery channels

### Toast (in-app)

Enabled by default. A popup notification appears in the application window for 5 seconds when an alert fires. Toast delivery is synchronous — it never blocks alert processing.

No setup required.

### Telegram

Sends an alert message (and optionally a chart screenshot) to one or more Telegram users via the Bot API.

**Setup:**

1. Create a bot using [@BotFather](https://t.me/BotFather) on Telegram. BotFather will give you a bot token of the form `123456789:ABCdef...`.
2. Open **Settings → Alerts → Telegram** and paste the token.
3. Click **Verify token** — this calls the Telegram `getMe` endpoint to confirm the token is valid and displays the bot's username.
4. Send `/start` (or any message) to your bot in the Telegram app.
5. Click **Discover subscribers** — this calls `getUpdates` and lists all chat IDs that have messaged the bot.
6. Select which subscribers should receive alerts (each can be toggled individually).

Multiple subscribers are supported. Each active subscriber receives every alert independently.

**Optional screenshot attachment:**

Enable **Attach chart screenshot** to have the chart's current view captured as a PNG and sent alongside the alert text. The screenshot is taken from the chart where the alert triggered. The Telegram message uses HTML parse mode.

**Alert message format:**

```
Alert: <alert_name>

Symbol: <symbol>
Price: <price>
<message>

HH:MM:SS UTC
```

### Webhook

Posts a JSON payload to an HTTP endpoint when an alert fires. Useful for integrating with external systems, bots, or logging pipelines.

**Setup:**

1. Open **Settings → Alerts → Webhook**.
2. Enter the full URL (must accept HTTP POST with a JSON body).
3. Enable the webhook.

**Payload format:**

```json
{
  "alert_name": "Price Cross Above MA",
  "symbol": "BTCUSDT",
  "message": "Price crossed above EMA 20",
  "price": 50001.5,
  "timestamp": 1700000000
}
```

| Field | Type | Description |
|---|---|---|
| `alert_name` | string | Name given to the alert |
| `symbol` | string | Ticker that triggered |
| `message` | string | Human-readable trigger description |
| `price` | float | Price at moment of trigger |
| `timestamp` | int | Unix seconds at moment of trigger |

Non-2xx HTTP responses from the webhook endpoint are currently logged but do not block alert processing or retry.

## Settings persistence

All notification settings (enabled channels, bot token, subscribers, webhook URL) are saved in the user profile on disk. The profile is encrypted at rest — see [exchanges.md](exchanges.md) for the storage model.
