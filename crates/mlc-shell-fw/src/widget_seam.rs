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

/// A table-preset descriptor for a trading panel (WDC plugin: mlc owns the data
/// and feeds rows per the contract; consumers render via `domkit::preset("table")`).
fn table_desc(instance: String, title: &str, columns: &[&str]) -> WidgetDescriptor {
    WidgetDescriptor {
        wdc: 1,
        instance: instance.clone(),
        preset: "table".into(),
        preset_version: 1,
        title: title.into(),
        data: json!({ "columns": columns, "rows": [] }),
        options: json!({}),
        actions: vec!["select-row".into()],
        source: src(&format!("widget/{instance}")),
    }
}

impl WidgetBackend for MlcWidgets<'_> {
    fn list(&self) -> Vec<WidgetDescriptor> {
        let symbol = "BTCUSDT".to_string();
        let mut out = vec![
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
        ];
        // Complex systems (trading panels) as contract widgets: the plugin (mlc)
        // owns the data and feeds it per the WDC; any consumer (HQ) renders them
        // via domkit::preset without compiling mlc in. Rows are filled by get().
        let ps = &self.chart.panels_store;
        for (id, _) in ps.trade_tape.iter() {
            out.push(table_desc(format!("mlc:trade_tape:{}", id.0), "Trade tape", &["Time", "Price", "Size"]));
        }
        for (id, _) in ps.l2_tape.iter() {
            out.push(table_desc(format!("mlc:l2:{}", id.0), "L2 tape", &["Time", "Type", "Side", "Price", "Qty"]));
        }
        for (id, _) in ps.big_trades.iter() {
            out.push(table_desc(format!("mlc:big_trades:{}", id.0), "Big trades", &["Time", "Side", "Price", "Size", "Notional"]));
        }
        for (id, _) in ps.liq_tape.iter() {
            out.push(table_desc(format!("mlc:liq:{}", id.0), "Liquidations", &["Time", "Side", "Price", "Size"]));
        }
        for (id, _) in ps.dom.iter() {
            out.push(table_desc(format!("mlc:dom:{}", id.0), "DOM ladder", &["Bid", "Sell", "Buy", "Ask"]));
        }
        out
    }

    fn get(&self, instance: &str) -> Option<WidgetDescriptor> {
        self.list().into_iter().find(|d| d.instance == instance)
    }

    fn execute(&self, instance: &str, verb: &str, payload: serde_json::Value) -> ExecResult {
        // Per-verb: only verbs the descriptor promises are honoured; the seam acks
        // and returns fresh fill data. State mutation itself runs in the shell
        // dispatcher (the backend's own credential), never in HQ.
        let d = match self.get(instance) {
            Some(d) => d,
            None => return ExecResult { ack: false, data: None },
        };
        if !d.actions.iter().any(|a| a == verb) {
            return ExecResult { ack: false, data: None };
        }
        let data = match (d.preset.as_str(), verb) {
            ("list", "select") => {
                let sel = payload.get("index").and_then(|v| v.as_u64()).unwrap_or(0);
                json!({ "items": d.data.get("items").cloned().unwrap_or(json!([])), "selected": sel })
            }
            ("chart.candles", "range") | ("chart.candles", "crosshair") => d.data.clone(),
            _ => d.data.clone(),
        };
        ExecResult { ack: true, data: Some(data) }
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
        // Per-verb execute: promised verbs ack with fresh fill; unknown verbs and
        // unknown instances are rejected (HQ only forwards what the backend promises).
        let sel = w.execute("mlc:watchlist", "select", json!({ "index": 2 }));
        assert!(sel.ack);
        assert_eq!(sel.data.unwrap()["selected"], json!(2));
        assert!(w.execute("mlc:BTCUSDT:1h", "range", json!({})).ack);
        assert!(!w.execute("mlc:status", "noop", json!({})).ack, "status promises no verbs");
        assert!(!w.execute("mlc:watchlist", "range", json!({})).ack, "verb not promised by list");
        assert!(!w.execute("mlc:nope", "select", json!({})).ack, "unknown instance");
    }
}
