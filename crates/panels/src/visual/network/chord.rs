use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChordId(pub u64);

#[derive(Clone, Debug)]
pub struct ChordGroup {
    pub index: usize,
    pub label: String,
    pub start_angle: f64,
    pub end_angle: f64,
    pub value: f64,
    pub color: [u8; 4],
}

#[derive(Clone, Debug)]
pub struct ChordRibbon {
    pub source_index: usize,
    pub target_index: usize,
    pub source_start_angle: f64,
    pub source_end_angle: f64,
    pub target_start_angle: f64,
    pub target_end_angle: f64,
    pub value: f64,
    pub color: [u8; 4],
}

#[derive(Clone, Debug, Default)]
pub struct ChordState {
    pub groups: Vec<ChordGroup>,
    pub ribbons: Vec<ChordRibbon>,
    pub matrix: Vec<Vec<f64>>,
    pub radius: f32,
    pub inner_radius: f32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChordConfig {
    pub pad_angle: f64,
    pub sort_groups: Option<ChordSort>,
    pub sort_subgroups: Option<ChordSort>,
    pub sort_chords: Option<ChordSort>,
}

#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub enum ChordSort {
    Ascending,
    Descending,
    None,
}

impl Default for ChordConfig {
    fn default() -> Self {
        Self {
            pad_angle: 0.0,
            sort_groups: None,
            sort_subgroups: None,
            sort_chords: None,
        }
    }
}

impl ChordState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Returns arcs as (start_angle, end_angle, color, label)
    pub fn arcs(&self, _cx: f32, _cy: f32, _r: f32) -> Vec<(f64, f64, [f32; 4], &str)> {
        self.groups
            .iter()
            .map(|group| {
                let color = [
                    group.color[0] as f32 / 255.0,
                    group.color[1] as f32 / 255.0,
                    group.color[2] as f32 / 255.0,
                    group.color[3] as f32 / 255.0,
                ];
                (group.start_angle, group.end_angle, color, group.label.as_str())
            })
            .collect()
    }

    /// Returns ribbons connecting groups
    pub fn ribbons(&self, _cx: f32, _cy: f32, _r: f32) -> Vec<ChordRibbon> {
        self.ribbons.clone()
    }

    // Render helpers
    pub fn visible_arcs(&self) -> &[ChordGroup] {
        &self.groups
    }

    pub fn visible_chords(&self) -> &[ChordRibbon] {
        &self.ribbons
    }

    pub fn arc_color(&self, arc_idx: usize) -> Option<[f32; 4]> {
        let arc = self.groups.get(arc_idx)?;
        Some([
            arc.color[0] as f32 / 255.0,
            arc.color[1] as f32 / 255.0,
            arc.color[2] as f32 / 255.0,
            arc.color[3] as f32 / 255.0,
        ])
    }

    pub fn chord_opacity(&self, chord_idx: usize) -> f32 {
        if chord_idx < self.ribbons.len() {
            0.6
        } else {
            0.3
        }
    }

    pub fn format_flow(&self, value: f64) -> String {
        if value >= 1_000_000.0 {
            format!("{:.2}M", value / 1_000_000.0)
        } else if value >= 1_000.0 {
            format!("{:.1}K", value / 1_000.0)
        } else {
            format!("{:.0}", value)
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ChordPanel {
    id: ChordId,
    title: String,
    config: ChordConfig,
}

impl ChordPanel {
    pub fn new(id: ChordId, title: String) -> Self {
        Self {
            id,
            title,
            config: ChordConfig::default(),
        }
    }

    pub fn id(&self) -> ChordId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "chord"
    }

    pub fn kind_label(&self) -> &'static str {
        "Chord Diagram"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (300.0, 300.0)
    }
}
