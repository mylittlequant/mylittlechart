use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct PhaseSpaceId(pub u64);

#[derive(Clone, Debug)]
pub struct PhaseSpaceState {
    /// Trajectories (time series in phase space)
    pub trajectories: Vec<Trajectory>,

    /// Dimension count (2D or 3D)
    pub dimensions: usize,

    /// Axis ranges
    pub ranges: Vec<(f32, f32)>,

    /// Camera (for 3D)
    pub camera: Option<Camera3D>,

    /// Attractor detection
    pub attractors: Vec<Attractor>,
}

#[derive(Clone, Debug)]
pub struct Trajectory {
    pub points: Vec<Vec<f32>>,
    pub color: [f32; 4],
    pub label: String,
}

#[derive(Clone, Debug)]
pub struct Attractor {
    pub center: Vec<f32>,
    pub radius: f32,
    pub attractor_type: AttractorType,
}

#[derive(Clone, Copy, Debug)]
pub enum AttractorType {
    FixedPoint,
    LimitCycle,
    StrangeAttractor,
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

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseSpaceConfig {
    /// Trajectory line width
    pub line_width: f32,

    /// Show attractor markers
    pub show_attractors: bool,

    /// Tail length (0 = full trajectory)
    pub tail_length: usize,

    /// 3D rendering
    pub use_3d: bool,
}

impl Default for PhaseSpaceState {
    fn default() -> Self {
        Self::new()
    }
}

impl PhaseSpaceState {
    pub fn new() -> Self {
        Self {
            trajectories: Vec::new(),
            dimensions: 2,
            ranges: vec![(0.0, 1.0), (0.0, 1.0)],
            camera: None,
            attractors: Vec::new(),
        }
    }

    /// Get trajectory points in screen space for a specific trajectory
    pub fn trajectory_points(&self, traj_idx: usize, w: f32, h: f32) -> Vec<(f32, f32)> {
        if traj_idx >= self.trajectories.len() {
            return Vec::new();
        }

        let traj = &self.trajectories[traj_idx];

        if self.dimensions == 2 {
            traj.points
                .iter()
                .filter(|p| p.len() >= 2)
                .map(|p| self.value_to_screen(p[0] as f64, p[1] as f64, w, h))
                .collect()
        } else {
            // For 3D, project to 2D (simplified: use x-y plane)
            traj.points
                .iter()
                .filter(|p| p.len() >= 2)
                .map(|p| self.value_to_screen(p[0] as f64, p[1] as f64, w, h))
                .collect()
        }
    }

    /// Convert phase space value coordinates to screen coordinates
    pub fn value_to_screen(&self, x: f64, y: f64, w: f32, h: f32) -> (f32, f32) {
        if self.ranges.len() < 2 {
            return (0.0, 0.0);
        }

        let x_range = self.ranges[0];
        let y_range = self.ranges[1];

        let nx = ((x as f32 - x_range.0) / (x_range.1 - x_range.0)).clamp(0.0, 1.0);
        let ny = ((y as f32 - y_range.0) / (y_range.1 - y_range.0)).clamp(0.0, 1.0);

        let sx = nx * w;
        let sy = (1.0 - ny) * h;

        (sx, sy)
    }

    // Render helpers
    pub fn visible_trajectories(&self) -> &[Trajectory] {
        &self.trajectories
    }

    pub fn trajectory_color(&self, traj_idx: usize) -> Option<[f32; 4]> {
        self.trajectories.get(traj_idx).map(|t| t.color)
    }

    pub fn format_state(&self, traj_idx: usize, point_idx: usize) -> String {
        if let Some(traj) = self.trajectories.get(traj_idx) {
            if let Some(point) = traj.points.get(point_idx) {
                if point.len() >= 2 {
                    return format!("({:.3}, {:.3})", point[0], point[1]);
                }
            }
        }
        String::from("N/A")
    }
}

impl Default for PhaseSpaceConfig {
    fn default() -> Self {
        Self {
            line_width: 1.5,
            show_attractors: true,
            tail_length: 0,
            use_3d: false,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PhaseSpacePanel {
    id: PhaseSpaceId,
    title: String,
}

impl PhaseSpacePanel {
    pub fn new(id: PhaseSpaceId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> PhaseSpaceId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "phase_space"
    }

    pub fn kind_label(&self) -> &'static str {
        "Phase Space"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
