//! Scroll state for scrollable containers (pure data, no rendering dependencies)

use web_time::Instant;

/// Below this many pixels of remaining distance to `target`, the animated
/// offset is considered settled — `animated_offset()` snaps to `target`
/// exactly and `is_animating()` reports `false`, so callers stop requesting
/// a repaint instead of chasing an asymptote forever.
const SETTLE_EPSILON_PX: f64 = 0.5;

/// Time constant (ms) of the exponential ease from the offset held at the
/// start of the current wheel gesture toward `target`. Frame-rate
/// independent by construction — `animated_offset()` is a pure function of
/// wall-clock elapsed time (via `Instant`), not a fixed per-frame step, so
/// it converges at the same real-world speed at 30fps, 60fps or 144fps.
/// Chosen so ~95% of the remaining distance closes within ~120ms, inside
/// the owner's "100-150ms" ask (`tau = 120ms / -ln(0.05) ~= 40ms`).
const EASE_TAU_MS: f64 = 40.0;

/// Pixels the scroll `target` moves per wheel notch. `delta_y` arrives
/// pre-normalized to ~20 per notch by the platform layer (native
/// `LineDelta * 20.0`, web `WheelEvent.deltaY` normalized to the same
/// contract — see `mlc-app-input-shell::ShellEvent::Scroll` doc and
/// `uzor-window-web`'s wheel listener), so `20.0 * SCROLL_STEP_PER_NOTCH`
/// is the actual per-notch pixel travel: ~90px, about 3 rows in these
/// modals' lists — far calmer than the previous 200px/notch (owner:
/// "сделать скролл колёсиком менее чувствительным").
const SCROLL_STEP_PER_NOTCH: f64 = 4.5;

/// Scroll state for a scrollable container
///
/// Include this in your modal/widget state to enable scrolling.
#[derive(Clone, Debug, Default)]
pub struct ScrollState {
    /// Offset the current ease started from. Equal to the fully-settled
    /// (painted) offset whenever `is_animating()` is `false`.
    pub offset: f64,
    /// Destination offset a wheel notch (or drag/programmatic scroll) wants
    /// the view to reach. `animated_offset()` eases `offset` toward this.
    pub target: f64,
    /// Wall-clock time the current ease toward `target` began; `None` once
    /// settled (no visible animation in flight).
    anim_start: Option<Instant>,
    /// `offset` value captured at `anim_start` — the ease's start point.
    anim_start_offset: f64,
    /// Is scrollbar handle being dragged?
    pub is_dragging: bool,
    /// Y position where drag started
    pub drag_start_y: Option<f64>,
    /// Scroll offset when drag started
    pub drag_start_offset: Option<f64>,
}

impl ScrollState {
    /// Create new scroll state
    pub fn new() -> Self {
        Self::default()
    }

    /// Reset scroll state (e.g., when content changes) — immediate, no animation.
    pub fn reset(&mut self) {
        self.offset = 0.0;
        self.target = 0.0;
        self.anim_start = None;
        self.anim_start_offset = 0.0;
        self.is_dragging = false;
        self.drag_start_y = None;
        self.drag_start_offset = None;
    }

    /// Start scrollbar drag
    pub fn start_drag(&mut self, y: f64) {
        self.is_dragging = true;
        self.drag_start_y = Some(y);
        self.drag_start_offset = Some(self.animated_offset());
    }

    /// End scrollbar drag
    pub fn end_drag(&mut self) {
        self.is_dragging = false;
        self.drag_start_y = None;
        self.drag_start_offset = None;
    }

    /// Handle mouse wheel scroll
    ///
    /// Moves `target` by one notch's worth of pixels (see
    /// [`SCROLL_STEP_PER_NOTCH`]) rather than jumping `offset` directly —
    /// [`Self::animated_offset`] eases the painted position toward it.
    /// Checkpoints the current animated position into `offset` first so a
    /// new notch arriving mid-ease restarts smoothly from wherever the view
    /// actually is, not from a stale base.
    ///
    /// Returns true if scroll was handled (content overflows)
    pub fn handle_wheel(&mut self, delta_y: f64, content_height: f64, viewport_height: f64) -> bool {
        if content_height <= viewport_height {
            return false;
        }
        let max_scroll = (content_height - viewport_height).max(0.0);
        self.offset = self.animated_offset();
        self.anim_start = Some(Instant::now());
        self.anim_start_offset = self.offset;
        self.target = (self.target + delta_y * SCROLL_STEP_PER_NOTCH).clamp(0.0, max_scroll);
        true
    }

    /// Handle scrollbar drag motion — immediate, no animation (the handle
    /// must track the cursor exactly, same frame).
    ///
    /// Call this in on_mouse_move when is_dragging is true
    pub fn handle_drag(&mut self, y: f64, track_height: f64, content_height: f64, viewport_height: f64) {
        if !self.is_dragging {
            return;
        }

        let Some(start_y) = self.drag_start_y else { return };
        let Some(start_offset) = self.drag_start_offset else { return };

        let max_scroll = (content_height - viewport_height).max(0.0);
        if max_scroll <= 0.0 {
            return;
        }

        let handle_height = (viewport_height / content_height * track_height).max(20.0);
        let scroll_range = track_height - handle_height;
        if scroll_range <= 0.0 {
            return;
        }

        let dy = y - start_y;
        let scroll_delta = dy / scroll_range * max_scroll;
        self.set_immediate((start_offset + scroll_delta).clamp(0.0, max_scroll));
    }

    /// Scroll to position — immediate, no animation (used e.g. to keep a
    /// selected row visible; the jump must land the same frame it fires).
    pub fn scroll_to(&mut self, offset: f64, content_height: f64, viewport_height: f64) {
        let max_scroll = (content_height - viewport_height).max(0.0);
        self.set_immediate(offset.clamp(0.0, max_scroll));
    }

    /// Ensure item is visible (scroll if needed) — immediate, no animation.
    pub fn ensure_visible(&mut self, item_y: f64, item_height: f64, viewport_height: f64, content_height: f64) {
        let max_scroll = (content_height - viewport_height).max(0.0);
        let current = self.animated_offset();
        // If item top is above viewport
        if item_y < current {
            self.set_immediate(item_y.max(0.0));
        }
        // If item bottom is below viewport
        else if item_y + item_height > current + viewport_height {
            self.set_immediate((item_y + item_height - viewport_height).clamp(0.0, max_scroll));
        }
    }

    /// Handle click on scrollbar track (jump to position) — immediate, no animation.
    pub fn handle_track_click(&mut self, click_y: f64, track_y: f64, track_height: f64, content_height: f64, viewport_height: f64) {
        let max_scroll = (content_height - viewport_height).max(0.0);
        if max_scroll <= 0.0 {
            return;
        }
        let relative_y = (click_y - track_y) / track_height;
        self.set_immediate((relative_y * max_scroll).clamp(0.0, max_scroll));
    }

    /// Clamp offset and target to a (possibly changed) valid range — e.g.
    /// content height shrank since the last frame.
    pub fn clamp(&mut self, content_height: f64, viewport_height: f64) {
        let max_scroll = (content_height - viewport_height).max(0.0);
        self.offset = self.offset.clamp(0.0, max_scroll);
        self.target = self.target.clamp(0.0, max_scroll);
        self.anim_start_offset = self.anim_start_offset.clamp(0.0, max_scroll);
    }

    /// Immediate, non-animated adjustment — for compensating a content-
    /// height change (e.g. a live feed appending rows above the viewport)
    /// where the caller wants the SAME items to stay visually in place,
    /// not an eased catch-up. Moves `offset`, `target` and the in-flight
    /// ease's start point together so any animation already running keeps
    /// converging on the right place afterward instead of un-doing the nudge.
    pub fn nudge(&mut self, delta: f64) {
        self.offset += delta;
        self.target += delta;
        self.anim_start_offset += delta;
    }

    /// Snap immediately to `value` — used by every "must land this frame"
    /// caller above (drag, scroll-to, ensure-visible, track click).
    fn set_immediate(&mut self, value: f64) {
        self.offset = value;
        self.target = value;
        self.anim_start = None;
        self.anim_start_offset = value;
    }

    /// Current, animated scroll offset — what should actually be painted
    /// this frame. Pure function of elapsed wall-clock time since the ease
    /// began; no per-frame mutation is needed to compute it (same shape as
    /// `MascotAnim::progress()`), so nothing but a wheel/drag/programmatic
    /// call is ever required to drive it forward — reading it repeatedly as
    /// time passes is what makes it animate.
    pub fn animated_offset(&self) -> f64 {
        match self.anim_start {
            None => self.offset,
            Some(start) => {
                let dt_ms = start.elapsed().as_secs_f64() * 1000.0;
                let alpha = 1.0 - (-dt_ms / EASE_TAU_MS).exp();
                let value = self.anim_start_offset + (self.target - self.anim_start_offset) * alpha;
                if (self.target - value).abs() < SETTLE_EPSILON_PX {
                    self.target
                } else {
                    value
                }
            }
        }
    }

    /// True while the animated offset is still visibly catching up to
    /// `target` — callers (shell repaint gates) use this to decide whether
    /// to keep requesting a redraw, same shape as the mascot hat's
    /// `MascotAnim::is_live()`.
    pub fn is_animating(&self) -> bool {
        self.anim_start.is_some() && (self.target - self.animated_offset()).abs() >= SETTLE_EPSILON_PX
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread::sleep;
    use std::time::Duration;

    // `delta_y` here mirrors what `handle_wheel` actually receives at the
    // real call site (`ChartApp::on_scroll`'s `scroll_step = -dy`): a
    // POSITIVE value scrolls the view forward/down, increasing offset.

    #[test]
    fn handle_wheel_moves_target_not_offset_instantly() {
        let mut s = ScrollState::new();
        // Content overflows: 1000px content, 200px viewport → max_scroll 800.
        assert!(s.handle_wheel(1.0, 1000.0, 200.0));
        assert!(s.target > 0.0);
        // Offset must NOT jump to target synchronously — it eases.
        assert!(s.animated_offset() < s.target);
        assert!(s.is_animating());
    }

    #[test]
    fn handle_wheel_no_overflow_is_noop() {
        let mut s = ScrollState::new();
        assert!(!s.handle_wheel(1.0, 100.0, 200.0));
        assert_eq!(s.target, 0.0);
        assert!(!s.is_animating());
    }

    #[test]
    fn animated_offset_converges_to_target_over_time() {
        let mut s = ScrollState::new();
        s.handle_wheel(10.0, 1000.0, 200.0);
        let target = s.target;
        assert!(target > 0.0);
        // Real-time sleep: converges well before half a second.
        sleep(Duration::from_millis(500));
        assert!((s.animated_offset() - target).abs() < 1e-6);
        assert!(!s.is_animating());
    }

    #[test]
    fn animated_offset_is_frame_rate_independent() {
        // Two scroll states given the identical wheel input; one is polled
        // many times (simulating a high frame rate), the other polled once
        // after the same wall-clock wait (simulating a low frame rate).
        // Both must land at the same value — polling frequency must not
        // change the converged result.
        let mut fast = ScrollState::new();
        let mut slow = ScrollState::new();
        fast.handle_wheel(10.0, 1000.0, 200.0);
        slow.handle_wheel(10.0, 1000.0, 200.0);

        for _ in 0..50 {
            let _ = fast.animated_offset(); // simulate many polls (high fps)
            sleep(Duration::from_millis(1));
        }
        sleep(Duration::from_millis(500)); // let both settle
        assert!((fast.animated_offset() - slow.animated_offset()).abs() < 1e-6);
    }

    #[test]
    fn handle_drag_sets_position_immediately() {
        let mut s = ScrollState::new();
        s.handle_wheel(10.0, 1000.0, 200.0); // start an ease in flight
        assert!(s.is_animating());
        s.start_drag(0.0);
        // track_height=100, content=1000, viewport=200 → handle=20, range=80
        s.handle_drag(40.0, 100.0, 1000.0, 200.0);
        // Immediate: offset, target and animated_offset must all agree NOW.
        assert_eq!(s.offset, s.target);
        assert_eq!(s.animated_offset(), s.offset);
        assert!(!s.is_animating());
    }

    #[test]
    fn scroll_to_sets_position_immediately() {
        let mut s = ScrollState::new();
        s.handle_wheel(10.0, 1000.0, 200.0);
        s.scroll_to(500.0, 1000.0, 200.0);
        assert_eq!(s.offset, 500.0);
        assert_eq!(s.target, 500.0);
        assert_eq!(s.animated_offset(), 500.0);
        assert!(!s.is_animating());
    }

    #[test]
    fn ensure_visible_sets_position_immediately() {
        let mut s = ScrollState::new();
        s.handle_wheel(10.0, 1000.0, 200.0);
        s.ensure_visible(900.0, 20.0, 200.0, 1000.0);
        assert_eq!(s.animated_offset(), s.offset);
        assert!(!s.is_animating());
        assert!(s.offset > 0.0);
    }

    #[test]
    fn handle_wheel_clamps_target_to_max_scroll() {
        let mut s = ScrollState::new();
        for _ in 0..100 {
            s.handle_wheel(100.0, 1000.0, 200.0);
        }
        assert_eq!(s.target, 800.0); // max_scroll = 1000 - 200
    }

    #[test]
    fn nudge_moves_offset_and_target_together_without_animating() {
        let mut s = ScrollState::new();
        s.scroll_to(100.0, 1000.0, 200.0);
        s.nudge(50.0);
        assert_eq!(s.offset, 150.0);
        assert_eq!(s.target, 150.0);
        assert!(!s.is_animating());
    }

    #[test]
    fn nudge_during_animation_keeps_ease_converging() {
        let mut s = ScrollState::new();
        s.handle_wheel(10.0, 1000.0, 200.0);
        let target_before = s.target;
        s.nudge(30.0);
        assert_eq!(s.target, target_before + 30.0);
        // Still animating, and eventually converges to the nudged target.
        sleep(Duration::from_millis(500));
        assert!((s.animated_offset() - s.target).abs() < 1e-6);
    }
}
