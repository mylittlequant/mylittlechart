use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IvSurfaceId(pub u64);

#[derive(Clone, Debug, Default)]
pub struct IvSurfaceState {
    pub symbol: String,
    pub surface_data: Vec<Vec<f64>>,  // [strike_idx][expiry_idx] = IV
    pub strikes: Vec<f64>,
    pub expiries: Vec<i64>,  // Unix timestamps
    pub spot_price: f64,
    pub rotation: (f32, f32),
    pub zoom: f32,
    pub color_range: (f64, f64),
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IvSurfaceConfig {
    pub color_gradient: ColorGradient,
    pub grid_lines: bool,
    pub show_projection: bool,
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

impl IvSurfaceState {
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            ..Default::default()
        }
    }

    /// Returns wireframe lines for 3D surface rendering
    pub fn wireframe_lines(&self, w: f32, h: f32) -> Vec<((f32, f32), (f32, f32))> {
        let mut lines = Vec::new();

        if self.surface_data.is_empty() || self.strikes.is_empty() || self.expiries.is_empty() {
            return lines;
        }

        for (s_idx, strike_data) in self.surface_data.iter().enumerate() {
            if s_idx >= self.strikes.len() {
                break;
            }
            let strike = self.strikes[s_idx];

            for e_idx in 0..strike_data.len().saturating_sub(1) {
                if e_idx >= self.expiries.len() {
                    break;
                }
                let expiry1 = self.expiries[e_idx] as f64;
                let expiry2 = self.expiries[e_idx + 1] as f64;
                let iv1 = strike_data[e_idx];
                let iv2 = strike_data[e_idx + 1];

                let p1 = self.project_point(strike, expiry1, iv1, w, h);
                let p2 = self.project_point(strike, expiry2, iv2, w, h);
                lines.push((p1, p2));
            }
        }

        for e_idx in 0..self.expiries.len() {
            let expiry = self.expiries[e_idx] as f64;

            for s_idx in 0..self.surface_data.len().saturating_sub(1) {
                if s_idx >= self.strikes.len() || s_idx + 1 >= self.strikes.len() {
                    break;
                }
                if e_idx >= self.surface_data[s_idx].len() || e_idx >= self.surface_data[s_idx + 1].len() {
                    continue;
                }

                let strike1 = self.strikes[s_idx];
                let strike2 = self.strikes[s_idx + 1];
                let iv1 = self.surface_data[s_idx][e_idx];
                let iv2 = self.surface_data[s_idx + 1][e_idx];

                let p1 = self.project_point(strike1, expiry, iv1, w, h);
                let p2 = self.project_point(strike2, expiry, iv2, w, h);
                lines.push((p1, p2));
            }
        }

        lines
    }

    fn project_point(&self, strike: f64, expiry: f64, iv: f64, w: f32, h: f32) -> (f32, f32) {
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

        let x = (x_rot * self.zoom as f64 + 0.5) as f32 * w;
        let y = ((0.5 - y_final) * self.zoom as f64) as f32 * h;

        (x, y)
    }

    /// Get visible data points for rendering
    pub fn visible_points(&self) -> Vec<(f64, i64, f64)> {
        let mut points = Vec::new();
        for (s_idx, strike_data) in self.surface_data.iter().enumerate() {
            if s_idx >= self.strikes.len() {
                break;
            }
            let strike = self.strikes[s_idx];
            for (e_idx, &iv) in strike_data.iter().enumerate() {
                if e_idx >= self.expiries.len() {
                    break;
                }
                let expiry = self.expiries[e_idx];
                points.push((strike, expiry, iv));
            }
        }
        points
    }

    /// Get color for IV value based on gradient
    pub fn iv_color(&self, iv: f64) -> [f32; 4] {
        let iv_norm = if self.color_range.1 > self.color_range.0 {
            ((iv - self.color_range.0) / (self.color_range.1 - self.color_range.0)).clamp(0.0, 1.0)
        } else {
            0.5
        };

        if iv_norm < 0.33 {
            [0.3, 0.6, 0.9, 1.0] // blue - low IV
        } else if iv_norm < 0.66 {
            [0.2, 0.8, 0.3, 1.0] // green - medium IV
        } else {
            [0.9, 0.2, 0.2, 1.0] // red - high IV
        }
    }

    /// Format IV value for display
    pub fn format_iv(&self, iv: f64) -> String {
        format!("{:.1}%", iv * 100.0)
    }
}

impl Default for IvSurfaceConfig {
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
pub struct IvSurfacePanel {
    id: IvSurfaceId,
    title: String,
}

impl IvSurfacePanel {
    pub fn new(id: IvSurfaceId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> IvSurfaceId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "iv_surface"
    }

    pub fn kind_label(&self) -> &'static str {
        "IV Surface"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
