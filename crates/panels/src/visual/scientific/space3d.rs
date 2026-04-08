use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct Space3DId(pub u64);

#[derive(Clone, Debug)]
pub struct Space3DState {
    /// Camera transform
    pub camera: Camera3D,

    /// 3D data points
    pub points: Vec<Point3D>,

    /// Surface meshes
    pub surfaces: Vec<Surface3D>,

    /// Axis ranges
    pub x_range: (f32, f32),
    pub y_range: (f32, f32),
    pub z_range: (f32, f32),

    /// Color mode
    pub color_mode: ColorMode3D,
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
pub struct Point3D {
    pub position: [f32; 3],
    pub color: [f32; 4],
    pub size: f32,
    pub label: Option<String>,
}

#[derive(Clone, Debug)]
pub struct Surface3D {
    pub vertices: Vec<Vertex3D>,
    pub indices: Vec<u32>,
    pub color: [f32; 4],
    pub wireframe: bool,
}

#[derive(Clone, Debug)]
pub struct Vertex3D {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Clone, Debug)]
pub enum ColorMode3D {
    Uniform([f32; 4]),
    ByValue(ColorMap3D),
    ByCluster(Vec<[f32; 4]>),
}

#[derive(Clone, Debug)]
pub struct ColorMap3D {
    pub min_value: f32,
    pub max_value: f32,
    pub gradient: Vec<[f32; 4]>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Space3DConfig {
    /// Background color
    pub background: [f32; 4],

    /// Default point size
    pub point_size: f32,

    /// Show axes
    pub show_axes: bool,

    /// Show grid
    pub show_grid: bool,

    /// Grid spacing
    pub grid_spacing: f32,

    /// Camera controls
    pub camera_speed: f32,
    pub rotation_sensitivity: f32,

    /// Lighting
    pub light_position: [f32; 3],
    pub ambient_strength: f32,
}

impl Default for Space3DState {
    fn default() -> Self {
        Self::new()
    }
}

impl Space3DState {
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
            surfaces: Vec::new(),
            x_range: (-1.0, 1.0),
            y_range: (-1.0, 1.0),
            z_range: (-1.0, 1.0),
            color_mode: ColorMode3D::Uniform([1.0, 1.0, 1.0, 1.0]),
        }
    }

    /// Project 3D point to screen space (returns screen x, y, depth)
    pub fn project_point(&self, x: f64, y: f64, z: f64, w: f32, h: f32) -> Option<(f32, f32, f32)> {
        let x = x as f32;
        let y = y as f32;
        let z = z as f32;

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

    /// Get all projected points (screen x, y, depth, color)
    pub fn projected_points(&self, w: f32, h: f32) -> Vec<(f32, f32, f32, [f32; 4])> {
        self.points
            .iter()
            .filter_map(|p| {
                self.project_point(p.position[0] as f64, p.position[1] as f64, p.position[2] as f64, w, h)
                    .map(|(sx, sy, depth)| (sx, sy, depth, p.color))
            })
            .collect()
    }

    /// Rotate camera around target
    pub fn rotate(&mut self, dx: f32, dy: f32) {
        let target = self.camera.target;
        let pos = self.camera.position;

        let dx = dx * 0.01;
        let dy = dy * 0.01;

        let rel = [pos[0] - target[0], pos[1] - target[1], pos[2] - target[2]];
        let radius = (rel[0] * rel[0] + rel[1] * rel[1] + rel[2] * rel[2]).sqrt();
        let theta = (rel[1] / (rel[0] * rel[0] + rel[2] * rel[2]).sqrt()).atan();
        let phi = (rel[0]).atan2(rel[2]);

        let new_theta = (theta + dy).clamp(-std::f32::consts::PI / 2.0 + 0.01, std::f32::consts::PI / 2.0 - 0.01);
        let new_phi = phi + dx;

        self.camera.position = [
            target[0] + radius * new_theta.cos() * new_phi.sin(),
            target[1] + radius * new_theta.sin(),
            target[2] + radius * new_theta.cos() * new_phi.cos(),
        ];
    }

    // Render helpers
    pub fn visible_points(&self) -> &[Point3D] {
        &self.points
    }

    pub fn rotate_camera(&mut self, dx: f32, dy: f32) {
        self.rotate(dx, dy);
    }

    pub fn depth_color(&self, depth: f32) -> [f32; 4] {
        let normalized = ((depth - self.camera.near) / (self.camera.far - self.camera.near)).clamp(0.0, 1.0);
        let intensity = 1.0 - normalized * 0.5;
        [intensity, intensity, intensity, 1.0]
    }
}

impl Default for Space3DConfig {
    fn default() -> Self {
        Self {
            background: [0.1, 0.1, 0.15, 1.0],
            point_size: 5.0,
            show_axes: true,
            show_grid: true,
            grid_spacing: 1.0,
            camera_speed: 0.1,
            rotation_sensitivity: 0.01,
            light_position: [5.0, 5.0, 5.0],
            ambient_strength: 0.3,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Space3DPanel {
    id: Space3DId,
    title: String,
}

impl Space3DPanel {
    pub fn new(id: Space3DId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> Space3DId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "space3d"
    }

    pub fn kind_label(&self) -> &'static str {
        "3D Space"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 300.0)
    }
}
