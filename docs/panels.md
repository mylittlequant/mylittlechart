# Trading Panels

mylittlechart includes 11 panels split into two groups: order flow panels (7) and trading suite panels (4).

Live animations and demo clips for each panel are on the project site: [chart.example](https://chart.example).

---

## Order Flow Panels

### DOM — Depth of Market

Displays the current order book as a price ladder with bid and ask quantities at each level. Updates in real time from the exchange Level 2 feed. Useful for reading order book imbalance, large limit orders, and short-term price pressure.

**Data:** Level 2 order book (live WebSocket).

---

### Footprint

Shows volume traded at each price level for every candle, split into buy-initiated and sell-initiated trades. Each cell displays the delta (buy volume minus sell volume) or raw bid/ask volume depending on the mode. Identifies where aggressive buyers and sellers are active within a bar.

**Data:** Trade tape (live WebSocket), aggregated by price and bar.

---

### Volume Profile

Plots the distribution of volume traded at each price level over a selected period. The horizontal histogram overlaid on the chart shows which price levels attracted the most activity. The Point of Control (POC) marks the single price level with the highest volume.

**Data:** Trade tape, aggregated by price.

---

### Liquidity Heatmap

Visualizes the density of resting limit orders across price and time as a heat-encoded grid. Darker or brighter cells indicate more liquidity at that price/time coordinate. Highlights areas where large orders are resting and may act as support or resistance.

**Data:** Level 2 order book snapshots over time.

---

### Big Trades

Filters the real-time trade tape to show only trades above a configurable size threshold. Large individual trades ("whale prints") are listed with their timestamp, side, price, and size. Useful for tracking large market participants.

**Data:** Trade tape (live WebSocket), filtered by minimum size.

---

### L2 Tape

Streams real-time Level 2 order book changes — adds, cancels, and modifications — as a scrolling list. Includes basic spoof detection that flags large orders which appear and disappear quickly without trading. Useful for reading short-term order flow and identifying layering.

**Data:** Level 2 order book diff stream (live WebSocket).

---

### Trade Tape

A real-time scrolling list of individual trades as they print from the exchange. Each row shows timestamp, side (buy/sell), price, and size. Color-coded for direction. Provides an unfiltered view of market activity.

**Data:** Trade stream (live WebSocket).

---

## Trading Suite Panels

> **Status:** Trading is not yet wired to live exchange APIs. These panels currently run against the in-process paper engine. See [exchanges.md](exchanges.md) for the trading roadmap.

### Trading Container

The main trading widget that hosts the order entry, position display, and account summary in a single panel. Acts as a container layout that can be embedded in the chart window alongside price charts.

**Data:** Paper engine state + top-of-book.

---

### Order Entry

Form for submitting orders: limit, market, and stop orders. Displays current best bid/ask. Validates order size against simulated account balance before submission.

**Data:** Paper engine balance + top-of-book.

---

### Position Manager

Lists all open positions on the paper engine. Shows entry price, current price, unrealized PnL, and position size. Allows closing individual positions directly from the panel.

**Data:** Paper engine positions.

---

### Risk Calculator

Calculates position size based on account balance, risk percentage, and stop-loss distance. Inputs: entry price, stop price, and risk-per-trade (percentage of account or fixed amount). Output: recommended lot size and maximum loss in account currency.

**Data:** Paper engine balance (or manual input).
