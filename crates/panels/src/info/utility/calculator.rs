use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CalculatorId(pub u64);

/// Calculator mode
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum CalculatorMode {
    Basic,
    CompoundInterest,
    Margin,
    PipValue,
    LotSize,
    Fibonacci,
}

/// Configuration for calculator panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatorConfig {
    pub mode: CalculatorMode,
    pub decimal_places: u8,
    pub auto_calculate: bool,
    pub history_enabled: bool,
}

impl Default for CalculatorConfig {
    fn default() -> Self {
        Self {
            mode: CalculatorMode::Basic,
            decimal_places: 2,
            auto_calculate: true,
            history_enabled: true,
        }
    }
}

/// Calculator history entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalculatorHistoryEntry {
    pub timestamp: i64,
    pub mode: CalculatorMode,
    pub inputs: HashMap<String, String>,
    pub results: HashMap<String, f64>,
}

/// Calculator state
#[derive(Clone, Debug, Default)]
pub struct CalculatorState {
    pub mode: CalculatorMode,
    pub inputs: HashMap<String, String>,
    pub results: HashMap<String, f64>,
    pub last_error: Option<String>,
    pub history: Vec<CalculatorHistoryEntry>,
}

impl Default for CalculatorMode {
    fn default() -> Self {
        CalculatorMode::Basic
    }
}

impl CalculatorState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the current display value
    pub fn display_value(&self) -> &str {
        self.inputs.get("display").map(|s| s.as_str()).unwrap_or("0")
    }

    /// Get the current mode label
    pub fn mode_label(&self) -> &str {
        match self.mode {
            CalculatorMode::Basic => "Basic",
            CalculatorMode::CompoundInterest => "Compound Interest",
            CalculatorMode::Margin => "Margin",
            CalculatorMode::PipValue => "Pip Value",
            CalculatorMode::LotSize => "Lot Size",
            CalculatorMode::Fibonacci => "Fibonacci",
        }
    }

    /// Get input fields for the current mode (label, value pairs)
    pub fn input_fields(&self) -> Vec<(&str, &str)> {
        match self.mode {
            CalculatorMode::Basic => {
                vec![("Expression", self.inputs.get("expression").map(|s| s.as_str()).unwrap_or(""))]
            }
            CalculatorMode::CompoundInterest => {
                vec![
                    ("Principal", self.inputs.get("principal").map(|s| s.as_str()).unwrap_or("")),
                    ("Rate (%)", self.inputs.get("rate").map(|s| s.as_str()).unwrap_or("")),
                    ("Time (years)", self.inputs.get("time").map(|s| s.as_str()).unwrap_or("")),
                    ("Compounds/Year", self.inputs.get("compounds").map(|s| s.as_str()).unwrap_or("")),
                ]
            }
            CalculatorMode::Margin => {
                vec![
                    ("Position Size", self.inputs.get("position_size").map(|s| s.as_str()).unwrap_or("")),
                    ("Entry Price", self.inputs.get("entry_price").map(|s| s.as_str()).unwrap_or("")),
                    ("Leverage", self.inputs.get("leverage").map(|s| s.as_str()).unwrap_or("")),
                ]
            }
            CalculatorMode::PipValue => {
                vec![
                    ("Lot Size", self.inputs.get("lot_size").map(|s| s.as_str()).unwrap_or("")),
                    ("Pair", self.inputs.get("pair").map(|s| s.as_str()).unwrap_or("")),
                    ("Account Currency", self.inputs.get("account_currency").map(|s| s.as_str()).unwrap_or("")),
                ]
            }
            CalculatorMode::LotSize => {
                vec![
                    ("Account Balance", self.inputs.get("balance").map(|s| s.as_str()).unwrap_or("")),
                    ("Risk %", self.inputs.get("risk_pct").map(|s| s.as_str()).unwrap_or("")),
                    ("Stop Loss (pips)", self.inputs.get("stop_loss_pips").map(|s| s.as_str()).unwrap_or("")),
                ]
            }
            CalculatorMode::Fibonacci => {
                vec![
                    ("High", self.inputs.get("high").map(|s| s.as_str()).unwrap_or("")),
                    ("Low", self.inputs.get("low").map(|s| s.as_str()).unwrap_or("")),
                ]
            }
        }
    }

    /// Get result fields (label, formatted value pairs)
    pub fn result_fields(&self) -> Vec<(&str, String)> {
        match self.mode {
            CalculatorMode::Basic => {
                if let Some(&result) = self.results.get("result") {
                    vec![("Result", format!("{:.2}", result))]
                } else {
                    vec![]
                }
            }
            CalculatorMode::CompoundInterest => {
                let mut fields = Vec::new();
                if let Some(&total) = self.results.get("total") {
                    fields.push(("Total Amount", format!("{:.2}", total)));
                }
                if let Some(&interest) = self.results.get("interest") {
                    fields.push(("Interest Earned", format!("{:.2}", interest)));
                }
                fields
            }
            CalculatorMode::Margin => {
                let mut fields = Vec::new();
                if let Some(&margin) = self.results.get("margin") {
                    fields.push(("Required Margin", format!("{:.2}", margin)));
                }
                if let Some(&value) = self.results.get("position_value") {
                    fields.push(("Position Value", format!("{:.2}", value)));
                }
                fields
            }
            CalculatorMode::PipValue => {
                if let Some(&value) = self.results.get("pip_value") {
                    vec![("Pip Value", format!("{:.2}", value))]
                } else {
                    vec![]
                }
            }
            CalculatorMode::LotSize => {
                if let Some(&lots) = self.results.get("lot_size") {
                    vec![("Lot Size", format!("{:.2}", lots))]
                } else {
                    vec![]
                }
            }
            CalculatorMode::Fibonacci => {
                let mut fields = Vec::new();
                for level in &["0.0", "23.6", "38.2", "50.0", "61.8", "100.0"] {
                    if let Some(&value) = self.results.get(*level) {
                        fields.push((*level, format!("{:.5}", value)));
                    }
                }
                fields
            }
        }
    }

    /// Get history entries
    pub fn history_entries(&self) -> &[CalculatorHistoryEntry] {
        &self.history
    }

    /// Format a single result value
    pub fn format_result(&self, key: &str) -> String {
        self.results.get(key)
            .map(|&val| format!("{:.2}", val))
            .unwrap_or_else(|| "—".to_string())
    }

    /// Format history entry for display
    pub fn format_history(&self, entry: &CalculatorHistoryEntry) -> String {
        let mode = match entry.mode {
            CalculatorMode::Basic => "Basic",
            CalculatorMode::CompoundInterest => "Compound Interest",
            CalculatorMode::Margin => "Margin",
            CalculatorMode::PipValue => "Pip Value",
            CalculatorMode::LotSize => "Lot Size",
            CalculatorMode::Fibonacci => "Fibonacci",
        };

        let timestamp = format_timestamp(entry.timestamp);

        let result_summary = if let Some(&result) = entry.results.get("result") {
            format!("{:.2}", result)
        } else {
            entry.results.keys().next()
                .and_then(|k| entry.results.get(k))
                .map(|&v| format!("{:.2}", v))
                .unwrap_or_else(|| "—".to_string())
        };

        format!("[{}] {}: {}", timestamp, mode, result_summary)
    }

    /// Get label for operation mode
    pub fn operation_label(&self) -> &'static str {
        match self.mode {
            CalculatorMode::Basic => "Calculate",
            CalculatorMode::CompoundInterest => "Compute Interest",
            CalculatorMode::Margin => "Calculate Margin",
            CalculatorMode::PipValue => "Calculate Pip",
            CalculatorMode::LotSize => "Calculate Lot",
            CalculatorMode::Fibonacci => "Calculate Levels",
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CalculatorPanel {
    id: CalculatorId,
    title: String,
}

impl CalculatorPanel {
    pub fn new(id: CalculatorId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> CalculatorId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "calculator"
    }

    pub fn kind_label(&self) -> &'static str {
        "Calculator"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (200.0, 250.0)
    }
}
