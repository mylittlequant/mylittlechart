//! What the hosting shell can do for the sidebar panels.
//!
//! Process-wide switches the shell sets once at boot, read by every surface
//! that would otherwise draw a control the shell cannot serve (the same
//! pattern as the checkout's entry-point switch in `mlc-app-layout`). A native
//! shell sets nothing and keeps every default.

use std::sync::atomic::{AtomicBool, Ordering};

static AGENTS_CONTROL: AtomicBool = AtomicBool::new(true);

/// Whether this shell can run agents and so shows the Agents control pane
/// (mode toggle, backend picker, spawn buttons, the pane grid). Default
/// `true`; the web shell sets `false` and the panel shows a notice instead.
pub fn agents_control_available() -> bool {
    AGENTS_CONTROL.load(Ordering::Relaxed)
}

/// Set by the shell at boot. See [`agents_control_available`].
pub fn set_agents_control_available(available: bool) {
    AGENTS_CONTROL.store(available, Ordering::Relaxed);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_shell_that_sets_nothing_keeps_the_control_pane() {
        assert!(agents_control_available());
    }
}
