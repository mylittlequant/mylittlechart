//! Gann primitives
//!
//! W.D. Gann technical analysis tools including boxes, squares, and fans.
//! Based on price-time relationships and geometric angles.

pub mod gann_box;
pub mod gann_square_fixed;
pub mod gann_square;
pub mod gann_fan;

pub use gann_box::GannBox;
pub use gann_square_fixed::GannSquareFixed;
pub use gann_square::GannSquare;
pub use gann_fan::GannFan;
