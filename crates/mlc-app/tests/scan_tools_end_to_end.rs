//! ChartApp-level proof for the owner report 2026-09-25 ("EWA scan doesn't
//! work, Harmonic scan doesn't work") — drives the SAME path a user's mouse
//! does, end to end: toolbar flyout action id -> `ChartToolbarState` tool
//! activation -> `DrawingManager::on_canvas_click` (`ClickBehavior::TwoPoint`
//! two-click/drag creation) -> `PrimitiveRegistry::create` -> the placed
//! scan's own `resolved_scan`/`render`.
//!
//! Root cause (`mlc-core::drawing::primitives_v2::ewa_scan_shared::
//! ewa_scan_window`'s own doc): `ElliottScan`/`HarmonicScan::resolved_scan`
//! used to feed `mli::ewa::swing::extract_pivots` the WHOLE loaded bar
//! series; `mli::ewa::scan::{scan_waves, scan_waves_with_harmonic_tolerance}`
//! silently truncate to their own oldest 500 pivots
//! (`MAX_PIVOTS_PER_SCAN`), so any series carrying more than ~500 `swing_n`
//! pivots (any real multi-month/year chart) had every RECENT pivot dropped
//! before a survey's own `completion_range` could ever select it — a scan
//! dragged over current price action always returned zero hits. Fix: bound
//! the bars fed to `mli::ewa::{swing, scan}` to a lookback window anchored
//! at the survey's own right edge (mirrors `smc_judge::judge_window`, the
//! SAME bounded-window discipline `SmcScan` already uses).
//!
//! `FibScan` never carried this defect (`mli::ewa::fib_swing::
//! select_fib_swings` has no such cap) — included here for completeness/
//! symmetry, not because it needed fixing.
//!
//! Run:
//!   cargo test --release -p mylittlechart-app --test scan_tools_end_to_end -- --nocapture

use std::sync::Arc;

use mlc_app::ChartApp;
use mlc_backend_data_bridge::DataBridge;
use mlc_core::drawing::primitives_v2::config::PropertyType;
use mlc_core::state::Timeframe;
use mlc_core::{Bar, PrimitiveTrait, RenderContext};

fn tf_h1() -> Timeframe {
    Timeframe::h1()
}

// ─── minimal headless RenderContext — no new crate dependency ─────────────
//
// Mirrors uzor's own `NoImageContext` test fixture
// (`uzor/src/core/render/context.rs`'s `#[cfg(test)]` module) — the
// established "boilerplate-minimal mock, no-op every draw call" shape this
// workspace already uses for a `RenderContext` in a test with no real
// canvas backend. Extended with the `mlc_core::RenderContext` supertrait's
// own required methods, `bars()` overridden to return the real series
// so `ctx.bars()` inside a scan's own `render()` resolves timestamps
// correctly instead of the trait's own empty-slice default.
struct NullCtx {
    bars: Vec<Bar>,
}

impl uzor::render::Painter for NullCtx {
    fn save(&mut self) {}
    fn restore(&mut self) {}
    fn translate(&mut self, _x: f64, _y: f64) {}
    fn rotate(&mut self, _angle: f64) {}
    fn scale(&mut self, _x: f64, _y: f64) {}
    fn set_fill_color(&mut self, _color: &str) {}
    fn set_global_alpha(&mut self, _alpha: f64) {}
    fn set_stroke_color(&mut self, _color: &str) {}
    fn set_stroke_width(&mut self, _width: f64) {}
    fn set_line_dash(&mut self, _pattern: &[f64]) {}
    fn set_line_cap(&mut self, _cap: &str) {}
    fn set_line_join(&mut self, _join: &str) {}
    fn begin_path(&mut self) {}
    fn move_to(&mut self, _x: f64, _y: f64) {}
    fn line_to(&mut self, _x: f64, _y: f64) {}
    fn close_path(&mut self) {}
    fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn arc(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64) {}
    fn ellipse(&mut self, _cx: f64, _cy: f64, _rx: f64, _ry: f64, _rot: f64, _s: f64, _e: f64) {}
    fn quadratic_curve_to(&mut self, _cpx: f64, _cpy: f64, _x: f64, _y: f64) {}
    fn bezier_curve_to(&mut self, _cp1x: f64, _cp1y: f64, _cp2x: f64, _cp2y: f64, _x: f64, _y: f64) {}
    fn stroke(&mut self) {}
    fn fill(&mut self) {}
}
impl uzor::render::TextRenderer for NullCtx {
    fn set_font(&mut self, _font: &str) {}
    fn set_text_align(&mut self, _align: uzor::render::TextAlign) {}
    fn set_text_baseline(&mut self, _baseline: uzor::render::TextBaseline) {}
    fn fill_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    fn stroke_text(&mut self, _text: &str, _x: f64, _y: f64) {}
}
impl uzor::render::TextMetrics for NullCtx {
    fn measure_text(&self, _text: &str) -> f64 {
        0.0
    }
    fn text_bounds(&self, _text: &str, _font: &str) -> uzor::render::TextBounds {
        uzor::render::TextBounds { x: 0.0, y: 0.0, w: 0.0, h: 0.0, ascent: 0.0, descent: 0.0 }
    }
}
impl uzor::render::Masking for NullCtx {
    fn clip(&mut self) {}
}
impl uzor::render::Effects for NullCtx {}
impl uzor::render::ShapeHelpers for NullCtx {
    fn fill_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    fn stroke_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
}
impl uzor::render::GradientPainter for NullCtx {}
impl uzor::render::UiEffectHelpers for NullCtx {}
impl uzor::render::BatchPainter for NullCtx {}
// `dpr()` belongs to the SUPERTRAIT `uzor::render::RenderContext` (composes
// Painter/TextRenderer/.../BatchPainter and adds `dpr`) — `mlc_core::
// RenderContext` extends it rather than redeclaring `dpr` itself, so it
// needs its own, separate `impl` block.
impl uzor::render::RenderContext for NullCtx {
    fn dpr(&self) -> f64 {
        1.0
    }
}
impl RenderContext for NullCtx {
    fn chart_width(&self) -> f64 {
        1000.0
    }
    fn chart_height(&self) -> f64 {
        600.0
    }
    fn bar_to_x(&self, bar: f64) -> f64 {
        bar * 8.0
    }
    fn price_to_y(&self, price: f64) -> f64 {
        600.0 - price
    }
    fn bars(&self) -> &[Bar] {
        &self.bars
    }
    fn set_coordinate_space(
        &mut self,
        _chart_width: f64,
        _chart_height: f64,
        _view_start: f64,
        _bar_spacing: f64,
        _price_min: f64,
        _price_max: f64,
    ) {
    }
}

// ─── deterministic, dependency-free noise (no `rand` crate) ───────────────

/// A tiny LCG — "realistic noisy series" needs pseudo-random OHLC without
/// pulling in a new dev-dependency for one test file.
struct Lcg(u64);
impl Lcg {
    fn next_unit(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1_442_695_040_888_963_407);
        ((self.0 >> 11) as f64) / (1u64 << 53) as f64
    }
}

/// A bounded pseudo-random walk — noisy OHLC standing in for "years of
/// ordinary price action" ahead of a genuine pattern, the SAME role
/// `elliott_scan.rs`'s/`harmonic_scan.rs`'s own `filler_zigzag_ohlc` plays
/// at the unit level, just non-repeating here (a real chart's own bars
/// never march in a perfect zigzag).
fn noisy_walk_ohlc(bars: usize, seed: u64) -> Vec<(f64, f64, f64, f64)> {
    let mut rng = Lcg(seed);
    let mut price = 1_000.0;
    let mut out = Vec::with_capacity(bars);
    for _ in 0..bars {
        let open = price;
        let drift = (rng.next_unit() - 0.5) * 6.0;
        price = (price + drift).max(50.0);
        let close = price;
        let high = open.max(close) + rng.next_unit() * 2.0;
        let low = (open.min(close) - rng.next_unit() * 2.0).max(1.0);
        out.push((open, high, low, close));
    }
    out
}

fn ramp(from: f64, to: f64, steps: usize, spread: f64) -> Vec<(f64, f64, f64, f64)> {
    (1..=steps)
        .map(|k| {
            let t = k as f64 / (steps as f64 + 1.0);
            let level = from + (to - from) * t;
            let (high, low) = if to >= from { (level, level - spread) } else { (level + spread, level) };
            (level, high, low, level)
        })
        .collect()
}

fn pivot_bar(is_high: bool, price: f64, spread: f64) -> (f64, f64, f64, f64) {
    if is_high {
        (price - spread / 2.0, price, price - spread, price - spread / 2.0)
    } else {
        (price + spread / 2.0, price + spread, price, price + spread / 2.0)
    }
}

/// A textbook 5-wave impulse — the SAME pivot prices `mlc-core`'s own
/// `elliott_scan.rs` test fixture uses (itself matching `mli::ewa::scan`'s
/// own `textbook_impulse_is_found_and_scores_well` fixture).
fn textbook_impulse_ohlc() -> Vec<(f64, f64, f64, f64)> {
    let is_high = [false, true, false, true, false, true];
    let prices = [100.0, 110.0, 105.0, 121.18, 115.0, 125.0];
    let steps = 3usize;
    let spread = 0.2;
    let mut ohlc = Vec::new();
    let lead_from = if is_high[0] { prices[0] - 8.0 } else { prices[0] + 8.0 };
    ohlc.extend(ramp(lead_from, prices[0], 2, spread));
    for i in 0..prices.len() {
        if i > 0 {
            ohlc.extend(ramp(prices[i - 1], prices[i], steps, spread));
        }
        ohlc.push(pivot_bar(is_high[i], prices[i], spread));
    }
    let last_price = *prices.last().unwrap_or(&0.0);
    let trail_to = if *is_high.last().unwrap_or(&false) { last_price - 8.0 } else { last_price + 8.0 };
    ohlc.extend(ramp(last_price, trail_to, 2, spread));
    ohlc
}

/// A textbook Gartley XABCD — the SAME pivot prices `mlc-core`'s own
/// `harmonic_scan.rs` test fixture uses.
fn textbook_gartley_ohlc() -> Vec<(f64, f64, f64, f64)> {
    let is_high = [false, true, false, true, false];
    let prices = [1000.0, 1100.0, 1038.2, 1080.2, 1021.4];
    let steps = 3usize;
    let spread = 0.2;
    let mut ohlc = Vec::new();
    let lead_from = if is_high[0] { prices[0] - 8.0 } else { prices[0] + 8.0 };
    ohlc.extend(ramp(lead_from, prices[0], 2, spread));
    for i in 0..prices.len() {
        if i > 0 {
            ohlc.extend(ramp(prices[i - 1], prices[i], steps, spread));
        }
        ohlc.push(pivot_bar(is_high[i], prices[i], spread));
    }
    let last_price = *prices.last().unwrap_or(&0.0);
    let trail_to = if *is_high.last().unwrap_or(&false) { last_price - 8.0 } else { last_price + 8.0 };
    ohlc.extend(ramp(last_price, trail_to, 2, spread));
    ohlc
}

/// One clear up-swing then a deeper down-swing — `FibScan`'s default
/// `LargestSwing` mode always resolves the single biggest segment in the
/// window, so this just needs one unambiguous largest leg.
fn clear_swing_ohlc() -> Vec<(f64, f64, f64, f64)> {
    let mut ohlc = Vec::new();
    ohlc.extend(ramp(500.0, 500.0, 2, 0.2));
    ohlc.push(pivot_bar(false, 500.0, 0.2));
    ohlc.extend(ramp(500.0, 700.0, 4, 0.2));
    ohlc.push(pivot_bar(true, 700.0, 0.2));
    ohlc.extend(ramp(700.0, 560.0, 4, 0.2));
    ohlc.push(pivot_bar(false, 560.0, 0.2));
    ohlc.extend(ramp(560.0, 600.0, 2, 0.2));
    ohlc
}

fn to_bars(ohlc: Vec<(f64, f64, f64, f64)>) -> Vec<Bar> {
    ohlc.into_iter().enumerate().map(|(i, (o, h, l, c))| Bar::new(i as i64, o, h, l, c)).collect()
}

// ─── ChartApp scaffolding (mirrors chart_app_smoke.rs's own Test 3 setup) ─

async fn new_app_with_bars(symbol: &str, bars: Vec<Bar>) -> (ChartApp, mlc_core::ids::BubbleId) {
    let (bridge, _live_rx, _connector_ready_rx) = DataBridge::new().await;
    let bridge = Arc::new(bridge);
    let core = mlc_backend::BackendCore::new(bridge, None);
    let mut app = ChartApp::new_empty(core);

    let chart_id = app.panel_app.panel_grid.active_container_id().expect("new_empty seeds one active window");
    let bid = app.create_auto_bubble([0.0, 0.0, 0.0, 0.0], symbol.to_string(), tf_h1());
    app.attach_chart_to_tag(chart_id, bid);
    {
        let win = app.panel_app.panel_grid.containers_mut().get_mut(&chart_id).expect("active window");
        win.auto_tag_id = bid;
        win.current_tag_id = bid;
    }
    app.core.bubbles_mut().set_bid_instrument_full(
        bid, symbol.to_string(), "binance".to_string(), "S".to_string(), tf_h1(),
    );
    app.core.bubbles_mut().replace_bars(bid, bars);
    (app, bid)
}

/// Activate a drawing tool via the EXACT widget-id shapes the real toolbar
/// flyout sends (`mlc_core::toolbar`'s own `DropdownItemDef::action` rows):
/// a click on the group button (`"toolbar:{dropdown_id}"`) opens the
/// flyout, then a click on the action row (`"dropdown:{dropdown_id}:
/// {action_id}"`) selects it — the SAME `dispatch_panel_click` path
/// `chart_app_smoke.rs`'s own R23 test (`toolbar_dropdown_open_closes_the_
/// open_settings_modal`) already exercises for `"cursor_tools"`.
fn activate_tool_via_flyout(app: &mut ChartApp, dropdown_id: &str, action_id: &str) {
    app.dispatch_panel_click(&format!("toolbar:{dropdown_id}"), 0.0, 0.0);
    assert_eq!(
        app.panel_app.toolbar_state.open_dropdown_id.as_deref(),
        Some(dropdown_id),
        "the toolbar group button did not open its own flyout — action id \
         routing changed, this test is not exercising the real path"
    );
    app.dispatch_panel_click(&format!("dropdown:{dropdown_id}:{action_id}"), 0.0, 0.0);
}

/// Two clicks bracketing the WHOLE loaded series — the drag/click creation
/// behaviour every `ClickBehavior::TwoPoint` scan shares (`DrawingManager::
/// on_canvas_click`, `mlc-backend/src/drawing/manager.rs`): the first call
/// primes `DrawingState::Creating`, the second finalizes the primitive via
/// `PrimitiveRegistry::create` — the SAME sequence a real mouse-down then
/// mouse-up drag funnels into via `ChartOutputAction::DrawingClick`.
fn drag_two_point_scan_over_whole_series(app: &mut ChartApp, bid: mlc_core::ids::BubbleId, bar_count: usize) {
    let mut drawings = app.core.drawings_mut();
    let primed = drawings.on_canvas_click(bid, 0, 0.0);
    assert!(!primed, "first click must only prime creation, not finalize it");
    let finalized = drawings.on_canvas_click(bid, (bar_count as i64 - 1) * 1000, 0.0);
    assert!(finalized, "second click must finalize the primitive");
}

/// Total-hits `StatRow` every one of the five scan primitives' own
/// `style_properties()` declares first in its `stat_block` — read through
/// `&dyn PrimitiveTrait` alone (no downcasting to the concrete scan type
/// needed): `ElliottScan`/`HarmonicScan::cached_hits.len()`, `FibScan::
/// cached_swings.len()`, `SmcScan::cached_hits.total()` all land here.
fn total_hits_stat(prim: &dyn PrimitiveTrait) -> Option<usize> {
    prim.style_properties().into_iter().find_map(|p| match p.prop_type {
        PropertyType::Stat { rows } => rows.first().and_then(|r| r.value.parse::<usize>().ok()),
        _ => None,
    })
}

/// Re-run the SAME production write path every real bar update takes
/// (`BubbleStore::replace_bars` -> `bar_derived_cache::
/// refresh_bar_derived_caches`) so the just-placed scan's own `cached_hits`/
/// `cached_swings` populate — mirrors a live bar arriving after the scan
/// was drawn, the ordinary case in production (a survey placed on an
/// already-loaded chart refreshes on the next tick, never only at the
/// instant it was drawn).
fn refresh_caches(app: &mut ChartApp, bid: mlc_core::ids::BubbleId, bars: Vec<Bar>) {
    app.core.bubbles_mut().replace_bars(bid, bars);
}

// ─── Elliott ────────────────────────────────────────────────────────────

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn elliott_scan_tool_via_flyout_finds_a_textbook_impulse_and_renders() {
    let bars = to_bars(textbook_impulse_ohlc());
    let bar_count = bars.len();
    let (mut app, bid) = new_app_with_bars("ELLIOTTUSDT", bars.clone()).await;

    activate_tool_via_flyout(&mut app, "pattern_tools", "elliott_scan");
    drag_two_point_scan_over_whole_series(&mut app, bid, bar_count);
    refresh_caches(&mut app, bid, bars.clone());

    let drawings = app.core.drawings();
    assert_eq!(drawings.count(bid), 1, "exactly one primitive must have been created");
    let prim = &drawings.primitives(bid)[0];
    assert_eq!(prim.type_id(), "elliott_scan", "the flyout action id must construct an ElliottScan");

    let hits = total_hits_stat(prim.as_ref());
    assert!(hits.unwrap_or(0) >= 1, "ElliottScan found no hits on a textbook impulse: {hits:?}");

    let mut ctx = NullCtx { bars };
    prim.render(&mut ctx, false);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn elliott_scan_finds_hits_on_a_realistic_noisy_long_series_with_default_thresholds() {
    let mut ohlc = noisy_walk_ohlc(2_500, 0xE11107);
    ohlc.extend(textbook_impulse_ohlc());
    let bars = to_bars(ohlc);
    let bar_count = bars.len();
    let (mut app, bid) = new_app_with_bars("ELLIOTTUSDT", bars.clone()).await;

    // No per-drawing override of swing_n/min_score/enabled_kinds — the
    // flyout-placed scan must find this on whatever `EwaEngineConfig`'s own
    // shipped defaults seed it with, the exact posture a real user's click
    // has.
    activate_tool_via_flyout(&mut app, "pattern_tools", "elliott_scan");
    drag_two_point_scan_over_whole_series(&mut app, bid, bar_count);
    refresh_caches(&mut app, bid, bars);

    let drawings = app.core.drawings();
    let prim = &drawings.primitives(bid)[0];
    assert_eq!(prim.type_id(), "elliott_scan");
    let hits = total_hits_stat(prim.as_ref());
    assert!(
        hits.unwrap_or(0) >= 1,
        "ElliottScan with DEFAULT thresholds found nothing on a {bar_count}-bar realistic \
         series with a genuine impulse in it: {hits:?} — the owner report reproduces"
    );
}

// ─── Harmonic ───────────────────────────────────────────────────────────

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn harmonic_scan_tool_via_flyout_finds_a_textbook_gartley_and_renders() {
    let bars = to_bars(textbook_gartley_ohlc());
    let bar_count = bars.len();
    let (mut app, bid) = new_app_with_bars("HARMONICUSDT", bars.clone()).await;

    activate_tool_via_flyout(&mut app, "pattern_tools", "harmonic_scan");
    drag_two_point_scan_over_whole_series(&mut app, bid, bar_count);
    refresh_caches(&mut app, bid, bars.clone());

    let drawings = app.core.drawings();
    assert_eq!(drawings.count(bid), 1);
    let prim = &drawings.primitives(bid)[0];
    assert_eq!(prim.type_id(), "harmonic_scan", "the flyout action id must construct a HarmonicScan");

    let hits = total_hits_stat(prim.as_ref());
    assert!(hits.unwrap_or(0) >= 1, "HarmonicScan found no hits on a textbook Gartley: {hits:?}");

    let mut ctx = NullCtx { bars };
    prim.render(&mut ctx, false);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn harmonic_scan_finds_hits_on_a_realistic_noisy_long_series_with_default_thresholds() {
    let mut ohlc = noisy_walk_ohlc(2_500, 0x4A0710);
    ohlc.extend(textbook_gartley_ohlc());
    let bars = to_bars(ohlc);
    let bar_count = bars.len();
    let (mut app, bid) = new_app_with_bars("HARMONICUSDT", bars.clone()).await;

    activate_tool_via_flyout(&mut app, "pattern_tools", "harmonic_scan");
    drag_two_point_scan_over_whole_series(&mut app, bid, bar_count);
    refresh_caches(&mut app, bid, bars);

    let drawings = app.core.drawings();
    let prim = &drawings.primitives(bid)[0];
    assert_eq!(prim.type_id(), "harmonic_scan");
    let hits = total_hits_stat(prim.as_ref());
    assert!(
        hits.unwrap_or(0) >= 1,
        "HarmonicScan with DEFAULT thresholds found nothing on a {bar_count}-bar realistic \
         series with a genuine Gartley in it: {hits:?} — the owner report reproduces"
    );
}

// ─── Fib (suspect, but structurally unaffected — see module doc) ──────────

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn fib_scan_tool_via_flyout_finds_a_clear_swing_and_renders() {
    let bars = to_bars(clear_swing_ohlc());
    let bar_count = bars.len();
    let (mut app, bid) = new_app_with_bars("FIBUSDT", bars.clone()).await;

    activate_tool_via_flyout(&mut app, "fib_tools", "fib_scan");
    drag_two_point_scan_over_whole_series(&mut app, bid, bar_count);
    refresh_caches(&mut app, bid, bars.clone());

    let drawings = app.core.drawings();
    assert_eq!(drawings.count(bid), 1);
    let prim = &drawings.primitives(bid)[0];
    assert_eq!(prim.type_id(), "fib_scan", "the flyout action id must construct a FibScan");

    let hits = total_hits_stat(prim.as_ref());
    assert!(hits.unwrap_or(0) >= 1, "FibScan found no swing on a clear up/down leg: {hits:?}");

    let mut ctx = NullCtx { bars };
    prim.render(&mut ctx, false);
}

#[cfg_attr(target_arch = "wasm32", wasm_bindgen_test::wasm_bindgen_test)]
#[cfg_attr(not(target_arch = "wasm32"), tokio::test(flavor = "multi_thread", worker_threads = 2))]
async fn fib_scan_finds_a_swing_on_a_realistic_noisy_long_series_with_default_thresholds() {
    let ohlc = noisy_walk_ohlc(2_500, 0xF1B5CA);
    let bars = to_bars(ohlc);
    let bar_count = bars.len();
    let (mut app, bid) = new_app_with_bars("FIBUSDT", bars.clone()).await;

    activate_tool_via_flyout(&mut app, "fib_tools", "fib_scan");
    drag_two_point_scan_over_whole_series(&mut app, bid, bar_count);
    refresh_caches(&mut app, bid, bars);

    let drawings = app.core.drawings();
    let prim = &drawings.primitives(bid)[0];
    assert_eq!(prim.type_id(), "fib_scan");
    let hits = total_hits_stat(prim.as_ref());
    assert!(
        hits.unwrap_or(0) >= 1,
        "FibScan with DEFAULT thresholds (LargestSwing) found no swing at all on a \
         {bar_count}-bar random walk: {hits:?} — a bounded random walk always has SOME \
         largest segment, so this would indicate the default mode itself is broken"
    );
}

