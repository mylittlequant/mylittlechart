//! Server-Sent Events stream.
//!
//! - `GET /v1/events` — SSE stream of BubbleEvents emitted by tick_writers

use std::convert::Infallible;

use axum::{
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse,
    },
    routing::get,
    Router,
};
use futures_util::stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use mlc_backend_core::BubbleEvent;

use crate::AppState;

async fn sse_events(State(s): State<AppState>) -> impl IntoResponse {
    let rx = s.events_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|msg| async move {
        let event = msg.ok()?;
        let type_name = event_type_name(&event);
        let data = serde_json::to_string(&event).unwrap_or_else(|_| "{}".to_string());
        Some(Ok::<Event, Infallible>(
            Event::default().event(type_name).data(data),
        ))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

fn event_type_name(e: &BubbleEvent) -> &'static str {
    match e {
        BubbleEvent::BarsLoaded { .. } => "BarsLoaded",
        BubbleEvent::BackfillCompleted { .. } => "BackfillCompleted",
        BubbleEvent::ScrollBarsAppended { .. } => "ScrollBarsAppended",
        BubbleEvent::BarUpdatePassthrough { .. } => "BarUpdatePassthrough",
        BubbleEvent::TradeUpdatePassthrough { .. } => "TradeUpdatePassthrough",
        BubbleEvent::MiniTickerUpdated { .. } => "MiniTickerUpdated",
        BubbleEvent::ConnectorReady { .. } => "ConnectorReady",
        BubbleEvent::SymbolsLoaded { .. } => "SymbolsLoaded",
        BubbleEvent::ErrorReported { .. } => "ErrorReported",
        BubbleEvent::OrderbookSnapshotRest { .. } => "OrderbookSnapshotRest",
        BubbleEvent::OrderbookDelta { .. } => "OrderbookDelta",
        BubbleEvent::OrderbookDeltaForMultiDom { .. } => "OrderbookDeltaForMultiDom",
        BubbleEvent::TradingUpdate { .. } => "TradingUpdate",
        BubbleEvent::FootprintBarAppended { .. } => "FootprintBarAppended",
        BubbleEvent::RenkoBarAppended { .. } => "RenkoBarAppended",
        BubbleEvent::PnfBarAppended { .. } => "PnfBarAppended",
        BubbleEvent::KagiBarAppended { .. } => "KagiBarAppended",
        BubbleEvent::CvdSampleAppended { .. } => "CvdSampleAppended",
        BubbleEvent::TpoSessionUpdated { .. } => "TpoSessionUpdated",
        BubbleEvent::TlbLineAppended { .. } => "TlbLineAppended",
        BubbleEvent::OiUpdatePassthrough { .. } => "OiUpdatePassthrough",
        BubbleEvent::FundingRatePassthrough { .. } => "FundingRatePassthrough",
        BubbleEvent::DomHeatmapSnapshotPassthrough { .. } => "DomHeatmapSnapshotPassthrough",
        BubbleEvent::DomTradeDotPassthrough { .. } => "DomTradeDotPassthrough",
        BubbleEvent::LiquidationEventPassthrough { .. } => "LiquidationEventPassthrough",
        BubbleEvent::LiquidationBucketPassthrough { .. } => "LiquidationBucketPassthrough",
    }
}

pub fn routes() -> Router<AppState> {
    Router::new().route("/v1/events", get(sse_events))
}

