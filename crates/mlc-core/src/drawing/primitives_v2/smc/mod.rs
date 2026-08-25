//! Smart money — markup engine #2 (`docs/mlc/plans/markup-engines-2026-08-25.md`,
//! Appendix A). Six CLAIM primitives, each anchored on the bars the analyst
//! marks and judged against `mli::smc` through the shared
//! [`smc_judge::judge_window`] bounded-lookback resolver
//! (`order_block`/`fair_value_gap`/`liquidity_sweep`/`structure_break`/
//! `amd_cycle` — `dealing_range` is the one exception, see its own module
//! doc for why), plus the survey (`smc_scan`).

mod smc_judge;
mod render_shapes;
pub mod order_block;
pub mod fair_value_gap;
pub mod liquidity_sweep;
pub mod structure_break;
pub mod dealing_range;
pub mod amd_cycle;
pub mod smc_scan;

pub use order_block::OrderBlock;
pub use fair_value_gap::FairValueGap;
pub use liquidity_sweep::LiquiditySweep;
pub use structure_break::StructureBreak;
pub use dealing_range::DealingRange;
pub use amd_cycle::AmdCycle;
pub use smc_scan::SmcScan;
