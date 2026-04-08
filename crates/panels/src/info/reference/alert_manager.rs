use serde::{Serialize, Deserialize};

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct AlertManagerId(pub u64);

#[derive(Clone, Debug)]
pub struct AlertManagerState {
    /// All alerts
    pub alerts: Vec<Alert>,
    /// Status filter
    pub status_filter: Option<AlertStatus>,
    /// Sort configuration
    pub sort: (AlertColumn, bool),
}

#[derive(Clone, Debug)]
pub struct Alert {
    pub id: String,
    pub symbol: String,
    pub condition: AlertCondition,
    pub status: AlertStatus,
    pub created_at: i64,
    pub triggered_at: Option<i64>,
    pub expiry: Option<i64>,
    pub notification: NotificationConfig,
}

#[derive(Clone, Debug)]
pub enum AlertCondition {
    PriceAbove(f64),
    PriceBelow(f64),
    PriceCrossUp(f64),
    PriceCrossDown(f64),
    VolumeAbove(f64),
    PercentChange(f64),
    Custom(String),
}

#[derive(Clone, Debug)]
pub enum AlertStatus {
    Active,
    Triggered,
    Expired,
    Disabled,
}

#[derive(Clone, Debug)]
pub struct NotificationConfig {
    pub sound: bool,
    pub popup: bool,
    pub email: Option<String>,
}

#[derive(Clone, Debug, Copy)]
pub enum AlertColumn {
    Symbol,
    Condition,
    Status,
    Created,
    Triggered,
}

impl AlertManagerState {
    pub fn new() -> Self {
        Self {
            alerts: Vec::new(),
            status_filter: None,
            sort: (AlertColumn::Created, false),
        }
    }

    /// Get visible alerts for rendering
    pub fn visible_alerts(&self, scroll_offset: usize, max_rows: usize) -> &[Alert] {
        let end = (scroll_offset + max_rows).min(self.alerts.len());
        &self.alerts[scroll_offset..end]
    }

    /// Format alert for display
    pub fn format_alert(&self, alert: &Alert) -> (String, String, String, String) {
        let symbol = alert.symbol.clone();
        let condition = self.format_condition(&alert.condition);
        let status = format!("{:?}", alert.status);
        let created = format_timestamp(alert.created_at);
        (symbol, condition, status, created)
    }

    fn format_condition(&self, condition: &AlertCondition) -> String {
        match condition {
            AlertCondition::PriceAbove(p) => format!("Price > {:.2}", p),
            AlertCondition::PriceBelow(p) => format!("Price < {:.2}", p),
            AlertCondition::PriceCrossUp(p) => format!("Cross Up {:.2}", p),
            AlertCondition::PriceCrossDown(p) => format!("Cross Down {:.2}", p),
            AlertCondition::VolumeAbove(v) => format!("Volume > {:.0}", v),
            AlertCondition::PercentChange(pct) => format!("Change {:.1}%", pct),
            AlertCondition::Custom(s) => s.clone(),
        }
    }

    /// Get color based on alert status
    pub fn status_color(&self, alert: &Alert) -> [f32; 4] {
        match alert.status {
            AlertStatus::Active => [0.3, 0.6, 0.9, 1.0],     // blue
            AlertStatus::Triggered => [0.2, 0.8, 0.3, 1.0],  // green
            AlertStatus::Expired => [0.5, 0.5, 0.5, 1.0],    // gray
            AlertStatus::Disabled => [0.7, 0.7, 0.7, 0.5],   // faded gray
        }
    }
}

fn format_timestamp(ts: i64) -> String {
    let secs = (ts / 1000) % 86400;
    let h = secs / 3600;
    let m = (secs % 3600) / 60;
    format!("{:02}:{:02}", h, m)
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertManagerConfig {
    /// Max active alerts
    pub max_alerts: usize,
    /// Auto-disable after trigger
    pub auto_disable: bool,
    /// Default notification method
    pub default_notification: NotificationConfigSerde,
    /// Alert persistence
    pub persist_alerts: bool,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotificationConfigSerde {
    pub sound: bool,
    pub popup: bool,
    pub email: Option<String>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AlertManagerPanel {
    id: AlertManagerId,
    title: String,
}

impl AlertManagerPanel {
    pub fn new(id: AlertManagerId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> AlertManagerId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "alert_manager"
    }

    pub fn kind_label(&self) -> &'static str {
        "Alerts"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (250.0, 200.0)
    }
}
