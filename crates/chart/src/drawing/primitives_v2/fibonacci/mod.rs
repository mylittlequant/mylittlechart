//! Fibonacci primitives
//!
//! Fibonacci-based technical analysis tools including retracements,
//! extensions, channels, time zones, fans, circles, arcs, spirals, and wedges.

pub mod retracement;
pub mod trend_extension;
pub mod channel;
pub mod time_zones;
pub mod speed_resistance;
pub mod trend_time;
pub mod circles;
pub mod spiral;
pub mod arcs;
pub mod wedge;
pub mod fan;

pub use retracement::FibRetracement;
pub use trend_extension::FibTrendExtension;
pub use channel::FibChannel;
pub use time_zones::FibTimeZones;
pub use speed_resistance::FibSpeedResistance;
pub use trend_time::FibTrendTime;
pub use circles::FibCircles;
pub use spiral::FibSpiral;
pub use arcs::FibArcs;
pub use wedge::FibWedge;
pub use fan::FibFan;
