use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct TransactionHistoryId(pub u64);

#[derive(Clone, Debug)]
pub struct TransactionHistoryState {
    /// Transaction records
    pub transactions: Vec<Transaction>,
    /// Time range filter
    pub time_range: TimeRange,
    /// Type filter (Deposit/Withdraw/Transfer/All)
    pub type_filter: TransactionTypeFilter,
    /// Asset filter
    pub asset_filter: Option<String>,
    /// Status filter
    pub status_filter: Option<TransactionStatus>,
}

#[derive(Clone, Debug)]
pub struct Transaction {
    pub id: String,
    pub tx_type: TransactionType,
    pub asset: String,
    pub amount: f64,
    pub status: TransactionStatus,
    pub tx_hash: Option<String>,
    pub network: Option<String>,
    pub address: Option<String>,
    pub timestamp: i64,
}

#[derive(Clone, Debug)]
pub enum TransactionType {
    Deposit,
    Withdraw,
    Transfer,
}

#[derive(Clone, Debug)]
pub enum TransactionStatus {
    Pending,
    Confirmed,
    Failed,
    Cancelled,
}

#[derive(Clone, Debug)]
pub enum TransactionTypeFilter {
    All,
    Deposit,
    Withdraw,
    Transfer,
}

#[derive(Clone, Debug)]
pub enum TimeRange {
    Today,
    Week,
    Month,
    All,
    Custom(i64, i64),
}

impl TransactionHistoryState {
    pub fn new() -> Self {
        Self {
            transactions: Vec::new(),
            time_range: TimeRange::Month,
            type_filter: TransactionTypeFilter::All,
            asset_filter: None,
            status_filter: None,
        }
    }

    /// Get visible transactions for rendering
    pub fn visible_transactions(&self, scroll_offset: usize, max_rows: usize) -> &[Transaction] {
        let end = (scroll_offset + max_rows).min(self.transactions.len());
        &self.transactions[scroll_offset..end]
    }

    /// Format transaction for display
    pub fn format_transaction(&self, tx: &Transaction) -> (String, String, String, String, String) {
        let time = format_timestamp(tx.timestamp);
        let tx_type = match tx.tx_type {
            TransactionType::Deposit => "Deposit",
            TransactionType::Withdraw => "Withdraw",
            TransactionType::Transfer => "Transfer",
        };
        let asset = tx.asset.clone();
        let amount = format!("{:.4}", tx.amount);
        let status = self.format_status(&tx.status);
        (time, tx_type.to_string(), asset, amount, status)
    }

    fn format_status(&self, status: &TransactionStatus) -> String {
        match status {
            TransactionStatus::Pending => "Pending".to_string(),
            TransactionStatus::Confirmed => "Confirmed".to_string(),
            TransactionStatus::Failed => "Failed".to_string(),
            TransactionStatus::Cancelled => "Cancelled".to_string(),
        }
    }

    /// Get color based on transaction status
    pub fn status_color(&self, tx: &Transaction) -> [f32; 4] {
        match tx.status {
            TransactionStatus::Confirmed => [0.2, 0.8, 0.3, 1.0], // green
            TransactionStatus::Pending => [0.9, 0.7, 0.2, 1.0],   // yellow
            TransactionStatus::Failed => [0.9, 0.2, 0.2, 1.0],    // red
            TransactionStatus::Cancelled => [0.5, 0.5, 0.5, 1.0], // gray
        }
    }

    /// Get color based on transaction type
    pub fn type_color(&self, tx: &Transaction) -> [f32; 4] {
        match tx.tx_type {
            TransactionType::Deposit => [0.2, 0.8, 0.3, 1.0],  // green
            TransactionType::Withdraw => [0.9, 0.2, 0.2, 1.0], // red
            TransactionType::Transfer => [0.3, 0.6, 0.9, 1.0], // blue
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    let s = secs % 60;
    format!("{:02}:{:02}:{:02}", h, m, s)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionHistoryConfig {
    /// Show tx hash column
    pub show_tx_hash: bool,
    /// Show network column
    pub show_network: bool,
    /// Link to explorer
    pub enable_explorer_links: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransactionHistoryPanel {
    id: TransactionHistoryId,
    title: String,
}

impl TransactionHistoryPanel {
    pub fn new(id: TransactionHistoryId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> TransactionHistoryId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "transaction_history"
    }

    pub fn kind_label(&self) -> &'static str {
        "Transactions"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 200.0)
    }
}
