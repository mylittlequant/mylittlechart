//! Host-held factory. `ContractFactory` is the whole indicator universe, so a
//! by-value build or clone overflows a normal thread stack. Construction and
//! cloning run on a 16 MiB thread. The chart only stores the box.

use std::collections::HashMap;

use mli::contract::MarketSample;
use mli::engine::contract_engine::{output_ids_of, ContractFactory, IndicatorOrder};
use mli::engine::indicator_id::IndicatorId;

use super::stream_event::StreamEvent;
use crate::Bar;

const WIDE_STACK: usize = 16 * 1024 * 1024;

/// One bar of named outputs, in `output_ids_of` order.
#[derive(Debug, Clone, PartialEq)]
pub struct OutputSample {
    pub lines: Vec<f64>,
    pub by_name: Vec<(String, f64)>,
}

impl OutputSample {
    pub fn main(&self) -> f64 {
        self.lines.first().copied().unwrap_or(f64::NAN)
    }

    pub fn line(&self, index: usize) -> Option<f64> {
        self.lines.get(index).copied()
    }

    pub fn map(&self) -> HashMap<String, f64> {
        self.by_name.iter().cloned().collect()
    }
}

/// Boxed [`ContractFactory`] plus the id it was built for.
pub struct ContractInstance {
    id: IndicatorId,
    factory: Box<ContractFactory>,
}

impl Clone for ContractInstance {
    fn clone(&self) -> Self {
        let ptr = &*self.factory as *const ContractFactory as usize;
        let id = self.id;
        std::thread::Builder::new()
            .stack_size(WIDE_STACK)
            .spawn(move || {
                // The parent joins before `self` is dropped, so the borrow holds.
                let src = unsafe { &*(ptr as *const ContractFactory) };
                ContractInstance {
                    id,
                    factory: Box::new(src.clone()),
                }
            })
            .expect("wide stack")
            .join()
            .expect("factory clone")
    }
}

impl ContractInstance {
    /// Cold-start instance. `period` overrides the primary lookback when the
    /// chart stored one. Other axes stay on the contract default.
    pub fn build(id: IndicatorId, period: Option<usize>) -> Option<Self> {
        std::thread::Builder::new()
            .stack_size(WIDE_STACK)
            .spawn(move || {
                let mut order = IndicatorOrder::from_defaults(id)?;
                if let Some(period) = period {
                    order = order.with_period(mli::contract::Param::Solo(period));
                }
                let factory = order.build_solo().ok()?;
                Some(ContractInstance {
                    id,
                    factory: Box::new(factory),
                })
            })
            .ok()?
            .join()
            .ok()?
    }

    /// Primary lookback of every migrated id. One wide thread for the whole set.
    pub fn primary_periods() -> HashMap<IndicatorId, Option<usize>> {
        std::thread::Builder::new()
            .stack_size(WIDE_STACK)
            .spawn(|| {
                let mut out = HashMap::new();
                for id in IndicatorId::all() {
                    if !ContractFactory::is_migrated(id) {
                        continue;
                    }
                    let Some(order) = IndicatorOrder::from_defaults(id) else {
                        continue;
                    };
                    out.insert(id, order.primary_period());
                }
                out
            })
            .expect("wide stack")
            .join()
            .expect("period table")
    }

    pub fn id(&self) -> IndicatorId {
        self.id
    }

    pub fn feed_bar(&mut self, bar: &Bar) {
        self.factory.feed(
            bar.time,
            MarketSample::Bar {
                open: bar.open,
                high: bar.high,
                low: bar.low,
                close: bar.close,
                volume: bar.volume,
            },
        );
    }

    /// Bar plus an aggregated buy/sell delta. CVD and Vdelta consume the
    /// delta. Every other id is fed the bar.
    pub fn feed_bar_with_delta(&mut self, bar: &Bar, delta: Option<(f64, f64)>) {
        if let Some((buy, sell)) = delta {
            match self.factory.as_mut() {
                ContractFactory::Cvd(core, _) => {
                    core.update_with_delta(buy, sell);
                    return;
                }
                ContractFactory::Vdelta(core, _) => {
                    core.update_with_delta(buy, sell);
                    return;
                }
                _ => {}
            }
        }
        self.feed_bar(bar);
    }

    pub fn feed_event(&mut self, ev: &StreamEvent) {
        match ev {
            StreamEvent::Tick(tick) => {
                self.factory.feed(tick.time, MarketSample::Tick(tick));
            }
            StreamEvent::OrderBook(book) => {
                self.factory.feed(book.timestamp, MarketSample::OrderBook(book));
            }
            StreamEvent::OrderbookDelta(delta) => {
                self.factory
                    .feed(delta.timestamp, MarketSample::OrderbookDelta(delta));
            }
            StreamEvent::Funding(rate) => {
                self.factory.feed(rate.timestamp, MarketSample::Funding(rate));
            }
            StreamEvent::OpenInterest(oi) => {
                self.factory.feed(oi.timestamp, MarketSample::OpenInterest(oi));
            }
            StreamEvent::Liquidation(liq) => {
                self.factory
                    .feed(liq.timestamp, MarketSample::Liquidation(liq));
            }
        }
    }

    pub fn sample(&self) -> OutputSample {
        let mut lines = Vec::new();
        let mut by_name = Vec::new();
        for oid in output_ids_of(self.id) {
            let value = self.factory.read(*oid);
            lines.push(value);
            by_name.push((oid.name().to_string(), value));
        }
        OutputSample { lines, by_name }
    }
}
