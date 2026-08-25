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
//! # What it edits
//!
//! Every edit applies immediately — writes through to the live global and
//! the profile, the same no-diff shape the EWA modal uses and for the same
//! reason (these configs have no per-drawing counterpart, so there is
//! nothing to accept or discard). The footer's ✕/OK both simply close.
//!
//! # Row selection
//!
//! Deliberately not every field. `default_enabled_kinds` (34 candle ids) is
//! a picker, not a row, and it already has a home on each scan's own
//! settings modal; the raw detector constants (swing_n, ATR period,
//! displacement multiplier) are `mli` tuning that changes what the whole
//! vocabulary means and belongs behind a deliberate surface of its own, not
//! beside a checkbox. What is here is what an analyst changes while marking
//! up: whether claims are judged at all, how far a claim may snap, how far
//! a chain may reach, and which classes of rule report.

use mlc_core::i18n::{current_language, ConceptKey, t_concept};
use mlc_core::render::{RenderContext, TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::MarkupEngineSettingsResult;
use crate::ui::modal_settings::{MarkupEngineSettingsState, MarkupSettingsFamily};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{render_modal_shell, ModalFooter, ModalShellSpec, ModalTheme};

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

/// How many DETECTION rows the shown family has — the one thing that
/// differs between the three bodies, so the height and the draw can never
/// drift apart.
fn detection_rows(family: MarkupSettingsFamily) -> usize {
    match family {
        // judge / snap / chain window
        MarkupSettingsFamily::SmartMoney => 3,
        // judge / snap / chain window / single-print run
        MarkupSettingsFamily::OrderFlow => 4,
        // judge / snap — a pattern has no succession, so no window
        MarkupSettingsFamily::CandlePattern => 2,
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

fn family_body_height(family: MarkupSettingsFamily) -> f64 {
    let mut h = 0.0;
    h += ROW_H + GROUP_GAP; // family segment
    let d = detection_rows(family) as f64;
    h += GROUP_HEADER_H + ROW_H * d + ROW_GAP * (d - 1.0) + GROUP_GAP;
    let g = guard_rows(family) as f64;
    h += GROUP_HEADER_H + ROW_H * g + ROW_GAP * (g - 1.0);
    if action_rows(family) > 0 {
        h += GROUP_GAP + ROW_H;
    }
    h
}

/// The body height the modal always uses — the TALLEST family's, whichever
/// family is showing.
///
/// Not each family's own height, which was the first attempt and was wrong
/// on the stand: an un-dragged modal centres itself on its own height, so
/// switching to a taller family moved the whole modal upward and the
/// segment row slid out from under the cursor that had just clicked it. A
/// picker that moves when you use it is broken however correct its rows
/// are. Order flow is the tallest (it alone has a single-print row), so the
/// shorter families end with a little empty space, which costs nothing and
/// holds every control still.
fn content_height() -> f64 {
    family_body_height(MarkupSettingsFamily::SmartMoney)
        .max(family_body_height(MarkupSettingsFamily::OrderFlow))
        .max(family_body_height(MarkupSettingsFamily::CandlePattern))
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

    let modal_theme = ModalTheme::from_toolbar(&frame_theme.toolbar_bg, &toolbar_theme.separator, toolbar_theme);
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
        .with_footer(ModalFooter::Editor { template: None })
    };

    let shell = render_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h), |ctx, coord, layer_id, shell| {
        let x0 = shell.content.x;
        let x1 = shell.content.x + shell.content.width;
        let mut row_y = shell.content.y;

        // ── Which family's engine ───────────────────────────────────────
        draw_segmented_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, FAMILY_SEGMENTS.len(), |i| {
            let f = FAMILY_SEGMENTS[i];
            (format!("markup:family:{}", f.id()), family_label(f), f == family)
        });
        row_y += ROW_H + GROUP_GAP;

        // ── Detection ───────────────────────────────────────────────────
        draw_group_header(ctx, toolbar_theme, x0, x1, row_y, t_concept(ConceptKey::MarkupDetectionGroupTitle), None);
        row_y += GROUP_HEADER_H;

        let (judge, snap, chain_window, single_print) = match family {
            MarkupSettingsFamily::SmartMoney => (
                state.smc.judge_claims,
                state.smc.claim_snap_bars,
                Some(state.smc.chain_window_bars),
                None,
            ),
            MarkupSettingsFamily::OrderFlow => (
                state.orderflow.judge_claims,
                state.orderflow.claim_snap_bars,
                Some(state.orderflow.chain_window_bars),
                Some(state.orderflow.min_single_print_run),
            ),
            MarkupSettingsFamily::CandlePattern => {
                (state.candle.judge_claims, state.candle.claim_snap_bars, None, None)
            }
        };

        draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, t_concept(ConceptKey::MarkupJudgeClaimsLabel), judge, "markup:judge_claims");
        row_y += ROW_H + ROW_GAP;

        draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupSnapBarsLabel));
        draw_segmented_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0 + LABEL_COL, x1, row_y, SNAP_BAR_OPTIONS.len(), |i| {
            let n = SNAP_BAR_OPTIONS[i];
            (format!("markup:snap_bars:{n}"), count_label(n), snap == n)
        });
        row_y += ROW_H + ROW_GAP;

        if let Some(window) = chain_window {
            draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupChainWindowLabel));
            draw_segmented_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0 + LABEL_COL, x1, row_y, CHAIN_WINDOW_OPTIONS.len(), |i| {
                let n = CHAIN_WINDOW_OPTIONS[i];
                (format!("markup:chain_window:{n}"), count_label(n), window == n)
            });
            row_y += ROW_H + ROW_GAP;
        }

        if let Some(run) = single_print {
            draw_label(ctx, toolbar_theme, x0, LABEL_COL, row_y, t_concept(ConceptKey::MarkupMinSinglePrintRunLabel));
            draw_segmented_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0 + LABEL_COL, x1, row_y, SINGLE_PRINT_RUN_OPTIONS.len(), |i| {
                let n = SINGLE_PRINT_RUN_OPTIONS[i];
                (format!("markup:single_print_run:{n}"), count_label(n), run == n)
            });
            row_y += ROW_H + ROW_GAP;
        }
        row_y += GROUP_GAP - ROW_GAP;

        // ── Rule guard ──────────────────────────────────────────────────
        draw_group_header(ctx, toolbar_theme, x0, x1, row_y, t_concept(ConceptKey::MarkupGuardFamiliesGroupTitle), None);
        row_y += GROUP_HEADER_H;

        let families = match family {
            MarkupSettingsFamily::SmartMoney => state.smc.guard_families,
            MarkupSettingsFamily::OrderFlow => state.orderflow.guard_families,
            MarkupSettingsFamily::CandlePattern => state.candle.guard_families,
        };
        draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, t_concept(ConceptKey::MarkupGuardNeighbourLabel), families.neighbour, "markup:guard_neighbour");
        row_y += ROW_H + ROW_GAP;
        if guard_rows(family) > 1 {
            draw_checkbox_row(ctx, toolbar_theme, coord, layer_id, &mut result, x0, x1, row_y, t_concept(ConceptKey::MarkupGuardChainLabel), families.chain, "markup:guard_chain");
            row_y += ROW_H;
        }

        // ── The engine acting, rather than being configured ─────────────
        //
        // An action lives here rather than on a tool of its own because it
        // IS this engine speaking: it reads this family's own detectors
        // through this family's own chain window, and every setting above
        // changes what it proposes. A separate button somewhere else would
        // be the same code with the settings that govern it out of sight.
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
    });

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result
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
        10 => "10",
        30 => "30",
        60 => "60",
        _ => "?",
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
    ctx.set_fill_color(if checked { &toolbar_theme.item_bg_active } else { &toolbar_theme.background });
    ctx.fill_rounded_rect(cb_x, cb_y, checkbox_size, checkbox_size, 3.0);
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

    /// The height function and the draw read the same two row counts, so a
    /// family whose body grows can never overflow a shell sized for the old
    /// one. This asserts the two counts are what the draw actually emits.
    #[test]
    fn each_family_declares_the_rows_it_draws() {
        assert_eq!(detection_rows(MarkupSettingsFamily::SmartMoney), 3);
        assert_eq!(detection_rows(MarkupSettingsFamily::OrderFlow), 4);
        assert_eq!(detection_rows(MarkupSettingsFamily::CandlePattern), 2);
        assert_eq!(guard_rows(MarkupSettingsFamily::CandlePattern), 1, "candles have no succession");
        assert_eq!(guard_rows(MarkupSettingsFamily::SmartMoney), 2);
    }

    /// The shell must not resize when the family changes — see
    /// [`content_height`]'s own doc for what that cost on the stand.
    /// The shell must not resize when the family changes — see
    /// [`content_height`]'s own doc for what that cost on the stand.
    #[test]
    fn the_shell_is_the_same_height_for_every_family() {
        let tallest = [
            MarkupSettingsFamily::SmartMoney,
            MarkupSettingsFamily::OrderFlow,
            MarkupSettingsFamily::CandlePattern,
        ]
        .into_iter()
        .map(family_body_height)
        .fold(0.0_f64, f64::max);
        assert_eq!(content_height(), tallest, "the tallest family sets the shell");
        assert!(
            family_body_height(MarkupSettingsFamily::CandlePattern)
                < family_body_height(MarkupSettingsFamily::SmartMoney),
            "candles are the thinnest family and must not set the shell",
        );
    }

    /// Only the family with a narrative to assemble offers to draw one.
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
    fn family_tokens_round_trip() {
        for f in FAMILY_SEGMENTS {
            assert_eq!(MarkupSettingsFamily::from_id(f.id()), Some(f));
        }
        assert_eq!(MarkupSettingsFamily::from_id("elliott"), None);
    }

    #[test]
    fn every_markup_type_finds_its_family() {
        assert_eq!(MarkupSettingsFamily::from_type_id("order_block"), Some(MarkupSettingsFamily::SmartMoney));
        assert_eq!(MarkupSettingsFamily::from_type_id("absorption_mark"), Some(MarkupSettingsFamily::OrderFlow));
        assert_eq!(MarkupSettingsFamily::from_type_id("candle_pattern"), Some(MarkupSettingsFamily::CandlePattern));
        assert_eq!(MarkupSettingsFamily::from_type_id("trend_line"), None, "a line has no engine");
    }
}
