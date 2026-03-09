//! Mathematical utilities
//!
//! Platform-independent mathematical functions for chart rendering.

/// Catmull-Rom spline interpolation
///
/// Takes a series of control points and generates a smooth curve through them.
/// This is useful for rendering smooth line series on charts.
///
/// # Arguments
///
/// * `points` - Control points as (x, y) tuples
/// * `segments_per_curve` - Number of interpolated segments between each pair of control points
///
/// # Returns
///
/// A vector of interpolated (x, y) points forming a smooth curve.
///
/// # Examples
///
/// ```
/// use zengeld_chart::catmull_rom_spline;
///
/// let points = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.5), (3.0, 1.5)];
/// let smooth = catmull_rom_spline(&points, 10);
/// assert!(smooth.len() > points.len());
/// ```
pub fn catmull_rom_spline(points: &[(f64, f64)], segments_per_curve: usize) -> Vec<(f64, f64)> {
    if points.len() < 2 {
        return points.to_vec();
    }
    if points.len() == 2 {
        return points.to_vec();
    }

    let mut result = Vec::with_capacity(points.len() * segments_per_curve);

    for i in 0..points.len() - 1 {
        let p0 = if i == 0 { points[0] } else { points[i - 1] };
        let p1 = points[i];
        let p2 = points[i + 1];
        let p3 = if i + 2 < points.len() {
            points[i + 2]
        } else {
            points[i + 1]
        };

        for j in 0..segments_per_curve {
            let t = j as f64 / segments_per_curve as f64;
            let t2 = t * t;
            let t3 = t2 * t;

            let x = 0.5
                * ((2.0 * p1.0)
                    + (-p0.0 + p2.0) * t
                    + (2.0 * p0.0 - 5.0 * p1.0 + 4.0 * p2.0 - p3.0) * t2
                    + (-p0.0 + 3.0 * p1.0 - 3.0 * p2.0 + p3.0) * t3);

            let y = 0.5
                * ((2.0 * p1.1)
                    + (-p0.1 + p2.1) * t
                    + (2.0 * p0.1 - 5.0 * p1.1 + 4.0 * p2.1 - p3.1) * t2
                    + (-p0.1 + 3.0 * p1.1 - 3.0 * p2.1 + p3.1) * t3);

            result.push((x, y));
        }
    }

    // Add the last point
    if let Some(last) = points.last() {
        result.push(*last);
    }

    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_points() {
        let points: Vec<(f64, f64)> = vec![];
        let result = catmull_rom_spline(&points, 10);
        assert!(result.is_empty());
    }

    #[test]
    fn test_single_point() {
        let points = vec![(1.0, 2.0)];
        let result = catmull_rom_spline(&points, 10);
        assert_eq!(result.len(), 1);
        assert_eq!(result[0], (1.0, 2.0));
    }

    #[test]
    fn test_two_points() {
        let points = vec![(0.0, 0.0), (1.0, 1.0)];
        let result = catmull_rom_spline(&points, 10);
        assert_eq!(result.len(), 2);
        assert_eq!(result[0], (0.0, 0.0));
        assert_eq!(result[1], (1.0, 1.0));
    }

    #[test]
    fn test_three_points() {
        let points = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.0)];
        let result = catmull_rom_spline(&points, 10);
        // Should have 10 * 2 + 1 = 21 points (10 per segment, plus final point)
        assert_eq!(result.len(), 21);
        // First point should be start
        assert_eq!(result[0], (0.0, 0.0));
        // Last point should be end
        assert_eq!(result[20], (2.0, 0.0));
    }

    #[test]
    fn test_passes_through_control_points() {
        let points = vec![(0.0, 0.0), (1.0, 2.0), (2.0, 1.0), (3.0, 3.0)];
        let result = catmull_rom_spline(&points, 10);

        // First and last points should match exactly
        assert_eq!(result[0], points[0]);
        assert_eq!(*result.last().unwrap(), *points.last().unwrap());

        // Middle control points should appear at t=0 of their segments
        // Second control point is at index 10 (after first segment of 10 points)
        assert!((result[10].0 - 1.0).abs() < 1e-10);
        assert!((result[10].1 - 2.0).abs() < 1e-10);
    }

    #[test]
    fn test_smoothness() {
        let points = vec![(0.0, 0.0), (1.0, 1.0), (2.0, 0.5), (3.0, 1.5)];
        let result = catmull_rom_spline(&points, 10);

        // Check that consecutive points are not too far apart (smooth curve)
        for i in 1..result.len() {
            let dx = result[i].0 - result[i - 1].0;
            let dy = result[i].1 - result[i - 1].1;
            let dist = (dx * dx + dy * dy).sqrt();
            // Distance between consecutive points should be reasonable
            assert!(dist < 1.0, "Distance {} too large at index {}", dist, i);
        }
    }
}
