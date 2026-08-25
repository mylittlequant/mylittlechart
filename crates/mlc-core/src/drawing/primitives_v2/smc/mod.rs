//! Smart money — markup engine #2 (`docs/mlc/plans/markup-engines-2026-08-25.md`,
//! Appendix A). Four CLAIM primitives, each anchored on a single bar and
//! judged against `mli::smc` through the shared [`smc_judge::judge_window`]
//! bounded-lookback resolver.

mod smc_judge;
pub mod order_block;
pub mod fair_value_gap;
pub mod liquidity_sweep;
pub mod structure_break;

pub use order_block::OrderBlock;
pub use fair_value_gap::FairValueGap;
pub use liquidity_sweep::LiquiditySweep;
pub use structure_break::StructureBreak;
