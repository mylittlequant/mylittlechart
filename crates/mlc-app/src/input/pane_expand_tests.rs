//! ChartApp-level tests of the pane-expand rulings (docs/mlc/plans/
//! pane-expand-2026-09-25.md): double-click on the main pane collapses its
//! sub-panes and a second one restores; double-click on a sub-pane expands
//! it and a second one restores; a zero-sub-pane chart has nothing to
//! expand; an armed drawing tool wins the double-click; the state is
//! RAM-only.

use std::sync::Arc;

use digdigdig3::{AccountType, ExchangeId};
use mlc_backend_data_bridge::DataBridge;
use mlc_core::state::modal_data::ExpandedPane;
use mlc_core::state::{ChartId, Timeframe};

use crate::chart_container_leaf::ChartContainerLeaf;
use crate::ChartApp;

/// Returns the app, the active container and the instance ids of the
/// `sub_pane_count` RSI sub-panes attached to it.
async fn seeded_app(sub_pane_count: usize) -> (ChartApp, ChartId, Vec<u64>) {
    let (bridge, _live_rx, _ready_rx) = DataBridge::new().await;
    let core = mlc_backend::BackendCore::new(Arc::new(bridge), None);
    let mut app = ChartApp::new_empty(core);
    app.seed_default_chart(ExchangeId::Binance, "BTCUSDT", Timeframe::m1(), AccountType::Spot, 10);
    let cid = app.panel_app.panel_grid.active_container_id().expect("seeded active container");
    let bid = app.bubble_for_chart(cid).expect("seeded bubble");
    let ids: Vec<u64> = (0..sub_pane_count)
        .map(|_| app.core.indicators_mut().add(Some(bid), "rsi").expect("rsi indicator"))
        .collect();
    if let Some(window) = app.panel_app.panel_grid.active_container_mut() {
        for &instance_id in &ids {
            window.inner_docking.tree_mut().add_leaf(ChartContainerLeaf::SubPane { instance_id });
        }
        crate::chart_container::pin_vertical_stack_layout(&mut window.inner_docking);
    }
    app.active_frame_layout = Some(app.build_extended_layout());
    (app, cid, ids)
}

fn main_point(app: &ChartApp) -> (f64, f64) {
    let ext = app.active_frame_layout.as_ref().expect("layout cached");
    let r = ext.main_chart.chart;
    (r.x + r.width * 0.5, r.y + r.height * 0.5)
}

fn sub_point(app: &ChartApp) -> (f64, f64) {
    let ext = app.active_frame_layout.as_ref().expect("layout cached");
    let r = ext.sub_panes.first().expect("one sub-pane").content;
    (r.x + r.width * 0.5, r.y + r.height * 0.5)
}

/// Re-cache the layout the way a frame would, so the next double-click hits
/// the geometry the previous one produced.
fn refresh_layout(app: &mut ChartApp) {
    app.active_frame_layout = Some(app.build_extended_layout());
}

#[tokio::test]
async fn main_double_click_collapses_sub_panes_and_second_restores() {
    let (mut app, cid, _ids) = seeded_app(1).await;
    let (x, y) = main_point(&app);

    app.on_double_click(x, y);
    assert_eq!(app.expanded_pane.get(&cid.0).copied(), Some(ExpandedPane::Main));

    refresh_layout(&mut app);
    let ext = app.active_frame_layout.as_ref().expect("layout cached");
    assert_eq!(
        ext.sub_panes[0].content.height,
        mlc_core::types::PANE_HEADER_STRIP_HEIGHT,
        "sub-pane collapses to the header strip while main is expanded"
    );
    let (x, y) = main_point(&app);
    app.on_double_click(x, y);
    assert_eq!(app.expanded_pane.get(&cid.0), None, "second double-click restores");
}

#[tokio::test]
async fn sub_pane_double_click_expands_it_and_second_restores() {
    let (mut app, cid, ids) = seeded_app(1).await;
    let sub_iid = ids[0];
    let (x, y) = sub_point(&app);

    app.on_double_click(x, y);
    assert_eq!(app.expanded_pane.get(&cid.0).copied(), Some(ExpandedPane::SubPane(sub_iid)));

    refresh_layout(&mut app);
    let (x, y) = sub_point(&app);
    app.on_double_click(x, y);
    assert_eq!(app.expanded_pane.get(&cid.0), None, "second double-click restores");
}

#[tokio::test]
async fn main_toggle_is_a_no_op_without_sub_panes() {
    let (mut app, cid, _ids) = seeded_app(0).await;
    let (x, y) = main_point(&app);

    app.on_double_click(x, y);
    assert!(app.expanded_pane.is_empty(), "nothing to collapse on a chart without sub-panes");
    app.toggle_main_pane_expand(cid);
    assert!(app.expanded_pane.is_empty());
}

#[tokio::test]
async fn armed_drawing_tool_wins_the_double_click() {
    let (mut app, cid, _ids) = seeded_app(1).await;
    let bid = app.bubble_for_chart(cid).expect("seeded bubble");
    app.core.drawings_mut().set_tool(bid, Some("trend_line"));
    let (x, y) = main_point(&app);

    app.on_double_click(x, y);
    assert!(app.expanded_pane.is_empty(), "drawing tool armed: pane expand must not fire");
}

#[tokio::test]
async fn price_scale_double_click_does_not_expand() {
    let (mut app, _cid, _ids) = seeded_app(1).await;
    let ext = app.active_frame_layout.as_ref().expect("layout cached");
    let r = ext.main_chart.price_scale;
    let (x, y) = (r.x + r.width * 0.5, r.y + r.height * 0.5);

    app.on_double_click(x, y);
    assert!(app.expanded_pane.is_empty(), "price-scale reset keeps its own double-click");
}

#[tokio::test]
async fn expand_state_is_ram_only() {
    let (mut app, cid, ids) = seeded_app(1).await;
    let sub_iid = ids[0];
    app.panel_app.autosave_enabled = true;
    app.panel_app.active_preset_id = "p".to_string();
    app.pending_autosave = None;

    app.toggle_main_pane_expand(cid);
    assert_eq!(app.expanded_pane.get(&cid.0).copied(), Some(ExpandedPane::Main));
    app.toggle_sub_pane_expand(cid, sub_iid);
    assert_eq!(app.expanded_pane.get(&cid.0).copied(), Some(ExpandedPane::SubPane(sub_iid)));
    assert!(app.pending_autosave.is_none(), "expand state must never schedule a preset write");
}
