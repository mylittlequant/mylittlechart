//! Kinetic Scrolling - Exponential decay physics
//!
//! Implements smooth momentum scrolling that continues after
//! the user releases the mouse during a drag operation.

// =============================================================================
// Kinetic Constants
// =============================================================================

/// Exponential decay factor per millisecond
///
/// This value provides natural-feeling deceleration.
pub const KINETIC_DAMPING: f64 = 0.997;

/// Minimum velocity threshold to stop animation
///
/// Below this value, kinetic scrolling stops to prevent
/// endless micro-movements.
pub const KINETIC_MIN_VELOCITY: f64 = 0.001;

/// Velocity multiplier from drag speed
///
/// Converts drag velocity to kinetic velocity.
pub const KINETIC_FRICTION: f64 = 0.3;

// =============================================================================
// Kinetic State
// =============================================================================

/// State for kinetic (momentum) scrolling
#[derive(Clone, Debug, Default)]
pub struct KineticState {
    /// Current velocity in pixels per millisecond
    pub velocity: f64,
    /// Timestamp of last update (milliseconds)
    pub last_time: f64,
    /// Whether kinetic scrolling is currently active
    pub active: bool,
}

impl KineticState {
    /// Create a new kinetic state
    pub fn new() -> Self {
        Self::default()
    }

    /// Start kinetic scrolling with the given velocity
    ///
    /// Call this when the user releases the mouse after dragging.
    pub fn start(&mut self, velocity: f64, now: f64) {
        self.velocity = velocity;
        self.last_time = now;
        self.active = velocity.abs() > KINETIC_MIN_VELOCITY;
    }

    /// Stop kinetic scrolling
    pub fn stop(&mut self) {
        self.active = false;
        self.velocity = 0.0;
    }

    /// Update kinetic state and return bar delta if active
    ///
    /// Call this on each animation frame. Returns Some(bar_delta) if
    /// scrolling is active and should continue, None if stopped.
    ///
    /// The bar_delta can be used to update the viewport's view_start.
    pub fn update(&mut self, now: f64, bar_spacing: f64) -> Option<f64> {
        if !self.active {
            return None;
        }

        let dt = now - self.last_time;
        self.last_time = now;

        // Apply exponential decay
        let damping = KINETIC_DAMPING.powf(dt);
        self.velocity *= damping;

        // Calculate movement
        let bar_delta = self.velocity * dt / bar_spacing;

        // Stop if velocity is too low
        if self.velocity.abs() < KINETIC_MIN_VELOCITY {
            self.active = false;
            self.velocity = 0.0;
            return None;
        }

        Some(bar_delta)
    }

    /// Check if kinetic scrolling is currently active
    #[inline]
    pub fn is_active(&self) -> bool {
        self.active
    }

    /// Calculate velocity from drag movement
    ///
    /// Call this on mouse move during drag to track velocity.
    /// Returns the velocity that can be passed to start().
    pub fn calc_velocity(dx: f64, dt: f64) -> f64 {
        if dt > 0.0 {
            (dx / dt) * KINETIC_FRICTION
        } else {
            0.0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_kinetic_start_stop() {
        let mut kinetic = KineticState::new();
        assert!(!kinetic.is_active());

        kinetic.start(0.5, 1000.0);
        assert!(kinetic.is_active());

        kinetic.stop();
        assert!(!kinetic.is_active());
        assert_eq!(kinetic.velocity, 0.0);
    }

    #[test]
    fn test_kinetic_start_with_low_velocity() {
        let mut kinetic = KineticState::new();

        // Very low velocity should not activate
        kinetic.start(0.0001, 1000.0);
        assert!(!kinetic.is_active());
    }

    #[test]
    fn test_kinetic_update() {
        let mut kinetic = KineticState::new();
        kinetic.start(1.0, 1000.0);

        // After 100ms, velocity should decay
        let delta = kinetic.update(1100.0, 10.0);
        assert!(delta.is_some());

        // Velocity should have decreased
        assert!(kinetic.velocity < 1.0);
        assert!(kinetic.velocity > 0.5); // But not by too much in 100ms
    }

    #[test]
    fn test_kinetic_stops_eventually() {
        let mut kinetic = KineticState::new();
        kinetic.start(0.1, 0.0);

        // Simulate many frames
        let mut time = 0.0;
        let mut iterations = 0;
        while kinetic.is_active() && iterations < 10000 {
            time += 16.0; // ~60fps
            kinetic.update(time, 10.0);
            iterations += 1;
        }

        assert!(!kinetic.is_active());
        assert!(iterations < 10000, "Should stop within reasonable time");
    }

    #[test]
    fn test_calc_velocity() {
        // 100px in 50ms = 2 px/ms * 0.3 = 0.6 velocity
        let velocity = KineticState::calc_velocity(100.0, 50.0);
        assert!((velocity - 0.6).abs() < 0.001);

        // Zero dt should give zero velocity (avoid division by zero)
        let velocity = KineticState::calc_velocity(100.0, 0.0);
        assert_eq!(velocity, 0.0);
    }
}
