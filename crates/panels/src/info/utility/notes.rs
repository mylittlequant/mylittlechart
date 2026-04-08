use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct NotesId(pub u64);

/// Notes scope
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum NotesScope {
    Global,
    PerSymbol,
    PerStrategy,
    PerTag(String),
}

/// Configuration for notes panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NotesConfig {
    pub scope: NotesScope,
    pub auto_save_interval_secs: u64,
    pub markdown_enabled: bool,
    pub font_size: f32,
    pub max_notes: usize,
}

impl Default for NotesConfig {
    fn default() -> Self {
        Self {
            scope: NotesScope::PerSymbol,
            auto_save_interval_secs: 30,
            markdown_enabled: false,
            font_size: 14.0,
            max_notes: 1000,
        }
    }
}

/// Individual note
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Note {
    pub key: String,
    pub content: String,
    pub created_at: i64,
    pub updated_at: i64,
    pub tags: Vec<String>,
}

/// Notes state
#[derive(Clone, Debug, Default)]
pub struct NotesState {
    pub notes: HashMap<String, Note>,
    pub current_key: String,
    pub cursor_position: usize,
    pub scroll_offset: f32,
    pub dirty: bool,
    pub last_save_time: Option<i64>,
}

impl NotesState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get the currently active note
    pub fn current_note(&self) -> Option<&Note> {
        self.notes.get(&self.current_key)
    }

    /// Get visible notes for rendering (sorted by updated_at)
    pub fn visible_notes(&self) -> Vec<&Note> {
        let mut notes: Vec<&Note> = self.notes.values().collect();
        notes.sort_by(|a, b| b.updated_at.cmp(&a.updated_at));
        notes
    }

    /// Format a single note for display
    pub fn format_note(&self, note: &Note) -> (String, String, String) {
        let key = note.key.clone();
        let preview = note.content
            .lines()
            .next()
            .unwrap_or("")
            .chars()
            .take(50)
            .collect::<String>();
        let updated = format_timestamp(note.updated_at);
        (key, preview, updated)
    }

    /// Get list of notes with title and preview for sidebar (title, preview)
    pub fn note_list(&self) -> Vec<(&str, &str)> {
        let mut notes: Vec<_> = self.notes.iter()
            .map(|(key, note)| {
                let preview = note.content
                    .lines()
                    .next()
                    .unwrap_or("")
                    .chars()
                    .take(50)
                    .collect::<String>();
                (key.as_str(), preview)
            })
            .collect();

        // Sort by updated_at descending
        notes.sort_by(|a, b| {
            let a_note = self.notes.get(a.0).unwrap();
            let b_note = self.notes.get(b.0).unwrap();
            b_note.updated_at.cmp(&a_note.updated_at)
        });

        notes.into_iter()
            .map(|(key, _preview_owned)| {
                let note = self.notes.get(key).unwrap();
                (key, note.content.lines().next().unwrap_or(""))
            })
            .collect()
    }

    /// Get the visible text content to display
    pub fn visible_text(&self) -> &str {
        self.current_note()
            .map(|note| note.content.as_str())
            .unwrap_or("")
    }

    /// Get word count of current note
    pub fn word_count(&self) -> usize {
        self.visible_text()
            .split_whitespace()
            .count()
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotesPanel {
    id: NotesId,
    title: String,
}

impl NotesPanel {
    pub fn new(id: NotesId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> NotesId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "notes"
    }

    pub fn kind_label(&self) -> &'static str {
        "Notes"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (200.0, 200.0)
    }
}
