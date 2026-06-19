//! E2E tier: full ChartApp lifecycle, headless.
//!
//! Real bridge channels, real tick() loop, fake data sources. Closest to
//! the live binary without the GPU stack.
//!
//! NOT a substitute for the live `mylittlechart.exe` for owner-facing
//! reports — see `feedback_unit_tests_for_debug_only`.
//!
//! TODO: scaffold after integration tier. Reproduce the live spinner
//! symptom end-to-end (4 tabs same instrument, BarsLoaded over the bridge,
//! drive tick() to "scene-ready" check, assert every chart's
//! awaiting_first_live_frame flips false).

#[test]
#[ignore = "scaffold — to be implemented after the integration tier"]
fn four_tabs_same_instrument_all_retire_spinner() {
    unimplemented!()
}
