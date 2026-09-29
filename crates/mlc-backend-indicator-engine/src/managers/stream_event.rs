//! Non-bar stream events ready to feed into a live `ComputeInstance`.
//!
//! `mlc-backend-data-bridge` already delivers every stream listed here to
//! `BackendCore` (Arc D plan, `docs/mlc/plans/
//! indicator-streams-arc-d-2026-09-25.md` §2) — a variant exists here IFF a
//! real producer exists. Adding a variant with no producer would be a
//! parallel API nothing ever calls, which is exactly the shape mli's own
//! hard rule bans ("NEVER introduce parallel APIs",
//! `mylittleindicators/CLAUDE.md`).

use crate::types::{FundingRate, Liquidation, OpenInterest, OrderBook, OrderbookDelta};
use crate::{StreamKind, Tick};

/// One non-bar stream event, ready to feed into a live `ComputeInstance` via
/// [`super::indicator_calculator::IndicatorCalculator::feed_stream_event`].
#[derive(Debug, Clone)]
pub enum StreamEvent {
    Tick(Tick),
    OrderBook(OrderBook),
    OrderbookDelta(OrderbookDelta),
    Funding(FundingRate),
    OpenInterest(OpenInterest),
    Liquidation(Liquidation),
}

impl StreamEvent {
    /// The `StreamKind` this event carries — used to filter which
    /// `ComputeInstance`s (by their catalog-declared `input_stream`/
    /// `aux_streams`) should receive it.
    pub fn kind(&self) -> StreamKind {
        match self {
            Self::Tick(_) => StreamKind::Tick,
            Self::OrderBook(_) => StreamKind::OrderBook,
            Self::OrderbookDelta(_) => StreamKind::OrderbookDelta,
            Self::Funding(_) => StreamKind::Funding,
            Self::OpenInterest(_) => StreamKind::OpenInterest,
            Self::Liquidation(_) => StreamKind::Liquidation,
        }
    }
}
