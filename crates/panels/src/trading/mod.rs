pub mod order_flow;
pub mod market_data;
pub mod trading;

// Re-export for convenience (keep backward compat paths)
pub use order_flow::dom;
pub use market_data::watchlist;
pub use trading::trade_log;

// Also re-export new modules at top level for easy access
pub use order_flow::{footprint, volume_profile, liquidity_heatmap, big_trades, l2_tape};
pub use market_data::{time_sales, ticker_tape, market_depth_graph, tick_tape_chart};
pub use trading::{order_entry, position_manager, risk_calculator, trading_container};
