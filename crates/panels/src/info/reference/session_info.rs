use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SessionInfoId(pub u64);

#[derive(Clone, Debug)]
pub struct SessionInfoState {
    /// Market sessions
    pub sessions: Vec<MarketSession>,
    /// Current time
    pub current_time: i64,
}

#[derive(Clone, Debug)]
pub struct MarketSession {
    pub market_name: String,
    pub exchange: String,
    pub status: SessionStatus,
    pub open_time: Option<String>,
    pub close_time: Option<String>,
    pub next_open: Option<i64>,
    pub timezone: String,
    pub is_holiday: bool,
}

#[derive(Clone, Debug)]
pub enum SessionStatus {
    Open,
    Closed,
    PreMarket,
    AfterHours,
    Break,
    Holiday,
}

impl SessionInfoState {
    pub fn new() -> Self {
        Self {
            sessions: Vec::new(),
            current_time: 0,
        }
    }

    /// Get visible sessions for rendering
    pub fn visible_sessions(&self) -> &[MarketSession] {
        &self.sessions
    }

    /// Format session for display
    pub fn format_session(&self, session: &MarketSession) -> (String, String, String, String) {
        let name = session.market_name.clone();
        let exchange = session.exchange.clone();
        let status = self.format_status(&session.status);
        let time = if let (Some(ref open), Some(ref close)) = (&session.open_time, &session.close_time) {
            format!("{} - {}", open, close)
        } else {
            "—".to_string()
        };
        (name, exchange, status, time)
    }

    fn format_status(&self, status: &SessionStatus) -> String {
        match status {
            SessionStatus::Open => "Open".to_string(),
            SessionStatus::Closed => "Closed".to_string(),
            SessionStatus::PreMarket => "Pre-Market".to_string(),
            SessionStatus::AfterHours => "After Hours".to_string(),
            SessionStatus::Break => "Break".to_string(),
            SessionStatus::Holiday => "Holiday".to_string(),
        }
    }

    /// Get color based on session status
    pub fn status_color(&self, session: &MarketSession) -> [f32; 4] {
        if session.is_holiday {
            return [0.9, 0.5, 0.2, 1.0]; // orange - holiday
        }

        match session.status {
            SessionStatus::Open => [0.2, 0.8, 0.3, 1.0],        // green
            SessionStatus::Closed => [0.5, 0.5, 0.5, 1.0],      // gray
            SessionStatus::PreMarket => [0.9, 0.7, 0.2, 1.0],   // yellow
            SessionStatus::AfterHours => [0.9, 0.5, 0.2, 1.0],  // orange
            SessionStatus::Break => [0.7, 0.7, 0.7, 1.0],       // light gray
            SessionStatus::Holiday => [0.9, 0.5, 0.2, 1.0],     // orange
        }
    }

    /// Calculate countdown to next event (in seconds)
    pub fn countdown_to_event(&self, session: &MarketSession) -> Option<i64> {
        session.next_open.map(|next| next - self.current_time)
    }

    /// Format countdown as HH:MM:SS
    pub fn format_countdown(&self, seconds: i64) -> String {
        if seconds < 0 {
            return "—".to_string();
        }
        let h = seconds / 3600;
        let m = (seconds % 3600) / 60;
        let s = seconds % 60;
        format!("{:02}:{:02}:{:02}", h, m, s)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInfoConfig {
    /// Markets to track
    pub tracked_markets: Vec<String>,
    /// Show countdown to open/close
    pub show_countdown: bool,
    /// Timezone for display
    pub display_timezone: String,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SessionInfoPanel {
    id: SessionInfoId,
    title: String,
}

impl SessionInfoPanel {
    pub fn new(id: SessionInfoId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> SessionInfoId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "session_info"
    }

    pub fn kind_label(&self) -> &'static str {
        "Session Info"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (200.0, 150.0)
    }
}
