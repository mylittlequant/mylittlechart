//! Always-on wasm-only frame/persistence instrumentation for the web shell.
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
//! # Cost when the HUD is off
//!
//! [`FrameTimer`] is a handful of `performance.now()` reads (JS property
//! gets, not allocations) per frame, folded into a fixed-size ring buffer
//! ([`PerfMeter`]) that is allocated once at boot and never grows. Long-frame
//! warnings and the periodic summary are always on (owner's spec — a half-
//! freeze must leave a trace whether or not anyone opened the HUD), but both
//! are rate-limited and only format a `String` when they actually log
//! (≤ 1/s and 1/10s respectively). The `<div id="mlc-perf">` HUD itself is
//! created only when explicitly enabled (`?perf=1` or
//! `localStorage["mlc.perf"] == "1"`) and never touches the DOM otherwise.
//!
//! # Phase boundaries (see `lib.rs::run_render_loop`'s RAF closure)
//!
//! `Events` (DOM → `dispatch_event`) → `Tick` (`chart.tick` +
//! watchlist/template drains + cloud `drive`) → `Drains` (cold-apply/theme/
//! clipboard/profile-command drains, the `MLC_DEBUG` diag block, and —
//! after `Render` — screenshot capture, live-share/REST-cure drains,
//! background-warm) → `Persist` (`persistence::save_persistent_state`) →
//! `Render` (`render_frame`). `Drains` accumulates across all three of its
//! disjoint spans; the other phases are contiguous. This intentionally
//! groups every non-tick/persist/render bookkeeping drain into one bucket —
//! the WHERE brief names screenshot/share/nick drains as "drains", even
//! though a couple of them execute after the render call in the actual
//! code order.
//!
//! Network (fetch) timing is deliberately absent: this shell never calls
//! `fetch` itself — all cloud/archive-proxy calls go through
//! `mlc_cloud::FetchTransport` (a different crate, out of scope per the
//! task brief) — so there is no single shell-owned call site to wrap.

use wasm_bindgen::JsCast;

/// Frames kept in the ring buffer — at a steady 60fps this is ~5s of
/// history, which doubles as the "5s window" the owner's stats are over.
const FRAME_RING_CAP: usize = 300;

/// Number of named phases tracked per frame — must match [`Phase`]'s variant
/// count; used to size `FrameTimer::totals`.
const PHASE_COUNT: usize = 5;

/// Minimum gap between long-frame `console.warn` lines.
const WARN_RATE_LIMIT_MS: f64 = 1_000.0;
/// Gap between the always-on `console.info` summary lines.
const SUMMARY_INTERVAL_MS: f64 = 10_000.0;
/// Gap between HUD `<div>` text updates.
const HUD_UPDATE_INTERVAL_MS: f64 = 500.0;
/// A frame counts as "long" for the always-on warning above this total.
const LONG_FRAME_MS: f32 = 100.0;
/// Threshold for the window's `>50ms` bucket count.
const SLOW_FRAME_MS: f32 = 50.0;

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

/// `?perf=1` in the page URL, or `localStorage["mlc.perf"] == "1"`.
/// Same split grammar as `lib.rs::share_token_from_url` /
/// `apply_dev_proxy_gate_from_url` — checked once at boot, next to those.
pub fn hud_enabled() -> bool {
    let from_url = web_sys::window()
        .and_then(|w| w.location().search().ok())
        .map(|search| {
            search
                .trim_start_matches('?')
                .split('&')
                .any(|p| p == "perf=1")
        })
        .unwrap_or(false);
    if from_url {
        return true;
    }
    web_sys::window()
        .and_then(|w| w.local_storage().ok().flatten())
        .and_then(|storage| storage.get_item("mlc.perf").ok().flatten())
        .map(|v| v == "1")
        .unwrap_or(false)
}

/// Named phase buckets a [`FrameTimer`] accumulates into. Order matches the
/// module doc's phase-boundary writeup.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Phase {
    Events = 0,
    Tick = 1,
    Drains = 2,
    Persist = 3,
    Render = 4,
}

/// One frame's phase breakdown — `Copy` so the ring buffer is a plain fixed
/// array with no per-frame heap traffic.
#[derive(Clone, Copy, Default)]
struct FrameSample {
    events_ms: f32,
    tick_ms: f32,
    drains_ms: f32,
    persist_ms: f32,
    render_ms: f32,
    total_ms: f32,
    events_drained: u16,
}

/// Which named phase had the largest share of a given [`FrameSample`].
fn dominant_phase(s: &FrameSample) -> &'static str {
    let mut name = "events";
    let mut max = s.events_ms;
    if s.tick_ms > max {
        max = s.tick_ms;
        name = "tick";
    }
    if s.drains_ms > max {
        max = s.drains_ms;
        name = "drains";
    }
    if s.persist_ms > max {
        max = s.persist_ms;
        name = "persist";
    }
    if s.render_ms > max {
        name = "render";
    }
    name
}

/// Per-frame stopwatch — created at the top of the RAF closure, `enter()`
/// called at each phase boundary, `finish()` folds the result into the
/// shared [`PerfMeter`]. Each method is exactly one `performance.now()`
/// read; a full frame is `start` + up to 6 `enter` + `finish` — the "handful
/// of `performance.now()` calls" the task brief asks for.
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
        meter.record_frame(FrameSample {
            events_ms: self.totals[Phase::Events as usize],
            tick_ms: self.totals[Phase::Tick as usize],
            drains_ms: self.totals[Phase::Drains as usize],
            persist_ms: self.totals[Phase::Persist as usize],
            render_ms: self.totals[Phase::Render as usize],
            total_ms,
            events_drained: self.events_drained,
        });
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

fn fmt_heap(heap_mb: Option<f64>) -> String {
    match heap_mb {
        Some(mb) => format!("{:.1}MiB", mb),
        None => "n/a".to_string(),
    }
}

/// `<div id="mlc-perf">` — created once, only when [`hud_enabled`] says so;
/// updated in place, never recreated.
struct Hud {
    div: web_sys::HtmlElement,
    last_update_ms: f64,
}

impl Hud {
    /// Appends the HUD element to `document.body`. Inline `style` only — the
    /// site CSP (`style-src 'unsafe-inline'`) allows it and no external
    /// asset is needed for an 11px monospace overlay.
    fn create() -> Option<Self> {
        let win = web_sys::window()?;
        let doc = win.document()?;
        let body = doc.body()?;
        let el = doc.create_element("div").ok()?;
        el.set_id("mlc-perf");
        let _ = el.set_attribute(
            "style",
            "position:fixed;top:4px;left:4px;z-index:2147483647;\
             font-family:monospace;font-size:11px;line-height:1.4;\
             background:rgba(0,0,0,0.65);color:#0f0;padding:6px 8px;\
             white-space:pre;pointer-events:none;",
        );
        body.append_child(&el).ok()?;
        let div = el.dyn_into::<web_sys::HtmlElement>().ok()?;
        Some(Self { div, last_update_ms: 0.0 })
    }

    fn set_text(&self, text: &str) {
        self.div.set_inner_text(text);
    }
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

    persist_total_writes: u64,
    last_persist_ms: f32,
    max_persist_ms: f32,

    boot_wasm_mb: f64,
    last_warn_ms: f64,
    last_summary_ms: f64,

    hud: Option<Hud>,
}

impl PerfMeter {
    pub fn new() -> Self {
        let hud = if hud_enabled() { Hud::create() } else { None };
        if hud_enabled() && hud.is_none() {
            web_sys::console::warn_1(
                &"[mlc-perf] ?perf=1 / mlc.perf set but the HUD element could not be created".into(),
            );
        }
        Self {
            frames: [FrameSample::default(); FRAME_RING_CAP],
            frame_head: 0,
            frame_len: 0,
            persist_total_writes: 0,
            last_persist_ms: 0.0,
            max_persist_ms: 0.0,
            boot_wasm_mb: wasm_memory_mb(),
            last_warn_ms: 0.0,
            last_summary_ms: now_ms(),
            hud,
        }
    }

    fn push_frame(&mut self, sample: FrameSample) {
        self.frames[self.frame_head] = sample;
        self.frame_head = (self.frame_head + 1) % FRAME_RING_CAP;
        if self.frame_len < FRAME_RING_CAP {
            self.frame_len += 1;
        }
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
    /// sample into the ring; long-frame warning / periodic summary / HUD
    /// text are each independently rate-limited and only pay for a
    /// `window_stats()` pass + `String` formatting when at least one of them
    /// is due this frame.
    fn record_frame(&mut self, sample: FrameSample) {
        self.push_frame(sample);
        let now = now_ms();

        let want_warn = sample.total_ms > LONG_FRAME_MS && now - self.last_warn_ms >= WARN_RATE_LIMIT_MS;
        let want_summary = now - self.last_summary_ms >= SUMMARY_INTERVAL_MS;
        let want_hud = self.hud.as_ref().is_some_and(|h| now - h.last_update_ms >= HUD_UPDATE_INTERVAL_MS);

        if !(want_warn || want_summary || want_hud) {
            return;
        }

        let stats = self.window_stats();
        let wasm_mb = wasm_memory_mb();
        let heap_mb = js_heap_used_mb();

        if want_warn {
            self.last_warn_ms = now;
            web_sys::console::warn_1(&format!(
                "[mlc-perf] long frame {:.1}ms — events={:.1} tick={:.1} drains={:.1} persist={:.1} render={:.1} | \
                 wasm={:.1}MiB heap={} | events_drained={} persist_writes={} (last {:.1}ms, max {:.1}ms)",
                sample.total_ms, sample.events_ms, sample.tick_ms, sample.drains_ms, sample.persist_ms, sample.render_ms,
                wasm_mb, fmt_heap(heap_mb),
                sample.events_drained, self.persist_total_writes, self.last_persist_ms, self.max_persist_ms,
            ).into());
        }

        if want_summary {
            self.last_summary_ms = now;
            web_sys::console::info_1(&format!(
                "[mlc-perf] p50={:.1}ms p95={:.1}ms max={:.1}ms >50ms={} >100ms={} worst={} | wasm={:.1}MiB(+{:.1}) heap={}",
                stats.p50, stats.p95, stats.max, stats.over_50, stats.over_100, stats.worst_phase,
                wasm_mb, wasm_mb - self.boot_wasm_mb, fmt_heap(heap_mb),
            ).into());
        }

        if want_hud {
            let text = format!(
                "MLC PERF (?perf=1)\n\
                 frame p50/p95/max: {:.1}/{:.1}/{:.1} ms\n\
                 >50ms {}  >100ms {}  worst: {}\n\
                 wasm mem: {:.1} MiB (+{:.1})\n\
                 js heap: {}\n\
                 persist: {} writes / last {:.1}ms / max {:.1}ms",
                stats.p50, stats.p95, stats.max,
                stats.over_50, stats.over_100, stats.worst_phase,
                wasm_mb, wasm_mb - self.boot_wasm_mb,
                fmt_heap(heap_mb),
                self.persist_total_writes,
                self.last_persist_ms, self.max_persist_ms,
            );
            // `hud` re-borrowed here (not above) so `window_stats()` and the
            // warn/summary formatting above don't hold it mutably borrowed.
            if let Some(hud) = self.hud.as_mut() {
                hud.last_update_ms = now;
                hud.set_text(&text);
            }
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
}

impl Default for PerfMeter {
    fn default() -> Self {
        Self::new()
    }
}
