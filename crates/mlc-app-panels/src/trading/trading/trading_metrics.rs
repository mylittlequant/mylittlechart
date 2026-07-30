//! TradingMetrics — one-row horizontal strip with the trading session's
//! fast results (owner spec 2026-07-28): the Total-PnL summary rows that
//! used to live INSIDE PositionManager / TradeLog, abstracted into their
//! own panel. Hosted by the trading container right under the bottom
//! strip (above the ticker tape).

use mlc_backend_trading_engine::SharedTradingSnapshot;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// Which segments the metrics strip shows (owner spec 2026-07-28) —
/// toggled via the trading container's common gear modal.
#[derive(Clone, Copy, Debug)]
pub struct MetricsConfig {
    pub upnl: bool,
    pub rpnl: bool,
    pub balance: bool,
    pub positions: bool,
    pub orders: bool,
    pub trades: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self { upnl: true, rpnl: true, balance: true, positions: true, orders: true, trades: true }
    }
}

/// TradingMetrics panel state.
#[derive(Clone, Debug, Default)]
pub struct TradingMetricsState {
    /// Shared trading snapshot (source of truth).
    pub snapshot: Option<SharedTradingSnapshot>,
    /// Segment inclusion toggles — common-gear-modal setting.
    pub config: MetricsConfig,
    /// Best free balance across assets (same resolution RiskCalculator uses).
    pub balance: f64,
    /// Total unrealized PnL across open positions.
    pub unrealized: f64,
    /// Total realized PnL across symbols.
    pub realized: f64,
    pub positions: usize,
    pub open_orders: usize,
    pub trades: usize,
}

impl TradingMetricsState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn set_snapshot(&mut self, snap: SharedTradingSnapshot) {
        self.snapshot = Some(snap);
    }

    pub fn sync_from_snapshot(&mut self) {
        let snap = match &self.snapshot {
            Some(s) => match s.read() {
                Ok(guard) => guard,
                Err(_) => return,
            },
            None => return,
        };
        self.balance = snap.balances.iter().map(|b| b.free).fold(0.0_f64, f64::max);
        self.unrealized = snap.positions.iter().map(|p| p.unrealized_pnl).sum();
        self.realized = snap.pnl.values().map(|p| p.realized).sum();
        self.positions = snap.positions.len();
        self.open_orders = snap.open_orders.len();
        self.trades = snap.recent_fills.len();
    }
}

fn format_money(v: f64) -> String {
    if v.abs() >= 1_000_000.0 {
        format!("${:.2}M", v / 1_000_000.0)
    } else if v.abs() >= 1_000.0 {
        format!("${:.2}K", v / 1_000.0)
    } else {
        format!("${:.2}", v)
    }
}

impl TradingPanel for TradingMetricsState {
    fn kind(&self) -> &'static str { "trading_metrics" }
    fn label(&self) -> &'static str { "Metrics" }

    fn render(
        &self,
        ctx: &mut dyn RenderContext,
        x: f32,
        y: f32,
        w: f32,
        h: f32,
        theme: &crate::panel_theme::PanelTheme,
        coordinator: &mut uzor::InputCoordinator,
        slot_prefix: &str,
    ) {
        use uzor::input::Sense;

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let body_id = format!("{}:metrics:body", slot_prefix);
        let gear_id = format!("{}:metrics:gear", slot_prefix);
        let hovered = coordinator.is_hovered(&uzor::WidgetId::from(body_id.as_str()))
            || coordinator.is_hovered(&uzor::WidgetId::from(gear_id.as_str()));
        coordinator.register(
            body_id.as_str(),
            uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
            Sense::CLICK | Sense::HOVER,
        );

        let mid_y = (y + h / 2.0) as f64;
        let pnl_color = |v: f64| -> &String {
            if v > 0.0 { &theme.pm_pnl_positive }
            else if v < 0.0 { &theme.pm_pnl_negative }
            else { &theme.pm_pnl_neutral }
        };

        // (enabled, label, value, color) segments left-to-right; positions
        // advance by measured text widths so the strip degrades gracefully
        // when narrow (clipped by the panel bounds).
        let cfg = &self.config;
        let segments: [(bool, &str, String, &String); 6] = [
            (cfg.upnl, "uPnL", format!("{:+.2}", self.unrealized), pnl_color(self.unrealized)),
            (cfg.rpnl, "rPnL", format!("{:+.2}", self.realized), pnl_color(self.realized)),
            (cfg.balance, "Bal", format_money(self.balance), &theme.text_primary),
            (cfg.positions, "Pos", format!("{}", self.positions), &theme.text_primary),
            (cfg.orders, "Ord", format!("{}", self.open_orders), &theme.text_primary),
            (cfg.trades, "Trades", format!("{}", self.trades), &theme.text_primary),
        ];

        ctx.save();
        ctx.clip_rect(x as f64, y as f64, w as f64, h as f64);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let mut cx = (x + 8.0) as f64;
        for (enabled, label, value, color) in &segments {
            if !enabled {
                continue;
            }
            ctx.set_font("9px sans-serif");
            ctx.set_fill_color(&theme.text_muted);
            ctx.fill_text(label, cx, mid_y);
            cx += ctx.measure_text(label) + 4.0;

            ctx.set_font("10px monospace");
            ctx.set_fill_color(color);
            ctx.fill_text(value, cx, mid_y);
            cx += ctx.measure_text(value) + 14.0;
        }
        ctx.restore();

        // Hover-revealed gear at the right edge — opens the container's
        // common settings modal (routed in the app layer).
        if hovered {
            const GEAR_W: f64 = 22.0;
            let gx = (x + w) as f64 - GEAR_W;
            ctx.set_fill_color(&theme.header_bg);
            ctx.fill_rect(gx, y as f64, GEAR_W, h as f64);
            let gear_hov = coordinator.is_hovered(&uzor::WidgetId::from(gear_id.as_str()));
            let gear_sz = 12.0;
            uzor::render::draw_svg_icon(
                ctx,
                mlc_core::state::icons::ICON_SETTINGS,
                gx + (GEAR_W - gear_sz) / 2.0,
                mid_y - gear_sz / 2.0,
                gear_sz,
                gear_sz,
                if gear_hov { &theme.text_primary } else { &theme.text_muted },
            );
            coordinator.register(
                gear_id.as_str(),
                uzor::Rect::new(gx, y as f64, GEAR_W, h as f64),
                Sense::CLICK | Sense::HOVER,
            );
        }
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }
}
