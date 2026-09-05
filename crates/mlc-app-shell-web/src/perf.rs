//! Always-on wasm-only frame/persistence instrumentation for the web shell —
//! MEASUREMENT ONLY. Owner's ruling 2026-09-05: dev tooling must not ship
//! inside the product. This module never writes to the DOM and never calls
//! `console.*`; it just keeps a rolling record and answers when asked.
//! Display/collection live entirely in dig2browser's CDP driver
//! (`dig2browser/tools/cdp-control/cdp_control.py`, `perf` and `hud`
//! verbs) — that tool installs its own on-page HUD `<div>` and polls
//! [`PerfMeter::snapshot_js`] over CDP `Runtime.evaluate`; nothing here is
//! user-visible or console-visible in a shipped build.
//!
//! # Why this exists
//!
//! Owner report: the web app "half-freezes" — frames keep ticking, but the
//! mouse half-sticks, the crosshair stops drawing, clicks either don't land
//! or land and stick, then it passes; feels like a cumulative RAM hit;
//! native is instant. Static analysis produced hypotheses, not proof:
//!
//! - H1: a click sets dirty flags and the SAME frame's
//!   `persistence::save_persistent_state` writes ALL presets
//!   (`to_string_pretty` → hex encode → synchronous `localStorage.setItem`
//!   per preset) plus a `build_window_state`/`collect_state_into_preset`
//!   deep clone.
//! - H2: a `.to_vec()` of the whole bar series happens on click.
//! - H3: a tick backlog gets drained in the click frame.
//! - H4: a full repaint runs every frame regardless.
//!
//! This module does not fix any of that — it MEASURES it, so the freeze can
//! be attributed to a phase (and, for persistence, to bytes/writes) instead
//! of guessed at. `mlc-app` itself is not instrumented (out of scope,
//! `mylittlechart/CLAUDE.md` crate contract) — the shell times around the
//! calls it already makes into `ChartApp`/`persistence`.
//!
//! # Cost
//!
//! [`FrameTimer`] is a handful of `performance.now()` reads (JS property
//! gets, not allocations) per frame, folded into a fixed-size ring buffer
//! ([`PerfMeter`]) that is allocated once at boot and never grows. A second,
//! much smaller ring keeps the last [`LONG_FRAME_RING_CAP`] frames whose
//! total exceeded [`LONG_FRAME_MS`] — a plain `Copy` push, no formatting.
//! Nothing is written anywhere per frame beyond those two ring pushes: no
//! DOM, no `console.*`, no rate-limited timers. The only place a `String`
//! or a JS object gets built at all is [`PerfMeter::snapshot_js`], and only
//! when `window.__MLC_PERF()` is actually called from outside the page (see
//! `lib.rs::run_render_loop` for where that global is installed).
//!
//! # Phase boundaries (see `lib.rs::run_render_loop`'s RAF closure)
//!
//! `Events` (DOM → `dispatch_event`) → `Tick`, split into three sub-phases
//! so a 2.2s "tick" (owner report 2026-09-05) points at ONE of them instead
//! of the whole bucket: `tick_chart` (`chart.tick`), `tick_drains`
//! (watchlist + template action drains) and `tick_cloud` (cloud session
//! `drive`) → `Drains` (cold-apply/theme/clipboard/profile-command drains,
//! the `MLC_DEBUG` diag block, and — after `Render` — screenshot capture,
//! live-share/REST-cure drains, background-warm) → `Persist`
//! (`persistence::save_persistent_state`) → `Render`, split into four
//! sub-phases matching `render_frame`'s actual sections (`lib.rs`):
//! `render_setup` (canvas resize/ctx creation + the monitoring-panel
//! sample), `render_chart` (tab-strip state sync + `chart.render`, the base
//! panel/bubble paint), `render_overlay` (`render_overlay` /
//! `render_toolbar_only` / `render_modal_layer` / panel overlay popups) and
//! `render_present` (the tooltip re-arm + the chrome-strip paint —
//! `mlc_app_input_shell::render` + backdrop dim + tab list + tooltips —
//! the last layer composited before the frame is done; canvas2d has no
//! separate `put_image_data`/blit step to time here, it draws straight
//! onto the visible context). `Drains` accumulates across all three of its
//! disjoint spans; the other phases (and their sub-phases) are contiguous.
//! This intentionally groups every non-tick/persist/render bookkeeping
//! drain into one bucket — the WHERE brief names screenshot/share/nick
//! drains as "drains", even though a couple of them execute after the
//! render call in the actual code order.
//!
//! Network (fetch) timing is deliberately absent: this shell never calls
//! `fetch` itself — all cloud/archive-proxy calls go through
//! `mlc_cloud::FetchTransport` (a different crate, out of scope per the
//! task brief) — so there is no single shell-owned call site to wrap.

use wasm_bindgen::{JsCast, JsValue};

/// Frames kept in the ring buffer — at a steady 60fps this is ~5s of
/// history, which doubles as the "5s window" the owner's stats are over.
const FRAME_RING_CAP: usize = 300;

/// Number of named phases tracked per frame — must match [`Phase`]'s variant
/// count; used to size `FrameTimer::totals`.
const PHASE_COUNT: usize = 10;

/// A frame counts as "long" for the long-frame ring below this total.
const LONG_FRAME_MS: f32 = 100.0;
/// Threshold for the window's `>50ms` bucket count.
const SLOW_FRAME_MS: f32 = 50.0;
/// Long-frame ring capacity — the last N frames whose total exceeded
/// [`LONG_FRAME_MS`], oldest first in [`PerfMeter::snapshot_js`]'s output.
const LONG_FRAME_RING_CAP: usize = 20;

/// `performance.now()` — matches the pattern already used throughout
/// `lib.rs` (`win.performance().map(|p| p.now())`) rather than caching a
/// `Window`/`Performance` handle across frames.
pub fn now_ms() -> f64 {
    web_sys::window()
        .and_then(|w| w.performance())
        .map(|p| p.now())
        .unwrap_or(0.0)
}

/// Wasm linear memory size in MiB — `wasm_bindgen::memory()` (the module's
/// own `WebAssembly.Memory` export) → `buffer()` → `ArrayBuffer.byteLength`.
/// This is the "high-water" number the `alloc_probe` module's doc comment
/// already cross-references (`__MLC_WASM_MEM_MB()` in `index.html`); wasm
/// linear memory only grows, so a climbing value here alongside a flat
/// `alloc_probe::LIVE` is dlmalloc fragmentation, not real retention.
pub fn wasm_memory_mb() -> f64 {
    wasm_bindgen::memory()
        .dyn_into::<js_sys::WebAssembly::Memory>()
        .ok()
        .map(|mem| mem.buffer())
        .and_then(|buf| buf.dyn_into::<js_sys::ArrayBuffer>().ok())
        .map(|ab| ab.byte_length() as f64 / (1024.0 * 1024.0))
        .unwrap_or(0.0)
}

/// Chrome/Chromium-only `performance.memory.usedJSHeapSize`, in MiB.
/// `None` on every other engine (Firefox/Safari expose no such property) —
/// this is a "when present" bonus number, never a load-bearing metric.
pub fn js_heap_used_mb() -> Option<f64> {
    let win = web_sys::window()?;
    let performance = win.performance()?;
    let mem = js_sys::Reflect::get(&performance, &"memory".into()).ok()?;
    let used = js_sys::Reflect::get(&mem, &"usedJSHeapSize".into()).ok()?;
    let bytes = used.as_f64()?;
    if bytes <= 0.0 {
        return None;
    }
    Some(bytes / (1024.0 * 1024.0))
}

/// Named phase buckets a [`FrameTimer`] accumulates into. Order matches the
/// module doc's phase-boundary writeup. `Tick` and `Render` are each split
/// into sub-phases (owner report 2026-09-05: a bare "tick=2254.7ms" or
/// "render=760.7ms" names a bucket, not an address) so `dominant_phase` can
/// point at the actual slow step.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Events = 0,
    TickChart = 1,
    TickDrains = 2,
    TickCloud = 3,
    Drains = 4,
    Persist = 5,
    RenderSetup = 6,
    RenderChart = 7,
    RenderOverlay = 8,
    RenderPresent = 9,
}

/// One frame's phase breakdown — `Copy` so the ring buffer is a plain fixed
/// array with no per-frame heap traffic.
#[derive(Clone, Copy, Default)]
struct FrameSample {
    events_ms: f32,
    tick_chart_ms: f32,
    tick_drains_ms: f32,
    tick_cloud_ms: f32,
    drains_ms: f32,
    persist_ms: f32,
    render_setup_ms: f32,
    render_chart_ms: f32,
    render_overlay_ms: f32,
    render_present_ms: f32,
    total_ms: f32,
    events_drained: u16,
}

/// Which named sub-phase had the largest share of a given [`FrameSample`].
fn dominant_phase(s: &FrameSample) -> &'static str {
    let mut name = "events";
    let mut max = s.events_ms;
    if s.tick_chart_ms > max {
        max = s.tick_chart_ms;
        name = "tick_chart";
    }
    if s.tick_drains_ms > max {
        max = s.tick_drains_ms;
        name = "tick_drains";
    }
    if s.tick_cloud_ms > max {
        max = s.tick_cloud_ms;
        name = "tick_cloud";
    }
    if s.drains_ms > max {
        max = s.drains_ms;
        name = "drains";
    }
    if s.persist_ms > max {
        max = s.persist_ms;
        name = "persist";
    }
    if s.render_setup_ms > max {
        max = s.render_setup_ms;
        name = "render_setup";
    }
    if s.render_chart_ms > max {
        max = s.render_chart_ms;
        name = "render_chart";
    }
    if s.render_overlay_ms > max {
        max = s.render_overlay_ms;
        name = "render_overlay";
    }
    if s.render_present_ms > max {
        name = "render_present";
    }
    name
}

/// One entry in the long-frame ring — captured whenever a frame's
/// `total_ms` exceeds [`LONG_FRAME_MS`]. Display lives entirely in
/// `dig2browser/tools/cdp-control/cdp_control.py` now; this struct only
/// carries what [`PerfMeter::snapshot_js`] needs to describe the frame.
#[derive(Clone, Copy, Default)]
struct LongFrameEntry {
    /// `performance.now()` at the moment this frame was recorded (shared
    /// with `FrameTimer::finish`'s own `now` read — no extra JS call).
    at_ms: f64,
    sample: FrameSample,
    dominant: &'static str,
}

/// Per-frame stopwatch — created at the top of the RAF closure, `enter()`
/// called at each phase boundary, `finish()` folds the result into the
/// shared [`PerfMeter`]. Each method is exactly one `performance.now()`
/// read; a full frame is `start` + up to 10 `enter` (one per `Tick`/`Render`
/// sub-phase boundary) + `finish` — still the "handful of
/// `performance.now()` calls" the task brief asks for, none of them
/// allocating.
pub struct FrameTimer {
    start_ms: f64,
    last_mark_ms: f64,
    current: Phase,
    totals: [f32; PHASE_COUNT],
    events_drained: u16,
}

impl FrameTimer {
    pub fn start() -> Self {
        let now = now_ms();
        Self {
            start_ms: now,
            last_mark_ms: now,
            current: Phase::Events,
            totals: [0.0; PHASE_COUNT],
            events_drained: 0,
        }
    }

    /// Count of DOM events drained this frame — set once, right after
    /// `poll_events()`, before the dispatch loop consumes the `Vec`.
    pub fn set_events_drained(&mut self, n: usize) {
        self.events_drained = n.min(u16::MAX as usize) as u16;
    }

    /// Close out the current phase and start the next one. Called multiple
    /// times with `Phase::Drains` across the frame (cold-apply/theme/
    /// clipboard/profile-command drains, the diag block, and the post-
    /// render drains all accumulate into the same bucket) — see the module
    /// doc's phase-boundary writeup for why.
    pub fn enter(&mut self, phase: Phase) {
        let now = now_ms();
        let delta = (now - self.last_mark_ms).max(0.0) as f32;
        self.totals[self.current as usize] += delta;
        self.current = phase;
        self.last_mark_ms = now;
    }

    /// Close out the final phase, compute `total_ms` from wall-clock start
    /// to now (which also captures anything NOT between two `enter()` calls,
    /// e.g. the RAF-scheduling call itself), and record the sample.
    pub fn finish(mut self, meter: &mut PerfMeter) {
        let now = now_ms();
        let delta = (now - self.last_mark_ms).max(0.0) as f32;
        self.totals[self.current as usize] += delta;
        let total_ms = (now - self.start_ms).max(0.0) as f32;
        meter.record_frame(
            FrameSample {
                events_ms: self.totals[Phase::Events as usize],
                tick_chart_ms: self.totals[Phase::TickChart as usize],
                tick_drains_ms: self.totals[Phase::TickDrains as usize],
                tick_cloud_ms: self.totals[Phase::TickCloud as usize],
                drains_ms: self.totals[Phase::Drains as usize],
                persist_ms: self.totals[Phase::Persist as usize],
                render_setup_ms: self.totals[Phase::RenderSetup as usize],
                render_chart_ms: self.totals[Phase::RenderChart as usize],
                render_overlay_ms: self.totals[Phase::RenderOverlay as usize],
                render_present_ms: self.totals[Phase::RenderPresent as usize],
                total_ms,
                events_drained: self.events_drained,
            },
            now,
        );
    }
}

/// p50/p95/max/long-frame-counts/dominant-phase over the current ring
/// buffer contents (up to [`FRAME_RING_CAP`] frames — the "5s window").
struct WindowStats {
    p50: f32,
    p95: f32,
    max: f32,
    over_50: u32,
    over_100: u32,
    worst_phase: &'static str,
}

/// Nearest-rank percentile over an already-sorted slice.
fn percentile(sorted: &[f32], p: f32) -> f32 {
    if sorted.is_empty() {
        return 0.0;
    }
    let idx = ((sorted.len() - 1) as f32 * p).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

/// Builds the flat `{events, tick_chart, ..., render_present}` object shared
/// by `snapshot_js`'s `"last"` (which adds `total`/`events_drained` on top)
/// and each `"long_frames"` entry's nested `"phases"`.
fn phases_to_js(s: &FrameSample) -> js_sys::Object {
    let obj = js_sys::Object::new();
    let entries: [(&str, f32); 10] = [
        ("events", s.events_ms),
        ("tick_chart", s.tick_chart_ms),
        ("tick_drains", s.tick_drains_ms),
        ("tick_cloud", s.tick_cloud_ms),
        ("drains", s.drains_ms),
        ("persist", s.persist_ms),
        ("render_setup", s.render_setup_ms),
        ("render_chart", s.render_chart_ms),
        ("render_overlay", s.render_overlay_ms),
        ("render_present", s.render_present_ms),
    ];
    for (key, val) in entries {
        set_prop(&obj, key, JsValue::from(val as f64));
    }
    obj
}

/// `Reflect::set` with the error dropped — every property set below is on an
/// object this module just created, so the only failure mode is "impossible".
fn set_prop(obj: &js_sys::Object, key: &str, val: JsValue) {
    let _ = js_sys::Reflect::set(obj, &JsValue::from_str(key), &val);
}

/// Shared frame/persistence instrumentation state — one instance lives on
/// [`crate::WebAppState`] (same sharing shape as `bridge`/`profile_manager`:
/// a plain field behind the shell's single `Rc<RefCell<WebAppState>>`, no
/// separate handle needed since `FrameTimer::enter` never touches it — only
/// `finish()` and `persistence::save_persistent_state` do).
pub struct PerfMeter {
    frames: [FrameSample; FRAME_RING_CAP],
    frame_head: usize,
    frame_len: usize,

    /// Most recently recorded sample — the `"last"` key of `snapshot_js`'s
    /// output, kept separately so a snapshot never has to reach back into
    /// the ring at `frame_head - 1`.
    last_sample: FrameSample,

    long_frames: [LongFrameEntry; LONG_FRAME_RING_CAP],
    long_frame_head: usize,
    long_frame_len: usize,

    persist_total_writes: u64,
    last_persist_ms: f32,
    max_persist_ms: f32,

    boot_wasm_mb: f64,
}

impl PerfMeter {
    pub fn new() -> Self {
        Self {
            frames: [FrameSample::default(); FRAME_RING_CAP],
            frame_head: 0,
            frame_len: 0,
            last_sample: FrameSample::default(),
            long_frames: [LongFrameEntry::default(); LONG_FRAME_RING_CAP],
            long_frame_head: 0,
            long_frame_len: 0,
            persist_total_writes: 0,
            last_persist_ms: 0.0,
            max_persist_ms: 0.0,
            boot_wasm_mb: wasm_memory_mb(),
        }
    }

    fn push_frame(&mut self, sample: FrameSample) {
        self.frames[self.frame_head] = sample;
        self.frame_head = (self.frame_head + 1) % FRAME_RING_CAP;
        if self.frame_len < FRAME_RING_CAP {
            self.frame_len += 1;
        }
    }

    fn push_long_frame(&mut self, entry: LongFrameEntry) {
        self.long_frames[self.long_frame_head] = entry;
        self.long_frame_head = (self.long_frame_head + 1) % LONG_FRAME_RING_CAP;
        if self.long_frame_len < LONG_FRAME_RING_CAP {
            self.long_frame_len += 1;
        }
    }

    /// Oldest-first iterator over the long-frame ring's current contents —
    /// `snapshot_js`'s `"long_frames"` array is specified oldest-first, so
    /// this walks starting at `long_frame_head` once the ring has wrapped
    /// (that slot holds the next one to be overwritten, i.e. the oldest
    /// still-retained entry) and from `0` while it hasn't.
    fn long_frames_oldest_first(&self) -> impl Iterator<Item = &LongFrameEntry> {
        let n = self.long_frame_len;
        let start = if n < LONG_FRAME_RING_CAP { 0 } else { self.long_frame_head };
        (0..n).map(move |i| &self.long_frames[(start + i) % LONG_FRAME_RING_CAP])
    }

    /// Stack-allocated (no heap traffic) percentile/max/long-frame-count
    /// pass over the ring buffer's current contents. Buffer order doesn't
    /// matter for these stats, so `frames[..frame_len]` is always exactly
    /// the current window regardless of where `frame_head` wrapped to.
    fn window_stats(&self) -> WindowStats {
        let n = self.frame_len;
        if n == 0 {
            return WindowStats { p50: 0.0, p95: 0.0, max: 0.0, over_50: 0, over_100: 0, worst_phase: "-" };
        }
        let mut totals = [0f32; FRAME_RING_CAP];
        let mut over_50 = 0u32;
        let mut over_100 = 0u32;
        let mut worst_total = f32::MIN;
        let mut worst_sample = FrameSample::default();
        for (i, s) in self.frames[..n].iter().enumerate() {
            totals[i] = s.total_ms;
            if s.total_ms > SLOW_FRAME_MS {
                over_50 += 1;
            }
            if s.total_ms > LONG_FRAME_MS {
                over_100 += 1;
            }
            if s.total_ms > worst_total {
                worst_total = s.total_ms;
                worst_sample = *s;
            }
        }
        let slice = &mut totals[..n];
        slice.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
        WindowStats {
            p50: percentile(slice, 0.50),
            p95: percentile(slice, 0.95),
            max: slice[n - 1],
            over_50,
            over_100,
            worst_phase: dominant_phase(&worst_sample),
        }
    }

    /// Called once per frame from `FrameTimer::finish`. Always pushes the
    /// sample into the ring and updates `last_sample`; a frame over
    /// [`LONG_FRAME_MS`] additionally gets a `Copy` push into the long-frame
    /// ring. No formatting, no DOM, no console — see the module doc's
    /// "Cost" section.
    fn record_frame(&mut self, sample: FrameSample, at_ms: f64) {
        self.push_frame(sample);
        self.last_sample = sample;
        if sample.total_ms > LONG_FRAME_MS {
            self.push_long_frame(LongFrameEntry {
                at_ms,
                sample,
                dominant: dominant_phase(&sample),
            });
        }
    }

    /// Fed by `persistence::save_persistent_state` — one call per
    /// `save_persistent_state` invocation (not per individual
    /// `localStorage.setItem`), so `writes` is the count of successful
    /// storage writes THAT call made (profile + watchlists + settings
    /// snapshot + templates + one per preset). Directly answers H1 (does a
    /// click-triggered save spike because it wrote every preset, not just
    /// the dirty one) — by write count and wall time, never by bytes, so the
    /// instrument does not serialize anything the save itself does not.
    pub fn record_persist(&mut self, ms: f64, writes: usize) {
        self.last_persist_ms = ms as f32;
        self.max_persist_ms = self.max_persist_ms.max(self.last_persist_ms);
        self.persist_total_writes = self.persist_total_writes.saturating_add(writes as u64);
    }

    /// On-demand snapshot for `window.__MLC_PERF()` — installed once at
    /// boot in `lib.rs::run_render_loop`. Built fresh on every call (no
    /// caching): one `window_stats()` stack pass plus a handful of
    /// `Reflect::set` calls, paid only when JS actually asks. Shape (all
    /// numbers in ms / MiB):
    ///
    /// ```text
    /// {
    ///   "v": 1,
    ///   "frames": <ring len>,
    ///   "window": {"p50","p95","max","over_50","over_100","worst_phase"},
    ///   "last": {"total","events","tick_chart","tick_drains","tick_cloud",
    ///            "drains","persist","render_setup","render_chart",
    ///            "render_overlay","render_present","events_drained"},
    ///   "memory": {"wasm_mb","wasm_growth_mb","heap_mb" (null when absent)},
    ///   "persist": {"writes","last_ms","max_ms"},
    ///   "long_frames": [{"at_ms","total","dominant",
    ///                    "phases":{the 10 sub-phase names},
    ///                    "events_drained"} … oldest first, max 20],
    /// }
    /// ```
    ///
    /// The caller (`lib.rs`) is responsible for the `{"busy":true}` case —
    /// this method assumes it already holds a live `&WebAppState` borrow.
    pub fn snapshot_js(&self) -> JsValue {
        let stats = self.window_stats();
        let wasm_mb = wasm_memory_mb();
        let heap_mb = js_heap_used_mb();

        let root = js_sys::Object::new();
        set_prop(&root, "v", JsValue::from(1u32));
        set_prop(&root, "frames", JsValue::from(self.frame_len as u32));

        let window_obj = js_sys::Object::new();
        set_prop(&window_obj, "p50", JsValue::from(stats.p50 as f64));
        set_prop(&window_obj, "p95", JsValue::from(stats.p95 as f64));
        set_prop(&window_obj, "max", JsValue::from(stats.max as f64));
        set_prop(&window_obj, "over_50", JsValue::from(stats.over_50));
        set_prop(&window_obj, "over_100", JsValue::from(stats.over_100));
        set_prop(&window_obj, "worst_phase", JsValue::from_str(stats.worst_phase));
        set_prop(&root, "window", window_obj.into());

        let last_obj = phases_to_js(&self.last_sample);
        set_prop(&last_obj, "total", JsValue::from(self.last_sample.total_ms as f64));
        set_prop(&last_obj, "events_drained", JsValue::from(self.last_sample.events_drained as f64));
        set_prop(&root, "last", last_obj.into());

        let memory_obj = js_sys::Object::new();
        set_prop(&memory_obj, "wasm_mb", JsValue::from(wasm_mb));
        set_prop(&memory_obj, "wasm_growth_mb", JsValue::from(wasm_mb - self.boot_wasm_mb));
        set_prop(&memory_obj, "heap_mb", heap_mb.map(JsValue::from).unwrap_or(JsValue::NULL));
        set_prop(&root, "memory", memory_obj.into());

        let persist_obj = js_sys::Object::new();
        set_prop(&persist_obj, "writes", JsValue::from(self.persist_total_writes as f64));
        set_prop(&persist_obj, "last_ms", JsValue::from(self.last_persist_ms as f64));
        set_prop(&persist_obj, "max_ms", JsValue::from(self.max_persist_ms as f64));
        set_prop(&root, "persist", persist_obj.into());

        let long_frames_arr = js_sys::Array::new();
        for entry in self.long_frames_oldest_first() {
            let entry_obj = js_sys::Object::new();
            set_prop(&entry_obj, "at_ms", JsValue::from(entry.at_ms));
            set_prop(&entry_obj, "total", JsValue::from(entry.sample.total_ms as f64));
            set_prop(&entry_obj, "dominant", JsValue::from_str(entry.dominant));
            set_prop(&entry_obj, "phases", phases_to_js(&entry.sample).into());
            set_prop(&entry_obj, "events_drained", JsValue::from(entry.sample.events_drained as f64));
            long_frames_arr.push(&entry_obj);
        }
        set_prop(&root, "long_frames", long_frames_arr.into());

        root.into()
    }
}

impl Default for PerfMeter {
    fn default() -> Self {
        Self::new()
    }
}
