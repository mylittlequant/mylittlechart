//! Inner 1D vertical stack layout for ChartContainer.
//!
//! A chart container holds exactly one main chart on top and 0..N indicator
//! sub-panes stacked vertically below. The geometry is purely 1D: every
//! child shares 100% of the container's content height; one horizontal
//! separator sits between each adjacent pair.
//!
//! No uzor docking tree, no nested branches, no N-ary layout grid -- this
//! module owns the math directly.
//!
//! Invariants:
//! * `proportions.len() == 1 + sub_instance_ids.len()`
//! * `proportions[0]` is the main chart's share, `proportions[i+1]` is the
//!   share for sub-pane at `sub_instance_ids[i]`.
//! * Sum of all proportions is normalized to 1.0 after every mutation.
//! * Pixel heights derived from proportions respect per-pane minimums
//!   (`min_main_px` for the chart, `min_sub_px` for each indicator).
//!
//! The separator between root child `K` and `K+1` is "separator index K".
//! Drag math: shifting the separator by `delta_y` pixels transfers
//! `delta_y / total_h` worth of share between the two siblings, clamped so
//! neither falls below its min pixel height for the current total height.

#[derive(Clone, Debug)]
pub struct InnerLayout {
    /// Share per slot, parallel to `[main, sub_0, sub_1, ...]`.
    /// Normalized to sum to 1.0.
    pub proportions: Vec<f64>,
    /// Indicator instance ids parallel to `proportions[1..]`.
    pub sub_instance_ids: Vec<u64>,
    /// Min pixel height of the main chart.
    pub min_main_px: f64,
    /// Min pixel height of each indicator sub-pane.
    pub min_sub_px: f64,
    /// Separator thickness in pixels (subtracted from total before share split).
    pub separator_px: f64,
}

impl Default for InnerLayout {
    fn default() -> Self {
        Self {
            proportions: vec![1.0],
            sub_instance_ids: Vec::new(),
            // Minimum heights derived from the sub-pane label/button strip geometry:
            // - left pill: margin_top(2) + pill_height(22) = 24 px
            // - right overlay: margin(4) + bar_height(18) + bar margin(2) = 24 px
            // Add 2 px breathing room -> 26 px minimum for any pane.
            // The main chart uses the same floor so drag stops at a readable size.
            min_main_px: 26.0,
            min_sub_px: 26.0,
            separator_px: 1.0,
        }
    }
}

impl InnerLayout {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn sub_count(&self) -> usize {
        self.sub_instance_ids.len()
    }

    pub fn has_indicator(&self, instance_id: u64) -> bool {
        self.sub_instance_ids.iter().any(|&id| id == instance_id)
    }

    /// Find the index of the sub-pane with `instance_id` in `sub_instance_ids`
    /// (0-based, NOT including the main chart slot).
    pub fn sub_index(&self, instance_id: u64) -> Option<usize> {
        self.sub_instance_ids.iter().position(|&id| id == instance_id)
    }

    /// Add an indicator sub-pane to the stack. The new pane gets `new_share`
    /// (default 0.2) of the **main chart's** current share, taken from main.
    /// Existing sub-panes keep their absolute shares; everything is then
    /// renormalized.
    pub fn add_indicator(&mut self, instance_id: u64) {
        if self.has_indicator(instance_id) {
            return;
        }
        let new_share = self.proportions.first().copied().unwrap_or(1.0) * 0.2;
        let new_share = new_share.max(0.05);
        if let Some(first) = self.proportions.first_mut() {
            *first = (*first - new_share).max(0.05);
        }
        self.proportions.push(new_share);
        self.sub_instance_ids.push(instance_id);
        self.normalize();
    }

    /// Remove an indicator sub-pane. The freed share goes back to the main
    /// chart. No-op if the indicator is not in the stack.
    pub fn remove_indicator(&mut self, instance_id: u64) {
        let idx = match self.sub_index(instance_id) {
            Some(i) => i,
            None => return,
        };
        let removed_share = self.proportions.get(idx + 1).copied().unwrap_or(0.0);
        self.proportions.remove(idx + 1);
        self.sub_instance_ids.remove(idx);
        if let Some(first) = self.proportions.first_mut() {
            *first += removed_share;
        }
        self.normalize();
    }

    /// Replace the indicator list with `want_ids` in the given order. Existing
    /// sub-panes preserve their share; new ids get a default share carved
    /// from main; ids no longer in `want_ids` are removed.
    pub fn sync_with(&mut self, want_ids: &[u64]) {
        // Remove vanished ids first.
        let to_remove: Vec<u64> = self
            .sub_instance_ids
            .iter()
            .filter(|id| !want_ids.contains(id))
            .copied()
            .collect();
        for id in to_remove {
            self.remove_indicator(id);
        }
        // Add new ids.
        for &id in want_ids {
            if !self.has_indicator(id) {
                self.add_indicator(id);
            }
        }
        // Reorder to match want_ids order.
        if self.sub_instance_ids != want_ids {
            let main_share = self.proportions.first().copied().unwrap_or(1.0);
            let old_shares: std::collections::HashMap<u64, f64> = self
                .sub_instance_ids
                .iter()
                .zip(self.proportions.iter().skip(1))
                .map(|(&id, &s)| (id, s))
                .collect();
            self.sub_instance_ids = want_ids.to_vec();
            let mut new_props = Vec::with_capacity(want_ids.len() + 1);
            new_props.push(main_share);
            for id in want_ids {
                new_props.push(old_shares.get(id).copied().unwrap_or(0.1));
            }
            self.proportions = new_props;
            self.normalize();
        }
    }

    /// Seed proportions from a `(instance_id -> share)` map. Used on preset
    /// restore where the legacy format stores `sub_pane_height_ratios`.
    /// `want_ids` defines the order; missing entries get a uniform fallback.
    pub fn seed_from_ratios(
        &mut self,
        want_ids: &[u64],
        ratios: &std::collections::HashMap<u64, f32>,
    ) {
        self.sub_instance_ids = want_ids.to_vec();
        // Cap any single ratio at 0.4 -- legacy presets sometimes stored
        // 1.0 / huge values that would collapse the main chart on restore.
        // Cap total sub_share at 0.7 so the main chart keeps >= 30 %.
        let raw_subs: Vec<f64> = want_ids
            .iter()
            .map(|id| {
                let r = ratios.get(id).copied().unwrap_or(0.15) as f64;
                r.clamp(0.05, 0.4)
            })
            .collect();
        let sub_total: f64 = raw_subs.iter().sum::<f64>().min(0.7);
        let scale = if raw_subs.iter().sum::<f64>() > sub_total && !raw_subs.is_empty() {
            sub_total / raw_subs.iter().sum::<f64>()
        } else {
            1.0
        };
        let main_share = (1.0 - sub_total).max(0.05);
        let mut props = Vec::with_capacity(want_ids.len() + 1);
        props.push(main_share);
        for r in raw_subs {
            props.push((r * scale).max(0.01));
        }
        self.proportions = props;
        self.normalize();
    }

    /// Renormalize proportions to sum to 1.0. Idempotent.
    pub fn normalize(&mut self) {
        let total: f64 = self.proportions.iter().sum();
        if total > 0.0 {
            for v in &mut self.proportions {
                *v /= total;
            }
        } else if !self.proportions.is_empty() {
            let share = 1.0 / self.proportions.len() as f64;
            for v in &mut self.proportions {
                *v = share;
            }
        }
    }

    /// Compute pixel heights for `[main, sub_0, sub_1, ...]` given the
    /// total content height (excludes time scale and any toolbars carved
    /// out by the caller). Enforces per-slot minimums by clamping then
    /// rescaling the unclamped slots to absorb the remainder.
    pub fn heights(&self, content_height: f64) -> Vec<f64> {
        let n = self.proportions.len();
        if n == 0 {
            return Vec::new();
        }
        // Reserve space for the separators between slots (one per sub-pane,
        // sitting above it -- main has no separator above).
        let sep_total = self.separator_px * self.sub_count() as f64;
        let budget = (content_height - sep_total).max(0.0);
        if budget <= 0.0 {
            return vec![0.0; n];
        }

        // Raw heights from proportions.
        let mut h: Vec<f64> = self
            .proportions
            .iter()
            .map(|&p| p * budget)
            .collect();

        // Enforce minimums via water-fill. If sum of minimums exceeds budget,
        // each gets its min and content overflows the rect (caller's problem).
        let mins: Vec<f64> = (0..n)
            .map(|i| if i == 0 { self.min_main_px } else { self.min_sub_px })
            .collect();
        let total_min: f64 = mins.iter().sum();
        if total_min >= budget {
            return mins;
        }
        // Identify slots already at/below their min; clamp them and rescale
        // the remaining slots to absorb the slack.
        let mut clamped = vec![false; n];
        loop {
            let free_budget: f64 = budget
                - clamped
                    .iter()
                    .zip(mins.iter())
                    .filter_map(|(&c, &m)| if c { Some(m) } else { None })
                    .sum::<f64>();
            let free_prop_total: f64 = self
                .proportions
                .iter()
                .enumerate()
                .filter_map(|(i, &p)| if clamped[i] { None } else { Some(p) })
                .sum();
            if free_prop_total <= 0.0 {
                break;
            }
            let mut any_new = false;
            for i in 0..n {
                if clamped[i] {
                    continue;
                }
                let candidate = self.proportions[i] / free_prop_total * free_budget;
                if candidate < mins[i] {
                    h[i] = mins[i];
                    clamped[i] = true;
                    any_new = true;
                } else {
                    h[i] = candidate;
                }
            }
            if !any_new {
                break;
            }
        }
        h
    }

    /// Apply a separator drag. `sep_idx` = 0 means the separator between
    /// main and the first sub-pane; `sep_idx = K` means the separator
    /// between root child K and K+1. `delta_y` is per-event pixels, positive
    /// = drag down (shrinks slot K, grows slot K+1).
    ///
    /// Returns true if the drag was applied (clamped to min if needed).
    pub fn drag_separator(&mut self, sep_idx: usize, delta_y: f64, content_height: f64) -> bool {
        let n = self.proportions.len();
        if sep_idx + 1 >= n {
            return false;
        }
        let heights = self.heights(content_height);
        let h_a = heights[sep_idx];
        let h_b = heights[sep_idx + 1];
        let min_a = if sep_idx == 0 { self.min_main_px } else { self.min_sub_px };
        let min_b = self.min_sub_px;
        let total_pair = h_a + h_b;
        if total_pair <= 0.0 {
            return false;
        }
        // Shift delta_y from A to B (positive delta grows B, shrinks A).
        let new_a = (h_a - delta_y).clamp(min_a, total_pair - min_b);
        let new_b = total_pair - new_a;
        // Convert back to shares of total content_height (use the same
        // denominator the renderer uses so the share writes back cleanly).
        let sep_total = self.separator_px * self.sub_count() as f64;
        let budget = (content_height - sep_total).max(1.0);
        let share_a = new_a / budget;
        let share_b = new_b / budget;
        self.proportions[sep_idx] = share_a;
        self.proportions[sep_idx + 1] = share_b;
        self.normalize();
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_then_remove_preserves_main() {
        let mut l = InnerLayout::new();
        l.add_indicator(101);
        l.add_indicator(202);
        assert_eq!(l.sub_count(), 2);
        l.remove_indicator(101);
        assert_eq!(l.sub_count(), 1);
        assert_eq!(l.sub_instance_ids, vec![202]);
        let total: f64 = l.proportions.iter().sum();
        assert!((total - 1.0).abs() < 1e-9);
    }

    #[test]
    fn heights_respect_mins() {
        let mut l = InnerLayout::new();
        l.add_indicator(1);
        l.add_indicator(2);
        l.proportions = vec![0.05, 0.475, 0.475];
        l.normalize();
        let h = l.heights(400.0);
        assert!(h[0] >= l.min_main_px);
        assert!(h[1] >= l.min_sub_px);
        assert!(h[2] >= l.min_sub_px);
    }

    #[test]
    fn drag_separator_round_trip_grows_back() {
        let mut l = InnerLayout::new();
        l.add_indicator(1);
        let content_h = 500.0;
        let h0 = l.heights(content_h);
        // Drag separator 0 down 100 px (shrink main, grow sub).
        assert!(l.drag_separator(0, 100.0, content_h));
        let h1 = l.heights(content_h);
        assert!(h1[1] > h0[1]);
        // Drag back up 200 px (now shrink sub past start, clamps at min).
        assert!(l.drag_separator(0, -200.0, content_h));
        let h2 = l.heights(content_h);
        assert!(h2[1] >= l.min_sub_px);
        assert!(h2[0] > h1[0]);
    }

    #[test]
    fn seed_from_ratios_matches_order() {
        let mut l = InnerLayout::new();
        let want = vec![10_u64, 20, 30];
        let mut ratios = std::collections::HashMap::new();
        ratios.insert(10_u64, 0.15_f32);
        ratios.insert(20_u64, 0.20_f32);
        ratios.insert(30_u64, 0.10_f32);
        l.seed_from_ratios(&want, &ratios);
        assert_eq!(l.sub_instance_ids, want);
        let total: f64 = l.proportions.iter().sum();
        assert!((total - 1.0).abs() < 1e-9);
    }
}
