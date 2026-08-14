//! E2E proof: an Elliott wave count survives the APP'S OWN save -> load
//! round trip — not a bare `BubbleStore::restore_from_snapshots` call fed a
//! hand-built `Vec<DataBubbleSnapshot>` (that narrower proof already exists
//! in `mlc-backend`'s `drawing::manager::wave_link_restore_tests`, which
//! starts from snapshots already in hand and never touches disk).
//!
//! # The real save path
//! `ChartApp::collect_state_into_preset` (`mlc-app/src/input/mod.rs:8842`)
//! builds the in-RAM `ChartPreset` from the live `BubbleStore`/panel grid,
//! then `mlc_core::preset::storage::save_preset`
//! (`mlc-core/src/preset/storage.rs:138`) writes it as JSON — the exact
//! call the live shells make for every preset in `core.presets().values()`
//! (`mlc-app-shell-vello/src/main.rs:2241-2244`,
//! `mlc-app-shell-web/src/persistence.rs:188`). `mlc-app` itself never
//! calls the disk writer directly; a shell does, so this test calls it the
//! same way a shell would, on the preset `collect_state_into_preset` just
//! built.
//!
//! # The real load path
//! `mlc_core::preset::storage::load_preset`
//! (`mlc-core/src/preset/storage.rs:177`) reads the bytes back — called,
//! once per preset file on disk, by `mlc-app/src/user_manager/
//! bootstrap.rs::load_user_state_from_disk`, which `ChartApp::new` runs on
//! every boot (via `ChartPanelApp::load_user_state`,
//! `mlc-app/src/lib.rs:3186-3191`). A FRESH `ChartApp::new(..)` therefore
//! performs the real disk read the moment it boots — no separate call
//! needed from this test. `ChartApp::load_preset(id)` (public,
//! `mlc-app/src/lib.rs:4986`) then fires the production `LoadPreset` cold
//! path; `HeadlessChartApp::pump_once`'s `drain_pending_cold_apply()` runs
//! the real `ChartApp::apply_preset_cold`
//! (`mlc-app/src/persistence/hydration_impl.rs:26`), which calls
//! `BubbleStore::restore_from_snapshots` (`mlc-backend/src/bubble/
//! store.rs`) — the one function every restore path is required to go
//! through, and the one that rebuilds the derived guard cache
//! (`mlc-backend::bubble::wave_guards`).
//!
//! # Storage isolation
//! `mlc_core::storage::set_global_storage` (the documented test/embedder
//! override hook — see `mlc-vault/src/storage/mod.rs`) redirects the REAL
//! `FilesystemStorage` backend to a scratch temp dir before either
//! `ChartApp` in this test can touch disk, so this test never reads or
//! writes the owner's actual profile directory. Same production `Storage`
//! trait and `FilesystemStorage` impl the live binary uses — only the root
//! path differs.

use std::sync::Arc;

use mlc_app_test_harness::headless::HeadlessChartApp;
use mlc_core::drawing::primitives_v2::elliott::{ElliottImpulse, WaveLink, WaveSlot};
use mlc_core::storage::{FilesystemStorage, set_global_storage};

/// Redirect `mlc_core`'s global storage to an isolated scratch dir before
/// anything in this test binary can call `global_storage()` first. Safe
/// here specifically because this file is its own `[[test]]` binary (its
/// own process, per `Cargo.toml`) — the override is a one-shot `OnceLock`
/// and nothing else in this process can race it.
fn use_scratch_storage() -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "mlc_preset_wave_link_round_trip_{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let _ = std::fs::create_dir_all(&dir);
    set_global_storage(Arc::new(FilesystemStorage::new(dir.clone())));
    dir
}

#[test]
fn wave_count_survives_the_apps_own_save_load_round_trip() {
    let scratch = use_scratch_storage();

    // ---- Save side: a real ChartApp, a real drawn+subdivided count ------
    let mut saver = HeadlessChartApp::new("BTCUSDT");
    let chart_id = saver.active_chart_id().expect("boot seeds a default chart");
    let bid = saver.chart_bubble(chart_id).expect("default chart has a bubble attached");

    let parent = ElliottImpulse::new(
        [(0, 0.0), (1_000, 10.0), (2_000, 5.0), (3_000, 15.0), (4_000, 10.0), (5_000, 20.0)],
        "#ffffff",
    );
    let parent_id = saver.app.core.drawings_mut().add_external(bid, Box::new(parent));

    // The real gesture: `DrawingsMut::create_wave_child` — the same call
    // the subdivide-as picker makes (`create_wave_child_and_record` in
    // `mlc-app/src/input/modals.rs`), never a hand-stamped `WaveLink`.
    let child_id = saver
        .app
        .core
        .drawings_mut()
        .create_wave_child(bid, parent_id, WaveSlot::Impulse2, "elliott_correction:zigzag")
        .expect("wave 2 of a complete impulse accepts an elliott_correction:zigzag child");

    // A `follows` chain too: two SEPARATE top-level structures, not a
    // subdivision — built via the matching real gesture,
    // `DrawingsMut::create_wave_successor`.
    let head = ElliottImpulse::new(
        [(10_000, 20.0), (11_000, 30.0), (12_000, 25.0), (13_000, 35.0), (14_000, 30.0), (15_000, 40.0)],
        "#ffffff",
    );
    let head_id = saver.app.core.drawings_mut().add_external(bid, Box::new(head));
    let successor_id = saver
        .app
        .core
        .drawings_mut()
        .create_wave_successor(bid, head_id, "elliott_correction")
        .expect("a complete impulse accepts an elliott_correction successor");

    // Sanity BEFORE the round trip — the live gesture actually produced
    // what the rest of this test assumes.
    saver
        .app
        .with_bubble(bid, |b| {
            let parent = b
                .primitives
                .iter()
                .find(|p| p.data().id == parent_id)
                .expect("parent present pre-save");
            assert_eq!(
                parent.subdivided_slots(),
                &[WaveSlot::Impulse2],
                "live create_wave_child must register the subdivision in the parent's own \
                 guard cache immediately — create_wave_child calls refresh_wave_guards itself",
            );
            let child = b
                .primitives
                .iter()
                .find(|p| p.data().id == child_id)
                .expect("child present pre-save");
            assert_eq!(
                child.wave_link(),
                Some(WaveLink { parent_id, slot: WaveSlot::Impulse2 }),
                "live create_wave_child must stamp the child's own wave_link before any save",
            );
            let successor = b
                .primitives
                .iter()
                .find(|p| p.data().id == successor_id)
                .expect("successor present pre-save");
            assert_eq!(
                successor.wave_follows(),
                Some(head_id),
                "live create_wave_successor must stamp follows before any save",
            );
        })
        .expect("bubble `bid` exists pre-save");

    // ---- Save through the REAL writer ------------------------------------
    let preset_id = "wave_link_round_trip_test_preset".to_string();
    saver.app.collect_state_into_preset(&preset_id, "wave link round trip");
    let preset = saver
        .app
        .core
        .presets()
        .get(&preset_id)
        .cloned()
        .expect("collect_state_into_preset must have inserted the preset into core.presets()");
    mlc_core::preset::storage::save_preset(&preset)
        .expect("save_preset must write the preset to the (scratch-redirected) disk backend");

    // ---- Drop the whole in-memory state -----------------------------------
    drop(saver);

    // ---- Load side: a FRESH ChartApp, boots through the REAL disk read ---
    let mut loader = HeadlessChartApp::new("BTCUSDT");
    loader.resize(1280, 800);
    assert!(
        loader.app.core.presets().contains_key(&preset_id),
        "a fresh ChartApp::new boot must have found our saved preset file via its own \
         load_user_state_from_disk -> mlc_core::preset::storage::load_preset bootstrap — if \
         this fails, LOAD is reading a different byte source than SAVE wrote to",
    );

    loader.app.load_preset(&preset_id);
    loader.pump_once();

    // ---- Assert AFTER the round trip --------------------------------------
    let (parent_slots, child_link, successor_follows) = loader
        .app
        .with_bubble(bid, |b| {
            let parent = b.primitives.iter().find(|p| p.data().id == parent_id).unwrap_or_else(|| {
                panic!(
                    "parent primitive (persisted id={}) is MISSING from the restored bubble — \
                     ids were re-stamped across the round trip; every id-based assertion below \
                     is invalid until that re-stamping is accounted for",
                    parent_id,
                )
            });
            let child = b.primitives.iter().find(|p| p.data().id == child_id).unwrap_or_else(|| {
                panic!(
                    "child primitive (persisted id={}) is MISSING from the restored bubble — \
                     ids were re-stamped across the round trip",
                    child_id,
                )
            });
            let successor = b.primitives.iter().find(|p| p.data().id == successor_id).unwrap_or_else(|| {
                panic!(
                    "successor primitive (persisted id={}) is MISSING from the restored bubble \
                     — ids were re-stamped across the round trip",
                    successor_id,
                )
            });
            (
                parent.subdivided_slots().to_vec(),
                child.wave_link(),
                successor.wave_follows(),
            )
        })
        .unwrap_or_else(|| {
            panic!(
                "bubble bid={:?} itself is MISSING from the restored store — the BUBBLE id \
                 (not just a primitive id) was re-stamped across the round trip",
                bid,
            )
        });

    assert_eq!(
        child_link,
        Some(WaveLink { parent_id, slot: WaveSlot::Impulse2 }),
        "child's wave_link().parent_id must still resolve to parent_id={} after the FULL \
         save_preset -> disk -> ChartApp::new (load_preset) -> ChartApp::load_preset -> \
         apply_preset_cold round trip",
        parent_id,
    );

    assert_eq!(
        parent_slots,
        vec![WaveSlot::Impulse2],
        "LOAD-BEARING: parent's subdivided_slots() — #[serde(skip)] derived state, empty on a \
         bare deserialize — must be REBUILT by the app's own real cold-load path. The link \
         surviving (asserted above) is necessary but not sufficient: this is the assertion \
         that the derived-state fix actually engages when driven through save_preset / \
         load_preset / apply_preset_cold, not only through a hand-built snapshot list handed \
         straight to restore_from_snapshots (as mlc-backend's own wave_link_restore_tests do).",
    );

    assert_eq!(
        successor_follows,
        Some(head_id),
        "follows chain: successor's wave_follows() must still name head_id={} after the round \
         trip — two separate top-level structures joined by `follows`, not a subdivision",
        head_id,
    );

    let _ = std::fs::remove_dir_all(&scratch);
}
