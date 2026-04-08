use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct FlowFieldId(pub u64);

#[derive(Clone, Debug)]
pub struct FlowParticle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub age: f32,
    pub max_age: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct FlowFieldState {
    pub particles: Vec<FlowParticle>,
    pub field: Vec<Vec<(f32, f32)>>,
    pub grid_width: usize,
    pub grid_height: usize,
    pub width: f32,
    pub height: f32,
}

impl Default for FlowFieldState {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            field: Vec::new(),
            grid_width: 0,
            grid_height: 0,
            width: 800.0,
            height: 600.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlowFieldConfig {
    pub particle_count: usize,
    pub particle_speed: f32,
    pub particle_lifetime: f32,
    pub field_resolution: usize,
    pub trail_length: usize,
}

impl Default for FlowFieldConfig {
    fn default() -> Self {
        Self {
            particle_count: 2000,
            particle_speed: 2.0,
            particle_lifetime: 3.0,
            field_resolution: 32,
            trail_length: 10,
        }
    }
}

impl FlowFieldState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set viewport dimensions for physics simulation
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    /// Returns particle positions as (x, y, speed, color)
    pub fn particle_positions(&self) -> Vec<(f32, f32, f32, [f32; 4])> {
        self.particles
            .iter()
            .map(|particle| {
                let speed = (particle.vx * particle.vx + particle.vy * particle.vy).sqrt();
                let color = [
                    particle.color[0] as f32 / 255.0,
                    particle.color[1] as f32 / 255.0,
                    particle.color[2] as f32 / 255.0,
                    particle.color[3] as f32 / 255.0,
                ];
                (particle.x, particle.y, speed, color)
            })
            .collect()
    }

    /// Advance particles
    pub fn tick(&mut self, dt: f64) {
        let config = FlowFieldConfig::default();

        // Pre-compute field vectors to avoid borrowing issues
        let field_vectors: Vec<(f32, f32)> = self
            .particles
            .iter()
            .map(|p| self.vector_at(p.x, p.y))
            .collect();

        for (i, particle) in self.particles.iter_mut().enumerate() {
            // Get vector from field
            let (fx, fy) = field_vectors.get(i).copied().unwrap_or((0.0, 0.0));

            // Update velocity
            particle.vx += fx * config.particle_speed * dt as f32;
            particle.vy += fy * config.particle_speed * dt as f32;

            // Limit speed
            let speed = (particle.vx * particle.vx + particle.vy * particle.vy).sqrt();
            if speed > config.particle_speed {
                particle.vx = (particle.vx / speed) * config.particle_speed;
                particle.vy = (particle.vy / speed) * config.particle_speed;
            }

            // Update position
            particle.x += particle.vx * dt as f32;
            particle.y += particle.vy * dt as f32;

            // Age particle
            particle.age += dt as f32;

            // Respawn if too old (simple deterministic respawn)
            if particle.age > particle.max_age {
                particle.age = 0.0;
                particle.x = ((i * 73) as f32 % self.width).max(0.0);
                particle.y = ((i * 97) as f32 % self.height).max(0.0);
                particle.vx = 0.0;
                particle.vy = 0.0;
            }
        }
    }

    /// Get flow field vector at position
    pub fn vector_at(&self, x: f32, y: f32) -> (f32, f32) {
        if self.grid_width == 0 || self.grid_height == 0 || self.field.is_empty() {
            return (0.0, 0.0);
        }

        let grid_x = ((x / self.width) * self.grid_width as f32).floor() as usize;
        let grid_y = ((y / self.height) * self.grid_height as f32).floor() as usize;

        let grid_x = grid_x.min(self.grid_width - 1);
        let grid_y = grid_y.min(self.grid_height - 1);

        self.field
            .get(grid_y)
            .and_then(|row| row.get(grid_x))
            .copied()
            .unwrap_or((0.0, 0.0))
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct FlowFieldPanel {
    id: FlowFieldId,
    title: String,
    config: FlowFieldConfig,
}

impl FlowFieldPanel {
    pub fn new(id: FlowFieldId, title: String) -> Self {
        Self {
            id,
            title,
            config: FlowFieldConfig::default(),
        }
    }

    pub fn id(&self) -> FlowFieldId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "flow_field"
    }

    pub fn kind_label(&self) -> &'static str {
        "Flow Field"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
