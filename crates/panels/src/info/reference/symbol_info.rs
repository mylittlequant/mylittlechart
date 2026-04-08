use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SymbolInfoId(pub u64);

#[derive(Clone, Debug)]
pub struct SymbolInfoState {
    /// Symbol being displayed
    pub symbol: String,
    /// Symbol metadata
    pub info: Option<SymbolInfo>,
    /// Current price
    pub current_price: Option<f64>,
    /// Trading session status
    pub session_status: Option<SessionStatus>,
}

#[derive(Clone, Debug)]
pub struct SymbolInfo {
    pub symbol: String,
    pub base_asset: String,
    pub quote_asset: String,
    pub exchange: String,
    pub contract_type: ContractType,
    pub tick_size: f64,
    pub lot_size: f64,
    pub min_order_size: f64,
    pub max_order_size: Option<f64>,
    pub maker_fee: f64,
    pub taker_fee: f64,
    pub listing_date: Option<i64>,
    pub status: String,
}

#[derive(Clone, Debug)]
pub enum ContractType {
    Spot,
    Future,
    Perpetual,
    Option,
}

#[derive(Clone, Debug)]
pub enum SessionStatus {
    Open,
    Closed,
    PreMarket,
    AfterHours,
}

impl SymbolInfoState {
    pub fn new() -> Self {
        Self {
            symbol: String::new(),
            info: None,
            current_price: None,
            session_status: None,
        }
    }

    /// Get list of info rows for rendering
    pub fn info_rows(&self) -> Vec<(&'static str, String)> {
        let mut rows = vec![("Symbol", self.symbol.clone())];

        if let Some(price) = self.current_price {
            rows.push(("Current Price", format!("{:.4}", price)));
        }

        if let Some(ref status) = self.session_status {
            rows.push(("Session", self.format_session(status)));
        }

        if let Some(ref info) = self.info {
            rows.push(("Exchange", info.exchange.clone()));
            rows.push(("Type", format!("{:?}", info.contract_type)));
            rows.push(("Base/Quote", format!("{}/{}", info.base_asset, info.quote_asset)));
            rows.push(("Tick Size", format!("{:.8}", info.tick_size)));
            rows.push(("Lot Size", format!("{:.8}", info.lot_size)));
            rows.push(("Min Order", format!("{:.8}", info.min_order_size)));
            rows.push(("Maker Fee", format!("{:.4}%", info.maker_fee * 100.0)));
            rows.push(("Taker Fee", format!("{:.4}%", info.taker_fee * 100.0)));
            rows.push(("Status", info.status.clone()));
        }

        rows
    }

    fn format_session(&self, status: &SessionStatus) -> String {
        match status {
            SessionStatus::Open => "Open".to_string(),
            SessionStatus::Closed => "Closed".to_string(),
            SessionStatus::PreMarket => "Pre-Market".to_string(),
            SessionStatus::AfterHours => "After Hours".to_string(),
        }
    }

    /// Get color for session status
    pub fn status_color(&self) -> [f32; 4] {
        if let Some(ref status) = self.session_status {
            match status {
                SessionStatus::Open => [0.2, 0.8, 0.3, 1.0],        // green
                SessionStatus::Closed => [0.5, 0.5, 0.5, 1.0],      // gray
                SessionStatus::PreMarket => [0.9, 0.7, 0.2, 1.0],   // yellow
                SessionStatus::AfterHours => [0.9, 0.5, 0.2, 1.0],  // orange
            }
        } else {
            [0.6, 0.6, 0.7, 1.0]
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolInfoConfig {
    /// Show advanced fields
    pub show_advanced: bool,
    /// Show fee information
    pub show_fees: bool,
    /// Fetch company info (for stocks)
    pub fetch_company_info: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SymbolInfoPanel {
    id: SymbolInfoId,
    title: String,
}

impl SymbolInfoPanel {
    pub fn new(id: SymbolInfoId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> SymbolInfoId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "symbol_info"
    }

    pub fn kind_label(&self) -> &'static str {
        "Symbol Info"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
