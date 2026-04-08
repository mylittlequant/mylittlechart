use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct VolatilitySurfaceId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct VolatilitySurfaceState {
    pub symbol: String,
    pub surface_data: Vec<Vec<f64>>,  // [expiry_idx][strike_idx] = IV
    pub strikes: Vec<f64>,
    pub expiries: Vec<i64>,  // Unix timestamps
    pub spot_price: f64,
    pub rotation: (f32, f32),  // (pitch, yaw) for 3D rotation
    pub zoom: f32,
    pub color_range: (f64, f64),  // Min/max IV for color mapping
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VolatilitySurfaceConfig {
    pub color_gradient: ColorGradient,
    pub grid_lines: bool,
    pub show_projection: bool,  // Show 2D projection on floor
    pub light_angle: (f32, f32),
    pub perspective_strength: f32,
    pub interpolation: InterpolationMode,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ColorGradient {
    pub stops: Vec<(f64, [f32; 3])>,  // (IV value, OKLCH color)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum InterpolationMode {
    Linear,
    Cubic,
    Spline,
}

impl VolatilitySurfaceState {
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            ..Default::default()
        }
    }

    /// Projects a 3D point (strike, expiry, iv) to 2D screen coordinates
    pub fn project_point(&self, strike: f64, expiry: f64, iv: f64, w: f32, h: f32) -> (f32, f32) {
        if self.strikes.is_empty() || self.expiries.is_empty() {
            return (0.0, 0.0);
        }

        let (min_strike, max_strike) = (self.strikes[0], self.strikes[self.strikes.len() - 1]);
        let (min_expiry, max_expiry) = (self.expiries[0] as f64, self.expiries[self.expiries.len() - 1] as f64);

        let strike_norm = ((strike - min_strike) / (max_strike - min_strike)).clamp(0.0, 1.0);
        let expiry_norm = ((expiry - min_expiry) / (max_expiry - min_expiry)).clamp(0.0, 1.0);

        let iv_norm = if self.color_range.1 > self.color_range.0 {
            ((iv - self.color_range.0) / (self.color_range.1 - self.color_range.0)).clamp(0.0, 1.0)
        } else {
            0.5
        };

        let (pitch, yaw) = self.rotation;
        let pitch_rad = (pitch as f64).to_radians();
        let yaw_rad = (yaw as f64).to_radians();

        let x_3d = strike_norm - 0.5;
        let y_3d = expiry_norm - 0.5;
        let z_3d = iv_norm * 0.5;

        let x_rot = x_3d * yaw_rad.cos() - y_3d * yaw_rad.sin();
        let y_rot = x_3d * yaw_rad.sin() + y_3d * yaw_rad.cos();
        let z_rot = z_3d;

        let y_final = y_rot * pitch_rad.cos() - z_rot * pitch_rad.sin();
        let _z_final = y_rot * pitch_rad.sin() + z_rot * pitch_rad.cos();

        let x = (x_rot * self.zoom as f64 + 0.5) as f32 * w;
        let y = ((0.5 - y_final) * self.zoom as f64) as f32 * h;

        (x, y)
    }

    /// Returns wireframe lines for 3D surface rendering
    pub fn wireframe_lines(&self, w: f32, h: f32) -> Vec<((f32, f32), (f32, f32))> {
        let mut lines = Vec::new();

        if self.surface_data.is_empty() || self.strikes.is_empty() || self.expiries.is_empty() {
            return lines;
        }

        for (e_idx, expiry_data) in self.surface_data.iter().enumerate() {
            if e_idx >= self.expiries.len() {
                break;
            }
            let expiry = self.expiries[e_idx] as f64;

            for s_idx in 0..expiry_data.len().saturating_sub(1) {
                if s_idx >= self.strikes.len() {
                    break;
                }
                let strike1 = self.strikes[s_idx];
                let strike2 = self.strikes[s_idx + 1];
                let iv1 = expiry_data[s_idx];
                let iv2 = expiry_data[s_idx + 1];

                let p1 = self.project_point(strike1, expiry, iv1, w, h);
                let p2 = self.project_point(strike2, expiry, iv2, w, h);
                lines.push((p1, p2));
            }
        }

        for s_idx in 0..self.strikes.len() {
            let strike = self.strikes[s_idx];

            for e_idx in 0..self.surface_data.len().saturating_sub(1) {
                if e_idx >= self.expiries.len() || e_idx + 1 >= self.expiries.len() {
                    break;
                }
                if s_idx >= self.surface_data[e_idx].len() || s_idx >= self.surface_data[e_idx + 1].len() {
                    continue;
                }

                let expiry1 = self.expiries[e_idx] as f64;
                let expiry2 = self.expiries[e_idx + 1] as f64;
                let iv1 = self.surface_data[e_idx][s_idx];
                let iv2 = self.surface_data[e_idx + 1][s_idx];

                let p1 = self.project_point(strike, expiry1, iv1, w, h);
                let p2 = self.project_point(strike, expiry2, iv2, w, h);
                lines.push((p1, p2));
            }
        }

        lines
    }

    /// Returns visible grid points for rendering
    pub fn visible_grid_points(&self, w: f32, h: f32) -> Vec<(f32, f32, f64)> {
        let mut points = Vec::new();

        if self.surface_data.is_empty() || self.strikes.is_empty() || self.expiries.is_empty() {
            return points;
        }

        for (e_idx, expiry_data) in self.surface_data.iter().enumerate() {
            if e_idx >= self.expiries.len() {
                break;
            }
            let expiry = self.expiries[e_idx] as f64;

            for (s_idx, &iv) in expiry_data.iter().enumerate() {
                if s_idx >= self.strikes.len() {
                    break;
                }
                let strike = self.strikes[s_idx];
                let (x, y) = self.project_point(strike, expiry, iv, w, h);
                points.push((x, y, iv));
            }
        }

        points
    }

    /// Maps IV value to color using gradient
    pub fn iv_color(&self, iv: f64) -> [f32; 4] {
        let (min_iv, max_iv) = self.color_range;
        let norm_iv = if max_iv > min_iv {
            ((iv - min_iv) / (max_iv - min_iv)).clamp(0.0, 1.0)
        } else {
            0.5
        };

        // Interpolate between blue (low) -> green (mid) -> red (high)
        if norm_iv < 0.5 {
            let t = (norm_iv * 2.0) as f32;
            [0.6 + t * 0.05, 0.2, 240.0 - t * 95.0, 1.0]
        } else {
            let t = ((norm_iv - 0.5) * 2.0) as f32;
            [0.65 - t * 0.05, 0.15 + t * 0.03, 145.0 - t * 120.0, 1.0]
        }
    }

    /// Returns strike axis labels and positions
    pub fn strike_axis(&self) -> Vec<(f64, String)> {
        self.strikes.iter()
            .map(|&strike| (strike, format!("{:.0}", strike)))
            .collect()
    }

    /// Returns expiry axis labels and positions
    pub fn expiry_axis(&self) -> Vec<(i64, String)> {
        self.expiries.iter()
            .enumerate()
            .map(|(i, &expiry)| {
                // Simple formatting: show as relative index or timestamp
                (expiry, format!("T+{}", i))
            })
            .collect()
    }
}

impl Default for VolatilitySurfaceConfig {
    fn default() -> Self {
        Self {
            color_gradient: ColorGradient {
                stops: vec![
                    (0.0, [0.6, 0.2, 240.0]),   // Low IV: blue
                    (0.5, [0.65, 0.15, 145.0]), // Medium IV: green
                    (1.0, [0.60, 0.18, 25.0]),  // High IV: red
                ],
            },
            grid_lines: true,
            show_projection: false,
            light_angle: (45.0, 45.0),
            perspective_strength: 0.3,
            interpolation: InterpolationMode::Linear,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct VolatilitySurfacePanel {
    id: VolatilitySurfaceId,
    title: String,
}

impl VolatilitySurfacePanel {
    pub fn new(id: VolatilitySurfaceId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> VolatilitySurfaceId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "volatility_surface"
    }

    pub fn kind_label(&self) -> &'static str {
        "Vol Surface"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
