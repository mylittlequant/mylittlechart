pub mod analytics;
pub mod calendar;
pub mod news;
pub mod options;
pub mod portfolio;
pub mod reference;
pub mod utility;

// Backward compatibility re-exports (old paths still work)
pub use reference::table;
pub use analytics::graph;
pub use analytics::timeline;
pub use news::news_feed as news_feed_mod;

// Convenience re-exports for all modules
// Analytics
pub use analytics::{correlation_matrix, spread_chart, statistics, market_replay, performance_analytics, sector_heatmap, pairs_trading};
// Calendar
pub use calendar::{economic_calendar, earnings_calendar, dividend_calendar, options_expiry, ipo_calendar};
// News
pub use news::{news_feed, rss_feed, social_sentiment, analyst_ratings, sec_filings};
// Options
pub use options::{options_chain, greeks_panel, iv_surface, option_flow, payoff_diagram};
// Portfolio
pub use portfolio::{account_summary, portfolio_overview, transaction_history, risk_metrics};
// Reference
pub use reference::{symbol_info, market_overview, screener, alert_manager, session_info};
// Utility
pub use utility::{calculator, notes, journal, connection_status};
