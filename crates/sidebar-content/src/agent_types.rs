//! Agent rendering types shared between sidebar-content and chart-app.
//!
//! These types are owned by `sidebar-content` because the rendering crate needs
//! them to render the Agents panel, and `chart-app` depends on `sidebar-content`.

/// A single terminal cell with character and colors.
#[derive(Clone, Debug)]
pub struct TermCell {
    pub ch: String,
    pub fg: [u8; 3], // RGB
    pub bg: [u8; 3], // RGB
    pub bold: bool,
}

impl Default for TermCell {
    fn default() -> Self {
        Self {
            ch: " ".to_string(),
            fg: [204, 204, 204], // light gray
            bg: [0, 0, 0],       // black
            bold: false,
        }
    }
}

/// Terminal grid — rows x cols of cells.
#[derive(Clone, Debug)]
pub struct TermGrid {
    pub cells: Vec<Vec<TermCell>>,
    pub cols: u16,
    pub rows: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
}

impl TermGrid {
    pub fn empty(cols: u16, rows: u16) -> Self {
        Self {
            cells: vec![vec![TermCell::default(); cols as usize]; rows as usize],
            cols,
            rows,
            cursor_row: 0,
            cursor_col: 0,
        }
    }
}

/// Chat message role.
#[derive(Clone, Debug, PartialEq)]
pub enum ChatRole {
    User,
    Assistant,
    Tool,
    Thinking,
    Error,
}

/// A single chat message.
#[derive(Clone, Debug)]
pub struct ChatMessage {
    pub role: ChatRole,
    pub content: String,
    pub tool_name: Option<String>,
}

/// The rendering mode of the agent panel.
#[derive(Clone, Debug)]
pub enum AgentSnapshotMode {
    Pty(TermGrid),
    Chat(Vec<ChatMessage>),
    Idle,
}

/// Snapshot of agent state for rendering — no OS handles.
#[derive(Clone, Debug)]
pub struct AgentRenderSnapshot {
    pub mode: AgentSnapshotMode,
    pub session_active: bool,
}
