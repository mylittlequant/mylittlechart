use crate::types::{ExchangeId, AccountType};

#[derive(Debug, thiserror::Error)]
pub enum TradingError {
    #[error("exchange error: {0}")]
    Exchange(#[from] digdigdig3::ExchangeError),

    #[error("capability not supported: {0}")]
    Unsupported(&'static str),

    #[error("order not found: {client_id}")]
    OrderNotFound { client_id: String },

    #[error("no active session for {exchange:?}/{account_type:?}")]
    NoSession { exchange: ExchangeId, account_type: AccountType },

    #[error("connector not ready for {0:?}")]
    ConnectorNotReady(ExchangeId),

    #[error("paper engine error: {0}")]
    Paper(String),

    #[error("persistence: {0}")]
    Persist(#[from] std::io::Error),

    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type TradingResult<T> = Result<T, TradingError>;
