use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct IsosurfaceId(pub u64);

#[derive(Clone, Debug)]
pub struct IsosurfaceState {
    /// Camera
    pub camera: Camera3D,

    /// Volumetric data (3D grid)
    pub volume: Vec<f32>,
    pub width: usize,
    pub height: usize,
    pub depth: usize,

    /// Isosurface level
    pub isovalue: f32,

    /// Generated mesh
    pub mesh: Option<IsosurfaceMesh>,

    /// Rendering mode
    pub render_mode: SurfaceRenderMode,
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
pub struct IsosurfaceMesh {
    pub vertices: Vec<Vertex3D>,
    pub indices: Vec<u32>,
}

#[derive(Clone, Debug)]
pub struct Vertex3D {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub color: [f32; 4],
}

#[derive(Clone, Copy, Debug)]
pub enum SurfaceRenderMode {
    Solid,
    Wireframe,
    SolidWithWireframe,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IsosurfaceConfig {
    /// Background color
    pub background: [f32; 4],

    /// Surface color
    pub surface_color: [f32; 4],

    /// Wireframe color
    pub wireframe_color: [f32; 4],

    /// Lighting
    pub light_position: [f32; 3],
    pub ambient: f32,
    pub diffuse: f32,
    pub specular: f32,

    /// Camera
    pub camera_speed: f32,
}

impl Default for IsosurfaceState {
    fn default() -> Self {
        Self::new()
    }
}

impl IsosurfaceState {
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
            volume: Vec::new(),
            width: 0,
            height: 0,
            depth: 0,
            isovalue: 0.5,
            mesh: None,
            render_mode: SurfaceRenderMode::Solid,
        }
    }

    /// Get projected triangles for rendering (screen-space triangle vertices)
    pub fn projected_triangles(&self, w: f32, h: f32) -> Vec<[(f32, f32); 3]> {
        let mesh = match &self.mesh {
            Some(m) => m,
            None => return Vec::new(),
        };

        let mut triangles = Vec::new();

        for chunk in mesh.indices.chunks(3) {
            if chunk.len() != 3 {
                continue;
            }

            let i0 = chunk[0] as usize;
            let i1 = chunk[1] as usize;
            let i2 = chunk[2] as usize;

            if i0 >= mesh.vertices.len() || i1 >= mesh.vertices.len() || i2 >= mesh.vertices.len() {
                continue;
            }

            let v0 = &mesh.vertices[i0];
            let v1 = &mesh.vertices[i1];
            let v2 = &mesh.vertices[i2];

            if let (Some(p0), Some(p1), Some(p2)) = (
                self.project_vertex(&v0.position, w, h),
                self.project_vertex(&v1.position, w, h),
                self.project_vertex(&v2.position, w, h),
            ) {
                triangles.push([p0, p1, p2]);
            }
        }

        triangles
    }

    fn project_vertex(&self, pos: &[f32; 3], w: f32, h: f32) -> Option<(f32, f32)> {
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

        let rel = [pos[0] - eye[0], pos[1] - eye[1], pos[2] - eye[2]];
        let cam_x = right[0] * rel[0] + right[1] * rel[1] + right[2] * rel[2];
        let cam_y = up_cross[0] * rel[0] + up_cross[1] * rel[1] + up_cross[2] * rel[2];
        let cam_z = forward[0] * rel[0] + forward[1] * rel[1] + forward[2] * rel[2];

        if cam_z < self.camera.near || cam_z > self.camera.far {
            return None;
        }

        let aspect = w / h;
        let fov_rad = self.camera.fov.to_radians();
        let tan_half_fov = (fov_rad / 2.0).tan();

        let proj_x = cam_x / (cam_z * tan_half_fov * aspect);
        let proj_y = cam_y / (cam_z * tan_half_fov);

        let screen_x = (proj_x + 1.0) * w / 2.0;
        let screen_y = (1.0 - proj_y) * h / 2.0;

        Some((screen_x, screen_y))
    }

    /// Extract isosurface using marching cubes (simplified placeholder)
    pub fn extract_surface(&mut self) {
        if self.volume.is_empty() || self.width == 0 || self.height == 0 || self.depth == 0 {
            return;
        }

        // Simplified placeholder: create a simple mesh
        let mut vertices = Vec::new();
        let mut indices = Vec::new();

        // Scan volume and create vertices where value crosses isovalue
        for z in 0..self.depth.saturating_sub(1) {
            for y in 0..self.height.saturating_sub(1) {
                for x in 0..self.width.saturating_sub(1) {
                    let idx = z * self.width * self.height + y * self.width + x;
                    if idx >= self.volume.len() {
                        continue;
                    }

                    let v = self.volume[idx];
                    if (v - self.isovalue).abs() < 0.1 {
                        let pos = [
                            x as f32 / self.width as f32 * 2.0 - 1.0,
                            y as f32 / self.height as f32 * 2.0 - 1.0,
                            z as f32 / self.depth as f32 * 2.0 - 1.0,
                        ];
                        vertices.push(Vertex3D {
                            position: pos,
                            normal: [0.0, 1.0, 0.0],
                            color: [0.5, 0.7, 1.0, 1.0],
                        });
                    }
                }
            }
        }

        // Create simple triangles (placeholder)
        for i in (0..vertices.len().saturating_sub(2)).step_by(3) {
            indices.push(i as u32);
            indices.push((i + 1) as u32);
            indices.push((i + 2) as u32);
        }

        self.mesh = Some(IsosurfaceMesh { vertices, indices });
    }

    // Render helpers
    pub fn visible_triangles(&self) -> Vec<[usize; 3]> {
        if let Some(mesh) = &self.mesh {
            mesh.indices
                .chunks(3)
                .filter_map(|chunk| {
                    if chunk.len() == 3 {
                        Some([chunk[0] as usize, chunk[1] as usize, chunk[2] as usize])
                    } else {
                        None
                    }
                })
                .collect()
        } else {
            Vec::new()
        }
    }

    pub fn surface_normal(&self, vertex_idx: usize) -> Option<[f32; 3]> {
        self.mesh.as_ref()?.vertices.get(vertex_idx).map(|v| v.normal)
    }

    pub fn depth_shade(&self, depth: f32) -> f32 {
        let normalized = ((depth - self.camera.near) / (self.camera.far - self.camera.near)).clamp(0.0, 1.0);
        1.0 - normalized * 0.5
    }
}

impl Default for IsosurfaceConfig {
    fn default() -> Self {
        Self {
            background: [0.1, 0.1, 0.15, 1.0],
            surface_color: [0.5, 0.7, 1.0, 1.0],
            wireframe_color: [1.0, 1.0, 1.0, 1.0],
            light_position: [5.0, 5.0, 5.0],
            ambient: 0.3,
            diffuse: 0.6,
            specular: 0.3,
            camera_speed: 0.1,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct IsosurfacePanel {
    id: IsosurfaceId,
    title: String,
}

impl IsosurfacePanel {
    pub fn new(id: IsosurfaceId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> IsosurfaceId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "isosurface"
    }

    pub fn kind_label(&self) -> &'static str {
        "Isosurface"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
