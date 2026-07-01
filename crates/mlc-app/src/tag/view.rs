//! Build the lightweight `mlc_core::GroupView` for render layers.
//!
//! `mlc-layout` accepts `&[GroupView]` instead of `&BubbleStore` so the
//! render layer doesn't speak about `DataBubble` as a type — that keeps
//! the layout crate firmly inside the `mlc-core` dep-graph (no upward
//! dep on `mlc-backend-core`).
//!
//! This helper does the aggregation work (per-symbol indicator +
//! primitive counts) that used to live inside the modal renderer.

use mlc_backend_core::bubble::BubbleView;
use mlc_core::GroupView;
use std::collections::HashMap;

/// Project a backend [`BubbleView`] into the app/render-side
/// `GroupView`.  Pure read, no state.
pub fn build_group_view(bubble: BubbleView<'_>) -> GroupView {
    let mut per_symbol: HashMap<String, (usize, usize)> = HashMap::new();
    if !bubble.symbol().is_empty() {
        per_symbol.insert(bubble.symbol().to_string(), (0, 0));
    }
    for cfg in bubble.indicator_configs() {
        per_symbol.entry(cfg.symbol.clone()).or_insert((0, 0)).0 += 1;
    }
    for sym in bubble.primitive_symbols() {
        per_symbol.entry(sym.to_string()).or_insert((0, 0)).1 += 1;
    }
    let mut per_symbol_counts: Vec<(String, usize, usize)> = per_symbol
        .into_iter()
        .map(|(sym, (ic, pc))| (sym, ic, pc))
        .collect();
    // Current group symbol first, then alphabetical.
    let current_sym = bubble.symbol().to_string();
    per_symbol_counts.sort_by(|a, b| {
        let a_current = a.0 == current_sym;
        let b_current = b.0 == current_sym;
        b_current.cmp(&a_current).then(a.0.cmp(&b.0))
    });

    GroupView {
        id: bubble.id(),
        color: bubble.color(),
        auto_created: bubble.auto_created(),
        symbol: bubble.symbol().to_string(),
        timeframe_name: bubble.timeframe().name.clone(),
        members_count: bubble.members_count(),
        sync_flags: bubble.sync_flags(),
        per_symbol_counts,
    }
}
