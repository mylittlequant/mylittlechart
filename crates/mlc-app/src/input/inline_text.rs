//! Inline on-chart text editing of placed text primitives (inline-prim-text
//! arc, owner ask 2026-08-26: double-click a Text / Note / AnchoredText /
//! Callout / Comment / Signpost / PriceNote → edit its text right on the
//! chart, no modal).
//!
//! The editable text lives in the shared `TextFieldStore` under
//! [`crate::text_input::PRIM_INLINE_TEXT`]; the identity of what is being
//! edited lives in `ChartApp::inline_prim_text_edit`. The editing box is
//! drawn per frame by `ChartApp::render_inline_prim_text_editor` (called
//! from `render_overlay`), which also pushes fresh geometry into the store
//! via `update_field` — the `draw_oe_field_row` division of labor (render
//! pushes geometry, the store's generic pointer handling consumes it).

use crate::ChartApp;
use crate::sglog;
use uzor::WidgetId;
use uzor::input::TextFieldConfig;

impl ChartApp {
    /// Double-click on a placed text primitive starts the inline edit,
    /// seeded with the primitive's current `PrimitiveData::text.content`.
    ///
    /// The hit-test is the SAME two stages `try_wave_gesture` runs (main
    /// chart `hit_test`, then per-sub-pane `hit_test_in_pane` — the two
    /// entry points every click path already uses) — never a second
    /// geometry search. Locked primitives (`PrimitiveData::locked`) and
    /// primitives without registry `supports_text` are skipped; the
    /// primitive is selected so the inline toolbar and the settings modal
    /// agree with what is being edited (mirrors `try_wave_gesture`'s own
    /// select-then-act sequence).
    pub(crate) fn try_start_inline_prim_text_edit(&mut self, x: f64, y: f64) -> bool {
        let Some(bid) = self.panel_app.panel_grid.active_container().map(|w| w.current_tag_id) else {
            return false;
        };
        // Never hijack the double-click that FINISHES an in-progress
        // multipoint drawing — that one belongs to the engine's
        // DoubleClick forward further down `on_double_click`.
        if self.core.drawings().is_drawing(bid) {
            return false;
        }

        let _fallback;
        let extended = match self.active_frame_layout.as_ref() {
            Some(e) => e,
            None => { _fallback = self.build_extended_layout(); &_fallback }
        };
        let bars: Vec<mlc_core::Bar> = self.bars_for_bubble(bid);
        let chart_rect = extended.main_chart.chart;
        let local_x = x - chart_rect.x;
        let local_y = y - chart_rect.y;

        // Stage 0: user-text label plates — the registry the render pass
        // fills every frame (`engine::render::find_label_text_at`). A
        // double-click that lands on a LABEL owns the edit even when the
        // label sits off the primitive's body (a line's text hangs above
        // the line — the body's own hit-test never covers it). Runs BEFORE
        // the body stages so the label wins on overlaps.
        let mut resolved: Option<(u64, Option<u64>)> =
            mlc_core::engine::render::find_label_text_at(bid, None, local_x, local_y, 2.0)
                .map(|e| (e.prim_id, None));

        // Stage 1: main chart — the same `hit_test` call
        // `try_wave_gesture`'s own step 1 uses.
        if resolved.is_none()
            && local_x >= 0.0 && local_x <= chart_rect.width && local_y >= 0.0 && local_y <= chart_rect.height
        {
            if let Some(window) = self.panel_app.panel_grid.active_container() {
                let mut corrected_vp = window.viewport().clone();
                corrected_vp.chart_width = chart_rect.width;
                corrected_vp.chart_height = chart_rect.height;
                let price_scale = window.price_scale().clone();
                if let Some(prim_id) = self.core.drawings().hit_test(bid, local_x, local_y, &bars, &corrected_vp, &price_scale) {
                    resolved = Some((prim_id, None));
                }
            }
        }

        // Stage 2: sub-panes — label plates first (same rule as stage 0),
        // then the same `hit_test_in_pane` call `try_wave_gesture` uses.
        if resolved.is_none() {
            for pane_layout in extended.sub_panes.iter() {
                let content = pane_layout.content;
                let plx = x - content.x;
                let ply = y - content.y;
                if plx < 0.0 || plx > content.width || ply < 0.0 || ply > content.height {
                    continue;
                }
                let instance_id = pane_layout.instance_id;
                if let Some(entry) = mlc_core::engine::render::find_label_text_at(
                    bid, Some(instance_id), plx, ply, 2.0,
                ) {
                    resolved = Some((entry.prim_id, Some(instance_id)));
                    break;
                }
            }
        }
        if resolved.is_none() {
            for pane_layout in extended.sub_panes.iter() {
                let content = pane_layout.content;
                let plx = x - content.x;
                let ply = y - content.y;
                if plx < 0.0 || plx > content.width || ply < 0.0 || ply > content.height {
                    continue;
                }
                let instance_id = pane_layout.instance_id;
                let Some(window) = self.panel_app.panel_grid.active_container() else { continue };
                let (price_min, price_max) = window.sub_pane_y_state.get(&instance_id)
                    .map(|s| (s.price_min, s.price_max))
                    .unwrap_or((0.0, 100.0));
                let sub_price_scale = mlc_core::PriceScale::new(price_min, price_max);
                let mut sub_viewport = window.viewport().clone();
                sub_viewport.chart_height = content.height;
                if let Some(prim_id) = self.core.drawings().hit_test_in_pane(bid, plx, ply, instance_id, &bars, &sub_viewport, &sub_price_scale) {
                    resolved = Some((prim_id, Some(instance_id)));
                    break;
                }
            }
        }

        let Some((prim_id, hit_pane)) = resolved else { return false; };

        // Locked / non-text primitives never open the editor. The registry's
        // `supports_text` is the same gate the settings modal uses to show
        // its Text tab. Read out under the guard, drop it before the
        // selection writes below.
        let prim_info: Option<(bool, bool, String)> = {
            let guard = self.core.drawings();
            guard.primitives(bid).iter().find(|p| p.data().id == prim_id).map(|p| {
                let supports_text = mlc_core::drawing::PrimitiveRegistry::global()
                    .read()
                    .map(|r| r.supports_text(p.type_id()))
                    .unwrap_or(false);
                let seed = p.data().text.as_ref().map(|t| t.content.clone()).unwrap_or_default();
                (p.data().locked, supports_text, seed)
            })
        };
        let Some((locked, supports_text, seed)) = prim_info else { return false; };
        if locked || !supports_text {
            return false;
        }

        // Select so the inline toolbar and the settings modal agree with
        // what the editor is about — mirrors `try_wave_gesture`.
        self.blur_agent_focus_for_primitive_select();
        self.core.drawings_mut().set_current_pane(bid, hit_pane);
        self.core.drawings_mut().select(bid, prim_id);

        // Arm the shared TextFieldStore field, seeded + select-all — the
        // `start_oe_field_edit` / `start_prim_field_editing` contract.
        let wid = WidgetId::from(crate::text_input::PRIM_INLINE_TEXT);
        {
            let mut coord = self.input_coordinator.borrow_mut();
            coord.text_fields_mut().register(wid.clone(), TextFieldConfig::text());
            coord.text_fields_mut().set_text(&wid, &seed);
            coord.text_fields_mut().focus(wid.clone());
            if let Some(fs) = coord.text_fields_mut().field_state_mut(&wid) {
                fs.selection_start = Some(0);
            }
        }
        self.inline_prim_text_edit = Some(crate::InlinePrimTextEdit { bid, prim_id, pane: hit_pane });
        sglog!("[ChartApp] inline text edit started for primitive #{}", prim_id);
        true
    }

    /// Commit the active inline text edit: write the field's live text into
    /// `PrimitiveData::text.content` as ONE undo step (via
    /// `edit_primitive_recorded`, the same wrapper the settings modal's
    /// auto-commit uses), then blur + clear the edit state.
    ///
    /// Reached from Enter (`text_action_prim_inline_text`) and from
    /// click-away (`commit_inline_prim_text_edit_unless_inside`). Escape
    /// NEVER comes through here — see `cancel_inline_prim_text_edit`.
    pub(crate) fn commit_inline_prim_text_edit(&mut self) {
        let Some(edit) = self.inline_prim_text_edit.take() else { return };
        let wid = WidgetId::from(crate::text_input::PRIM_INLINE_TEXT);
        let text = self.input_coordinator.borrow().text_fields().text(&wid).to_string();
        self.input_coordinator.borrow_mut().text_fields_mut().blur();
        self.edit_primitive_recorded(edit.prim_id, |app| {
            app.write_inline_prim_text(edit.bid, edit.prim_id, &text)
        });
    }

    /// Cancel the active inline text edit WITHOUT writing — Escape's path.
    /// Clears state and blurs DIRECTLY, never routes through commit: Escape
    /// is the last-resort exit and must stay reachable even if a commit
    /// path is broken (the HARD INVARIANT documented on `on_escape`'s
    /// settings-modal editing arms).
    pub(crate) fn cancel_inline_prim_text_edit(&mut self) {
        if self.inline_prim_text_edit.take().is_some() {
            self.input_coordinator.borrow_mut().text_fields_mut().blur();
            sglog!("[ChartApp] inline text edit cancelled");
        }
    }

    /// Click-away commit, called at the top of `handle_canvas_click`: a
    /// canvas click OUTSIDE the editing box commits the edit (mirrors
    /// `start_oe_field_edit` committing the previous field before arming
    /// the next). A click INSIDE the box is caret placement, already
    /// handled by `TextFieldStore::on_drag_start` on the mousedown against
    /// the geometry the render hook pushes every frame.
    pub(crate) fn commit_inline_prim_text_edit_unless_inside(&mut self, x: f64, y: f64) {
        if self.inline_prim_text_edit.is_none() {
            return;
        }
        let wid = WidgetId::from(crate::text_input::PRIM_INLINE_TEXT);
        let inside = self.input_coordinator.borrow().text_fields()
            .field_state(&wid)
            .and_then(|s| s.last_rect)
            .map(|(rx, ry, rw, rh)| x >= rx && x <= rx + rw && y >= ry && y <= ry + rh)
            .unwrap_or(false);
        if !inside {
            self.commit_inline_prim_text_edit();
        }
    }

    /// The write half of `commit_inline_prim_text_edit`, mirroring the
    /// `text_content` arm of `commit_prim_editing_text_inner` — including
    /// its RwLock wedge fix: the `drawings()` read is bound to a plain
    /// `let` FIRST so the guard drops at the `;`, before `drawings_mut()`
    /// re-acquires the same lock (see that arm's own comment for the full
    /// wasm32 trap).
    fn write_inline_prim_text(&mut self, bid: mlc_core::BubbleId, prim_id: u64, text: &str) {
        let existing = self.core.drawings().get_data(bid, prim_id);
        if let Some(mut data) = existing {
            if let Some(ref mut t) = data.text {
                t.content = text.to_string();
            } else {
                use mlc_core::drawing::primitives_v2::PrimitiveText;
                data.text = Some(PrimitiveText {
                    content: text.to_string(),
                    ..PrimitiveText::default()
                });
            }
            self.core.drawings_mut().set_data(bid, prim_id, &data);
            self.sync_drawing_back_to_group();
            sglog!("[ChartApp] inline text committed for primitive #{}", prim_id);
        }
    }
}
