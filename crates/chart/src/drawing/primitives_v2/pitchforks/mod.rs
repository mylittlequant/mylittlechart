//! Pitchfork primitives
//!
//! Andrew's Pitchfork and its variants - trend analysis tools
//! that use three points to define a median line with parallel support/resistance.

pub mod pitchfork;
pub mod schiff;
pub mod modified_schiff;
pub mod inside_pitchfork;

pub use pitchfork::Pitchfork;
pub use schiff::SchiffPitchfork;
pub use modified_schiff::ModifiedSchiff;
pub use inside_pitchfork::InsidePitchfork;
