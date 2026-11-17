//! The mlc side of the widget data contract: a [`WidgetBackend`] the hatchery HQ
//! panel talks to over the plugin seam. HQ renders these descriptors at runtime
//! via `uzor::ui::domkit::preset`; execution verbs come back here and run with
//! mlc's own state/credential — HQ never links mlc at compile time.

use hatchery_hq_wire::widget::{ExecResult, SeamSource, WidgetBackend, WidgetDescriptor};
use serde_json::json;

/// Borrowed view of a live [`mlc_app::ChartApp`] as a widget backend.
pub struct MlcWidgets<'a> {
    pub chart: &'a mlc_app::ChartApp,
}

fn src(endpoint: &str) -> SeamSource {
    SeamSource { backend: "mlc".into(), endpoint: endpoint.into() }
}

impl WidgetBackend for MlcWidgets<'_> {
    fn list(&self) -> Vec<WidgetDescriptor> {
        let symbol = "BTCUSDT".to_string();
        vec![
            WidgetDescriptor {
                wdc: 1,
                instance: format!("mlc:{symbol}:1h"),
                preset: "chart.candles".into(),
                preset_version: 1,
                title: format!("{symbol} · 1H"),
                data: json!({ "symbol": symbol, "timeframe": "1h", "series": [] }),
                options: json!({ "theme": "dark" }),
                actions: vec!["range".into(), "crosshair".into()],
                source: src(&format!("widget/{symbol}-1h")),
            },
            WidgetDescriptor {
                wdc: 1,
                instance: "mlc:watchlist".into(),
                preset: "list".into(),
                preset_version: 1,
                title: "Watchlist".into(),
                data: json!({ "items": [], "selected": 0 }),
                options: json!({}),
                actions: vec!["select".into()],
                source: src("widget/watchlist"),
            },
            WidgetDescriptor {
                wdc: 1,
                instance: "mlc:status".into(),
                preset: "kv".into(),
                preset_version: 1,
                title: "Status".into(),
                data: json!({ "pairs": [ { "label": "backend", "value": "mlc" } ] }),
                options: json!({}),
                actions: vec![],
                source: src("widget/status"),
            },
        ]
    }

    fn get(&self, instance: &str) -> Option<WidgetDescriptor> {
        self.list().into_iter().find(|d| d.instance == instance)
    }

    fn execute(&self, _instance: &str, _verb: &str, _payload: serde_json::Value) -> ExecResult {
        // Verbs are applied to mlc's own state by the dispatcher; the seam acks
        // and may return fresh fill data. Placeholder ack until wired per-verb.
        ExecResult { ack: true, data: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mlc_widgets_descriptors_render_via_preset() {
        let app = mlc_app::ChartApp::new("BTCUSDT");
        let w = MlcWidgets { chart: &app };
        let ds = w.list();
        assert!(!ds.is_empty());
        for d in &ds {
            assert_eq!(d.wdc, 1);
            assert!(uzor::ui::domkit::preset(&d.preset, &d.data).is_some(), "preset {} renders", d.preset);
            assert!(w.get(&d.instance).is_some());
        }
        assert!(w.execute("mlc:status", "noop", json!({})).ack);
    }
}
