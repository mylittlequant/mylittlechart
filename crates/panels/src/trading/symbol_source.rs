//! Symbol source binding for trading panels in sidebar slots.
//!
//! Every trading panel (DOM, Footprint, L2 Tape, etc.) needs to know which
//! instrument to display. `SymbolSource` defines the binding strategy.

use serde::{Deserialize, Serialize};

/// How a trading panel resolves its target instrument.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum SymbolSource {
    /// Follows the currently focused chart leaf — default for new panels.
    /// When the user clicks a different chart, the panel switches to that symbol.
    HyperFocus,

    /// Pinned to a specific instrument. Doesn't change when chart focus changes.
    Fixed {
        symbol: String,
        exchange: String,
        account_type: String,
    },

    /// Bound to a specific chart leaf by its numeric ID.
    /// Inherits symbol/exchange/account_type from that leaf.
    /// If the leaf is removed, falls back to HyperFocus behavior.
    BoundToChart {
        leaf_id: u64,
    },
}

impl Default for SymbolSource {
    fn default() -> Self {
        Self::HyperFocus
    }
}

/// Resolved instrument key — the concrete symbol/exchange/account_type triple
/// that a panel should use for data subscription and display.
#[derive(Clone, Debug, PartialEq)]
pub struct ResolvedSymbol {
    pub symbol: String,
    pub exchange: String,
    pub account_type: String,
}

impl SymbolSource {
    /// Resolve the concrete instrument this source points to.
    ///
    /// - `active_symbol`: the symbol from the currently focused chart leaf (used by `HyperFocus`).
    /// - `chart_leaves`: lookup function from leaf ID to `ResolvedSymbol` (used by `BoundToChart`).
    ///
    /// Returns `None` when the source cannot be resolved (e.g. `HyperFocus` with no focused chart).
    pub fn resolve(
        &self,
        active_symbol: Option<&ResolvedSymbol>,
        chart_leaves: &dyn Fn(u64) -> Option<ResolvedSymbol>,
    ) -> Option<ResolvedSymbol> {
        match self {
            Self::HyperFocus => active_symbol.cloned(),
            Self::Fixed {
                symbol,
                exchange,
                account_type,
            } => Some(ResolvedSymbol {
                symbol: symbol.clone(),
                exchange: exchange.clone(),
                account_type: account_type.clone(),
            }),
            Self::BoundToChart { leaf_id } => {
                chart_leaves(*leaf_id).or_else(|| active_symbol.cloned())
            }
        }
    }

    /// Returns `true` if this source is `HyperFocus`.
    pub fn is_hyper_focus(&self) -> bool {
        matches!(self, Self::HyperFocus)
    }

    /// Returns `true` if this source is `Fixed`.
    pub fn is_fixed(&self) -> bool {
        matches!(self, Self::Fixed { .. })
    }

    /// Returns `true` if this source is `BoundToChart`.
    pub fn is_bound_to_chart(&self) -> bool {
        matches!(self, Self::BoundToChart { .. })
    }

    /// Short label suitable for display in UI controls.
    ///
    /// Returns `"Auto"` for `HyperFocus`, `"Pinned"` for `Fixed`, and `"Linked"` for `BoundToChart`.
    pub fn display_label(&self) -> &str {
        match self {
            Self::HyperFocus => "Auto",
            Self::Fixed { .. } => "Pinned",
            Self::BoundToChart { .. } => "Linked",
        }
    }
}
