mod data_bubble;
mod store;
mod render;
mod snapshot;
mod facade;
mod raw_layer;
mod view_fold;
mod trade_built_kline;
pub(crate) mod wave_guards;
pub(crate) mod bar_derived_cache;
pub(crate) mod markup_guards;
pub(crate) mod markup_weld;
pub use data_bubble::*;
pub use store::{BubbleStore, InstrumentKey, PendingKlineTeardown};
pub use render::{build_chart_render_state, ChartWindowRenderInputs};
pub use facade::{BubbleView, Bubbles, BubblesMut};
pub use raw_layer::{RawLayer, RawTrade};
pub use view_fold::{
    bar_to_bar_point, raw_trade_to_trade_point, FoldOutput, FoldParams, FoldRuntime,
    TradeFoldOutput, TradeFoldRuntime,
};
pub use trade_built_kline::TradeBuiltKlineRuntime;
