use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ParticleSystemId(pub u64);

#[derive(Clone, Debug)]
pub struct Particle {
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
    pub ax: f32,
    pub ay: f32,
    pub life: f32,
    pub size: f32,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct ParticleEmitter {
    pub x: f32,
    pub y: f32,
    pub rate: f32,
    pub velocity: (f32, f32),
    pub spread: f32,
    pub life: f32,
}

#[derive(Clone, Debug)]
pub struct ParticleSystemState {
    pub particles: Vec<Particle>,
    pub emitters: Vec<ParticleEmitter>,
    pub width: f32,
    pub height: f32,
}

impl Default for ParticleSystemState {
    fn default() -> Self {
        Self {
            particles: Vec::new(),
            emitters: Vec::new(),
            width: 800.0,
            height: 600.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParticleSystemConfig {
    pub max_particles: usize,
    pub gravity: (f32, f32),
    pub damping: f32,
    pub collision: bool,
}

impl Default for ParticleSystemConfig {
    fn default() -> Self {
        Self {
            max_particles: 10000,
            gravity: (0.0, 9.8),
            damping: 0.99,
            collision: false,
        }
    }
}

impl ParticleSystemState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Set viewport dimensions for physics simulation
    pub fn set_viewport(&mut self, width: f32, height: f32) {
        self.width = width;
        self.height = height;
    }

    /// Returns active particles as (x, y, size, color)
    pub fn active_particles(&self) -> Vec<(f32, f32, f32, [f32; 4])> {
        self.particles
            .iter()
            .filter(|p| p.life > 0.0)
            .map(|particle| {
                let color = [
                    particle.color[0] as f32 / 255.0,
                    particle.color[1] as f32 / 255.0,
                    particle.color[2] as f32 / 255.0,
                    particle.color[3] as f32 / 255.0,
                ];
                (particle.x, particle.y, particle.size, color)
            })
            .collect()
    }

    /// Update physics and spawn/kill particles
    pub fn tick(&mut self, dt: f64) {
        let config = ParticleSystemConfig::default();

        // Update existing particles
        for particle in &mut self.particles {
            if particle.life <= 0.0 {
                continue;
            }

            // Apply gravity
            particle.ax += config.gravity.0;
            particle.ay += config.gravity.1;

            // Update velocity
            particle.vx += particle.ax * dt as f32;
            particle.vy += particle.ay * dt as f32;

            // Apply damping
            particle.vx *= config.damping;
            particle.vy *= config.damping;

            // Update position
            particle.x += particle.vx * dt as f32;
            particle.y += particle.vy * dt as f32;

            // Reset acceleration
            particle.ax = 0.0;
            particle.ay = 0.0;

            // Decrease life
            particle.life -= dt as f32;

            // Collision with bounds (if enabled)
            if config.collision {
                if particle.x < 0.0 || particle.x > self.width {
                    particle.vx *= -0.8;
                    particle.x = particle.x.clamp(0.0, self.width);
                }
                if particle.y < 0.0 || particle.y > self.height {
                    particle.vy *= -0.8;
                    particle.y = particle.y.clamp(0.0, self.height);
                }
            }
        }

        // Remove dead particles
        self.particles.retain(|p| p.life > 0.0);

        // Spawn new particles from emitters (simple deterministic spawning)
        for (emitter_idx, emitter) in self.emitters.iter().enumerate() {
            let spawn_count = (emitter.rate * dt as f32) as usize;

            for i in 0..spawn_count {
                if self.particles.len() >= config.max_particles {
                    break;
                }

                let seed = (self.particles.len() + emitter_idx + i) as f32;
                let angle = (seed * 0.618033) * std::f32::consts::TAU;
                let spread = emitter.spread * ((seed * 0.314159) % 1.0);

                let vx = emitter.velocity.0 + spread * angle.cos();
                let vy = emitter.velocity.1 + spread * angle.sin();

                let particle = Particle {
                    x: emitter.x,
                    y: emitter.y,
                    vx,
                    vy,
                    ax: 0.0,
                    ay: 0.0,
                    life: emitter.life,
                    size: 2.0 + ((seed * 0.271828) % 1.0) * 3.0,
                    color: [255, 255, 255, 255],
                };

                self.particles.push(particle);
            }
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ParticleSystemPanel {
    id: ParticleSystemId,
    title: String,
    config: ParticleSystemConfig,
}

impl ParticleSystemPanel {
    pub fn new(id: ParticleSystemId, title: String) -> Self {
        Self {
            id,
            title,
            config: ParticleSystemConfig::default(),
        }
    }

    pub fn id(&self) -> ParticleSystemId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "particle_system"
    }

    pub fn kind_label(&self) -> &'static str {
        "Particles"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (400.0, 300.0)
    }
}
