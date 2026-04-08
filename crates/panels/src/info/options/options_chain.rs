use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct OptionsChainId(pub u64);

#[derive(Clone, Debug)]
pub struct OptionsChainState {
    /// Underlying symbol
    pub underlying: String,
    /// Selected expiration date
    pub expiration: String,
    /// All option contracts for this expiration
    pub chain: Vec<OptionContract>,
    /// ATM strike
    pub atm_strike: f64,
    /// Greeks calculation enabled
    pub show_greeks: bool,
}

#[derive(Clone, Debug)]
pub struct OptionContract {
    pub symbol: String,
    pub strike: f64,
    pub option_type: OptionType,
    pub last_price: f64,
    pub bid: f64,
    pub ask: f64,
    pub volume: u64,
    pub open_interest: u64,
    pub implied_volatility: f64,
    pub delta: Option<f64>,
    pub gamma: Option<f64>,
    pub theta: Option<f64>,
    pub vega: Option<f64>,
    pub expiration: String,
}

#[derive(Clone, Debug)]
pub enum OptionType {
    Call,
    Put,
}

impl OptionsChainState {
    pub fn new() -> Self {
        Self {
            underlying: String::new(),
            expiration: String::new(),
            chain: Vec::new(),
            atm_strike: 0.0,
            show_greeks: true,
        }
    }

    /// Get visible contracts for rendering
    pub fn visible_contracts(&self, scroll_offset: usize, max_rows: usize) -> &[OptionContract] {
        let end = (scroll_offset + max_rows).min(self.chain.len());
        &self.chain[scroll_offset..end]
    }

    /// Format contract field for display
    pub fn format_contract(&self, contract: &OptionContract, field: ContractField) -> String {
        match field {
            ContractField::Strike => format!("{:.2}", contract.strike),
            ContractField::Type => match contract.option_type {
                OptionType::Call => "CALL".to_string(),
                OptionType::Put => "PUT".to_string(),
            },
            ContractField::Last => format!("{:.2}", contract.last_price),
            ContractField::Bid => format!("{:.2}", contract.bid),
            ContractField::Ask => format!("{:.2}", contract.ask),
            ContractField::Volume => format!("{}", contract.volume),
            ContractField::OpenInterest => format!("{}", contract.open_interest),
            ContractField::IV => format!("{:.1}%", contract.implied_volatility * 100.0),
            ContractField::Delta => contract.delta.map(|d| format!("{:.3}", d)).unwrap_or_else(|| "—".to_string()),
            ContractField::Gamma => contract.gamma.map(|g| format!("{:.4}", g)).unwrap_or_else(|| "—".to_string()),
            ContractField::Theta => contract.theta.map(|t| format!("{:.4}", t)).unwrap_or_else(|| "—".to_string()),
            ContractField::Vega => contract.vega.map(|v| format!("{:.4}", v)).unwrap_or_else(|| "—".to_string()),
        }
    }

    /// Get color based on moneyness (ITM/ATM/OTM)
    pub fn itm_color(&self, contract: &OptionContract) -> [f32; 4] {
        let diff = (contract.strike - self.atm_strike).abs();
        if diff < 0.01 {
            [0.9, 0.7, 0.2, 1.0] // yellow - ATM
        } else {
            let is_itm = match contract.option_type {
                OptionType::Call => contract.strike < self.atm_strike,
                OptionType::Put => contract.strike > self.atm_strike,
            };
            if is_itm {
                [0.2, 0.8, 0.3, 1.0] // green - ITM
            } else {
                [0.6, 0.6, 0.7, 1.0] // neutral - OTM
            }
        }
    }
}

#[derive(Clone, Debug, Copy)]
pub enum ContractField {
    Strike,
    Type,
    Last,
    Bid,
    Ask,
    Volume,
    OpenInterest,
    IV,
    Delta,
    Gamma,
    Theta,
    Vega,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptionsChainConfig {
    /// Show extended Greeks (Rho, Vanna, etc.)
    pub show_extended_greeks: bool,
    /// Strike range (ATM ± N strikes)
    pub strike_range: usize,
    /// Highlight ITM/ATM/OTM
    pub highlight_moneyness: bool,
    /// Show volume bars
    pub show_volume_bars: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct OptionsChainPanel {
    id: OptionsChainId,
    title: String,
}

impl OptionsChainPanel {
    pub fn new(id: OptionsChainId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> OptionsChainId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "options_chain"
    }

    pub fn kind_label(&self) -> &'static str {
        "Options Chain"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
