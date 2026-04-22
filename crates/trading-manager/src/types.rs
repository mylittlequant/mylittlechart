// Types available at digdigdig3 crate root
pub use digdigdig3::{
    OrderSide, OrderType, OrderStatus, AccountType, ExchangeId, Symbol,
    ExchangeError, ExchangeResult,
    Order, Position, PositionSide, Balance,
    TradingCapabilities, AccountCapabilities,
    OrderUpdateEvent, BalanceUpdateEvent, PositionUpdateEvent,
};

// Types only available via digdigdig3::core
pub use digdigdig3::core::{
    OrderRequest, CancelRequest, CancelScope,
    AmendRequest, AmendFields, OrderHistoryFilter,
    PlaceOrderResponse, OrderResult, CancelAllResponse,
    MarginType, PositionMode,
    PositionModification, PositionQuery, BalanceQuery,
    TimeInForce, TriggerDirection,
    UserTrade, UserTradeFilter,
    AccountInfo, FeeInfo,
};

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct Fill {
    pub order_id: String,
    pub symbol: String,
    pub side: OrderSide,
    pub price: f64,
    pub quantity: f64,
    pub fee: f64,
    pub fee_asset: String,
    pub timestamp: i64,
    pub is_paper: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct PositionPnl {
    pub unrealized: f64,
    pub realized: f64,
    pub entry_price: f64,
    pub current_price: f64,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum SizePreset {
    BalancePct(f32),
    FixedQuote(f64),
    FixedBase(f64),
}

#[derive(Debug, Clone)]
pub struct SessionInfo {
    pub exchange_id: ExchangeId,
    pub account_type: AccountType,
    pub is_paper: bool,
    pub is_testnet: bool,
}
