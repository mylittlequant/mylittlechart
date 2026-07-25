use serde::{Serialize, Deserialize};
use mlc_backend_trading_engine::SharedTradingSnapshot;

use crate::panel_trait::TradingPanel;
use crate::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::i18n::{TradingKey, current_language};

/// RiskCalculator panel ID
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RiskCalculatorId(pub u64);

/// Which editable field is currently being typed into (W5 quality arc,
/// 2026-07-25 — same panel-edit pattern as `OrderEntryElement`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RiskCalcField {
    AccountSize,
    RiskPercent,
    EntryPrice,
    StopLoss,
    TakeProfit,
}

/// RiskCalculator panel state (heavy data)
#[derive(Clone, Debug)]
pub struct RiskCalculatorState {
    /// Input fields
    pub account_size: f64,
    /// True once the user has typed an Account Size — after that,
    /// `sync_from_snapshot` stops overwriting it from the live balance.
    pub account_size_overridden: bool,
    pub risk_percent: f64,       // e.g., 2.0 for 2%
    pub entry_price: f64,
    /// True once the user has typed an Entry Price — after that,
    /// `sync_reference_price` stops seeding it from the BBO.
    pub entry_price_touched: bool,
    pub stop_loss_price: f64,
    pub take_profit_price: Option<f64>,

    /// Calculated outputs
    pub risk_amount: f64,        // account_size * (risk_percent / 100)
    pub position_size: f64,      // risk_amount / risk_per_unit
    pub risk_per_unit: f64,      // abs(entry_price - stop_loss_price)
    pub potential_profit: Option<f64>, // (take_profit_price - entry_price) * position_size
    pub risk_reward_ratio: Option<f64>, // potential_profit / risk_amount

    /// Leverage (optional)
    pub leverage: Option<u32>,
    pub margin_required: f64,

    /// Validation
    pub errors: Vec<String>,

    /// Currently active/editing field (`None` = no field being edited).
    pub editing_field: Option<RiskCalcField>,
    /// Inline text editing state (mirrors `OrderEntryState`).
    pub editing_text: String,
    pub editing_cursor: usize,
    pub editing_blink_time: u64,

    /// Shared snapshot from TradingManager — feeds the default Account Size
    /// (until overridden). Only ever set for the container-hosted instance;
    /// the standalone `RiskCalculator` panel has no session to bind and
    /// keeps its constructor default forever.
    pub snapshot: Option<SharedTradingSnapshot>,
    /// Reference price (container DOM mid) — seeds Entry Price on first
    /// arrival if the user hasn't typed one yet. Same wiring choice as
    /// `OrderEntryState::reference_price`.
    pub reference_price: Option<f64>,
    /// Set by the `"risk_calculator:apply"` click; read + cleared by the
    /// app layer, which writes `position_size` (and, for an empty Limit
    /// price, `entry_price`) into the container's sibling `OrderEntryState`.
    /// This panel has no access to another panel's state, hence the flag.
    pub apply_requested: bool,
}

impl RiskCalculatorState {
    pub fn new() -> Self {
        Self {
            account_size: 10000.0,
            account_size_overridden: false,
            risk_percent: 2.0,
            entry_price: 0.0,
            entry_price_touched: false,
            stop_loss_price: 0.0,
            take_profit_price: None,
            risk_amount: 0.0,
            position_size: 0.0,
            risk_per_unit: 0.0,
            potential_profit: None,
            risk_reward_ratio: None,
            leverage: None,
            margin_required: 0.0,
            errors: Vec::new(),
            editing_field: None,
            editing_text: String::new(),
            editing_cursor: 0,
            editing_blink_time: 0,
            snapshot: None,
            reference_price: None,
            apply_requested: false,
        }
    }

    /// True once every input required to compute risk_amount/position_size/
    /// risk_per_unit/margin_required is present and non-degenerate. Render
    /// uses this to show a dash instead of a fake `$0.00` for those rows
    /// when the inputs are incomplete.
    pub fn inputs_ready(&self) -> bool {
        self.account_size > 0.0
            && self.risk_percent > 0.0
            && self.entry_price > 0.0
            && self.stop_loss_price > 0.0
            && self.entry_price != self.stop_loss_price
    }

    /// Calculate all output fields from input fields. Always resets the
    /// computed fields first — an incomplete input set (e.g. entry unset)
    /// must never leave a stale computed value from a previous complete
    /// pass on screen; the render layer's own `inputs_ready()` check
    /// decides dash-vs-value, but the stored fields stay honest either way.
    pub fn calculate(&mut self) {
        self.errors.clear();
        self.risk_amount = 0.0;
        self.position_size = 0.0;
        self.risk_per_unit = 0.0;
        self.margin_required = 0.0;
        self.potential_profit = None;
        self.risk_reward_ratio = None;

        if self.account_size <= 0.0 {
            self.errors.push("Account size must be positive".to_string());
            return;
        }
        if self.risk_percent <= 0.0 || self.risk_percent > 100.0 {
            self.errors.push("Risk percent must be between 0 and 100".to_string());
            return;
        }
        if self.entry_price <= 0.0 {
            self.errors.push("Entry price must be positive".to_string());
            return;
        }
        if self.stop_loss_price <= 0.0 {
            self.errors.push("Stop loss price must be positive".to_string());
            return;
        }

        self.risk_amount = self.account_size * (self.risk_percent / 100.0);
        self.risk_per_unit = (self.entry_price - self.stop_loss_price).abs();

        if self.risk_per_unit == 0.0 {
            self.errors.push("Entry and stop loss cannot be equal".to_string());
            self.risk_amount = 0.0;
            return;
        }

        self.position_size = self.risk_amount / self.risk_per_unit;

        self.margin_required = match self.leverage {
            Some(lev) if lev > 0 => (self.position_size * self.entry_price) / lev as f64,
            _ => self.position_size * self.entry_price,
        };

        if let Some(tp) = self.take_profit_price {
            if tp > 0.0 {
                let profit = (tp - self.entry_price).abs() * self.position_size;
                self.potential_profit = Some(profit);
                self.risk_reward_ratio = Some(profit / self.risk_amount);
            }
        }
    }

    /// Format output value as string
    pub fn format_output(&self, field: &str) -> String {
        match field {
            "risk_amount" => format!("${:.2}", self.risk_amount),
            "position_size" => format!("{:.4}", self.position_size),
            "risk_per_unit" => format!("${:.2}", self.risk_per_unit),
            "margin_required" => format!("${:.2}", self.margin_required),
            "potential_profit" => {
                if let Some(profit) = self.potential_profit {
                    format!("${:.2}", profit)
                } else {
                    "\u{2014}".to_string()
                }
            }
            "risk_reward_ratio" => {
                if let Some(rr) = self.risk_reward_ratio {
                    format!("1:{:.2}", rr)
                } else {
                    "\u{2014}".to_string()
                }
            }
            _ => "\u{2014}".to_string(),
        }
    }

    /// Get color for risk:reward ratio display
    pub fn risk_color(&self) -> [f32; 4] {
        if let Some(rr) = self.risk_reward_ratio {
            if rr >= 2.0 {
                [0.0, 0.8, 0.0, 1.0] // Green (good R:R)
            } else if rr >= 1.0 {
                [0.8, 0.8, 0.0, 1.0] // Yellow (acceptable)
            } else {
                [0.8, 0.0, 0.0, 1.0] // Red (poor R:R)
            }
        } else {
            [0.5, 0.5, 0.5, 1.0] // Gray (not set)
        }
    }

    /// Attach shared snapshot from TradingManager (container-hosted instance only).
    pub fn set_snapshot(&mut self, snap: SharedTradingSnapshot) {
        self.snapshot = Some(snap);
    }

    /// Pull the default Account Size from the live balance, unless the user
    /// has already typed an override.
    pub fn sync_from_snapshot(&mut self) {
        if self.account_size_overridden {
            return;
        }
        let snap = match &self.snapshot {
            Some(s) => match s.read() {
                Ok(guard) => guard,
                Err(_) => return,
            },
            None => return,
        };
        let balance = snap.balances.iter().map(|b| b.free).fold(0.0_f64, f64::max);
        drop(snap);
        if balance > 0.0 {
            self.account_size = balance;
            self.calculate();
        }
    }

    /// Push a fresh reference price (container DOM mid) in — seeds Entry
    /// Price on first arrival if the user hasn't typed one yet (same
    /// wiring choice as `OrderEntryState::reference_price`).
    pub fn sync_reference_price(&mut self, price: Option<f64>) {
        self.reference_price = price;
        if !self.entry_price_touched {
            if let Some(p) = price {
                if p > 0.0 {
                    self.entry_price = p;
                    self.calculate();
                }
            }
        }
    }

    /// True while a field is actively being edited.
    pub fn is_editing(&self) -> bool {
        self.editing_field.is_some()
    }

    /// Check if cursor should be visible (500ms blink cycle).
    pub fn is_cursor_visible(&self, now_ms: u64) -> bool {
        if self.editing_field.is_none() { return false; }
        let elapsed = now_ms.wrapping_sub(self.editing_blink_time);
        (elapsed / 500) % 2 == 0
    }

    /// Insert a character at the cursor of the field currently being
    /// edited. Only digits and a single '.' are accepted.
    pub fn handle_edit_char(&mut self, ch: char) -> bool {
        if self.editing_field.is_none() {
            return false;
        }
        if ch.is_ascii_digit() {
            self.editing_text.insert(self.editing_cursor, ch);
            self.editing_cursor += 1;
        } else if ch == '.' && !self.editing_text.contains('.') {
            self.editing_text.insert(self.editing_cursor, ch);
            self.editing_cursor += 1;
        }
        true
    }

    /// Handle a named key while a field is being edited.
    pub fn handle_edit_key(&mut self, key: mlc_core::input::KeyCode) -> bool {
        use mlc_core::input::KeyCode;
        if self.editing_field.is_none() {
            return false;
        }
        match key {
            KeyCode::Backspace => {
                if self.editing_cursor > 0 {
                    self.editing_cursor -= 1;
                    self.editing_text.remove(self.editing_cursor);
                }
                true
            }
            KeyCode::Delete => {
                if self.editing_cursor < self.editing_text.len() {
                    self.editing_text.remove(self.editing_cursor);
                }
                true
            }
            KeyCode::ArrowLeft => {
                self.editing_cursor = self.editing_cursor.saturating_sub(1);
                true
            }
            KeyCode::ArrowRight => {
                self.editing_cursor = (self.editing_cursor + 1).min(self.editing_text.len());
                true
            }
            KeyCode::Enter => {
                self.commit_editing();
                true
            }
            KeyCode::Escape => {
                self.cancel_editing();
                true
            }
            _ => false,
        }
    }

    /// Commit the currently-edited field: parse `editing_text` as f64 and
    /// write it into the matching field. On parse failure the field stays
    /// in edit mode (no partial commit).
    pub fn commit_editing(&mut self) {
        let Some(field) = self.editing_field else { return };
        let Ok(value) = self.editing_text.parse::<f64>() else { return };
        match field {
            RiskCalcField::AccountSize => {
                self.account_size = value;
                self.account_size_overridden = true;
            }
            RiskCalcField::RiskPercent => self.risk_percent = value,
            RiskCalcField::EntryPrice => {
                self.entry_price = value;
                self.entry_price_touched = true;
            }
            RiskCalcField::StopLoss => self.stop_loss_price = value,
            RiskCalcField::TakeProfit => self.take_profit_price = Some(value),
        }
        self.editing_field = None;
        self.editing_text.clear();
        self.editing_cursor = 0;
        self.calculate();
    }

    /// Cancel the currently-edited field, discarding the typed text.
    pub fn cancel_editing(&mut self) {
        self.editing_field = None;
        self.editing_text.clear();
        self.editing_cursor = 0;
    }

    /// Begin editing `field`, auto-committing whatever was being edited
    /// before (mirrors `OrderEntryState`'s per-field click handlers).
    fn start_edit(&mut self, field: RiskCalcField, current: f64, now_ms: u64) {
        self.commit_editing();
        self.editing_field = Some(field);
        self.editing_text = if current > 0.0 { format!("{}", current) } else { String::new() };
        self.editing_cursor = self.editing_text.len();
        self.editing_blink_time = now_ms;
    }

    /// Render-time display string for whichever field is being edited: the
    /// raw `editing_text` with a blinking cursor spliced in (visible half
    /// of the 500ms blink cycle).
    fn display_editing_text(&self, now_ms: u64) -> String {
        if self.is_cursor_visible(now_ms) {
            let mut s = self.editing_text.clone();
            let at = self.editing_cursor.min(s.len());
            s.insert(at, '|');
            s
        } else {
            self.editing_text.clone()
        }
    }
}

impl Default for RiskCalculatorState {
    fn default() -> Self {
        Self::new()
    }
}

const RC_TITLE_HEIGHT: f32 = 20.0;
const RC_ROW_HEIGHT: f32 = 20.0;
const RC_LEFT_PAD: f32 = 8.0;
const RC_APPLY_HEIGHT: f32 = 24.0;

impl TradingPanel for RiskCalculatorState {
    fn kind(&self) -> &'static str { "risk_calculator" }
    fn label(&self) -> &'static str { "Risk Calculator" }

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

        ctx.set_fill_color(&theme.panel_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, h as f64);

        ctx.set_fill_color(&theme.header_bg);
        ctx.fill_rect(x as f64, y as f64, w as f64, RC_TITLE_HEIGHT as f64);

        ctx.set_fill_color(&theme.text_header);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(TradingKey::RiskCalculator.get(current_language()), (x + w / 2.0) as f64, (y + RC_TITLE_HEIGHT / 2.0) as f64);

        let mut cursor_y = y + RC_TITLE_HEIGHT;
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let field_value_right = x + w - RC_LEFT_PAD;
        let draw_row = |ctx: &mut dyn RenderContext, row_y: f32, label: &str, value: &str, editing: bool| {
            ctx.set_fill_color(if editing { &theme.rc_input_bg } else { &theme.panel_bg });
            ctx.fill_rect(x as f64, row_y as f64, w as f64, RC_ROW_HEIGHT as f64);
            if editing {
                ctx.set_fill_color(&theme.rc_input_border);
                ctx.fill_rect(x as f64, (row_y + RC_ROW_HEIGHT - 1.0) as f64, w as f64, 1.0);
            }
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, (x + RC_LEFT_PAD) as f64, (row_y + RC_ROW_HEIGHT / 2.0) as f64);
            ctx.set_fill_color(&theme.text_primary);
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(value, field_value_right as f64, (row_y + RC_ROW_HEIGHT / 2.0) as f64);
        };

        // --- Editable input rows ---
        let account_size_str = if self.editing_field == Some(RiskCalcField::AccountSize) {
            self.display_editing_text(now_ms)
        } else {
            format!("${:.2}", self.account_size)
        };
        draw_row(ctx, cursor_y, "Account Size:", &account_size_str, self.editing_field == Some(RiskCalcField::AccountSize));
        coordinator.register(
            format!("{}:risk_calculator:account_size", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, cursor_y as f64, w as f64, RC_ROW_HEIGHT as f64),
            Sense::CLICK,
        );
        cursor_y += RC_ROW_HEIGHT;

        let risk_percent_str = if self.editing_field == Some(RiskCalcField::RiskPercent) {
            self.display_editing_text(now_ms)
        } else {
            format!("{:.1}%", self.risk_percent)
        };
        draw_row(ctx, cursor_y, "Risk %:", &risk_percent_str, self.editing_field == Some(RiskCalcField::RiskPercent));
        coordinator.register(
            format!("{}:risk_calculator:risk_percent", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, cursor_y as f64, w as f64, RC_ROW_HEIGHT as f64),
            Sense::CLICK,
        );
        cursor_y += RC_ROW_HEIGHT;

        let entry_price_str = if self.editing_field == Some(RiskCalcField::EntryPrice) {
            self.display_editing_text(now_ms)
        } else {
            format!("{:.4}", self.entry_price)
        };
        draw_row(ctx, cursor_y, "Entry Price:", &entry_price_str, self.editing_field == Some(RiskCalcField::EntryPrice));
        coordinator.register(
            format!("{}:risk_calculator:entry_price", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, cursor_y as f64, w as f64, RC_ROW_HEIGHT as f64),
            Sense::CLICK,
        );
        cursor_y += RC_ROW_HEIGHT;

        let stop_loss_str = if self.editing_field == Some(RiskCalcField::StopLoss) {
            self.display_editing_text(now_ms)
        } else {
            format!("{:.4}", self.stop_loss_price)
        };
        draw_row(ctx, cursor_y, "Stop Loss:", &stop_loss_str, self.editing_field == Some(RiskCalcField::StopLoss));
        coordinator.register(
            format!("{}:risk_calculator:stop_loss", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, cursor_y as f64, w as f64, RC_ROW_HEIGHT as f64),
            Sense::CLICK,
        );
        cursor_y += RC_ROW_HEIGHT;

        let take_profit_str = if self.editing_field == Some(RiskCalcField::TakeProfit) {
            self.display_editing_text(now_ms)
        } else {
            self.take_profit_price.map(|tp| format!("{:.4}", tp)).unwrap_or_else(|| "\u{2014}".to_string())
        };
        draw_row(ctx, cursor_y, "Take Profit:", &take_profit_str, self.editing_field == Some(RiskCalcField::TakeProfit));
        coordinator.register(
            format!("{}:risk_calculator:take_profit", slot_prefix).as_str(),
            uzor::Rect::new(x as f64, cursor_y as f64, w as f64, RC_ROW_HEIGHT as f64),
            Sense::CLICK,
        );
        cursor_y += RC_ROW_HEIGHT;

        ctx.set_fill_color(&theme.separator);
        ctx.fill_rect((x + RC_LEFT_PAD) as f64, cursor_y as f64, (w - RC_LEFT_PAD * 2.0) as f64, 1.0);
        cursor_y += 6.0;

        // --- Computed output rows — dash when inputs incomplete, never a
        // fake $0.00 (owner spec: "remove dead zero-rows look"). ---
        let ready = self.inputs_ready();
        let good_rr = self.risk_reward_ratio.map_or(false, |rr| rr >= 2.0);
        let leverage_str = self.leverage.map(|lev| format!("{}x", lev)).unwrap_or_else(|| "1x".to_string());

        // (label, value, color_key) — color_key: "risk", "profit", "good_rr", "default"
        let computed_rows: [(&str, String, &str); 6] = [
            ("Risk Amount:", if ready { self.format_output("risk_amount") } else { "\u{2014}".to_string() }, "risk"),
            ("Position Size:", if ready { self.format_output("position_size") } else { "\u{2014}".to_string() }, "default"),
            ("Risk/Unit:", if ready { self.format_output("risk_per_unit") } else { "\u{2014}".to_string() }, "default"),
            ("Potential Profit:", if ready { self.format_output("potential_profit") } else { "\u{2014}".to_string() }, "profit"),
            ("R:R Ratio:", if ready { self.format_output("risk_reward_ratio") } else { "\u{2014}".to_string() }, if good_rr { "good_rr" } else { "default" }),
            ("Margin Req:", if ready { self.format_output("margin_required") } else { "\u{2014}".to_string() }, "default"),
        ];

        for (label, value, color_key) in &computed_rows {
            if cursor_y + RC_ROW_HEIGHT > y + h - RC_APPLY_HEIGHT {
                break;
            }
            let row_mid_y = (cursor_y + RC_ROW_HEIGHT / 2.0) as f64;
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_font("10px monospace");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(label, (x + RC_LEFT_PAD) as f64, row_mid_y);

            let value_color = match *color_key {
                "risk"    => &theme.rc_risk,
                "profit"  => &theme.rc_profit,
                "good_rr" => &theme.rc_good_rr,
                _         => &theme.text_primary,
            };
            ctx.set_fill_color(value_color);
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(value, (x + w - RC_LEFT_PAD) as f64, row_mid_y);
            cursor_y += RC_ROW_HEIGHT;
        }
        // Leverage row is a static display (no leverage editor in this arc).
        if cursor_y + RC_ROW_HEIGHT <= y + h - RC_APPLY_HEIGHT {
            let row_mid_y = (cursor_y + RC_ROW_HEIGHT / 2.0) as f64;
            ctx.set_fill_color(&theme.text_muted);
            ctx.set_text_align(TextAlign::Left);
            ctx.fill_text("Leverage:", (x + RC_LEFT_PAD) as f64, row_mid_y);
            ctx.set_fill_color(&theme.text_primary);
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(&leverage_str, (x + w - RC_LEFT_PAD) as f64, row_mid_y);
            cursor_y += RC_ROW_HEIGHT;
        }

        // --- Apply size button ---
        let apply_y = (y + h - RC_APPLY_HEIGHT).max(cursor_y);
        if apply_y + RC_APPLY_HEIGHT <= y + h + 0.5 {
            let apply_bg = if ready { &theme.accent } else { &theme.oe_tab_inactive };
            ctx.set_fill_color(apply_bg);
            ctx.fill_rect((x + RC_LEFT_PAD) as f64, apply_y as f64, (w - RC_LEFT_PAD * 2.0) as f64, (RC_APPLY_HEIGHT - 4.0) as f64);
            ctx.set_fill_color(if ready { &theme.oe_buy_button_text } else { &theme.text_muted });
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Apply Size \u{2192} Order Entry", (x + w / 2.0) as f64, (apply_y + (RC_APPLY_HEIGHT - 4.0) / 2.0) as f64);
            coordinator.register(
                format!("{}:risk_calculator:apply", slot_prefix).as_str(),
                uzor::Rect::new((x + RC_LEFT_PAD) as f64, apply_y as f64, (w - RC_LEFT_PAD * 2.0) as f64, (RC_APPLY_HEIGHT - 4.0) as f64),
                Sense::CLICK,
            );
        }

        if !self.errors.is_empty() {
            ctx.set_fill_color(&theme.sell_bright);
            ctx.set_font("9px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            if let Some(error) = self.errors.first() {
                ctx.fill_text(error, (x + RC_LEFT_PAD) as f64, (y + h - 4.0) as f64);
            }
        }
    }

    fn handle_click(&mut self, local_id: &str, _x: f64, _y: f64) -> bool {
        let now_ms = web_time::SystemTime::now()
            .duration_since(web_time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        match local_id {
            "risk_calculator:account_size" => {
                self.start_edit(RiskCalcField::AccountSize, self.account_size, now_ms);
                true
            }
            "risk_calculator:risk_percent" => {
                self.start_edit(RiskCalcField::RiskPercent, self.risk_percent, now_ms);
                true
            }
            "risk_calculator:entry_price" => {
                self.start_edit(RiskCalcField::EntryPrice, self.entry_price, now_ms);
                true
            }
            "risk_calculator:stop_loss" => {
                self.start_edit(RiskCalcField::StopLoss, self.stop_loss_price, now_ms);
                true
            }
            "risk_calculator:take_profit" => {
                self.start_edit(RiskCalcField::TakeProfit, self.take_profit_price.unwrap_or(0.0), now_ms);
                true
            }
            "risk_calculator:apply" => {
                self.commit_editing();
                if self.inputs_ready() {
                    self.apply_requested = true;
                }
                true
            }
            _ => false,
        }
    }
}

/// RiskCalculator panel configuration
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskCalculatorConfig {
    /// Default risk percent
    pub default_risk_percent: f64,

    /// Max risk percent allowed
    pub max_risk_percent: f64,

    /// Show leverage fields
    pub show_leverage: bool,

    /// Color coding for R:R ratio
    pub good_rr_threshold: f64,  // e.g., 2.0 (1:2 or better)
}

impl Default for RiskCalculatorConfig {
    fn default() -> Self {
        Self {
            default_risk_percent: 2.0,
            max_risk_percent: 10.0,
            show_leverage: false,
            good_rr_threshold: 2.0,
        }
    }
}

/// RiskCalculator panel wrapper (lightweight, lives in PanelKind)
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RiskCalculatorPanel {
    id: RiskCalculatorId,
    title: String,
}

impl RiskCalculatorPanel {
    pub fn new(id: RiskCalculatorId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> RiskCalculatorId { self.id }
    pub fn title(&self) -> &str { &self.title }
    pub fn set_title(&mut self, title: String) { self.title = title; }

    pub fn type_id(&self) -> &'static str { "risk_calculator" }
    pub fn kind_label(&self) -> &'static str { "Risk Calculator" }
    pub fn min_size(&self) -> (f32, f32) { (100.0, 80.0) }
}
