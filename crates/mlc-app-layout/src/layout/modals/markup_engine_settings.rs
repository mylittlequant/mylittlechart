//! Price Action Engine Settings modal — the non-Elliott sibling of
//! [`super::ewa_engine_settings`].
//!
//! # One modal, three engines
//!
//! Smart money, order flow and candle patterns each have a process-global
//! engine config, each already persisted in the profile, and until now none
//! of them was reachable from anywhere: the values were read on every
//! detection and editable from nothing.
//!
//! This is ONE modal with a family segment at the top rather than three
//! sibling files. The three configs differ in their rows, not in their
//! shape, and the plan this comes from
//! (`docs/mlc/plans/markup-engine-parity-2026-08-25.md`, §4) says the
//! parity being built is parity of MACHINERY, not of ceremony. Three files
//! would have meant three copies of the shell, the drag handling, the
//! result plumbing and the click routing, to end at the same place: the
//! entrance sets the family from the subject that opened it, so opening
//! from an order block lands on Smart Money.
//!
//! # Four tabs (A0 autodetectors, piece 8, `docs/mlc/plans/
//! autodetectors-a0-implementation-2026-09-24.md`)
//!
//! **What to detect** / **Thresholds** / **Style** / **Coverage** — reusing
//! the generic `ModalTabs::Horizontal` widget
//! (`crate::ui::widgets::modal`), not a new tab-strip component.
//!
//! - **What to detect**: whether an engine judges claims at all
//!   (`judge_claims`) and which cross-primitive rule classes it reports
//!   (`guard_families`) — global, per family, unchanged content from the
//!   modal's pre-tab body, just filed under this tab now.
//! - **Thresholds**: every numeric detector knob — the snap/chain-window
//!   pair every family already carried (moved here from the old
//!   "Detection" group) plus the family's own detector constants
//!   (`detector_rows`, 6 for Smart Money / piece 9, 2 for Order Flow /
//!   piece 10, 0 for candles — all global, all through the SAME
//!   `SmcEngineConfig`/`OrderFlowEngineConfig` this modal already writes,
//!   no new engine-config plumbing needed for either family).
//! - **Style**: no cosmetic (colour/line) setting exists on any of the
//!   three engines today — legitimately thin in A0; nothing invented to
//!   fill it.
//! - **Coverage**: PER-DRAWING, not global — the scan primitive that
//!   opened this modal (`state.source_drawing`) has its own `coverage`
//!   ("range"/"all_bars") and `shape` ("two_lines"/"rectangle") fields, a
//!   placement choice rather than a detector constant. Edits here apply
//!   through the SAME generic `apply_property` path
//!   `primitive_settings`'s own Pills rows already use for these two ids
//!   on that primitive — see `mlc_core::drawing::primitives_v2::scan_shape`
//!   and each scan's own `style_properties()` for the row these mirror.
//!
//! # What it edits
//!
//! Every edit applies immediately — writes through to the live global (or,
//! on the Coverage tab, the live drawing) and the profile, the same no-diff
//! shape the EWA modal uses and for the same reason (these configs have no
//! separate pending copy, so there is nothing to accept or discard). The
//! footer's ✕/OK both simply close.
//!
//! # Row selection
//!
//! Deliberately not every field. `default_enabled_kinds`/`enabled_structures`
//! (the per-kind toggle tables) are per-drawing pickers that already have a
//! home on each scan's own settings surface; what is here is the family-wide
//! knobs an analyst changes while marking up.

use mlc_core::i18n::{current_language, ConceptKey, t_concept};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use mlc_core::drawing::primitives_v2::ScanShape;
use uzor::types::Rect as WidgetRect;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::MarkupEngineSettingsResult;
use crate::ui::modal_settings::{MarkupEngineSettingsState, MarkupSettingsFamily, SettingsTab};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    render_modal_shell, ModalFooter, ModalShellSpec, ModalTab, ModalTabs, ModalTheme, TabFit,
};

use super::primitive_settings_vid::{draw_group_header, draw_label, draw_toggle_segment, ROW_H};

/// Matches `ewa_engine_settings`'s own documented duplicate of
/// `primitive_settings_vid::GROUP_HEADER_H`, which is not `pub`.
const GROUP_HEADER_H: f64 = 22.0;
const GROUP_GAP: f64 = 12.0;
const ROW_GAP: f64 = 6.0;
const SEGMENT_GAP: f64 = 4.0;

const MODAL_W: f64 = 460.0;
const LABEL_COL: f64 = 150.0;

/// The snap radius options, in bars. `0` is a real choice — it disables
/// snapping entirely, which `SmcEngineConfig::claim_snap_bars` documents as
/// its own off value.
const SNAP_BAR_OPTIONS: [usize; 4] = [0, 1, 3, 5];

/// The chain window options, in bars. `0` disables chaining, which is what
/// `chain_window_bars` reads as everywhere it is used.
const CHAIN_WINDOW_OPTIONS: [usize; 4] = [0, 10, 30, 60];

/// Order flow's own single-print run threshold — how many adjacent
/// single-print levels make a run worth marking.
const SINGLE_PRINT_RUN_OPTIONS: [usize; 4] = [1, 2, 3, 5];

/// `SmcEngineConfig::swing_n` options (A0 piece 9).
const SWING_N_OPTIONS: [usize; 4] = [1, 2, 3, 5];
/// `SmcEngineConfig::atr_period` options (A0 piece 9).
const ATR_PERIOD_OPTIONS: [usize; 4] = [7, 14, 21, 50];
/// `SmcEngineConfig::sweep_recovery_bars` options (A0 piece 9).
const SWEEP_RECOVERY_BARS_OPTIONS: [usize; 4] = [0, 1, 2, 3];
/// `SmcEngineConfig::displacement_atr_mult` options (A0 piece 9).
const DISPLACEMENT_ATR_MULT_OPTIONS: [f64; 4] = [1.5, 2.0, 2.5, 3.0];
/// `SmcEngineConfig::equal_level_atr_tol` options (A0 piece 9).
const EQUAL_LEVEL_ATR_TOL_OPTIONS: [f64; 4] = [0.05, 0.1, 0.15, 0.2];

const FAMILY_SEGMENTS: [MarkupSettingsFamily; 3] = [
    MarkupSettingsFamily::SmartMoney,
    MarkupSettingsFamily::OrderFlow,
    MarkupSettingsFamily::CandlePattern,
];

fn family_label(family: MarkupSettingsFamily) -> &'static str {
    match family {
        MarkupSettingsFamily::SmartMoney => t_concept(ConceptKey::MarkupFamilySmartMoney),
        MarkupSettingsFamily::OrderFlow => t_concept(ConceptKey::MarkupFamilyOrderFlow),
        MarkupSettingsFamily::CandlePattern => t_concept(ConceptKey::MarkupFamilyCandles),
    }
}

fn tab_label(tab: SettingsTab) -> &'static str {
    match tab {
        SettingsTab::WhatToDetect => t_concept(ConceptKey::MarkupTabWhatToDetect),
        SettingsTab::Thresholds => t_concept(ConceptKey::MarkupTabThresholds),
        SettingsTab::Style => t_concept(ConceptKey::MarkupTabStyle),
        SettingsTab::Coverage => t_concept(ConceptKey::MarkupTabCoverage),
    }
}

/// How many GUARD rows the shown family has.
fn guard_rows(family: MarkupSettingsFamily) -> usize {
    match family {
        // A candle pattern is a self-contained reading of its own bars and
        // `markup::chain::successor_kinds` gives the family no successors,
        // so a chain switch there would toggle rules that cannot fire.
        MarkupSettingsFamily::CandlePattern => 1,
        _ => 2,
    }
}

/// How many ACTION rows the shown family has — a row that DOES something
/// rather than setting something.
///
/// Smart money alone, because it is the only family with a narrative to
/// assemble: `markup::propose` walks sweeps into shifts into origins, and
/// order flow's three-event chain and the candle family's absent one have
/// nothing to propose that the survey does not already paint.
fn action_rows(family: MarkupSettingsFamily) -> usize {
    match family {
        MarkupSettingsFamily::SmartMoney => 1,
        _ => 0,
    }
}

/// How many of the pre-existing bar-count threshold rows (snap window,
/// chain window, order flow's single-print run) the shown family carries —
/// unchanged content from the old "Detection" group, filed under the
/// Thresholds tab now (A0 piece 8) rather than renamed or dropped.
fn legacy_threshold_rows(family: MarkupSettingsFamily) -> usize {
    match family {
        // snap, chain window
        MarkupSettingsFamily::SmartMoney => 2,
        // snap, chain window, single-print run
        MarkupSettingsFamily::OrderFlow => 3,
        // snap only — a pattern has no succession, so no chain window
        MarkupSettingsFamily::CandlePattern => 1,
    }
}

/// How many DETECTOR-CONSTANT rows the shown family's own Thresholds tab
/// adds beyond [`legacy_threshold_rows`] — 6 for Smart Money (A0 piece 9),
/// 0 for Order Flow (piece 10 wires this in) and candles (the arc's own
/// line: no candle-pattern detector constant is surfaced in A0). The
/// height function and the draw below read this SAME count, so a family
/// whose Thresholds tab later grows can never overflow a shell sized for
/// the old one.
fn detector_rows(family: MarkupSettingsFamily) -> usize {
    match family {
        MarkupSettingsFamily::SmartMoney => 6,
        MarkupSettingsFamily::OrderFlow => 0,
        MarkupSettingsFamily::CandlePattern => 0,
    }
}

/// The family segment's own row height, shared by every tab (it sits above
/// the tab strip's content, drawn once regardless of which tab is active).
fn family_segment_height() -> f64 {
    ROW_H + GROUP_GAP
}

fn what_to_detect_height(family: MarkupSettingsFamily) -> f64 {
    let mut h = 0.0;
    // Detection group: judge_claims alone now (snap/chain/single-print
    // moved to Thresholds, see `legacy_threshold_rows`'s own doc).
    h += GROUP_HEADER_H + ROW_H + GROUP_GAP;
    // Rule guard group.
    let g = guard_rows(family) as f64;
    h += GROUP_HEADER_H + ROW_H * g + ROW_GAP * (g - 1.0).max(0.0);
    if action_rows(family) > 0 {
        h += GROUP_GAP + ROW_H;
    }
    h
}

fn thresholds_height(family: MarkupSettingsFamily) -> f64 {
    let mut h = 0.0;
    let legacy = legacy_threshold_rows(family) as f64;
    h += GROUP_HEADER_H + ROW_H * legacy + ROW_GAP * (legacy - 1.0).max(0.0);
    let new = detector_rows(family) as f64;
    if new > 0.0 {
        h += GROUP_GAP + ROW_H * new + ROW_GAP * (new - 1.0).max(0.0);
    }
    h
}

/// No cosmetic setting exists on any of the three engines today — see the
/// module's own doc for why this stays literally empty in A0 rather than
/// inventing content to fill it.
fn style_height() -> f64 {
    0.0
}

/// Coverage pills row + Shape segmented row — fixed, independent of
/// `family` (the tab edits ONE specific drawing, not a family-wide config).
fn coverage_height() -> f64 {
    ROW_H * 2.0 + ROW_GAP
}

fn tab_body_height(family: MarkupSettingsFamily, tab: SettingsTab) -> f64 {
    match tab {
        SettingsTab::WhatToDetect => what_to_detect_height(family),
        SettingsTab::Thresholds => thresholds_height(family),
        SettingsTab::Style => style_height(),
        SettingsTab::Coverage => coverage_height(),
    }
}

/// The body height the modal always uses — the TALLEST (family, tab) pair's,
/// whichever family/tab is actually showing.
///
/// Not each combination's own height, which was the pre-tabs modal's first
/// attempt and was wrong on the stand: an un-dragged modal centres itself on
/// its own height, so switching to a taller combination moved the whole
/// modal upward and the control that had just been clicked slid out from
/// under the cursor. A picker that moves when you use it is broken however
/// correct its rows are — see `SettingsTab`'s own doc, "switching tabs never
/// changes the OUTER modal frame size mid-session".
fn content_height() -> f64 {
    let mut max = 0.0_f64;
    for family in FAMILY_SEGMENTS {
        for tab in SettingsTab::ALL {
            max = max.max(family_segment_height() + tab_body_height(family, tab));
        }
    }
    max
}

/// Render the Price Action Engine Settings modal.
pub fn render_markup_engine_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &MarkupEngineSettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> MarkupEngineSettingsResult {
    let mut result = MarkupEngineSettingsResult::default();
    let _lang = current_language();
    let family = state.family;
    let body_h = content_height();

    let tab_items: Vec<ModalTab<'_>> = SettingsTab::ALL
        .iter()
        .map(|&tab| ModalTab::new(tab.id(), tab_label(tab)))
        .collect();
    let active_tab_idx = SettingsTab::ALL.iter().position(|&t| t == state.active_tab).unwrap_or(0);

    let modal_theme = ModalTheme::from_toolbar(&frame_theme.modal_bg, &toolbar_theme.separator, toolbar_theme);
    let spec = ModalShellSpec {
        placement: crate::ui::widgets::ModalPlacement::At(Some(
            state.position.unwrap_or(((screen_w - MODAL_W) / 2.0, (screen_h - body_h) / 2.0)),
        )),
        // Above the modal that opened it, not beside it. Both entrances to
        // an engine-settings modal are inside ANOTHER modal (a primitive's
        // own Subject tab, the wave-gesture popup), so this is by
        // definition a modal stacked over a modal — the case
        // `ZLayer::ModalOverlay` exists for, and the one
        // `template_name_modal` already documents.
        //
        // On the default `ZLayer::Modal` it was unreachable, and the
        // mechanism is worth writing down because nothing about it is
        // visible on screen: `hit_test_at_filtered` sorts layers by
        // z-order with a STABLE sort, so two modals at the same z keep
        // registration order — the one underneath is scanned FIRST, and
        // being modal it raises the barrier that skips every non-chrome
        // layer after it. The modal drawn on top therefore received no
        // clicks at all: rows inside the lower modal's rect went to the
        // lower modal, everything else fell through to the backdrop and
        // closed the very modal that was clicked.
        layer: crate::ui::z_order::ZLayer::ModalOverlay,
        ..ModalShellSpec::new(
            "markup_engine_settings",
            t_concept(ConceptKey::MarkupEngineSettingsTitle),
            MODAL_W,
            body_h,
        )
        .with_tabs(ModalTabs::Horizontal { items: &tab_items, active: active_tab_idx, fit: TabFit::Equal })
        .with_footer(ModalFooter::Editor { template: None })
    };

    let shell = render_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h), |ctx, coord, layer_id, shell| {
        let x0 = shell.content.x;
        let x1 = shell.content.x + shell.content.width;
        let mut row_y = shell.content.y;

        // ── Which family's engine (every tab reads/writes this family's
        // own global config, EXCEPT Coverage, which always edits the one
        // drawing that opened the modal regardless of this segment) ──────
        draw_segmented_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, FAMILY_SEGMENTS.len(), |i| {
            let f = FAMILY_SEGMENTS[i];
            (format!("markup:family:{}", f.id()), family_label(f), f == family)
        });
        row_y += ROW_H + GROUP_GAP;

        match state.active_tab {
            SettingsTab::WhatToDetect => draw_what_to_detect_tab(ctx, toolbar_theme, coord, layer_id, &mut result, state, family, x0, x1, row_y),
            SettingsTab::Thresholds => draw_thresholds_tab(ctx, toolbar_theme, coord, layer_id, &mut result, state, family, x0, x1, row_y),
            SettingsTab::Style => {
                // Deliberately empty — see the module's own doc.
            }
            SettingsTab::Coverage => draw_coverage_tab(ctx, toolbar_theme, coord, layer_id, &mut result, state, x0, x1, row_y),
        }
    });

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result
}

/// What to detect: whether this family's own claims are judged at all, plus
/// which cross-primitive rule classes it reports — and, for Smart Money
/// alone, the action that assembles what it found.
#[allow(clippy::too_many_arguments)]
fn draw_what_to_detect_tab(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut MarkupEngineSettingsResult,
    state: &MarkupEngineSettingsState,
    family: MarkupSettingsFamily,
    x0: f64,
    x1: f64,
    mut row_y: f64,
) {
    draw_group_header(ctx, toolbar_theme, x0, x1, row_y, t_concept(ConceptKey::MarkupDetectionGroupTitle), None);
    row_y += GROUP_HEADER_H;

    let judge = match family {
        MarkupSettingsFamily::SmartMoney => state.smc.judge_claims,
        MarkupSettingsFamily::OrderFlow => state.orderflow.judge_claims,
        MarkupSettingsFamily::CandlePattern => state.candle.judge_claims,
    };
    draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, result, x0, x1, row_y, t_concept(ConceptKey::MarkupJudgeClaimsLabel), judge, "markup:judge_claims");
    row_y += ROW_H + GROUP_GAP;

    draw_group_header(ctx, toolbar_theme, x0, x1, row_y, t_concept(ConceptKey::MarkupGuardFamiliesGroupTitle), None);
    row_y += GROUP_HEADER_H;

    let families = match family {
        MarkupSettingsFamily::SmartMoney => state.smc.guard_families,
        MarkupSettingsFamily::OrderFlow => state.orderflow.guard_families,
        MarkupSettingsFamily::CandlePattern => state.candle.guard_families,
    };
    draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, result, x0, x1, row_y, t_concept(ConceptKey::MarkupGuardNeighbourLabel), families.neighbour, "markup:guard_neighbour");
    row_y += ROW_H + ROW_GAP;
    if guard_rows(family) > 1 {
        draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, result, x0, x1, row_y, t_concept(ConceptKey::MarkupGuardChainLabel), families.chain, "markup:guard_chain");
        row_y += ROW_H;
    }

    // The engine acting, rather than being configured — see this function's
    // own doc for why only Smart Money offers it.
    if action_rows(family) > 0 {
        row_y += GROUP_GAP;
        let rect = WidgetRect::new(x0, row_y + 1.0, (x1 - x0).max(20.0), ROW_H - 2.0);
        draw_toggle_segment(ctx, toolbar_theme, rect, t_concept(ConceptKey::MarkupProposeSetupsLabel), false);
        result.content_items.push(("markup:propose".to_string(), rect));
        coord.register_on_layer(
            "markup_engine_settings:markup:propose",
            rect,
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
            layer_id,
        );
    }
}

/// Thresholds: the pre-existing bar-count rows (moved here unchanged from
/// the old "Detection" group) plus each family's own detector constants.
#[allow(clippy::too_many_arguments)]
fn draw_thresholds_tab(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut MarkupEngineSettingsResult,
    state: &MarkupEngineSettingsState,
    family: MarkupSettingsFamily,
    x0: f64,
    x1: f64,
    mut row_y: f64,
) {
    draw_group_header(ctx, toolbar_theme, x0, x1, row_y, t_concept(ConceptKey::MarkupDetectionGroupTitle), None);
    row_y += GROUP_HEADER_H;

    let (snap, chain_window, single_print) = match family {
        MarkupSettingsFamily::SmartMoney => (state.smc.claim_snap_bars, Some(state.smc.chain_window_bars), None),
        MarkupSettingsFamily::OrderFlow => {
            (state.orderflow.claim_snap_bars, Some(state.orderflow.chain_window_bars), Some(state.orderflow.min_single_print_run))
        }
        MarkupSettingsFamily::CandlePattern => (state.candle.claim_snap_bars, None, None),
    };

    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupSnapBarsLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, SNAP_BAR_OPTIONS.len(), |i| {
        let n = SNAP_BAR_OPTIONS[i];
        (format!("markup:snap_bars:{n}"), count_label(n), snap == n)
    });
    row_y += ROW_H + ROW_GAP;

    if let Some(window) = chain_window {
        draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupChainWindowLabel));
        draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, CHAIN_WINDOW_OPTIONS.len(), |i| {
            let n = CHAIN_WINDOW_OPTIONS[i];
            (format!("markup:chain_window:{n}"), count_label(n), window == n)
        });
        row_y += ROW_H + ROW_GAP;
    }

    if let Some(run) = single_print {
        draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupMinSinglePrintRunLabel));
        draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, SINGLE_PRINT_RUN_OPTIONS.len(), |i| {
            let n = SINGLE_PRINT_RUN_OPTIONS[i];
            (format!("markup:single_print_run:{n}"), count_label(n), run == n)
        });
        row_y += ROW_H + ROW_GAP;
    }

    if detector_rows(family) == 0 {
        return;
    }
    row_y += GROUP_GAP - ROW_GAP;

    match family {
        MarkupSettingsFamily::SmartMoney => draw_smc_detector_rows(ctx, toolbar_theme, coord, layer_id, result, &state.smc, x0, x1, row_y),
        // Piece 10 fills Order Flow's own row here.
        MarkupSettingsFamily::OrderFlow | MarkupSettingsFamily::CandlePattern => {}
    }
}

/// Smart Money's own 6 detector-constant rows (A0 piece 9) — every field
/// already exists on `SmcEngineConfig` and already has its own reader
/// (`to_smc_config`); this is the first row any of them gets.
#[allow(clippy::too_many_arguments)]
fn draw_smc_detector_rows(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut MarkupEngineSettingsResult,
    cfg: &mlc_core::state::smc_engine::SmcEngineConfig,
    x0: f64,
    x1: f64,
    mut row_y: f64,
) {
    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupThresholdSwingNLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, SWING_N_OPTIONS.len(), |i| {
        let n = SWING_N_OPTIONS[i];
        (format!("markup:swing_n:{n}"), count_label(n), cfg.swing_n == n)
    });
    row_y += ROW_H + ROW_GAP;

    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupThresholdAtrPeriodLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, ATR_PERIOD_OPTIONS.len(), |i| {
        let n = ATR_PERIOD_OPTIONS[i];
        (format!("markup:atr_period:{n}"), count_label(n), cfg.atr_period == n)
    });
    row_y += ROW_H + ROW_GAP;

    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupThresholdSweepRecoveryBarsLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, SWEEP_RECOVERY_BARS_OPTIONS.len(), |i| {
        let n = SWEEP_RECOVERY_BARS_OPTIONS[i];
        (format!("markup:sweep_recovery_bars:{n}"), count_label(n), cfg.sweep_recovery_bars == n)
    });
    row_y += ROW_H + ROW_GAP;

    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupThresholdOrderBlockZoneLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, 2, |i| {
        let zone = if i == 0 {
            mlc_core::state::smc_engine::OrderBlockZoneMode::FullRange
        } else {
            mlc_core::state::smc_engine::OrderBlockZoneMode::BodyOnly
        };
        let label = if i == 0 { t_concept(ConceptKey::MarkupOrderBlockZoneFullRange) } else { t_concept(ConceptKey::MarkupOrderBlockZoneBodyOnly) };
        let id = if i == 0 { "markup:order_block_zone:full_range" } else { "markup:order_block_zone:body_only" };
        (id.to_string(), label, cfg.order_block_zone == zone)
    });
    row_y += ROW_H + ROW_GAP;

    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupThresholdDisplacementAtrMultLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, DISPLACEMENT_ATR_MULT_OPTIONS.len(), |i| {
        let v = DISPLACEMENT_ATR_MULT_OPTIONS[i];
        (format!("markup:displacement_atr_mult:{v}"), float_label(v), (cfg.displacement_atr_mult - v).abs() < 1e-9)
    });
    row_y += ROW_H + ROW_GAP;

    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupThresholdEqualLevelAtrTolLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, EQUAL_LEVEL_ATR_TOL_OPTIONS.len(), |i| {
        let v = EQUAL_LEVEL_ATR_TOL_OPTIONS[i];
        (format!("markup:equal_level_atr_tol:{v}"), float_label(v), (cfg.equal_level_atr_tol - v).abs() < 1e-9)
    });
}

/// Coverage: the ONE drawing that opened this modal's own `coverage`/
/// `shape` — a placement choice, never a family-wide config (see the
/// module's own doc). `detector_rows(family)` plays no part here: this tab
/// does not vary by family at all.
#[allow(clippy::too_many_arguments)]
fn draw_coverage_tab(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut MarkupEngineSettingsResult,
    state: &MarkupEngineSettingsState,
    x0: f64,
    x1: f64,
    mut row_y: f64,
) {
    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::ScanCoverageLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, 2, |i| {
        let (id, label) = if i == 0 {
            ("range", t_concept(ConceptKey::ScanCoverageRange))
        } else {
            ("all_bars", t_concept(ConceptKey::ScanCoverageAllBars))
        };
        (format!("markup:coverage:{id}"), label, state.scan_coverage == id)
    });
    row_y += ROW_H + ROW_GAP;

    draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::ScanShapeLabel));
    draw_segmented_row(ctx, toolbar_theme, coord, layer_id, result, x0 + LABEL_COL, x1, row_y, 2, |i| {
        let (shape, label) = if i == 0 {
            (ScanShape::TwoLines, t_concept(ConceptKey::ScanShapeTwoLines))
        } else {
            (ScanShape::Rectangle, t_concept(ConceptKey::ScanShapeRectangle))
        };
        (format!("markup:shape:{}", shape.id()), label, state.scan_shape == shape.id())
    });
}

/// A bar count as a segment label. `0` prints as `Off` because that is what
/// it means everywhere these options are read — not "zero bars of reach"
/// but "do not do this at all".
fn count_label(n: usize) -> &'static str {
    match n {
        0 => "Off",
        1 => "1",
        2 => "2",
        3 => "3",
        5 => "5",
        7 => "7",
        10 => "10",
        14 => "14",
        21 => "21",
        30 => "30",
        50 => "50",
        60 => "60",
        _ => "?",
    }
}

/// A ratio/tolerance value as a segment label — the 8 distinct values
/// `DISPLACEMENT_ATR_MULT_OPTIONS`/`EQUAL_LEVEL_ATR_TOL_OPTIONS` carry
/// between them, spelled out rather than formatted at draw time so the
/// catalogue stays the single source of truth for both the value AND its
/// label (the same posture `count_label` already takes for bar counts).
fn float_label(v: f64) -> &'static str {
    if (v - 1.5).abs() < 1e-9 {
        "1.5"
    } else if (v - 2.0).abs() < 1e-9 {
        "2.0"
    } else if (v - 2.5).abs() < 1e-9 {
        "2.5"
    } else if (v - 3.0).abs() < 1e-9 {
        "3.0"
    } else if (v - 0.05).abs() < 1e-9 {
        "0.05"
    } else if (v - 0.1).abs() < 1e-9 {
        "0.1"
    } else if (v - 0.15).abs() < 1e-9 {
        "0.15"
    } else if (v - 0.2).abs() < 1e-9 {
        "0.2"
    } else {
        "?"
    }
}

/// One checkbox row — a local copy of `ewa_engine_settings`'s own, for the
/// same reason that one is local: `primitive_settings_vid::draw_bool_row` is
/// typed to `PrimitiveSettingsResult`.
#[allow(clippy::too_many_arguments)]
fn draw_checkbox_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut MarkupEngineSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    label: &str,
    checked: bool,
    field_id: &str,
) {
    let checkbox_size = 16.0;
    let cb_x = x0;
    let cb_y = y + (ROW_H - checkbox_size) / 2.0;
    crate::ui::overlay_fill::fill_checkbox(
        ctx, checked, &toolbar_theme.item_bg_active, &toolbar_theme.dropdown_bg, &toolbar_theme.background,
        cb_x, cb_y, checkbox_size, checkbox_size, 3.0,
    );
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cb_x, cb_y, checkbox_size, checkbox_size, 3.0);
    if checked {
        ctx.set_stroke_color(&toolbar_theme.item_text_active);
        ctx.set_stroke_width(2.0);
        ctx.begin_path();
        ctx.move_to(cb_x + 3.0, cb_y + checkbox_size * 0.55);
        ctx.line_to(cb_x + checkbox_size * 0.42, cb_y + checkbox_size - 3.0);
        ctx.line_to(cb_x + checkbox_size - 3.0, cb_y + 3.0);
        ctx.stroke();
    }
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, cb_x + checkbox_size + 8.0, y + ROW_H / 2.0);

    let rect = WidgetRect::new(x0, y, (x1 - x0).max(checkbox_size), ROW_H);
    result.content_items.push((field_id.to_string(), rect));
    let hit_id = format!("markup_engine_settings:{field_id}");
    coord.register_on_layer(hit_id.as_str(), rect, uzor::input::Sense::CLICK | uzor::input::Sense::HOVER, layer_id);
}

/// One row of `count` equal-width segments — the local twin of
/// `ewa_engine_settings::draw_segmented_row`, differing only in the widget
/// id prefix and the result type it fills.
#[allow(clippy::too_many_arguments)]
fn draw_segmented_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut MarkupEngineSettingsResult,
    x0: f64,
    x1: f64,
    y: f64,
    count: usize,
    resolve: impl Fn(usize) -> (String, &'static str, bool),
) {
    if count == 0 {
        return;
    }
    let n = count as f64;
    let w = (((x1 - x0) - SEGMENT_GAP * (n - 1.0)) / n).max(20.0);
    let mut x = x0;
    for i in 0..count {
        let (field_id, label, is_current) = resolve(i);
        let rect = WidgetRect::new(x, y + 2.0, w, ROW_H - 4.0);
        draw_toggle_segment(ctx, toolbar_theme, rect, label, is_current);
        result.content_items.push((field_id.clone(), rect));
        let hit_id = format!("markup_engine_settings:{field_id}");
        coord.register_on_layer(hit_id.as_str(), rect, uzor::input::Sense::CLICK | uzor::input::Sense::HOVER, layer_id);
        x += w + SEGMENT_GAP;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The height function and the draw read the same row counts, so a
    /// family/tab combination whose body grows can never overflow a shell
    /// sized for the old one.
    #[test]
    fn each_family_declares_the_rows_it_draws() {
        assert_eq!(guard_rows(MarkupSettingsFamily::CandlePattern), 1, "candles have no succession");
        assert_eq!(guard_rows(MarkupSettingsFamily::SmartMoney), 2);
        assert_eq!(legacy_threshold_rows(MarkupSettingsFamily::SmartMoney), 2);
        assert_eq!(legacy_threshold_rows(MarkupSettingsFamily::OrderFlow), 3);
        assert_eq!(legacy_threshold_rows(MarkupSettingsFamily::CandlePattern), 1);
        assert_eq!(detector_rows(MarkupSettingsFamily::SmartMoney), 6);
        assert_eq!(detector_rows(MarkupSettingsFamily::OrderFlow), 0);
        assert_eq!(detector_rows(MarkupSettingsFamily::CandlePattern), 0);
    }

    /// The shell must not resize when the family OR the tab changes — see
    /// [`content_height`]'s own doc for what that cost on the stand.
    #[test]
    fn the_shell_is_the_same_height_for_every_family_and_tab() {
        let tallest = FAMILY_SEGMENTS
            .into_iter()
            .flat_map(|f| SettingsTab::ALL.into_iter().map(move |t| (f, t)))
            .map(|(f, t)| family_segment_height() + tab_body_height(f, t))
            .fold(0.0_f64, f64::max);
        assert_eq!(content_height(), tallest, "the tallest (family, tab) pair sets the shell");
        assert!(
            family_segment_height() + tab_body_height(MarkupSettingsFamily::SmartMoney, SettingsTab::Thresholds)
                > family_segment_height() + tab_body_height(MarkupSettingsFamily::CandlePattern, SettingsTab::Thresholds),
            "Smart Money's own 6 detector rows must not be absorbed into candles' thinner Thresholds tab",
        );
        assert!(
            family_segment_height() + tab_body_height(MarkupSettingsFamily::SmartMoney, SettingsTab::Style) < content_height(),
            "the empty Style tab must not set the shell",
        );
    }

    /// Repeated opens (same family, same tab) must report the identical
    /// height — a pure function of `family`/`tab`, nothing else.
    #[test]
    fn height_is_stable_across_repeated_opens() {
        for family in FAMILY_SEGMENTS {
            for tab in SettingsTab::ALL {
                let first = tab_body_height(family, tab);
                let second = tab_body_height(family, tab);
                assert_eq!(first, second);
            }
        }
    }

    /// Switching tabs never changes the outer modal frame size mid-session
    /// — every tab reports a height at or below the shell's own
    /// `content_height`, computed once, independent of which tab is active.
    #[test]
    fn switching_tabs_never_exceeds_the_shells_own_height() {
        let shell_h = content_height();
        for family in FAMILY_SEGMENTS {
            for tab in SettingsTab::ALL {
                assert!(family_segment_height() + tab_body_height(family, tab) <= shell_h);
            }
        }
    }

    #[test]
    fn only_smart_money_offers_an_action() {
        assert_eq!(action_rows(MarkupSettingsFamily::SmartMoney), 1);
        assert_eq!(action_rows(MarkupSettingsFamily::OrderFlow), 0);
        assert_eq!(action_rows(MarkupSettingsFamily::CandlePattern), 0);
    }

    /// Zero is not "zero bars", it is off — every reader of these options
    /// treats it that way, so the label has to as well.
    #[test]
    fn zero_reads_as_off() {
        assert_eq!(count_label(0), "Off");
        assert_eq!(count_label(30), "30");
    }

    #[test]
    fn every_float_option_has_a_label() {
        for v in DISPLACEMENT_ATR_MULT_OPTIONS {
            assert_ne!(float_label(v), "?", "{v}");
        }
        for v in EQUAL_LEVEL_ATR_TOL_OPTIONS {
            assert_ne!(float_label(v), "?", "{v}");
        }
    }

    #[test]
    fn family_tokens_round_trip() {
        for f in FAMILY_SEGMENTS {
            assert_eq!(MarkupSettingsFamily::from_id(f.id()), Some(f));
        }
        assert_eq!(MarkupSettingsFamily::from_id("elliott"), None);
    }

    #[test]
    fn tab_tokens_round_trip() {
        for t in SettingsTab::ALL {
            assert_eq!(SettingsTab::from_id(t.id()), Some(t));
        }
        assert_eq!(SettingsTab::from_id("bogus"), None);
    }

    #[test]
    fn every_markup_type_finds_its_family() {
        assert_eq!(MarkupSettingsFamily::from_type_id("order_block"), Some(MarkupSettingsFamily::SmartMoney));
        assert_eq!(MarkupSettingsFamily::from_type_id("absorption_mark"), Some(MarkupSettingsFamily::OrderFlow));
        assert_eq!(MarkupSettingsFamily::from_type_id("candle_pattern"), Some(MarkupSettingsFamily::CandlePattern));
        assert_eq!(MarkupSettingsFamily::from_type_id("trend_line"), None, "a line has no engine");
    }
}
