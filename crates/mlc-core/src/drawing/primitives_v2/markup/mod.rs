//! The shared markup layer — what the Elliott engine grew that is not about
//! Elliott.
//!
//! `docs/mlc/research/ewa-engine-map-2026-08-25.md` mapped that engine
//! mechanism by mechanism and found that most of it is general markup
//! machinery living in `elliott/` because Elliott needed it first. This
//! module is where the general half moves to as the other families grow
//! into it (`docs/mlc/plans/markup-engine-parity-2026-08-25.md`).
//!
//! What is here today is the first piece the other families needed: a way
//! to say WHICH family a primitive belongs to, so a pass over a whole
//! collection can apply one family's own cross-primitive rules without
//! touching the trend lines and Fibonacci grids drawn beside them.
//!
//! [`elliott::guard::Violation`](super::elliott::guard::Violation) is
//! deliberately NOT moved here. It already carries the whole-structure
//! shape every non-Elliott engine reports (`Violation::whole`), and its one
//! Elliott-shaped field is an `Option<WaveSlot>` that those engines leave
//! `None`. Relocating it would mean generalizing that field across some
//! twenty Elliott construction sites and five render files for no change in
//! behaviour — churn against the plan's own rule that Elliott must not move
//! while the other families are built.

pub mod chain;
pub mod link;

pub use chain::{
    follows_stat_row, may_follow, successor_kinds, with_follows_row, ChainFact, MarkupBias,
    MarkupFollows,
};
pub use link::{link_stat_row, slot_name_key, slots, with_link_row, MarkupLink, SlotId};

/// Which markup family a primitive belongs to, for a pass that must apply
/// one family's own cross-primitive rules and leave every other drawing
/// alone.
///
/// `None` from [`super::Primitive::markup_family`] means "not a markup
/// claim" — a trend line, a Fibonacci grid, an icon. Elliott is absent on
/// purpose: it already has its own family-wide pass
/// (`mlc_backend::bubble::wave_guards`) keyed on `wave_degree().is_some()`,
/// and folding it in here would mean rewriting a working engine to prove a
/// point about symmetry.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MarkupFamily {
    /// `order_block`, `fair_value_gap`, `liquidity_sweep`, `structure_break`,
    /// `dealing_range`, `amd_cycle`, `smc_scan`.
    SmartMoney,
    /// `imbalance_zone`, `absorption_mark`, `delta_divergence`,
    /// `order_flow_scan`.
    OrderFlow,
    /// `candle_pattern`, `candle_pattern_scan`.
    CandlePattern,
}
