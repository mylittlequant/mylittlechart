//! `DrawingService` / `DrawingServiceMut` — typed wrapper over
//! `BackendCore::drawings: RwLock<DrawingRegistry>`.
//!
//! Mirrors `DrawingRegistry`'s registry-level API 1:1 (`get`, `get_mut` via
//! `with_panel_mut`, `iter`, `remove`, `drain_matching`) so existing
//! `DrawingManager` call chains move verbatim from `guard.get(pid).method()`
//! to `svc.get(pid).method()`. `with_panel_mut` is the universal write entry
//! point (wraps `entry_or_default`, per Audit-A note N1); `swap_symbol`
//! collapses the 8x-repeated snapshot+swap+restore compound (Audit-A N6).
//!
//! The guard never leaves this module — only `&DrawingManager` /
//! `&mut DrawingManager` borrows scoped to the service's lifetime (read
//! side) or to a closure call (write side) escape, exactly as before.

use std::sync::{RwLockReadGuard, RwLockWriteGuard};

use mlc_core::drawing::primitives_v2::registry::PrimitiveRegistry;
use mlc_core::drawing::primitives_v2::Primitive;
use mlc_core::drawing::{DrawingManager, PrimitiveData, PrimitiveSnapshot};
use mlc_core::state::chart_container_leaf::ChartPanelId;
use mlc_core::{PriceScale, Viewport};

use crate::DrawingRegistry;

/// Read-only view over the `DrawingRegistry`. Construct via
/// `BackendCore::drawings_svc()`.
pub struct DrawingService<'a> {
    reg: RwLockReadGuard<'a, DrawingRegistry>,
}

impl<'a> DrawingService<'a> {
    pub(crate) fn new(reg: RwLockReadGuard<'a, DrawingRegistry>) -> Self {
        Self { reg }
    }

    /// Immutable borrow of the manager for `panel_id`. Mirrors
    /// `DrawingRegistry::get`.
    pub fn get(&self, panel_id: ChartPanelId) -> Option<&DrawingManager> {
        self.reg.get(panel_id)
    }

    /// Borrow the DM for a panel for the duration of `f`. Equivalent to
    /// `get(pid).map(f)`; kept for callers that prefer the closure form.
    pub fn with_panel<R>(&self, pid: ChartPanelId, f: impl FnOnce(&DrawingManager) -> R) -> Option<R> {
        self.reg.get(pid).map(f)
    }

    /// Iterate over all `(ChartPanelId, &DrawingManager)` pairs. Render-loop
    /// helper (Audit-A note N7) and the daemon's list-all-panels route.
    pub fn iter(&self) -> impl Iterator<Item = (ChartPanelId, &DrawingManager)> {
        self.reg.iter()
    }

    // ---- High-frequency explicit read methods (Audit-A Section 3) --------

    pub fn get_data_at(&self, pid: ChartPanelId, idx: usize) -> Option<PrimitiveData> {
        self.reg.get(pid).and_then(|dm| dm.get_data_at(idx))
    }

    pub fn primitives(&self, pid: ChartPanelId) -> Option<&[Box<dyn Primitive>]> {
        self.reg.get(pid).map(DrawingManager::primitives)
    }

    pub fn is_drawing(&self, pid: ChartPanelId) -> bool {
        self.reg.get(pid).is_some_and(DrawingManager::is_drawing)
    }

    pub fn current_tool(&self, pid: ChartPanelId) -> Option<&str> {
        self.reg.get(pid).and_then(DrawingManager::current_tool)
    }

    pub fn find_index_by_id(&self, pid: ChartPanelId, id: u64) -> Option<usize> {
        self.reg.get(pid).and_then(|dm| dm.find_index_by_id(id))
    }

    pub fn count(&self, pid: ChartPanelId) -> usize {
        self.reg.get(pid).map(DrawingManager::count).unwrap_or(0)
    }

    pub fn last_index(&self, pid: ChartPanelId) -> Option<usize> {
        self.reg.get(pid).and_then(DrawingManager::last_index)
    }

    pub fn is_dragging(&self, pid: ChartPanelId) -> bool {
        self.reg.get(pid).is_some_and(DrawingManager::is_dragging)
    }

    pub fn dragging_primitive_id(&self, pid: ChartPanelId) -> Option<u64> {
        self.reg.get(pid).and_then(DrawingManager::dragging_primitive_id)
    }

    pub fn is_freehand_tool(&self, pid: ChartPanelId) -> bool {
        self.reg.get(pid).is_some_and(DrawingManager::is_freehand_tool)
    }

    pub fn hit_test(
        &self,
        pid: ChartPanelId,
        x: f64,
        y: f64,
        bars: &[mlc_core::Bar],
        viewport: &Viewport,
        price_scale: &PriceScale,
    ) -> Option<usize> {
        self.reg.get(pid).and_then(|dm| dm.hit_test(x, y, bars, viewport, price_scale))
    }
}

/// Mutable view over the `DrawingRegistry`. Construct via
/// `BackendCore::drawings_svc_mut()`.
pub struct DrawingServiceMut<'a> {
    reg: RwLockWriteGuard<'a, DrawingRegistry>,
}

impl<'a> DrawingServiceMut<'a> {
    pub(crate) fn new(reg: RwLockWriteGuard<'a, DrawingRegistry>) -> Self {
        Self { reg }
    }

    /// THE write entry point. Lazy-creates the DM if missing. Use for every
    /// write that isn't already covered by an explicit method below.
    /// Audit-A note N1. Closure form of [`Self::entry_or_default`] for
    /// one-off callers that don't already hold the service across several
    /// statements.
    pub fn with_panel_mut<R>(&mut self, pid: ChartPanelId, f: impl FnOnce(&mut DrawingManager) -> R) -> R {
        f(self.reg.entry_or_default(pid))
    }

    /// Mutable borrow, lazy-inserting a default `DrawingManager` on miss.
    /// Mirrors `DrawingRegistry::entry_or_default` directly — the chained-
    /// call form (`svc.entry_or_default(pid).method(...)`) most write
    /// call sites already use. The guard stays inside `DrawingServiceMut`;
    /// only this `&mut DrawingManager` borrow, scoped to the service's own
    /// lifetime, escapes — identical in shape to `get_mut`.
    pub fn entry_or_default(&mut self, pid: ChartPanelId) -> &mut DrawingManager {
        self.reg.entry_or_default(pid)
    }

    /// Immutable borrow — explicit-not-found semantics. Mirrors
    /// `DrawingRegistry::get`.
    pub fn get(&self, panel_id: ChartPanelId) -> Option<&DrawingManager> {
        self.reg.get(panel_id)
    }

    /// Mutable borrow — explicit-not-found semantics (no lazy-insert).
    /// Mirrors `DrawingRegistry::get_mut`. Used by the daemon REST routes
    /// where a missing panel must 404 rather than silently create one.
    pub fn get_mut(&mut self, panel_id: ChartPanelId) -> Option<&mut DrawingManager> {
        self.reg.get_mut(panel_id)
    }

    /// Remove the `DrawingManager` for `panel_id`. Mirrors
    /// `DrawingRegistry::remove`.
    pub fn remove(&mut self, panel_id: ChartPanelId) -> Option<DrawingManager> {
        self.reg.remove(panel_id)
    }

    /// Remove all `DrawingManager` entries whose `ChartPanelId` satisfies
    /// `predicate`. Mirrors `DrawingRegistry::drain_matching`.
    pub fn drain_matching<F>(&mut self, predicate: F)
    where
        F: Fn(ChartPanelId) -> bool,
    {
        self.reg.drain_matching(predicate);
    }

    /// Iterate over all `(ChartPanelId, &mut DrawingManager)` pairs. Mirrors
    /// `DrawingRegistry::iter_mut`.
    pub fn iter_mut(&mut self) -> impl Iterator<Item = (ChartPanelId, &mut DrawingManager)> {
        self.reg.iter_mut()
    }

    // ---- Compound op (Audit-A N6) -----------------------------------------

    /// Snapshot the panel's current primitives, then switch the panel to a
    /// new symbol/exchange/account_type and clear the live primitive list.
    /// Optionally restores a previously-saved snapshot (the target symbol's
    /// cached drawings) into the now-empty manager.
    ///
    /// Replaces the 8x-repeated
    /// `snapshot_drawings_for_symbol` + `set_current_symbol_key` +
    /// `clear_all_primitives` + `restore_drawings_into_manager` block.
    /// The caller is still responsible for stashing the returned
    /// `old_primitives` into the window's per-symbol cache
    /// (`ChartContainer::symbol_drawings`, keyed `"symbol:exchange:at"` —
    /// `ChartContainer` lives in `mlc-app`, outside this service's reach)
    /// — this method only owns the `DrawingRegistry`-side half of the
    /// compound.
    pub fn swap_symbol(
        &mut self,
        pid: ChartPanelId,
        symbol: &str,
        exchange: &str,
        account_type: &str,
        restore_snaps: Option<&[PrimitiveSnapshot]>,
    ) -> SwapSymbolResult {
        let dm = self.reg.entry_or_default(pid);
        let old_primitives: Vec<PrimitiveSnapshot> = dm
            .primitives()
            .iter()
            .map(|p| PrimitiveSnapshot::from_primitive(p.as_ref()))
            .collect();

        dm.set_current_symbol_key(symbol, exchange, account_type);
        dm.clear_all_primitives();

        let mut restored = 0usize;
        if let Some(snaps) = restore_snaps {
            if let Ok(reg) = PrimitiveRegistry::global().read() {
                for snap in snaps {
                    if let Some(prim) = reg.from_json(&snap.type_id, &snap.json) {
                        dm.add_external_primitive(prim);
                        restored += 1;
                    }
                }
            }
        }

        SwapSymbolResult { old_primitives, restored }
    }
}

/// Result of [`DrawingServiceMut::swap_symbol`]. `old_primitives` is the
/// snapshot taken from the panel's primitives *before* the clear — the
/// caller stashes it into the window's per-symbol drawing cache
/// (`ChartContainer::symbol_drawings`). `restored` is the count of
/// primitives rebuilt from `restore_snaps`, if any were supplied.
pub struct SwapSymbolResult {
    pub old_primitives: Vec<PrimitiveSnapshot>,
    pub restored: usize,
}
