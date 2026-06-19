//! INTEGRATION tier: a single LiveUpdate arm flowing through the actual
//! `ChartApp::tick()` writer logic into bubble storage.
//!
//! Composes: ChartApp (without GPU) + a fake LiveUpdate mpsc sender +
//! BubbleStore + the tick() arm under test. NOT the full event loop;
//! NOT a real bridge.
//!
//! TODO: scaffold once the unit tier confirms the routing fix is correct.
//! The integration tier should catch arm-level regressions
//! (BarsLoaded / BackfillComplete / ScrollBarsLoaded / BarUpdate /
//! TradeUpdate / FootprintUpdate / RenkoBarUpdate / PnfBarUpdate /
//! KagiBarUpdate / ThreeLineBreakUpdate / CvdUpdate / TpoUpdate /
//! OiUpdate / FundingUpdate / DomUpdate / LiqUpdate) without needing the
//! full vello binary.

#[test]
#[ignore = "scaffold — to be implemented after the unit tier passes"]
fn bars_loaded_writes_to_every_attached_bubble() {
    // 1. Build ChartApp with 4 charts on same instrument.
    // 2. Send LiveUpdate::BarsLoaded over the bridge channel.
    // 3. Call ChartApp::tick() once.
    // 4. Assert every chart's bubble.bars == 300.
    unimplemented!()
}
