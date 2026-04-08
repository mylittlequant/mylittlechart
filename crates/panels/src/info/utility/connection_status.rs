use serde::{Serialize, Deserialize};
use std::collections::HashMap;

#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionStatusId(pub u64);

/// Connection type
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConnectionType {
    RestAPI,
    WebSocket,
    WebSocketPrivate,
}

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ConnectionState {
    Connected,
    Connecting,
    Disconnected,
    Reconnecting,
    Error,
}

/// Configuration for connection status panel
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatusConfig {
    pub update_interval_ms: u64,
    pub show_disconnected: bool,
    pub alert_on_disconnect: bool,
    pub latency_warning_ms: u64,
    pub latency_critical_ms: u64,
}

impl Default for ConnectionStatusConfig {
    fn default() -> Self {
        Self {
            update_interval_ms: 1000,
            show_disconnected: true,
            alert_on_disconnect: true,
            latency_warning_ms: 500,
            latency_critical_ms: 1000,
        }
    }
}

/// Connection status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionStatus {
    pub exchange_id: String,
    pub connection_type: ConnectionType,
    pub status: ConnectionState,
    pub latency_ms: Option<u64>,
    pub last_message_time: i64,
    pub reconnect_count: u32,
    pub messages_per_second: f64,
    pub uptime_secs: u64,
    pub error_count: u32,
    pub last_error: Option<String>,
}

/// Global connection status
#[derive(Debug, Clone, Default)]
pub struct GlobalConnectionStatus {
    pub total_connections: usize,
    pub connected: usize,
    pub disconnected: usize,
    pub reconnecting: usize,
    pub average_latency_ms: f64,
    pub total_messages_per_second: f64,
}

/// Row for rendering connection status
#[derive(Debug, Clone)]
pub struct ConnectionRow {
    pub name: String,
    pub status: String,
    pub latency: String,
    pub uptime: String,
    pub color: [f32; 4],
}

/// Connection status state
#[derive(Clone, Debug, Default)]
pub struct ConnectionStatusState {
    pub connections: HashMap<String, ConnectionStatus>,
    pub last_update: i64,
    pub global_status: GlobalConnectionStatus,
}

impl ConnectionStatusState {
    pub fn new() -> Self {
        Self::default()
    }

    /// Get visible connections for rendering (all connections)
    pub fn visible_connections(&self) -> Vec<&ConnectionStatus> {
        let mut conns: Vec<&ConnectionStatus> = self.connections.values().collect();
        conns.sort_by_key(|c| &c.exchange_id);
        conns
    }

    /// Format a single connection for display
    pub fn format_connection(&self, conn: &ConnectionStatus) -> (String, String, String, String) {
        let name = format!("{} ({})", conn.exchange_id, Self::format_connection_type(conn.connection_type));

        let status = match conn.status {
            ConnectionState::Connected => "Connected",
            ConnectionState::Connecting => "Connecting...",
            ConnectionState::Disconnected => "Disconnected",
            ConnectionState::Reconnecting => "Reconnecting...",
            ConnectionState::Error => "Error",
        }.to_string();

        let latency = conn.latency_ms
            .map(|lat| format!("{} ms", lat))
            .unwrap_or_else(|| "N/A".to_string());

        let uptime = Self::format_uptime(conn.uptime_secs);

        (name, status, latency, uptime)
    }

    /// Get formatted connection rows for rendering
    pub fn connection_rows(&self) -> Vec<ConnectionRow> {
        let mut rows: Vec<ConnectionRow> = self.connections.iter()
            .map(|(_key, conn)| {
                let status_str = match conn.status {
                    ConnectionState::Connected => "Connected",
                    ConnectionState::Connecting => "Connecting...",
                    ConnectionState::Disconnected => "Disconnected",
                    ConnectionState::Reconnecting => "Reconnecting...",
                    ConnectionState::Error => "Error",
                };

                let latency_str = conn.latency_ms
                    .map(|lat| format!("{} ms", lat))
                    .unwrap_or_else(|| "N/A".to_string());

                let uptime_str = Self::format_uptime(conn.uptime_secs);

                let is_connected = conn.status == ConnectionState::Connected;
                let latency = conn.latency_ms.map(|l| l as f64).unwrap_or(0.0);
                let color = Self::status_color(is_connected, latency);

                ConnectionRow {
                    name: format!("{} ({})", conn.exchange_id, Self::format_connection_type(conn.connection_type)),
                    status: status_str.to_string(),
                    latency: latency_str,
                    uptime: uptime_str,
                    color,
                }
            })
            .collect();

        // Sort by name
        rows.sort_by(|a, b| a.name.cmp(&b.name));
        rows
    }

    fn format_connection_type(conn_type: ConnectionType) -> &'static str {
        match conn_type {
            ConnectionType::RestAPI => "REST",
            ConnectionType::WebSocket => "WS",
            ConnectionType::WebSocketPrivate => "WS-Private",
        }
    }

    fn format_uptime(secs: u64) -> String {
        let hours = secs / 3600;
        let mins = (secs % 3600) / 60;
        let secs = secs % 60;

        if hours > 0 {
            format!("{}h {}m", hours, mins)
        } else if mins > 0 {
            format!("{}m {}s", mins, secs)
        } else {
            format!("{}s", secs)
        }
    }

    /// Get status color based on connection state and latency (RGBA)
    pub fn status_color(connected: bool, latency: f64) -> [f32; 4] {
        if !connected {
            [0.9, 0.3, 0.3, 1.0] // red - disconnected
        } else if latency >= 1000.0 {
            [0.9, 0.3, 0.3, 1.0] // red - critical latency
        } else if latency >= 500.0 {
            [0.9, 0.8, 0.2, 1.0] // yellow - warning latency
        } else {
            [0.2, 0.8, 0.2, 1.0] // green - healthy
        }
    }

    /// Get overall system health (label, color)
    pub fn overall_health(&self) -> (&str, [f32; 4]) {
        let total = self.global_status.total_connections;
        if total == 0 {
            return ("No Connections", [0.6, 0.6, 0.6, 1.0]); // gray
        }

        let connected_ratio = self.global_status.connected as f64 / total as f64;
        let avg_latency = self.global_status.average_latency_ms;

        if connected_ratio == 1.0 && avg_latency < 500.0 {
            ("Healthy", [0.2, 0.8, 0.2, 1.0]) // green
        } else if connected_ratio >= 0.7 {
            ("Degraded", [0.9, 0.8, 0.2, 1.0]) // yellow
        } else {
            ("Down", [0.9, 0.3, 0.3, 1.0]) // red
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ConnectionStatusPanel {
    id: ConnectionStatusId,
    title: String,
}

impl ConnectionStatusPanel {
    pub fn new(id: ConnectionStatusId, title: String) -> Self {
        Self { id, title }
    }

    pub fn id(&self) -> ConnectionStatusId {
        self.id
    }

    pub fn title(&self) -> &str {
        &self.title
    }

    pub fn set_title(&mut self, title: String) {
        self.title = title;
    }

    pub fn type_id(&self) -> &'static str {
        "connection_status"
    }

    pub fn kind_label(&self) -> &'static str {
        "Connection"
    }

    pub fn min_size(&self) -> (f32, f32) {
        (200.0, 100.0)
    }
}
