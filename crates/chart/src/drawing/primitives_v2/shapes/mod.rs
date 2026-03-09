//! Shape-based primitives
//!
//! This module contains geometric shape drawing tools:
//! - Rectangle: box defined by two corners
//! - Rotated Rectangle: rectangle that can be rotated
//! - Circle: perfect circle
//! - Ellipse: oval shape
//! - Triangle: three-point shape
//! - Arc: curved line segment
//! - Path: free-form connected points
//! - Polyline: connected straight lines
//! - Curve: Bezier curve
//! - Double Curve: S-curve with two control points

pub mod rectangle;
pub mod circle;
pub mod ellipse;
pub mod triangle;
pub mod arc;
pub mod polyline;
pub mod path;
pub mod rotated_rectangle;
pub mod curve;
pub mod double_curve;

// Re-export primitive types
pub use rectangle::Rectangle;
pub use circle::Circle;
pub use ellipse::Ellipse;
pub use triangle::Triangle;
pub use arc::Arc;
pub use polyline::Polyline;
pub use path::Path;
pub use rotated_rectangle::RotatedRectangle;
pub use curve::Curve;
pub use double_curve::DoubleCurve;
