//! mlc-test-harness — headless debug harness for mlc.
//!
//! # Purpose
//!
//! Fast iteration on routing / lifecycle / data-flow logic WITHOUT the
//! 16-minute `mlc-app-vello` release build, WITHOUT a real DataBridge / WS
//! connection, WITHOUT the GPU stack.
//!
//! # Tiers
//!
//! - **unit** (`tests/unit/`) — pure logic in isolation. No `ChartApp`, no
//!   bridge, no GPU. Touches single types directly (e.g. `BubbleStore`,
//!   ownership routing, viewport snap math). Compiles + runs in seconds.
//! - **integration** (`tests/integration/`) — composes several mlc-core /
//!   mlc-app subsystems and exercises a small slice (e.g. simulate a
//!   BarsLoaded `LiveUpdate` going through the writer arm into the bubble).
//!   Still headless, still no real bridge.
//! - **e2e** (`tests/e2e/`) — full `ChartApp` lifecycle headless. Real
//!   bridge channels, real tick() loop, fake data sources. Slowest but
//!   closest to the live binary; still NOT a substitute for the real
//!   `mylittlechart.exe` for owner-facing reports (see `feedback_unit_tests_for_debug_only`).
//!
//! # Reporting rule
//!
//! Any tier here is a **private debug accelerator**. The owner accepts
//! "it works" ONLY from a live binary run on `main`. Don't report
//! "harness passes" as proof.

pub mod fixtures;
