# Trading Panels

mylittlechart includes 11 panels split into two groups: order flow panels (7) and trading suite panels (4).

---

## Order Flow Panels

### DOM — Depth of Market

Displays the current order book as a price ladder with bid and ask quantities at each level. Updates in real time from the exchange Level 2 feed. Useful for reading order book imbalance, large limit orders, and short-term price pressure.

**Data:** Level 2 order book (live WebSocket).

![DOM panel](screenshots/panels/dom.png)

---

### Footprint

Shows volume traded at each price level for every candle, split into buy-initiated and sell-initiated trades. Each cell displays the delta (buy volume minus sell volume) or raw bid/ask volume depending on the mode. Identifies where aggressive buyers and sellers are active within a bar.

**Data:** Trade tape (live WebSocket), aggregated by price and bar.

![Footprint panel](screenshots/panels/footprint.png)

---

### Volume Profile

Plots the distribution of volume traded at each price level over a selected period. The horizontal histogram overlaid on the chart shows which price levels attracted the most activity. The Point of Control (POC) marks the single price level with the highest volume.

**Data:** Trade tape, aggregated by price.

![Volume Profile panel](screenshots/panels/volume-profile.png)

---

### Liquidity Heatmap

Visualizes the density of resting limit orders across price and time as a heat-encoded grid. Darker or brighter cells indicate more liquidity at that price/time coordinate. Highlights areas where large orders are resting and may act as support or resistance.

**Data:** Level 2 order book snapshots over time.

![Liquidity Heatmap panel](screenshots/panels/liquidity-heatmap.png)

---

### Big Trades

Filters the real-time trade tape to show only trades above a configurable size threshold. Large individual trades ("whale prints") are listed with their timestamp, side, price, and size. Useful for tracking large market participants.

**Data:** Trade tape (live WebSocket), filtered by minimum size.

![Big Trades panel](screenshots/panels/big-trades.png)

---

### L2 Tape

Streams real-time Level 2 order book changes — adds, cancels, and modifications — as a scrolling list. Includes basic spoof detection that flags large orders which appear and disappear quickly without trading. Useful for reading short-term order flow and identifying layering.

**Data:** Level 2 order book diff stream (live WebSocket).

![L2 Tape panel](screenshots/panels/l2-tape.png)

---

### Trade Tape

A real-time scrolling list of individual trades as they print from the exchange. Each row shows timestamp, side (buy/sell), price, and size. Color-coded for direction. Provides an unfiltered view of market activity.

**Data:** Trade stream (live WebSocket).

![Trade Tape panel](screenshots/panels/trade-tape.png)

---

## Trading Suite Panels

### Trading Container

The main trading widget that hosts the order entry, position display, and account summary in a single panel. Acts as a container layout that can be embedded in the chart window alongside price charts.

**Data:** Account data (requires API keys), order book top-of-book.

![Trading Container panel](screenshots/panels/trading-container.png)

---

### Order Entry

Form for submitting new orders to the exchange: limit, market, and stop orders. Displays current best bid/ask. Validates order size against account balance before submission.

**Data:** Account balance and positions (requires API keys), top-of-book.

![Order Entry panel](screenshots/panels/order-entry.png)

---

### Position Manager

Lists all open positions for the current exchange and account. Shows entry price, current price, unrealized PnL, and position size. Allows closing individual positions directly from the panel.

**Data:** Open positions (requires API keys, live polling).

![Position Manager panel](screenshots/panels/position-manager.png)

---

### Risk Calculator

Calculates position size based on account balance, risk percentage, and stop-loss distance. Inputs: entry price, stop price, and risk-per-trade (percentage of account or fixed amount). Output: recommended lot size and maximum loss in account currency.

**Data:** Account balance (requires API keys for live balance; manual input available without keys).

![Risk Calculator panel](screenshots/panels/risk-calculator.png)
