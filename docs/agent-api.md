# Agent API Reference

The Agent API is a local HTTP server embedded in the mylittlechart process. It starts automatically when the application launches and is available at:

```
http://127.0.0.1:17420
```

The server binds only to `127.0.0.1`. It is not reachable from the network. There is no authentication — access is restricted to local processes on the same machine.

All write operations (viewport, symbol switch, indicator CRUD, primitives CRUD) are asynchronous: the handler queues a command and returns `202 Accepted` immediately. The render thread applies the command on the next frame.

---

## Endpoints

### GET /health

Liveness check. Returns server version and uptime.

**Response:**
```json
{
  "status": "ok",
  "version": "0.5.569",
  "uptime_secs": 42
}
```

**Example:**
```bash
curl http://127.0.0.1:17420/health
```

---

### GET /api/v1/bars

Returns cached OHLCV bars for a given exchange, symbol, and timeframe. Data comes from the in-memory bar cache populated by live WebSocket feeds.

**Query parameters:**

| Parameter | Required | Description |
|---|---|---|
| `exchange` | Yes | Exchange identifier, e.g. `binance` |
| `symbol` | Yes | Trading pair, e.g. `BTCUSDT` |
| `timeframe` | Yes | Timeframe string, e.g. `1h`, `15m`, `1d` |
| `limit` | No | Max number of bars to return (most-recent bars kept) |
| `account_type` | No | `S` = Spot (default), `FC` = FuturesCross, `M` = Margin |

**Response:**
```json
{
  "exchange": "binance",
  "symbol": "BTCUSDT",
  "timeframe": "1h",
  "count": 500,
  "bars": [
    { "t": 1700000000, "o": 35000.0, "h": 35500.0, "l": 34800.0, "c": 35200.0, "v": 1234.56 }
  ]
}
```

`t` is a Unix timestamp in seconds.

**Example:**
```bash
curl "http://127.0.0.1:17420/api/v1/bars?exchange=binance&symbol=BTCUSDT&timeframe=1h&limit=100"
```

---

### GET /api/v1/indicators

Returns the current indicator snapshot — computed values for all active indicator instances.

**Query parameters:**

| Parameter | Required | Description |
|---|---|---|
| `symbol` | No | Filter to a single symbol, e.g. `BTCUSDT` |

**Response (all symbols):**
```json
{
  "symbols": {
    "BTCUSDT": [
      {
        "id": 1,
        "type_id": "ema",
        "type_name": "Exponential Moving Average",
        "symbol": "BTCUSDT",
        "window_id": null,
        "params": { "period": 20 },
        "outputs": [
          { "name": "value", "values": [34900.0, 35100.0, 35200.0] }
        ]
      }
    ]
  }
}
```

**Response (filtered by symbol):**
```json
{
  "symbol": "BTCUSDT",
  "indicators": [ ... ]
}
```

**Example:**
```bash
curl "http://127.0.0.1:17420/api/v1/indicators?symbol=BTCUSDT"
```

---

### GET /api/v1/windows

Lists all open OS windows with summary information.

**Response:**
```json
{
  "windows": [
    {
      "window_id": "window-0",
      "tab_count": 2,
      "chart_count": 3,
      "active_tab_id": "preset-abc"
    }
  ]
}
```

**Example:**
```bash
curl http://127.0.0.1:17420/api/v1/windows
```

---

### GET /api/v1/windows/:window_id/tabs

Returns all tabs (presets) in a window.

**Response:**
```json
{
  "window_id": "window-0",
  "tabs": [
    { "preset_id": "preset-abc", "name": "Main", "active": true },
    { "preset_id": "preset-xyz", "name": "Alts", "active": false }
  ]
}
```

**Example:**
```bash
curl http://127.0.0.1:17420/api/v1/windows/window-0/tabs
```

---

### GET /api/v1/windows/:window_id/layout

Returns the layout tree for a window. Leaves identify charts; splits describe how charts are arranged.

**Response:**
```json
{
  "window_id": "window-0",
  "layout": {
    "type": "split",
    "axis": "horizontal",
    "proportions": [0.5, 0.5],
    "children": [
      { "type": "leaf", "chart_id": 1, "leaf_id": 1 },
      { "type": "leaf", "chart_id": 2, "leaf_id": 2 }
    ]
  }
}
```

**Example:**
```bash
curl http://127.0.0.1:17420/api/v1/windows/window-0/layout
```

---

### GET /api/v1/windows/:window_id/charts

Returns all charts in a window with full detail (symbol, exchange, timeframe, viewport, indicator list, primitive list).

**Response:**
```json
{
  "window_id": "window-0",
  "charts": [
    {
      "chart_id": 1,
      "leaf_id": 1,
      "symbol": "BTCUSDT",
      "exchange": "binance",
      "timeframe": "1h",
      "bar_count": 500,
      "viewport": {
        "view_start": 480.0,
        "bar_spacing": 8.0,
        "chart_width": 1200.0,
        "chart_height": 600.0,
        "bars_visible": 150
      },
      "indicator_count": 2,
      "primitive_count": 1,
      "indicators": [
        { "id": 1, "type_id": "ema", "name": "EMA 20" }
      ],
      "primitives": [
        { "id": 1, "type_id": "trend_line" }
      ]
    }
  ]
}
```

**Example:**
```bash
curl http://127.0.0.1:17420/api/v1/windows/window-0/charts
```

---

### GET /api/v1/windows/:window_id/charts/:chart_id

Returns full detail for one chart. Same shape as individual entries in the `/charts` response.

**Example:**
```bash
curl http://127.0.0.1:17420/api/v1/windows/window-0/charts/1
```

---

### POST /api/v1/windows/:window_id/charts/:chart_id/viewport

Pan or zoom a chart. Returns `202 Accepted` immediately; the render thread applies the change on the next frame.

**Body (at least one field required):**
```json
{
  "view_start": 450.0,
  "bar_spacing": 8.0,
  "mode": "fit"
}
```

| Field | Type | Description |
|---|---|---|
| `view_start` | float | First visible bar index (leftmost bar position) |
| `bar_spacing` | float | Pixels per bar |
| `mode` | string | Named mode: `"focus"`, `"analyze"`, or `"fit"`. Overrides numeric fields when present. |

**Response:**
```json
{ "queued": true }
```

**Example:**
```bash
curl -X POST http://127.0.0.1:17420/api/v1/windows/window-0/charts/1/viewport \
  -H "Content-Type: application/json" \
  -d '{"mode": "fit"}'
```

---

### POST /api/v1/windows/:window_id/charts/:chart_id/symbol

Switch a chart to a different symbol, exchange, or timeframe. Returns `202 Accepted`.

**Body:**
```json
{
  "symbol": "ETHUSDT",
  "exchange": "binance",
  "timeframe": "4h",
  "account_type": "S"
}
```

| Field | Required | Description |
|---|---|---|
| `symbol` | Yes | Trading pair, e.g. `ETHUSDT` |
| `exchange` | Yes | Exchange identifier, e.g. `bybit` |
| `timeframe` | Yes | Timeframe string, e.g. `4h` |
| `account_type` | No | `S` = Spot (default), `FC` = FuturesCross, `M` = Margin |

**Example:**
```bash
curl -X POST http://127.0.0.1:17420/api/v1/windows/window-0/charts/1/symbol \
  -H "Content-Type: application/json" \
  -d '{"symbol":"ETHUSDT","exchange":"binance","timeframe":"1h"}'
```

---

### POST /api/v1/windows/:window_id/charts/:chart_id/screenshot

Captures a PNG screenshot of a chart and returns it base64-encoded. The render thread performs the GPU readback; the call blocks up to 5 seconds. Returns `504` on timeout.

**Body (optional):**
```json
{ "agent_id": "my-bot" }
```

**Response:**
```json
{
  "window_id": "window-0",
  "chart_id": 1,
  "width": 1200,
  "height": 600,
  "png_base64": "iVBORw0KGgoAAAANS..."
}
```

**Example:**
```bash
curl -X POST http://127.0.0.1:17420/api/v1/windows/window-0/charts/1/screenshot \
  | python -c "import sys,json,base64; d=json.load(sys.stdin); open('chart.png','wb').write(base64.b64decode(d['png_base64']))"
```

---

### GET /api/v1/windows/:window_id/charts/:chart_id/indicators

Lists indicators attached to a specific chart.

**Response:**
```json
{
  "chart_id": 1,
  "indicators": [
    { "id": 1, "type_id": "ema", "name": "EMA 20" }
  ]
}
```

---

### POST /api/v1/windows/:window_id/charts/:chart_id/indicators

Adds an indicator to a chart. Returns `202 Accepted`.

**Body:**
```json
{
  "type_id": "ema",
  "params": { "period": 20 },
  "agent_id": "optional-tag"
}
```

| Field | Required | Description |
|---|---|---|
| `type_id` | Yes | Indicator type from `/api/v1/catalog/indicators` |
| `params` | No | Parameter overrides (defaults applied for omitted keys) |
| `agent_id` | No | Optional tag for tracking which agent added this indicator |

**Example:**
```bash
curl -X POST http://127.0.0.1:17420/api/v1/windows/window-0/charts/1/indicators \
  -H "Content-Type: application/json" \
  -d '{"type_id":"rsi","params":{"period":14}}'
```

---

### PATCH /api/v1/windows/:window_id/charts/:chart_id/indicators/:indicator_id

Updates parameters of an existing indicator. Returns `202 Accepted`.

**Body:**
```json
{
  "params": { "period": 50 },
  "agent_id": "optional-tag"
}
```

---

### DELETE /api/v1/windows/:window_id/charts/:chart_id/indicators/:indicator_id

Removes an indicator from a chart. Returns `202 Accepted`.

**Query parameters:**

| Parameter | Description |
|---|---|
| `agent_id` | Optional tracking tag |

**Example:**
```bash
curl -X DELETE "http://127.0.0.1:17420/api/v1/windows/window-0/charts/1/indicators/1"
```

---

### GET /api/v1/windows/:window_id/charts/:chart_id/primitives

Lists drawing primitives on a chart.

**Response:**
```json
{
  "chart_id": 1,
  "primitives": [
    { "id": 1, "type_id": "trend_line" }
  ]
}
```

---

### POST /api/v1/windows/:window_id/charts/:chart_id/primitives

Adds a drawing primitive to a chart. Returns `202 Accepted`.

**Body:**
```json
{
  "type_id": "trend_line",
  "points": [[1700000000, 35000.0], [1700003600, 35500.0]],
  "style": {
    "color": "#e74c3c",
    "width": 2.0,
    "style": "solid",
    "fill_color": null,
    "fill_opacity": null
  },
  "agent_id": "optional-tag"
}
```

| Field | Required | Description |
|---|---|---|
| `type_id` | Yes | Primitive type from `/api/v1/catalog/primitives` |
| `points` | Yes | Array of `[timestamp_secs, price]` pairs |
| `style` | No | Style overrides; defaults to red `#e74c3c`, width 2, solid |

---

### PATCH /api/v1/windows/:window_id/charts/:chart_id/primitives/:primitive_id

Updates points or style of an existing primitive. Returns `202 Accepted`.

**Body:**
```json
{
  "points": [[1700000000, 35200.0], [1700003600, 35800.0]],
  "style": { "color": "#27ae60", "width": 1.5, "style": "dashed" }
}
```

All fields are optional; omit to leave unchanged.

---

### DELETE /api/v1/windows/:window_id/charts/:chart_id/primitives/:primitive_id

Removes a primitive from a chart. Returns `202 Accepted`.

**Query parameters:**

| Parameter | Description |
|---|---|
| `agent_id` | Optional tracking tag |

---

### GET /api/v1/catalog/indicators

Returns definitions of all available indicator types. Use this to discover `type_id` values for the indicator CRUD endpoints.

**Query parameters:**

| Parameter | Description |
|---|---|
| `search` | Optional case-insensitive substring filter (matches type_id, name, short_name, category) |

**Response:**
```json
{
  "total": 480,
  "indicators": [
    {
      "type_id": "ema",
      "name": "Exponential Moving Average",
      "short_name": "EMA",
      "category": "trend",
      "description": "...",
      "overlay": true,
      "params": [
        {
          "name": "period",
          "display_name": "Period",
          "param_type": "int",
          "default_value": 20,
          "min": 1,
          "max": null
        }
      ],
      "outputs": [
        { "name": "value", "color": "#2196f3" }
      ]
    }
  ]
}
```

**Example:**
```bash
curl "http://127.0.0.1:17420/api/v1/catalog/indicators?search=rsi"
```

---

### GET /api/v1/catalog/primitives

Returns definitions of all available drawing primitive types.

**Response:**
```json
{
  "total": 12,
  "primitives": [
    {
      "type_id": "trend_line",
      "display_name": "Trend Line",
      "kind": "lines",
      "click_behavior": "TwoPoint",
      "default_color": "#e74c3c",
      "supports_text": false,
      "has_levels": false
    }
  ]
}
```

---

### GET /api/v1/watchlists

Returns all user-configured watchlists with their symbol entries.

**Response:**
```json
{
  "watchlists": [
    {
      "id": 1,
      "name": "Majors",
      "active": true,
      "items": [
        { "symbol": "BTCUSDT", "exchange": "binance", "category": "crypto" },
        { "symbol": "ETHUSDT", "exchange": "binance", "category": "crypto" }
      ]
    }
  ]
}
```

**Example:**
```bash
curl http://127.0.0.1:17420/api/v1/watchlists
```

---

### GET /api/v1/connectors

Returns the connection status of all configured exchange connectors.

**Response:**
```json
{
  "connectors": [
    {
      "exchange_id": "binance",
      "active": true,
      "ws_active": true,
      "symbol_count": 5
    }
  ]
}
```

**Example:**
```bash
curl http://127.0.0.1:17420/api/v1/connectors
```

---

## Error responses

All endpoints return JSON errors with an `error` field:

```json
{ "error": "window not found: window-99" }
```

Common status codes:

| Code | Meaning |
|---|---|
| 200 | Success |
| 202 | Command queued (write operations) |
| 400 | Bad request (missing or invalid parameters) |
| 404 | Window, chart, indicator, or primitive not found |
| 504 | Render thread did not respond in time (screenshot timeout) |
