use serde::{Serialize, Deserialize};
use trading_manager::SharedTradingSnapshot;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};

/// PositionManager panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PositionManagerId(pub u64);

/// PositionManager panel state (heavy data)
#[derive(Clone, Debug)]
pub struct PositionManagerState {
    /// Open positions
    pub positions: Vec<Position>,
    /// Selected position index
    pub selected: Option<usize>,
    /// Edit mode (for TP/SL adjustment)
    pub edit_mode: Option<PositionEditMode>,
    /// Total unrealized PnL
    pub total_unrealized_pnl: f64,
    /// Shared snapshot from TradingManager
    pub snapshot: Option<SharedTradingSnapshot>,
    /// Scroll offset (rows scrolled down from top)
    pub scroll_offset: usize,
}

#[derive(Clone, Debug)]
pub struct Position {
    pub symbol: String,
    pub side: PositionSide,
    pub quantity: f64,
    pub entry_price: f64,
    pub mark_price: f64,
    pub unrealized_pnl: f64,
    pub liquidation_price: Option<f64>,
    pub leverage: u32,
}

#[derive(Clone, Debug)]
pub enum PositionSide {
    Long,
    Short,
}

#[derive(Clone, Debug)]
pub enum PositionEditMode {
    TakeProfit(usize),
    StopLoss(usize),
    Leverage(usize),
}

impl PositionManagerState {
    pub fn new() -> Self {
        Self {
            positions: Vec::new(),
            selected: None,
            edit_mode: None,
            total_unrealized_pnl: 0.0,
            snapshot: None,
            scroll_offset: 0,
        }
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

        self.positions.clear();
        for p in &snap.positions {
            let side = match p.side {
                trading_manager::PositionSide::Long => PositionSide::Long,
                trading_manager::PositionSide::Short => PositionSide::Short,
                trading_manager::PositionSide::Both => {
                    if p.quantity >= 0.0 { PositionSide::Long } else { PositionSide::Short }
                }
            };
            self.positions.push(Position {
                symbol: p.symbol.clone(),
                side,
                quantity: p.quantity.abs(),
                entry_price: p.entry_price,
                mark_price: p.mark_price.unwrap_or(p.entry_price),
                unrealized_pnl: p.unrealized_pnl,
                liquidation_price: p.liquidation_price,
                leverage: p.leverage,
            });
        }

        if let Some(sel) = self.selected {
            if sel >= self.positions.len() {
                self.selected = self.positions.len().checked_sub(1);
            }
        }

        self.total_unrealized_pnl = self.positions.iter().map(|p| p.unrealized_pnl).sum();
    }

    /// Get visible positions for rendering
    pub fn visible_positions(&self, scroll_offset: usize, max_rows: usize) -> &[Position] {
        let end = (scroll_offset + max_rows).min(self.positions.len());
        &self.positions[scroll_offset..end]
    }

    /// Format position field for display
    pub fn format_position(&self, pos: &Position, field: PositionField) -> String {
        match field {
            PositionField::Symbol => pos.symbol.clone(),
            PositionField::Side => match pos.side {
                PositionSide::Long => "LONG".to_string(),
                PositionSide::Short => "SHORT".to_string(),
            },
            PositionField::Quantity => format!("{:.4}", pos.quantity),
            PositionField::EntryPrice => format!("{:.4}", pos.entry_price),
            PositionField::MarkPrice => format!("{:.4}", pos.mark_price),
            PositionField::UnrealizedPnL => format!("{:+.2}", pos.unrealized_pnl),
            PositionField::Leverage => format!("{}x", pos.leverage),
            PositionField::LiqPrice => {
                pos.liquidation_price
                    .map(|p| format!("{:.4}", p))
                    .unwrap_or_else(|| "—".to_string())
            }
        }
    }

    /// Get color based on PnL
    pub fn pnl_color(&self, pos: &Position) -> [f32; 4] {
        if pos.unrealized_pnl > 0.0 {
            [0.2, 0.8, 0.3, 1.0] // green
        } else if pos.unrealized_pnl < 0.0 {
            [0.9, 0.2, 0.2, 1.0] // red
        } else {
            [0.6, 0.6, 0.7, 1.0] // neutral
        }
    }

    /// Get risk warning level based on position metrics
    pub fn risk_level(&self, pos: &Position) -> RiskLevel {
        if let Some(liq_price) = pos.liquidation_price {
            let distance_pct = ((pos.mark_price - liq_price) / pos.mark_price).abs() * 100.0;
            if distance_pct < 5.0 {
                RiskLevel::High
            } else if distance_pct < 15.0 {
                RiskLevel::Medium
            } else {
                RiskLevel::Low
            }
        } else {
            RiskLevel::Low
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub enum PositionField {
    Symbol,
    Side,
    Quantity,
    EntryPrice,
    MarkPrice,
    UnrealizedPnL,
    Leverage,
    LiqPrice,
}

#[derive(Clone, Debug, Copy, PartialEq)]
pub enum RiskLevel {
    Low,
    Medium,
    High,
}

const PM_HEADER_HEIGHT: f32 = 20.0;
const PM_ROW_HEIGHT: f32 = 20.0;
const PM_SUMMARY_HEIGHT: f32 = 20.0;
const PM_LEFT_PAD: f32 = 6.0;

impl TradingPanel for PositionManagerState {
    fn kind(&self) -> &'static str { "position_manager" }
    fn label(&self) -> &'static str { "Positions" }

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
        {
            let body_id = format!("{}:position_manager:body", slot_prefix);
            coordinator.register(
                body_id.as_str(),
                uzor::Rect::new(x as f64, y as f64, w as f64, h as f64),
                uzor::input::Sense::SCROLL,
            );
        }

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        let sym_w   = (w * 0.14).max(52.0);
        let side_w  = (w * 0.08).max(38.0);
        let qty_w   = (w * 0.10).max(44.0);
        let entry_w = (w * 0.14).max(56.0);
        let mark_w  = (w * 0.14).max(56.0);
        let pnl_w   = (w * 0.14).max(52.0);
        let liq_w   = (w * 0.14).max(52.0);

        let col_sym_x   = x + PM_LEFT_PAD;
        let col_side_x  = col_sym_x  + sym_w;
        let col_qty_x   = col_side_x + side_w;
        let col_entry_x = col_qty_x  + qty_w;
        let col_mark_x  = col_entry_x + entry_w;
        let col_pnl_x   = col_mark_x + mark_w;
        let col_liq_x   = col_pnl_x  + pnl_w;
        let col_lev_x   = col_liq_x  + liq_w;

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, PM_HEADER_HEIGHT as f64);

        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&theme.text_header);

        let header_mid_y = (y + PM_HEADER_HEIGHT / 2.0) as f64;
        ctx.fill_text("SYMBOL", col_sym_x   as f64, header_mid_y);
        ctx.fill_text("SIDE",   col_side_x  as f64, header_mid_y);
        ctx.fill_text("QTY",    col_qty_x   as f64, header_mid_y);
        ctx.fill_text("ENTRY",  col_entry_x as f64, header_mid_y);
        ctx.fill_text("MARK",   col_mark_x  as f64, header_mid_y);
        ctx.fill_text("PNL",    col_pnl_x   as f64, header_mid_y);
        ctx.fill_text("LIQ",    col_liq_x   as f64, header_mid_y);
        ctx.fill_text("LEV",    col_lev_x   as f64, header_mid_y);

        if self.positions.is_empty() {
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(&theme.text_header);
            ctx.fill_text("No open positions", (x + w / 2.0) as f64, (y + h / 2.0) as f64);
            return;
        }

        let content_h = h - PM_HEADER_HEIGHT - PM_SUMMARY_HEIGHT;
        let max_rows  = (content_h / PM_ROW_HEIGHT).floor() as usize;
        let visible   = self.visible_positions(self.scroll_offset, max_rows);

        for (row_idx, pos) in visible.iter().enumerate() {
            let row_y     = y + PM_HEADER_HEIGHT + (row_idx as f32 * PM_ROW_HEIGHT);
            let row_mid_y = (row_y + PM_ROW_HEIGHT / 2.0) as f64;

            let is_selected = self.selected == Some(row_idx);
            let row_bg = if is_selected { &theme.selected } else { &theme.panel_bg };
            ctx.set_fill_color(row_bg);
            ctx.fill_rect(x as f64, row_y as f64, w as f64, PM_ROW_HEIGHT as f64);

            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);

            ctx.set_fill_color(&theme.text_primary);
            ctx.fill_text(&pos.symbol, col_sym_x as f64, row_mid_y);

            let (side_text, side_color) = match pos.side {
                PositionSide::Long  => ("LONG",  &theme.pm_long),
                PositionSide::Short => ("SHORT", &theme.pm_short),
            };
            ctx.set_fill_color(side_color);
            ctx.fill_text(side_text, col_side_x as f64, row_mid_y);

            let qty_str = format!("{:.4}", pos.quantity);
            ctx.set_fill_color(&theme.text_primary);
            ctx.fill_text(&qty_str, col_qty_x as f64, row_mid_y);

            let entry_str = format!("{:.4}", pos.entry_price);
            ctx.fill_text(&entry_str, col_entry_x as f64, row_mid_y);

            let mark_str = format!("{:.4}", pos.mark_price);
            ctx.fill_text(&mark_str, col_mark_x as f64, row_mid_y);

            let pnl_color = if pos.unrealized_pnl > 0.0 { &theme.pm_pnl_positive }
                else if pos.unrealized_pnl < 0.0 { &theme.pm_pnl_negative }
                else { &theme.pm_pnl_neutral };
            let pnl_str = format!("{:+.2}", pos.unrealized_pnl);
            ctx.set_fill_color(pnl_color);
            ctx.fill_text(&pnl_str, col_pnl_x as f64, row_mid_y);

            let liq_str = pos.liquidation_price
                .map(|p| format!("{:.4}", p))
                .unwrap_or_else(|| "--".to_string());
            ctx.set_fill_color(&theme.pm_liquidation);
            ctx.fill_text(&liq_str, col_liq_x as f64, row_mid_y);

            let lev_str = format!("{}x", pos.leverage);
            ctx.set_fill_color(&theme.text_primary);
            ctx.fill_text(&lev_str, col_lev_x as f64, row_mid_y);

            ctx.set_fill_color(&theme.separator);
            ctx.fill_rect(x as f64, (row_y + PM_ROW_HEIGHT - 1.0) as f64, w as f64, 1.0);
        }

        let summary_y = y + h - PM_SUMMARY_HEIGHT;
        ctx.set_fill_color(&theme.pm_summary_bg);
        ctx.fill_rect(x as f64, summary_y as f64, w as f64, PM_SUMMARY_HEIGHT as f64);

        ctx.set_fill_color(&theme.separator);
        ctx.fill_rect(x as f64, summary_y as f64, w as f64, 1.0);

        let summary_mid_y = (summary_y + PM_SUMMARY_HEIGHT / 2.0) as f64;
        ctx.set_font("10px monospace");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.set_fill_color(&theme.text_header);
        ctx.fill_text("Total PnL:", (x + PM_LEFT_PAD) as f64, summary_mid_y);

        let total_pnl = self.total_unrealized_pnl;
        let total_color = if total_pnl > 0.0 { &theme.pm_pnl_positive }
            else if total_pnl < 0.0 { &theme.pm_pnl_negative }
            else { &theme.pm_pnl_neutral };
        let total_str = format!("{:+.2}", total_pnl);
        ctx.set_fill_color(total_color);
        ctx.fill_text(&total_str, (x + PM_LEFT_PAD + 70.0) as f64, summary_mid_y);
    }

    fn handle_click(&mut self, _local_id: &str, _x: f64, _y: f64) -> bool { false }

    fn handle_scroll(&mut self, local_id: &str, _dx: f64, dy: f64) -> bool {
        if local_id == "position_manager:body" {
            let delta = if dy < 0.0 { -1i64 } else if dy > 0.0 { 1i64 } else { 0 };
            let new_offset = (self.scroll_offset as i64 + delta).max(0) as usize;
            let max = self.positions.len().saturating_sub(1);
            self.scroll_offset = new_offset.min(max);
            true
        } else {
            false
        }
    }
}

/// PositionManager panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionManagerConfig {
    /// Show percentage PnL
    pub show_pnl_percent: bool,
    /// Show liquidation price
    pub show_liq_price: bool,
    /// Quick close confirmation
    pub require_close_confirmation: bool,
    /// Risk warning threshold (% of margin)
    pub risk_warning_threshold: f64,
}

/// PositionManager panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PositionManagerPanel {
    id: PositionManagerId,
    title: String,
}

impl PositionManagerPanel {
    pub fn new(id: PositionManagerId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PositionManagerId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "position_manager" }
    pub fn kind_label(&self) -> &'static str { "Positions" }
    pub fn min_size(&self) -> (f32, f32) { (100.0, 80.0) }
}
