use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PointCloudId(pub u64);

#[derive(Clone, Debug)]
pub struct PointCloudState {
    /// Camera
    pub camera: Camera3D,

    /// Point cloud data
    pub points: Vec<PointCloudPoint>,

    /// Level of detail (LOD) levels
    pub lod_levels: Vec<LODLevel>,

    /// Color attribute
    pub color_by: ColorAttribute,

    /// Point size
    pub point_size: f32,
}

#[derive(Clone, Debug)]
pub struct Camera3D {
    pub position: [f32; 3],
    pub target: [f32; 3],
    pub up: [f32; 3],
    pub fov: f32,
    pub near: f32,
    pub far: f32,
}

#[derive(Clone, Debug)]
pub struct PointCloudPoint {
    pub position: [f32; 3],
    pub color: [f32; 3],
    pub normal: Option<[f32; 3]>,
    pub intensity: Option<f32>,
}

#[derive(Clone, Debug)]
pub struct LODLevel {
    pub max_distance: f32,
    pub point_stride: usize,
    pub point_count: u32,
}

#[derive(Clone, Debug)]
pub enum ColorAttribute {
    Uniform([f32; 3]),
    ByHeight(ColorMapAttr),
    ByIntensity(ColorMapAttr),
    ByNormal,
    RGB,
}

#[derive(Clone, Debug)]
pub struct ColorMapAttr {
    pub min_value: f32,
    pub max_value: f32,
    pub gradient: Vec<[f32; 3]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PointCloudConfig {
    /// Background color
    pub background: [f32; 4],

    /// Point size (screen pixels)
    pub point_size: f32,

    /// Enable LOD
    pub use_lod: bool,

    /// LOD distance thresholds
    pub lod_distances: Vec<f32>,

    /// Camera settings
    pub camera_speed: f32,
    pub rotation_sensitivity: f32,
}

impl Default for PointCloudState {
    fn default() -> Self {
        Self::new()
    }
}

impl PointCloudState {
    pub fn new() -> Self {
        Self {
            camera: Camera3D {
                position: [5.0, 5.0, 5.0],
                target: [0.0, 0.0, 0.0],
                up: [0.0, 1.0, 0.0],
                fov: 45.0,
                near: 0.1,
                far: 100.0,
            },
            points: Vec::new(),
            lod_levels: Vec::new(),
            color_by: ColorAttribute::RGB,
            point_size: 2.0,
        }
    }

    /// Project 3D point to screen space (returns screen x, y, depth)
    pub fn project_point(&self, x: f32, y: f32, z: f32, w: f32, h: f32) -> Option<(f32, f32, f32)> {
        // View matrix
        let eye = self.camera.position;
        let target = self.camera.target;
        let up = self.camera.up;

        let forward = [
            target[0] - eye[0],
            target[1] - eye[1],
            target[2] - eye[2],
        ];
        let f_len = (forward[0] * forward[0] + forward[1] * forward[1] + forward[2] * forward[2]).sqrt();
        let forward = [forward[0] / f_len, forward[1] / f_len, forward[2] / f_len];

        let right_cross = [
            up[1] * forward[2] - up[2] * forward[1],
            up[2] * forward[0] - up[0] * forward[2],
            up[0] * forward[1] - up[1] * forward[0],
        ];
        let r_len = (right_cross[0] * right_cross[0] + right_cross[1] * right_cross[1] + right_cross[2] * right_cross[2]).sqrt();
        let right = [right_cross[0] / r_len, right_cross[1] / r_len, right_cross[2] / r_len];

        let up_cross = [
            forward[1] * right[2] - forward[2] * right[1],
            forward[2] * right[0] - forward[0] * right[2],
            forward[0] * right[1] - forward[1] * right[0],
        ];

        // Transform to camera space
        let pos = [x - eye[0], y - eye[1], z - eye[2]];
        let cam_x = right[0] * pos[0] + right[1] * pos[1] + right[2] * pos[2];
        let cam_y = up_cross[0] * pos[0] + up_cross[1] * pos[1] + up_cross[2] * pos[2];
        let cam_z = forward[0] * pos[0] + forward[1] * pos[1] + forward[2] * pos[2];

        if cam_z < self.camera.near || cam_z > self.camera.far {
            return None;
        }

        // Perspective projection
        let aspect = w / h;
        let fov_rad = self.camera.fov.to_radians();
        let tan_half_fov = (fov_rad / 2.0).tan();

        let proj_x = cam_x / (cam_z * tan_half_fov * aspect);
        let proj_y = cam_y / (cam_z * tan_half_fov);

        let screen_x = (proj_x + 1.0) * w / 2.0;
        let screen_y = (1.0 - proj_y) * h / 2.0;

        Some((screen_x, screen_y, cam_z))
    }

    /// Get visible points with LOD filtering (screen x, y, depth, color)
    pub fn visible_points(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, [f32; 4])> {
        let eye = self.camera.position;
        let mut visible = Vec::new();

        for (i, p) in self.points.iter().enumerate() {
            let dx = p.position[0] - eye[0];
            let dy = p.position[1] - eye[1];
            let dz = p.position[2] - eye[2];
            let dist = (dx * dx + dy * dy + dz * dz).sqrt();

            // Simple LOD: skip every other point if far away
            let lod_stride = if dist > 20.0 { 4 } else if dist > 10.0 { 2 } else { 1 };
            if i % lod_stride != 0 {
                continue;
            }

            if let Some((sx, sy, depth)) = self.project_point(p.position[0], p.position[1], p.position[2], w, h) {
                let color = [p.color[0], p.color[1], p.color[2], 1.0];
                visible.push((sx, sy, depth, color));
            }
        }

        visible
    }

    // Render helpers
    pub fn visible_points_lod(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, [f32; 4])> {
        self.visible_points(w, h)
    }

    pub fn color_by_attribute(&self, point_idx: usize, attr: &str) -> [f32; 4] {
        let point = match self.points.get(point_idx) {
            Some(p) => p,
            None => return [0.5, 0.5, 0.5, 1.0],
        };

        match attr {
            "height" => {
                let t = (point.position[1] + 1.0) / 2.0;
                [t, 0.5, 1.0 - t, 1.0]
            }
            "intensity" => {
                let i = point.intensity.unwrap_or(0.5);
                [i, i, i, 1.0]
            }
            _ => [point.color[0], point.color[1], point.color[2], 1.0],
        }
    }

    pub fn format_point(&self, point_idx: usize) -> String {
        if let Some(p) = self.points.get(point_idx) {
            format!("({:.2}, {:.2}, {:.2})", p.position[0], p.position[1], p.position[2])
        } else {
            String::from("N/A")
        }
    }
}

impl Default for PointCloudConfig {
    fn default() -> Self {
        Self {
            background: [0.0, 0.0, 0.0, 1.0],
            point_size: 2.0,
            use_lod: true,
            lod_distances: vec![10.0, 50.0, 200.0],
            camera_speed: 0.1,
            rotation_sensitivity: 0.01,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PointCloudPanel {
    id: PointCloudId,
    title: String,
}

impl PointCloudPanel {
    pub fn new(id: PointCloudId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PointCloudId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "point_cloud"
    }

    pub fn kind_label(&self) -> &'static str {
        "Point Cloud"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
