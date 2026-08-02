//! Projected-liquidation levels: cache maintenance.
//!
//! The model (`mlc_core::chart::liquidation_projection`) is pure and cheap
//! per point, but the consumption scan is `O(rows × bars_after_creation)`.
//! Running it per frame would burn a chart's whole budget on a picture that
//! only changes when a bar closes, open interest moves, or the owner turns a
//! knob — so it is computed HERE and cached on the bubble, keyed by exactly
//! those three things.
//!
//! The open-interest series it reads is the same `oi_delta` the OI Delta
//! sub-pane uses. That is deliberate: one subscription feeds both, and a
//! chart showing both cannot disagree with itself about what OI did.

use mlc_core::ids::BubbleId;

impl crate::ChartApp {
    /// Recompute the projected levels for every bubble whose projection
    /// overlay is live, when its inputs have changed.
    pub(crate) fn refresh_liq_projection(&mut self) {
        let targets: Vec<BubbleId> = {
            let mut seen: std::collections::HashSet<BubbleId> = std::collections::HashSet::new();
            self.panel_app
                .panel_grid
                .containers()
                .keys()
                .copied()
                .collect::<Vec<_>>()
                .into_iter()
                .filter_map(|cid| self.bubble_for_chart(cid))
                .filter(|bid| seen.insert(*bid))
                .filter(|bid| self.liq_projection_active(*bid))
                .collect()
        };

        for bid in targets {
            let params = self.liq_projection_params(bid);
            // Fingerprint the knobs that change the OUTPUT — the ink
            // (opacity, sensitivity, consumed visibility) does not, it is
            // read at draw time, so a colour tweak must not throw the cache
            // away.
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            {
                use std::hash::Hash;
                for t in &params.tiers {
                    t.leverage.to_bits().hash(&mut hasher);
                    t.weight.to_bits().hash(&mut hasher);
                }
                params.mmr.to_bits().hash(&mut hasher);
                params.row_pct.to_bits().hash(&mut hasher);
                params.long_share.to_bits().hash(&mut hasher);
                params.min_usd.to_bits().hash(&mut hasher);
                let _ = &mut hasher;
            }
            let param_hash = {
                use std::hash::Hasher;
                hasher.finish()
            };

            self.core.bubbles_mut().with_bubble_mut(bid, |bubble| {
                let bars_len = bubble.bars.len();
                let oi_len = bubble.oi_delta.as_ref().map(|v| v.len()).unwrap_or(0);
                let key = (bars_len, oi_len, param_hash);
                if bubble.liq_projection_key == key && bubble.liq_projection.is_some() {
                    return;
                }
                let levels = match bubble.oi_delta.as_ref() {
                    Some(oi) if bars_len > 0 && oi.len() >= 2 => {
                        mlc_core::chart::liquidation_projection::project(&bubble.bars, oi, &params)
                    }
                    // No open-interest series yet: an EMPTY projection, not
                    // a stale one. The overlay draws nothing and says so,
                    // rather than showing bands modelled from a different
                    // instrument's history.
                    _ => Vec::new(),
                };
                bubble.liq_projection = Some(levels);
                bubble.liq_projection_key = key;
            });
        }
    }
}
