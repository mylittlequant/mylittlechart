//! Pure geometry data types shared between backend state and frontend layout.

/// Simple rectangle for layout computation - no external dependencies
///
/// Coordinates use top-left origin (0,0 at top-left of screen).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LayoutRect {
    /// X coordinate of left edge
    pub x: f64,
    /// Y coordinate of top edge
    pub y: f64,
    /// Width of rectangle
    pub width: f64,
    /// Height of rectangle
    pub height: f64,
}

impl LayoutRect {
    /// Create a new rectangle
    #[inline]
    pub const fn new(x: f64, y: f64, width: f64, height: f64) -> Self {
        Self { x, y, width, height }
    }

    /// Create rectangle from min/max coordinates
    #[inline]
    pub fn from_min_max(min_x: f64, min_y: f64, max_x: f64, max_y: f64) -> Self {
        Self {
            x: min_x,
            y: min_y,
            width: max_x - min_x,
            height: max_y - min_y,
        }
    }

    /// Create a zero-sized rectangle at origin
    #[inline]
    pub const fn zero() -> Self {
        Self::new(0.0, 0.0, 0.0, 0.0)
    }

    /// Right edge X coordinate
    #[inline]
    pub fn right(&self) -> f64 {
        self.x + self.width
    }

    /// Bottom edge Y coordinate
    #[inline]
    pub fn bottom(&self) -> f64 {
        self.y + self.height
    }

    /// Left edge (alias for x)
    #[inline]
    pub fn left(&self) -> f64 {
        self.x
    }

    /// Top edge (alias for y)
    #[inline]
    pub fn top(&self) -> f64 {
        self.y
    }

    /// Center X coordinate
    #[inline]
    pub fn center_x(&self) -> f64 {
        self.x + self.width / 2.0
    }

    /// Center Y coordinate
    #[inline]
    pub fn center_y(&self) -> f64 {
        self.y + self.height / 2.0
    }

    /// Check if point is inside rectangle (inclusive)
    #[inline]
    pub fn contains(&self, px: f64, py: f64) -> bool {
        px >= self.x && px <= self.right() && py >= self.y && py <= self.bottom()
    }

    /// Check if rectangle is empty (zero or negative dimensions)
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.width <= 0.0 || self.height <= 0.0
    }

    /// Shrink rectangle by margin on all sides
    #[inline]
    pub fn shrink(&self, margin: f64) -> Self {
        Self {
            x: self.x + margin,
            y: self.y + margin,
            width: (self.width - 2.0 * margin).max(0.0),
            height: (self.height - 2.0 * margin).max(0.0),
        }
    }

    /// Expand rectangle by margin on all sides
    #[inline]
    pub fn expand(&self, margin: f64) -> Self {
        Self {
            x: self.x - margin,
            y: self.y - margin,
            width: self.width + 2.0 * margin,
            height: self.height + 2.0 * margin,
        }
    }

    /// Translate rectangle by offset
    #[inline]
    pub fn translate(&self, dx: f64, dy: f64) -> Self {
        Self {
            x: self.x + dx,
            y: self.y + dy,
            width: self.width,
            height: self.height,
        }
    }

    /// Intersect with another rectangle
    pub fn intersect(&self, other: &Self) -> Self {
        let x = self.x.max(other.x);
        let y = self.y.max(other.y);
        let right = self.right().min(other.right());
        let bottom = self.bottom().min(other.bottom());
        Self {
            x,
            y,
            width: (right - x).max(0.0),
            height: (bottom - y).max(0.0),
        }
    }
}
