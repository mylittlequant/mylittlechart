//! The settings modal's **Admin** tab — renderer.
//!
//! State and logic live in `crate::ui::admin_tab`; this draws them. A strip of
//! four sections (Coupons, Grants, User, Invites) sits above a scrolling body, laid
//! out like the other settings tabs: `ToolbarTheme` tokens only, translated
//! strings from `AdminKey`, text inputs through the shared `TextFieldStore`
//! (geometry published as content items, exactly like the Account tab's
//! gift-code box).

use mlc_core::engine::render::RenderContext;
use mlc_core::i18n::{current_language, AdminKey, Language};
use uzor::input::Sense;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;

use super::user_settings::wrap_to_width;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::admin_tab::{
    batch_headers, batch_lines, invite_headers, invite_lines, ledger_lines, user_card_fields, AdminAction,
    AdminConfirm, AdminField, AdminSection, AdminState, AdminUserCard, Fill, LifetimeKind, DEFAULT_INVITE_DAYS,
    MAX_NOTE_CHARS,
};
use crate::ui::modal_settings::UserSettingsState;
use crate::ui::scroll_widget::ScrollableContainer;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::types::WidgetState;
use crate::ui::widgets::{auto_btn_width, draw_input, draw_input_cursor, fit_text_to_width, InputConfig, WidgetTheme};

/// Height of the section strip and of one form row.
const STRIP_H: f64 = 26.0;
const FIELD_H: f64 = 26.0;
const BTN_H: f64 = 24.0;
const GAP: f64 = 8.0;
const LINE_H: f64 = 15.0;
const FONT: &str = "11px sans-serif";
const FONT_BOLD: &str = "600 12px sans-serif";
const FONT_SMALL: &str = "10px sans-serif";
const FONT_CODE: &str = "12px monospace";

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tone {
    Neutral,
    Primary,
    Danger,
}

/// Everything a draw helper needs, so the section bodies read as layout and
/// not as parameter lists.
struct Ui<'a> {
    ctx: &'a mut dyn RenderContext,
    coord: &'a mut uzor::input::InputCoordinator,
    layer_id: &'a uzor::input::LayerId,
    result: &'a mut UserSettingsResult,
    tt: &'a ToolbarTheme,
    frame_theme: &'a FrameTheme,
    lang: Language,
    time_ms: u64,
    /// Vertical extent `(top, bottom)` of the visible body. Hit zones are
    /// registered only inside it, so a row scrolled out of view cannot take
    /// clicks meant for the section strip above.
    band: (f64, f64),
    /// A request is queued or in flight: actions draw dead.
    busy: bool,
}

impl Ui<'_> {
    /// `r` clipped to the visible band, or `None` when it is entirely outside.
    fn on_band(&self, r: &WidgetRect) -> Option<WidgetRect> {
        let top = r.y.max(self.band.0);
        let bottom = (r.y + r.height).min(self.band.1);
        (bottom > top).then(|| WidgetRect::new(r.x, top, r.width, bottom - top))
    }

    fn visible(&self, y: f64, h: f64) -> bool {
        y + h > self.band.0 && y < self.band.1
    }

    fn text(&mut self, s: &str, x: f64, y: f64, font: &str, color: &str) {
        self.ctx.set_font(font);
        self.ctx.set_fill_color(color);
        self.ctx.set_text_align(TextAlign::Left);
        self.ctx.set_text_baseline(TextBaseline::Top);
        self.ctx.fill_text(s, x, y);
    }

    fn text_right(&mut self, s: &str, right: f64, y: f64, font: &str, color: &str) {
        self.ctx.set_font(font);
        self.ctx.set_fill_color(color);
        self.ctx.set_text_align(TextAlign::Right);
        self.ctx.set_text_baseline(TextBaseline::Top);
        self.ctx.fill_text(s, right, y);
        self.ctx.set_text_align(TextAlign::Left);
    }

    /// `s` cut to `w` pixels with an ellipsis, in `font`.
    fn fitted(&mut self, s: &str, w: f64, font: &str) -> String {
        self.ctx.set_font(font);
        fit_text_to_width(&*self.ctx, s, w)
    }

    fn text_fit(&mut self, s: &str, x: f64, y: f64, w: f64, font: &str, color: &str) {
        let shown = self.fitted(s, w, font);
        self.text(&shown, x, y, font, color);
    }

    /// Word-wrapped paragraph; returns its height.
    fn paragraph(&mut self, s: &str, x: f64, y: f64, w: f64, color: &str) -> f64 {
        self.ctx.set_font(FONT);
        let lines = wrap_to_width(&*self.ctx, s, w);
        for (i, line) in lines.iter().enumerate() {
            let ly = y + i as f64 * LINE_H;
            if self.visible(ly, LINE_H) {
                self.text(line, x, ly, FONT, color);
            }
        }
        lines.len() as f64 * LINE_H
    }

    fn rule(&mut self, x: f64, y: f64, w: f64) {
        if self.visible(y, 1.0) {
            let color = mlc_core::apply_opacity(&self.tt.separator, 0.8);
            self.ctx.set_fill_color(&color);
            self.ctx.fill_rect(x, y, w, 1.0);
        }
    }

    fn btn_w(&mut self, label: &str) -> f64 {
        auto_btn_width(self.ctx, label, FONT, 10.0, 56.0)
    }

    fn button(&mut self, action: &AdminAction, label: &str, r: WidgetRect, tone: Tone, enabled: bool) {
        if !self.visible(r.y, r.height) {
            return;
        }
        let id = action.hit_id();
        let hovered = enabled && self.coord.is_hovered(&uzor::WidgetId::from(id.as_str()));
        let tt = self.tt;
        let (fill_c, stroke_c, text_c) = if !enabled {
            (
                tt.item_bg_hover.clone(),
                tt.separator.clone(),
                mlc_core::apply_opacity(&tt.item_text, 0.45),
            )
        } else {
            match tone {
                Tone::Neutral => (
                    if hovered { tt.item_bg_active.clone() } else { tt.item_bg_hover.clone() },
                    tt.separator.clone(),
                    tt.item_text.clone(),
                ),
                Tone::Primary => (
                    mlc_core::apply_opacity(&tt.success, if hovered { 0.32 } else { 0.2 }),
                    mlc_core::apply_opacity(&tt.success, 0.6),
                    tt.success.clone(),
                ),
                Tone::Danger => (
                    mlc_core::apply_opacity(&tt.danger, if hovered { 0.28 } else { 0.15 }),
                    mlc_core::apply_opacity(&tt.danger, 0.6),
                    tt.danger.clone(),
                ),
            }
        };
        self.ctx.set_fill_color(&fill_c);
        self.ctx.fill_rounded_rect(r.x, r.y, r.width, r.height, 3.0);
        self.ctx.set_stroke_color(&stroke_c);
        self.ctx.set_stroke_width(1.0);
        self.ctx.stroke_rounded_rect(r.x, r.y, r.width, r.height, 3.0);
        let shown = self.fitted(label, (r.width - 8.0).max(1.0), FONT);
        self.ctx.set_fill_color(&text_c);
        self.ctx.set_text_align(TextAlign::Center);
        self.ctx.set_text_baseline(TextBaseline::Middle);
        self.ctx.fill_text(&shown, r.center_x(), r.center_y());
        self.ctx.set_text_align(TextAlign::Left);
        if enabled {
            if let Some(hit) = self.on_band(&r) {
                self.coord.register_on_layer(id.as_str(), hit, Sense::CLICK | Sense::HOVER, self.layer_id);
            }
        }
    }

    /// A row of buttons starting at `x`, wrapping onto further rows when the
    /// next one would pass `x + w`. Returns the height used.
    fn button_row(&mut self, x: f64, y: f64, w: f64, items: &[(AdminAction, String, Tone, bool)]) -> f64 {
        let mut bx = x;
        let mut by = y;
        for (action, label, tone, enabled) in items {
            let bw = self.btn_w(label).min(w);
            if bx > x && bx + bw > x + w {
                bx = x;
                by += BTN_H + 6.0;
            }
            self.button(action, label, WidgetRect::new(bx, by, bw, BTN_H), *tone, *enabled);
            bx += bw + 6.0;
        }
        by + BTN_H - y
    }

    fn label(&mut self, s: &str, x: f64, y: f64) {
        let color = mlc_core::apply_opacity(&self.tt.item_text, 0.75);
        self.text(s, x, y, FONT, &color);
    }

    fn heading(&mut self, s: &str, x: f64, y: f64) -> f64 {
        let color = self.tt.item_text.clone();
        self.text(s, x, y, FONT_BOLD, &color);
        22.0
    }

    /// One text input, drawn like the Account tab's gift-code box, with its
    /// geometry published for the store.
    fn field(&mut self, field: AdminField, r: WidgetRect, placeholder: &str) {
        if !self.visible(r.y, r.height) {
            return;
        }
        let wid = uzor::WidgetId::from(field.id());
        let text = self.coord.text_fields().text(&wid).to_string();
        let cursor = self.coord.text_fields().cursor(&wid);
        let (sel_start, sel_end) = match self.coord.text_fields().selection_range(&wid) {
            Some((lo, hi)) => (Some(lo), Some(hi)),
            None => (None, None),
        };
        let engaged = shows_live_field(self.coord.text_fields(), &wid);
        let widget_theme: WidgetTheme = toolbar_to_widget_theme(self.tt, self.frame_theme);
        let cfg = InputConfig::new(&text)
            .with_focused(engaged)
            .with_cursor(cursor)
            .with_selection(sel_start, sel_end);
        let cfg = if engaged { cfg } else { cfg.with_placeholder(placeholder) };
        let drawn = draw_input(self.ctx, &cfg, WidgetState::Normal, r, &widget_theme);
        if engaged && self.coord.text_fields().cursor_visible(self.time_ms) {
            let color = self.tt.item_text.clone();
            draw_input_cursor(self.ctx, drawn.cursor_x, drawn.cursor_y, drawn.cursor_height, &color);
        }
        if let Some(hit) = self.on_band(&r) {
            let key = field.content_key();
            self.result.content_items.push((key.clone(), hit));
            self.result.input_char_positions.push((key.clone(), drawn.char_x_positions));
            self.coord.register_on_layer(key.as_str(), hit, Sense::CLICK, self.layer_id);
        }
    }

    /// The status line under a form; returns its height (0 when empty).
    fn notice(&mut self, admin: &AdminState, x: f64, y: f64, w: f64) -> f64 {
        let Some(notice) = &admin.notice else { return 0.0 };
        let color = if notice.is_error() { self.tt.danger.clone() } else { self.tt.success.clone() };
        let text = notice.text(self.lang);
        self.paragraph(&text, x, y, w, &color) + 8.0
    }

    /// The question and Confirm / Cancel for the armed action; returns its
    /// height.
    fn confirm_bar(&mut self, confirm: &AdminConfirm, x: f64, y: f64, w: f64) -> f64 {
        let text = confirm.prompt(self.lang);
        let color = self.tt.item_text.clone();
        let h = self.paragraph(&text, x, y, w, &color);
        let by = y + h + 4.0;
        let yes = AdminKey::BtnConfirm.get(self.lang).to_string();
        let no = AdminKey::BtnCancel.get(self.lang).to_string();
        let busy = self.busy;
        let used = self.button_row(
            x,
            by,
            w,
            &[
                (AdminAction::ConfirmYes, yes, Tone::Danger, !busy),
                (AdminAction::ConfirmNo, no, Tone::Neutral, true),
            ],
        );
        h + 4.0 + used + 10.0
    }
}

/// Draw the Admin tab into `viewport`.
pub(super) fn render_admin_tab(
    ctx: &mut dyn RenderContext,
    viewport: WidgetRect,
    state: &UserSettingsState,
    tt: &ToolbarTheme,
    frame_theme: &FrameTheme,
    scroll_widget_theme: &WidgetTheme,
    current_time_ms: u64,
    coord: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let lang = current_language();
    let admin = &state.admin;

    // ── Section strip (fixed above the scrolling body) ───────────────────
    let body_top = viewport.y + STRIP_H + GAP;
    let body = WidgetRect::new(viewport.x, body_top, viewport.width, (viewport.height - STRIP_H - GAP).max(1.0));
    {
        let mut ui = Ui {
            ctx: &mut *ctx,
            coord: &mut *coord,
            layer_id,
            result: &mut *result,
            tt,
            frame_theme,
            lang,
            time_ms: current_time_ms,
            band: (viewport.y, viewport.y + viewport.height),
            busy: admin.busy,
        };
        draw_strip(&mut ui, admin, viewport);
    }

    // ── Body ─────────────────────────────────────────────────────────────
    let container = ScrollableContainer::new(body, &admin.scroll, None);
    container.begin(ctx);
    let x = body.x;
    let cw = container.content_width();
    let top = container.content_y();
    let end_y = {
        let mut ui = Ui {
            ctx: &mut *ctx,
            coord: &mut *coord,
            layer_id,
            result: &mut *result,
            tt,
            frame_theme,
            lang,
            time_ms: current_time_ms,
            band: (body.y, body.y + body.height),
            busy: admin.busy,
        };
        match admin.section {
            AdminSection::Coupons => coupons(&mut ui, admin, x, top, cw),
            AdminSection::Grants => grants(&mut ui, admin, x, top, cw),
            AdminSection::User => user(&mut ui, admin, x, top, cw),
            AdminSection::Invites => invites(&mut ui, admin, x, top, cw),
        }
    };
    let scroll_result = container.end(ctx, end_y - top, scroll_widget_theme);
    result.scroll_viewport_rect = Some(body);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        coord.register_on_layer("user_settings:scrollbar_handle", inflated, Sense::DRAG, layer_id);
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        coord.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        coord.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            Sense::SCROLL,
            layer_id,
        );
    }
}

/// Whether `id` is the ONE field that draws its focus ring and caret: the
/// user touched it AND the keyboard is still there. `is_engaged` alone is not
/// enough: the store clears the flag of the field being focused, never of the
/// one focus leaves, so after clicking through the three mint boxes all three
/// stay "engaged" and all three drew the ring at once.
fn shows_live_field(fields: &uzor::input::TextFieldStore, id: &uzor::WidgetId) -> bool {
    fields.is_engaged(id) && fields.is_focused(id)
}

/// The section buttons, with the working / loading indicator at the
/// right end.
fn draw_strip(ui: &mut Ui<'_>, admin: &AdminState, viewport: WidgetRect) {
    let tt = ui.tt;
    let count = AdminSection::ALL.len() as f64;
    let seg_gap = 6.0;
    let indicator = if admin.busy { AdminKey::Working.get(ui.lang) } else { "" };
    let indicator_w = if indicator.is_empty() {
        0.0
    } else {
        ui.ctx.set_font(FONT);
        ui.ctx.measure_text(indicator) + GAP
    };
    let strip_w = (viewport.width - indicator_w).max(count * 40.0);
    let seg_w = ((strip_w - seg_gap * (count - 1.0)) / count).max(1.0);
    for (i, section) in AdminSection::ALL.iter().enumerate() {
        let sx = viewport.x + i as f64 * (seg_w + seg_gap);
        let action = AdminAction::Section(*section);
        let id = action.hit_id();
        let active = admin.section == *section;
        let hovered = ui.coord.is_hovered(&uzor::WidgetId::from(id.as_str()));
        let bg = if active {
            &tt.item_bg_active
        } else if hovered {
            &tt.item_bg_hover
        } else {
            &tt.dropdown_bg
        };
        ui.ctx.set_fill_color(bg);
        ui.ctx.fill_rounded_rect(sx, viewport.y, seg_w, STRIP_H, 4.0);
        ui.ctx.set_stroke_color(&tt.separator);
        ui.ctx.set_stroke_width(1.0);
        ui.ctx.stroke_rounded_rect(sx, viewport.y, seg_w, STRIP_H, 4.0);
        let label = ui.fitted(section.label(ui.lang), (seg_w - 8.0).max(1.0), FONT);
        ui.ctx.set_fill_color(if active { &tt.item_text_active } else { &tt.item_text });
        ui.ctx.set_text_align(TextAlign::Center);
        ui.ctx.set_text_baseline(TextBaseline::Middle);
        ui.ctx.fill_text(&label, sx + seg_w / 2.0, viewport.y + STRIP_H / 2.0);
        ui.ctx.set_text_align(TextAlign::Left);
        ui.coord.register_on_layer(
            id.as_str(),
            WidgetRect::new(sx, viewport.y, seg_w, STRIP_H),
            Sense::CLICK | Sense::HOVER,
            ui.layer_id,
        );
    }
    if !indicator.is_empty() {
        let color = mlc_core::apply_opacity(&tt.item_text, 0.6);
        ui.text_right(indicator, viewport.x + viewport.width, viewport.y + 7.0, FONT, &color);
    }
}

// =============================================================================
// Coupons
// =============================================================================

fn coupons(ui: &mut Ui<'_>, admin: &AdminState, x: f64, mut cy: f64, cw: f64) -> f64 {
    let lang = ui.lang;
    let busy = ui.busy;

    // ── Mint form ────────────────────────────────────────────────────────
    cy += ui.heading(AdminKey::MintTitle.get(lang), x, cy);
    let days_w = 72.0;
    let count_w = 72.0;
    let mint_label = AdminKey::BtnMint.get(lang);
    let mint_w = ui.btn_w(mint_label).min(cw * 0.35);
    let note_w = (cw - days_w - count_w - mint_w - GAP * 3.0).max(80.0);
    let days_x = x;
    let count_x = days_x + days_w + GAP;
    let note_x = count_x + count_w + GAP;
    ui.label(AdminKey::LabelDays.get(lang), days_x, cy);
    ui.label(AdminKey::LabelCount.get(lang), count_x, cy);
    ui.label(AdminKey::LabelNote.get(lang), note_x, cy);
    cy += LINE_H;
    let note_hint = Fill::new(AdminKey::PlaceholderNote.get(lang)).arg("max", MAX_NOTE_CHARS).done();
    ui.field(AdminField::MintDays, WidgetRect::new(days_x, cy, days_w, FIELD_H), "30");
    ui.field(AdminField::MintCount, WidgetRect::new(count_x, cy, count_w, FIELD_H), "10");
    ui.field(AdminField::MintNote, WidgetRect::new(note_x, cy, note_w, FIELD_H), &note_hint);
    ui.button(
        &AdminAction::Mint,
        mint_label,
        WidgetRect::new(note_x + note_w + GAP, cy, mint_w, FIELD_H),
        Tone::Primary,
        !busy,
    );
    cy += FIELD_H + 8.0;
    cy += ui.notice(admin, x, cy, cw);

    // ── Freshly minted codes (shown once) ────────────────────────────────
    if let Some(minted) = &admin.minted {
        cy += 4.0;
        let pad = 10.0;
        let warn = ui.tt.warning.clone();
        ui.ctx.set_font(FONT);
        let lines = wrap_to_width(&*ui.ctx, AdminKey::MintedShownOnce.get(lang), (cw - pad * 2.0).max(40.0));
        let row_h = 26.0;
        let panel_h = pad + lines.len() as f64 * LINE_H + 6.0 + BTN_H + 8.0 + minted.codes.len() as f64 * row_h + pad;
        if ui.visible(cy, panel_h) {
            ui.ctx.set_fill_color(&mlc_core::apply_opacity(&warn, 0.1));
            ui.ctx.fill_rounded_rect(x, cy, cw, panel_h, 6.0);
            ui.ctx.set_stroke_color(&mlc_core::apply_opacity(&warn, 0.55));
            ui.ctx.set_stroke_width(1.0);
            ui.ctx.stroke_rounded_rect(x, cy, cw, panel_h, 6.0);
        }
        let mut py = cy + pad;
        for line in &lines {
            if ui.visible(py, LINE_H) {
                ui.text(line, x + pad, py, FONT, &warn);
            }
            py += LINE_H;
        }
        py += 6.0;
        let copy_all = AdminKey::BtnCopyAll.get(lang).to_string();
        let done = AdminKey::BtnDone.get(lang).to_string();
        ui.button_row(
            x + pad,
            py,
            cw - pad * 2.0,
            &[
                (AdminAction::CopyAll, copy_all, Tone::Primary, true),
                (AdminAction::DismissCodes, done, Tone::Neutral, true),
            ],
        );
        py += BTN_H + 8.0;
        let copy = AdminKey::BtnCopy.get(lang);
        let copy_w = ui.btn_w(copy);
        let item_text = ui.tt.item_text.clone();
        for (i, code) in minted.codes.iter().enumerate() {
            if ui.visible(py, row_h) {
                ui.text(code, x + pad, py + 6.0, FONT_CODE, &item_text);
                ui.button(
                    &AdminAction::CopyOne(i),
                    copy,
                    WidgetRect::new(x + cw - pad - copy_w, py + 1.0, copy_w, BTN_H),
                    Tone::Neutral,
                    true,
                );
            }
            py += row_h;
        }
        cy += panel_h + 12.0;
    }

    // ── Batches table ────────────────────────────────────────────────────
    cy += 6.0;
    ui.heading(AdminKey::BatchesTitle.get(lang), x, cy + 3.0);
    let refresh = AdminKey::BtnRefresh.get(lang);
    let refresh_w = ui.btn_w(refresh);
    ui.button(
        &AdminAction::RefreshBatches,
        refresh,
        WidgetRect::new(x + cw - refresh_w, cy, refresh_w, BTN_H),
        Tone::Neutral,
        !busy,
    );
    cy += BTN_H + 8.0;

    let muted = mlc_core::apply_opacity(&ui.tt.item_text, 0.6);
    let item_text = ui.tt.item_text.clone();
    match &admin.batches {
        None => {
            let msg = if busy { AdminKey::Loading.get(lang) } else { "" };
            if !msg.is_empty() {
                ui.text(msg, x, cy, FONT, &muted);
                cy += 22.0;
            }
        }
        Some(rows) if rows.is_empty() => {
            ui.text(AdminKey::BatchesEmpty.get(lang), x, cy, FONT, &muted);
            cy += 22.0;
        }
        Some(rows) => {
            let action_w = 72.0;
            let table_w = (cw - action_w).max(120.0);
            let fractions = [0.30, 0.08, 0.14, 0.12, 0.16, 0.20];
            let mut col_x = [x; 6];
            let mut col_w = [0.0; 6];
            let mut acc = x;
            for i in 0..6 {
                col_x[i] = acc;
                col_w[i] = table_w * fractions[i];
                acc += col_w[i];
            }
            for (i, header) in batch_headers(lang).iter().enumerate() {
                ui.text_fit(header, col_x[i], cy, (col_w[i] - 4.0).max(1.0), FONT_SMALL, &muted);
            }
            cy += 16.0;
            ui.rule(x, cy, cw);
            cy += 4.0;
            let revoke = AdminKey::BtnRevoke.get(lang);
            let armed = match &admin.confirm {
                Some(AdminConfirm::RevokeBatch { batch_id }) => Some(batch_id.as_str()),
                _ => None,
            };
            for line in batch_lines(rows) {
                let row_y = cy;
                let cells = [&line.note, &line.days, &line.redeemed, &line.revoked, &line.created, &line.by];
                for (i, cell) in cells.iter().enumerate() {
                    if ui.visible(row_y, 20.0) {
                        ui.text_fit(cell, col_x[i], row_y + 4.0, (col_w[i] - 4.0).max(1.0), FONT, &item_text);
                    }
                }
                ui.button(
                    &AdminAction::RevokeBatch(line.batch_id.clone()),
                    revoke,
                    WidgetRect::new(x + cw - action_w + 8.0, row_y, action_w - 8.0, 20.0),
                    Tone::Danger,
                    !busy && line.revocable,
                );
                cy += 24.0;
                if armed == Some(line.batch_id.as_str()) {
                    if let Some(confirm) = &admin.confirm {
                        cy += ui.confirm_bar(confirm, x, cy, cw);
                    }
                }
                ui.rule(x, cy - 2.0, cw);
            }
        }
    }
    cy + 12.0
}

// =============================================================================
// Invites
// =============================================================================

/// One row: "Login challenge" and the On / Off buttons. The button matching
/// the loaded flag is drawn as the current one; both stay clickable so the
/// tone stays visible (a same-value click does not send).
fn login_challenge_row(ui: &mut Ui<'_>, admin: &AdminState, x: f64, cy: f64, cw: f64) -> f64 {
    let lang = ui.lang;
    let on_label = AdminKey::ChallengeOn.get(lang);
    let off_label = AdminKey::ChallengeOff.get(lang);
    let on_w = ui.btn_w(on_label);
    let off_w = ui.btn_w(off_label);
    let off_x = x + cw - off_w;
    let on_x = off_x - GAP - on_w;
    let label_w = (on_x - x - GAP).max(1.0);
    let color = ui.tt.item_text.clone();
    ui.text_fit(AdminKey::LoginChallenge.get(lang), x, cy + 5.0, label_w, FONT, &color);
    let current = admin.login_challenge;
    let enabled = !ui.busy;
    ui.button(
        &AdminAction::SetLoginChallenge(true),
        on_label,
        WidgetRect::new(on_x, cy, on_w, BTN_H),
        if current == Some(true) { Tone::Primary } else { Tone::Neutral },
        enabled,
    );
    ui.button(
        &AdminAction::SetLoginChallenge(false),
        off_label,
        WidgetRect::new(off_x, cy, off_w, BTN_H),
        if current == Some(false) { Tone::Primary } else { Tone::Neutral },
        enabled,
    );
    cy + BTN_H + 12.0
}

fn invites(ui: &mut Ui<'_>, admin: &AdminState, x: f64, mut cy: f64, cw: f64) -> f64 {
    let lang = ui.lang;
    let busy = ui.busy;

    cy = login_challenge_row(ui, admin, x, cy, cw);

    // ── Issue form ───────────────────────────────────────────────────────
    cy += ui.heading(AdminKey::InviteTitle.get(lang), x, cy);
    let days_w = 72.0;
    let issue_label = AdminKey::BtnIssueInvite.get(lang);
    let issue_w = ui.btn_w(issue_label).min(cw * 0.35);
    let free_w = (cw - days_w - issue_w - GAP * 3.0).max(160.0);
    let nick_w = free_w * 0.4;
    let note_w = free_w - nick_w;
    let nick_x = x;
    let days_x = nick_x + nick_w + GAP;
    let note_x = days_x + days_w + GAP;
    ui.label(AdminKey::LabelNick.get(lang), nick_x, cy);
    ui.label(AdminKey::LabelDays.get(lang), days_x, cy);
    ui.label(AdminKey::LabelNote.get(lang), note_x, cy);
    cy += LINE_H;
    let note_hint = Fill::new(AdminKey::PlaceholderNote.get(lang)).arg("max", MAX_NOTE_CHARS).done();
    ui.field(
        AdminField::InviteNick,
        WidgetRect::new(nick_x, cy, nick_w, FIELD_H),
        AdminKey::PlaceholderInviteNick.get(lang),
    );
    ui.field(
        AdminField::InviteDays,
        WidgetRect::new(days_x, cy, days_w, FIELD_H),
        &DEFAULT_INVITE_DAYS.to_string(),
    );
    ui.field(AdminField::InviteNote, WidgetRect::new(note_x, cy, note_w, FIELD_H), &note_hint);
    ui.button(
        &AdminAction::IssueInvite,
        issue_label,
        WidgetRect::new(note_x + note_w + GAP, cy, issue_w, FIELD_H),
        Tone::Primary,
        !busy,
    );
    cy += FIELD_H + 8.0;
    cy += ui.notice(admin, x, cy, cw);

    // ── Freshly issued code (shown once) ─────────────────────────────────
    if let Some(issued) = &admin.issued_invite {
        cy += 4.0;
        let pad = 10.0;
        let warn = ui.tt.warning.clone();
        ui.ctx.set_font(FONT);
        let lines = wrap_to_width(&*ui.ctx, AdminKey::InviteShownOnce.get(lang), (cw - pad * 2.0).max(40.0));
        let row_h = 26.0;
        let panel_h = pad + lines.len() as f64 * LINE_H + 6.0 + row_h + 8.0 + BTN_H + pad;
        if ui.visible(cy, panel_h) {
            ui.ctx.set_fill_color(&mlc_core::apply_opacity(&warn, 0.1));
            ui.ctx.fill_rounded_rect(x, cy, cw, panel_h, 6.0);
            ui.ctx.set_stroke_color(&mlc_core::apply_opacity(&warn, 0.55));
            ui.ctx.set_stroke_width(1.0);
            ui.ctx.stroke_rounded_rect(x, cy, cw, panel_h, 6.0);
        }
        let mut py = cy + pad;
        for line in &lines {
            if ui.visible(py, LINE_H) {
                ui.text(line, x + pad, py, FONT, &warn);
            }
            py += LINE_H;
        }
        py += 6.0;
        let item_text = ui.tt.item_text.clone();
        if ui.visible(py, row_h) {
            ui.text_fit(&issued.code, x + pad, py + 6.0, (cw - pad * 2.0).max(40.0), FONT_CODE, &item_text);
        }
        py += row_h + 8.0;
        let copy = AdminKey::BtnCopy.get(lang).to_string();
        let done = AdminKey::BtnDone.get(lang).to_string();
        ui.button_row(
            x + pad,
            py,
            cw - pad * 2.0,
            &[
                (AdminAction::CopyInvite, copy, Tone::Primary, true),
                (AdminAction::DismissInvite, done, Tone::Neutral, true),
            ],
        );
        cy += panel_h + 12.0;
    }

    // ── Invites table ────────────────────────────────────────────────────
    cy += 6.0;
    ui.heading(AdminKey::SectionInvites.get(lang), x, cy + 3.0);
    let refresh = AdminKey::BtnRefresh.get(lang);
    let refresh_w = ui.btn_w(refresh);
    ui.button(
        &AdminAction::RefreshInvites,
        refresh,
        WidgetRect::new(x + cw - refresh_w, cy, refresh_w, BTN_H),
        Tone::Neutral,
        !busy,
    );
    cy += BTN_H + 8.0;

    let muted = mlc_core::apply_opacity(&ui.tt.item_text, 0.6);
    let item_text = ui.tt.item_text.clone();
    match &admin.invites {
        None => {
            if busy {
                ui.text(AdminKey::Loading.get(lang), x, cy, FONT, &muted);
                cy += 22.0;
            }
        }
        Some(rows) if rows.is_empty() => {
            ui.text(AdminKey::InvitesEmpty.get(lang), x, cy, FONT, &muted);
            cy += 22.0;
        }
        Some(rows) => {
            let action_w = 72.0;
            let table_w = (cw - action_w).max(120.0);
            let fractions = [0.24, 0.10, 0.18, 0.18, 0.30];
            let mut col_x = [x; 5];
            let mut col_w = [0.0; 5];
            let mut acc = x;
            for i in 0..5 {
                col_x[i] = acc;
                col_w[i] = table_w * fractions[i];
                acc += col_w[i];
            }
            for (i, header) in invite_headers(lang).iter().enumerate() {
                ui.text_fit(header, col_x[i], cy, (col_w[i] - 4.0).max(1.0), FONT_SMALL, &muted);
            }
            cy += 16.0;
            ui.rule(x, cy, cw);
            cy += 4.0;
            let revoke = AdminKey::BtnRevoke.get(lang);
            let armed = match &admin.confirm {
                Some(AdminConfirm::RevokeInvite { invite_id, .. }) => Some(invite_id.as_str()),
                _ => None,
            };
            for line in invite_lines(rows, lang) {
                let row_y = cy;
                let cells = [line.nick.as_str(), line.days.as_str(), line.created.as_str(), line.state, line.note.as_str()];
                for (i, cell) in cells.iter().enumerate() {
                    if ui.visible(row_y, 20.0) {
                        ui.text_fit(cell, col_x[i], row_y + 4.0, (col_w[i] - 4.0).max(1.0), FONT, &item_text);
                    }
                }
                ui.button(
                    &AdminAction::RevokeInvite(line.id.clone()),
                    revoke,
                    WidgetRect::new(x + cw - action_w + 8.0, row_y, action_w - 8.0, 20.0),
                    Tone::Danger,
                    !busy && line.revocable,
                );
                cy += 24.0;
                if armed == Some(line.id.as_str()) {
                    if let Some(confirm) = &admin.confirm {
                        cy += ui.confirm_bar(confirm, x, cy, cw);
                    }
                }
                ui.rule(x, cy - 2.0, cw);
            }
        }
    }
    cy + 12.0
}

// =============================================================================
// Grants
// =============================================================================

fn grants(ui: &mut Ui<'_>, admin: &AdminState, x: f64, mut cy: f64, cw: f64) -> f64 {
    let lang = ui.lang;
    let busy = ui.busy;
    let nick_w = cw.min(280.0);

    ui.label(AdminKey::LabelNick.get(lang), x, cy);
    cy += LINE_H;
    ui.field(
        AdminField::GrantNick,
        WidgetRect::new(x, cy, nick_w, FIELD_H),
        AdminKey::PlaceholderNick.get(lang),
    );
    cy += FIELD_H + 14.0;

    // ── Paid days ────────────────────────────────────────────────────────
    cy += ui.heading(AdminKey::PaidDaysTitle.get(lang), x, cy);
    ui.label(AdminKey::LabelDays.get(lang), x, cy);
    cy += LINE_H;
    let days_w = 72.0;
    ui.field(AdminField::GrantDays, WidgetRect::new(x, cy, days_w, FIELD_H), "30");
    let add = AdminKey::BtnAddPaidDays.get(lang);
    let add_w = ui.btn_w(add);
    ui.button(
        &AdminAction::PaidDays,
        add,
        WidgetRect::new(x + days_w + GAP, cy, add_w, FIELD_H),
        Tone::Primary,
        !busy,
    );
    cy += FIELD_H + 16.0;

    // ── Lifetime ─────────────────────────────────────────────────────────
    cy += ui.heading(AdminKey::LifetimeTitle.get(lang), x, cy);
    ui.field(
        AdminField::GrantNote,
        WidgetRect::new(x, cy, cw, FIELD_H),
        AdminKey::PlaceholderLifetimeNote.get(lang),
    );
    cy += FIELD_H + 8.0;
    let grant_paid = AdminKey::BtnGrantLifetimePaid.get(lang).to_string();
    let grant_anon = AdminKey::BtnGrantLifetimeAnon.get(lang).to_string();
    cy += ui.button_row(
        x,
        cy,
        cw,
        &[
            (AdminAction::LifetimeGrant(LifetimeKind::Paid), grant_paid, Tone::Primary, !busy),
            (AdminAction::LifetimeGrant(LifetimeKind::Anon), grant_anon, Tone::Primary, !busy),
        ],
    ) + 6.0;
    let revoke_paid = AdminKey::BtnRevokeLifetimePaid.get(lang).to_string();
    let revoke_anon = AdminKey::BtnRevokeLifetimeAnon.get(lang).to_string();
    cy += ui.button_row(
        x,
        cy,
        cw,
        &[
            (AdminAction::LifetimeRevoke(LifetimeKind::Paid), revoke_paid, Tone::Danger, !busy),
            (AdminAction::LifetimeRevoke(LifetimeKind::Anon), revoke_anon, Tone::Danger, !busy),
        ],
    ) + 8.0;
    let muted = mlc_core::apply_opacity(&ui.tt.item_text, 0.6);
    cy += ui.paragraph(AdminKey::HintRevokeAdminOnly.get(lang), x, cy, cw, &muted) + 10.0;

    // ── Armed action and result ──────────────────────────────────────────
    if let Some(confirm) = &admin.confirm {
        if !matches!(confirm, AdminConfirm::RevokeBatch { .. }) {
            cy += ui.confirm_bar(confirm, x, cy, cw);
        }
    }
    cy += ui.notice(admin, x, cy, cw);
    cy + 12.0
}

// =============================================================================
// User
// =============================================================================

fn user(ui: &mut Ui<'_>, admin: &AdminState, x: f64, mut cy: f64, cw: f64) -> f64 {
    let lang = ui.lang;
    let busy = ui.busy;

    ui.label(AdminKey::LabelNick.get(lang), x, cy);
    cy += LINE_H;
    let look = AdminKey::BtnLookup.get(lang);
    let look_w = ui.btn_w(look);
    let field_w = (cw - look_w - GAP).clamp(80.0, 320.0);
    ui.field(
        AdminField::UserNick,
        WidgetRect::new(x, cy, field_w, FIELD_H),
        AdminKey::PlaceholderNick.get(lang),
    );
    ui.button(
        &AdminAction::Lookup,
        look,
        WidgetRect::new(x + field_w + GAP, cy, look_w, FIELD_H),
        Tone::Primary,
        !busy,
    );
    cy += FIELD_H + 8.0;
    cy += ui.notice(admin, x, cy, cw);
    if let Some(card) = &admin.user {
        cy += user_card(ui, card, x, cy, cw);
    }
    cy + 12.0
}

/// The account card: labelled facts, then the lifetime-grant ledger.
fn user_card(ui: &mut Ui<'_>, card: &AdminUserCard, x: f64, y: f64, cw: f64) -> f64 {
    let lang = ui.lang;
    let pad = 10.0;
    let item_text = ui.tt.item_text.clone();
    let muted = mlc_core::apply_opacity(&ui.tt.item_text, 0.6);
    let fields = user_card_fields(card, lang);
    let ledger = ledger_lines(card, lang);
    let field_row = 18.0;
    let ledger_row = 34.0;
    let ledger_h = if ledger.is_empty() { 20.0 } else { 16.0 + ledger.len() as f64 * ledger_row };
    let panel_h = pad + fields.len() as f64 * field_row + 12.0 + 20.0 + ledger_h + pad;

    if ui.visible(y, panel_h) {
        ui.ctx.set_fill_color(&mlc_core::apply_opacity(&ui.tt.item_bg_hover, 0.5));
        ui.ctx.fill_rounded_rect(x, y, cw, panel_h, 6.0);
        ui.ctx.set_stroke_color(&ui.tt.separator);
        ui.ctx.set_stroke_width(1.0);
        ui.ctx.stroke_rounded_rect(x, y, cw, panel_h, 6.0);
    }

    let label_w = (cw * 0.34).clamp(110.0, 190.0);
    let mut py = y + pad;
    for f in &fields {
        if ui.visible(py, field_row) {
            ui.text_fit(f.label, x + pad, py, label_w - 6.0, FONT, &muted);
            ui.text_fit(&f.value, x + pad + label_w, py, (cw - pad * 2.0 - label_w).max(1.0), FONT, &item_text);
        }
        py += field_row;
    }
    py += 12.0;
    ui.heading(AdminKey::CardGrantsTitle.get(lang), x + pad, py);
    py += 20.0;
    if ledger.is_empty() {
        ui.text(AdminKey::CardNoGrants.get(lang), x + pad, py, FONT, &muted);
    } else {
        let inner = cw - pad * 2.0;
        let fractions = [0.30, 0.16, 0.27, 0.27];
        let headers = [
            AdminKey::ColGrant.get(lang),
            AdminKey::ColSource.get(lang),
            AdminKey::ColCreated.get(lang),
            AdminKey::ColRevoked.get(lang),
        ];
        let mut col_x = [x + pad; 4];
        let mut col_w = [0.0; 4];
        let mut acc = x + pad;
        for i in 0..4 {
            col_x[i] = acc;
            col_w[i] = inner * fractions[i];
            acc += col_w[i];
        }
        for (i, header) in headers.iter().enumerate() {
            ui.text_fit(header, col_x[i], py, (col_w[i] - 4.0).max(1.0), FONT_SMALL, &muted);
        }
        py += 16.0;
        for line in &ledger {
            let cells = [&line.grant, &line.source, &line.created, &line.revoked];
            for (i, cell) in cells.iter().enumerate() {
                if ui.visible(py, ledger_row) {
                    ui.text_fit(cell, col_x[i], py, (col_w[i] - 4.0).max(1.0), FONT, &item_text);
                }
            }
            if !line.reference.is_empty() && ui.visible(py, ledger_row) {
                ui.text_fit(&line.reference, col_x[0], py + 15.0, inner, FONT_SMALL, &muted);
            }
            py += ledger_row;
        }
    }
    panel_h + 8.0
}

#[cfg(test)]
mod tests {
    //! Drives the real renderer against a recording context: the tab draws, its
    //! rows carry the data, and nothing outside an admin's view leaks in.

    use super::*;
    use crate::layout::modals::user_settings::render_user_settings_modal;
    use crate::ui::admin_tab::{
        AdminBatchRow, AdminLedgerRow, AdminMinted, AdminOutcome, AdminUserCard, LifetimeKind,
    };
    use crate::ui::modal_settings::{CredentialsState, UserSettingsTab};

    #[derive(Default)]
    struct Rec {
        texts: Vec<String>,
    }

    impl uzor::render::Painter for Rec {
        fn save(&mut self) {}
        fn restore(&mut self) {}
        fn translate(&mut self, _x: f64, _y: f64) {}
        fn rotate(&mut self, _angle: f64) {}
        fn scale(&mut self, _x: f64, _y: f64) {}
        fn set_fill_color(&mut self, _color: &str) {}
        fn set_global_alpha(&mut self, _alpha: f64) {}
        fn set_stroke_color(&mut self, _color: &str) {}
        fn set_stroke_width(&mut self, _width: f64) {}
        fn set_line_dash(&mut self, _pattern: &[f64]) {}
        fn set_line_cap(&mut self, _cap: &str) {}
        fn set_line_join(&mut self, _join: &str) {}
        fn begin_path(&mut self) {}
        fn move_to(&mut self, _x: f64, _y: f64) {}
        fn line_to(&mut self, _x: f64, _y: f64) {}
        fn close_path(&mut self) {}
        fn rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        fn arc(&mut self, _cx: f64, _cy: f64, _r: f64, _s: f64, _e: f64) {}
        fn ellipse(&mut self, _cx: f64, _cy: f64, _rx: f64, _ry: f64, _rot: f64, _s: f64, _e: f64) {}
        fn quadratic_curve_to(&mut self, _cpx: f64, _cpy: f64, _x: f64, _y: f64) {}
        fn bezier_curve_to(&mut self, _a: f64, _b: f64, _c: f64, _d: f64, _x: f64, _y: f64) {}
        fn stroke(&mut self) {}
        fn fill(&mut self) {}
    }
    impl uzor::render::TextRenderer for Rec {
        fn set_font(&mut self, _font: &str) {}
        fn set_text_align(&mut self, _align: uzor::render::TextAlign) {}
        fn set_text_baseline(&mut self, _baseline: uzor::render::TextBaseline) {}
        fn fill_text(&mut self, text: &str, _x: f64, _y: f64) {
            self.texts.push(text.to_string());
        }
        fn stroke_text(&mut self, _text: &str, _x: f64, _y: f64) {}
    }
    impl uzor::render::TextMetrics for Rec {
        fn measure_text(&self, text: &str) -> f64 {
            text.chars().count() as f64 * 6.0
        }
        fn text_bounds(&self, _text: &str, _font: &str) -> uzor::render::TextBounds {
            uzor::render::TextBounds { x: 0.0, y: 0.0, w: 0.0, h: 0.0, ascent: 0.0, descent: 0.0 }
        }
    }
    impl uzor::render::Masking for Rec {
        fn clip(&mut self) {}
    }
    impl uzor::render::Effects for Rec {}
    impl uzor::render::ShapeHelpers for Rec {
        fn fill_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
        fn stroke_rect(&mut self, _x: f64, _y: f64, _w: f64, _h: f64) {}
    }
    impl uzor::render::GradientPainter for Rec {}
    impl uzor::render::UiEffectHelpers for Rec {}
    impl uzor::render::BatchPainter for Rec {}
    impl uzor::render::RenderContext for Rec {
        fn dpr(&self) -> f64 {
            1.0
        }
    }
    impl RenderContext for Rec {
        fn chart_width(&self) -> f64 {
            1000.0
        }
        fn chart_height(&self) -> f64 {
            600.0
        }
        fn bar_to_x(&self, bar: f64) -> f64 {
            bar
        }
        fn price_to_y(&self, price: f64) -> f64 {
            price
        }
        fn bars(&self) -> &[mlc_core::Bar] {
            &[]
        }
        fn set_coordinate_space(&mut self, _w: f64, _h: f64, _vs: f64, _bs: f64, _pmin: f64, _pmax: f64) {}
    }

    fn admin_state() -> UserSettingsState {
        let mut st = UserSettingsState::default();
        st.is_open = true;
        st.account_signed_in = true;
        st.account_roles = vec!["admin".to_string()];
        st.admin.served = true;
        st.active_tab = UserSettingsTab::Admin;
        st
    }

    fn render(st: &UserSettingsState) -> (Vec<String>, UserSettingsResult) {
        let mut ctx = Rec::default();
        let mut coord = uzor::input::InputCoordinator::new();
        let result = render_user_settings_modal(
            &mut ctx,
            1280.0,
            800.0,
            st,
            &CredentialsState::default(),
            &FrameTheme::default(),
            &ToolbarTheme::default(),
            0,
            &mut coord,
            false,
        );
        (ctx.texts, result)
    }

    fn has(texts: &[String], needle: &str) -> bool {
        texts.iter().any(|t| t == needle)
    }

    fn batch(id: &str, note: &str) -> AdminBatchRow {
        AdminBatchRow {
            batch_id: id.to_string(),
            note: note.to_string(),
            days: 30,
            count: 3,
            redeemed: 1,
            revoked: 1,
            created_at: "2026-09-29T10:00:00+00:00".to_string(),
            created_by: Some("alice".to_string()),
        }
    }

    #[test]
    fn only_the_focused_field_shows_the_ring_after_clicking_through_the_mint_boxes() {
        use uzor::input::{TextFieldConfig, TextFieldStore};
        let mut store = TextFieldStore::new();
        let ids = [AdminField::MintDays, AdminField::MintCount, AdminField::MintNote].map(|f| uzor::WidgetId::from(f.id()));
        for (i, id) in ids.iter().enumerate() {
            store.register(id.clone(), TextFieldConfig::text());
            store.update_field(id, (10.0 + 100.0 * i as f64, 10.0, 90.0, 24.0), vec![10.0 + 100.0 * i as f64]);
        }
        for i in 0..3 {
            store.on_drag_start(20.0 + 100.0 * i as f64, 20.0);
        }
        assert!(ids.iter().all(|id| store.is_engaged(id)), "the store leaves every clicked field engaged");
        let live: Vec<bool> = ids.iter().map(|id| shows_live_field(&store, id)).collect();
        assert_eq!(live, [false, false, true], "only the focused box draws the ring");

        // A press on nothing editable ends it everywhere.
        store.on_drag_start(900.0, 500.0);
        assert!(ids.iter().all(|id| !shows_live_field(&store, id)));
    }

    /// Owner-reported 0.5.651: the gift/trial code box could not be focused,
    /// selected or pasted into. The box drew from a differently spelled id than
    /// the store field, so what the store held never reached the screen.
    #[test]
    fn the_gift_code_box_draws_what_the_store_holds_and_arms_redeem() {
        use uzor::input::TextFieldConfig;
        let mut st = admin_state();
        st.active_tab = UserSettingsTab::Account;
        let wid = uzor::WidgetId::from(crate::ui::modal_settings::REDEEM_CODE_FIELD);
        let draw = |coord: &mut uzor::input::InputCoordinator| {
            let mut ctx = Rec::default();
            let result = render_user_settings_modal(
                &mut ctx,
                1280.0,
                800.0,
                &st,
                &CredentialsState::default(),
                &FrameTheme::default(),
                &ToolbarTheme::default(),
                0,
                coord,
                false,
            );
            (ctx.texts, result)
        };

        let mut coord = uzor::input::InputCoordinator::new();
        coord.text_fields_mut().register(wid.clone(), TextFieldConfig::text().with_max_len(32));
        let (texts, result) = draw(&mut coord);
        assert!(has(&texts, "XXXXXXXXXX"), "an empty box shows its hint: {texts:?}");
        assert!(!result.content_items.iter().any(|(k, _)| k == "account_redeem"), "Redeem is dead while empty");

        // A paste (or typing) lands in the store's field; the box must show it.
        coord.text_fields_mut().focus(wid.clone());
        coord.text_fields_mut().set_text(&wid, "K7Q2M9XPTA");
        coord.text_fields_mut().engage(&wid);
        let (texts, result) = draw(&mut coord);
        assert!(has(&texts, "K7Q2M9XPTA"), "the pasted code is drawn: {texts:?}");
        assert!(!has(&texts, "XXXXXXXXXX"), "the hint is gone once there is text");
        assert!(result.content_items.iter().any(|(k, _)| k == "account_redeem"), "Redeem is armed");
        assert!(
            result.content_items.iter().any(|(k, _)| k == "user_settings:account_redeem_input"),
            "the box publishes its geometry for the click claim"
        );
    }

    #[test]
    fn an_admin_gets_the_tab_and_the_coupons_form() {
        let (texts, result) = render(&admin_state());
        assert!(has(&texts, "Admin"), "tab title: {texts:?}");
        for section in ["Coupons", "Grants", "User"] {
            assert!(has(&texts, section), "section {section}: {texts:?}");
        }
        assert!(has(&texts, "Mint gift codes") && has(&texts, "Mint codes"));
        assert!(result.tab_rects.iter().any(|(id, _)| id == "admin"), "rail carries the admin tab");
        assert!(
            result.content_items.iter().any(|(k, _)| *k == AdminField::MintDays.content_key()),
            "the days box publishes its geometry"
        );
    }

    #[test]
    fn a_non_admin_never_sees_the_tab_even_if_it_was_the_active_one() {
        let mut st = admin_state();
        st.account_roles = vec!["moderator".to_string()];
        let (texts, result) = render(&st);
        assert!(!result.tab_rects.iter().any(|(id, _)| id == "admin"), "no admin tab in the rail");
        assert!(!has(&texts, "Mint codes"), "the admin body must not paint: {texts:?}");
        assert!(!has(&texts, "Coupons"));
    }

    #[test]
    fn a_signed_in_member_with_no_role_sees_no_admin_tab() {
        let mut st = admin_state();
        st.account_roles.clear();
        st.active_tab = UserSettingsTab::General;
        let (_texts, result) = render(&st);
        assert!(!result.tab_rects.iter().any(|(id, _)| id == "admin"));
    }

    #[test]
    fn the_batches_table_renders_its_rows_and_revoke_buttons() {
        let mut st = admin_state();
        st.admin.batches = Some(vec![batch("gb_2", "beta wave"), batch("gb_1", "")]);
        let (texts, _result) = render(&st);
        for header in ["Note", "Days", "Redeemed", "Revoked", "Created", "By"] {
            assert!(has(&texts, header), "header {header}: {texts:?}");
        }
        assert!(has(&texts, "beta wave"), "{texts:?}");
        assert!(has(&texts, "gb_1"), "a note-less batch shows its id");
        assert!(has(&texts, "1/3") && has(&texts, "2026-09-29") && has(&texts, "alice"));
        assert_eq!(texts.iter().filter(|t| *t == "Revoke").count(), 2, "one Revoke per batch");
    }

    #[test]
    fn revoke_is_disabled_on_a_batch_with_nothing_left_to_revoke() {
        let mut st = admin_state();
        let mut spent = batch("gb_1", "spent");
        spent.redeemed = 1;
        spent.revoked = 2; // redeemed + revoked == count
        st.admin.batches = Some(vec![batch("gb_2", "live"), spent]);
        let mut ctx = Rec::default();
        let mut coord = uzor::input::InputCoordinator::new();
        render_user_settings_modal(
            &mut ctx,
            1280.0,
            800.0,
            &st,
            &CredentialsState::default(),
            &FrameTheme::default(),
            &ToolbarTheme::default(),
            0,
            &mut coord,
            false,
        );
        assert_eq!(ctx.texts.iter().filter(|t| *t == "Revoke").count(), 2, "both rows draw the label");
        let target = |id: &str| coord.widget_rect(&uzor::WidgetId::from(AdminAction::RevokeBatch(id.into()).hit_id().as_str()));
        assert!(target("gb_2").is_some(), "a batch with unredeemed codes stays clickable");
        assert!(target("gb_1").is_none(), "a spent batch has no click target");
    }

    #[test]
    fn revoke_shows_a_confirmation_before_anything_is_sent() {
        let mut st = admin_state();
        st.admin.batches = Some(vec![batch("gb_2", "beta wave")]);
        st.admin.arm_revoke_batch("gb_2");
        let (texts, _result) = render(&st);
        assert!(texts.iter().any(|t| t.contains("gb_2") && t.contains("Revoke the unredeemed")), "{texts:?}");
        assert!(has(&texts, "Confirm") && has(&texts, "Cancel"));
        assert!(st.admin.pending.is_none());
    }

    #[test]
    fn minted_codes_are_drawn_once_with_the_shown_once_note() {
        let mut st = admin_state();
        st.admin.minted = Some(AdminMinted {
            batch_id: "gb_1".to_string(),
            days: 30,
            count: 2,
            note: String::new(),
            codes: vec!["K7Q2M9XPTA".to_string(), "B4N8R5WCDE".to_string()],
        });
        let (texts, _result) = render(&st);
        assert!(has(&texts, "K7Q2M9XPTA") && has(&texts, "B4N8R5WCDE"));
        assert!(texts.iter().any(|t| t.contains("Shown once")), "{texts:?}");
        assert!(has(&texts, "Copy all") && has(&texts, "Done"));
        assert_eq!(texts.iter().filter(|t| *t == "Copy").count(), 2, "one Copy per code");

        // Closing drops them from state, so a reopened tab draws none.
        st.close();
        st.open();
        st.admin.served = true;
        st.active_tab = UserSettingsTab::Admin;
        let (after, _result) = render(&st);
        assert!(!has(&after, "K7Q2M9XPTA") && !has(&after, "B4N8R5WCDE"));
    }

    #[test]
    fn the_grants_section_draws_its_forms() {
        let mut st = admin_state();
        st.admin.set_section(AdminSection::Grants);
        st.admin.arm_paid_days("alice", "30");
        let (texts, _result) = render(&st);
        for s in ["Paid days", "Add paid days", "Lifetime access", "Grant lifetime paid", "Revoke lifetime anonymous"] {
            assert!(has(&texts, s), "{s}: {texts:?}");
        }
        assert!(texts.iter().any(|t| t.contains("Add 30 paid day(s) to alice?")), "{texts:?}");
    }

    #[test]
    fn the_user_card_renders_a_sample() {
        let mut st = admin_state();
        st.admin.set_section(AdminSection::User);
        st.admin.user = Some(AdminUserCard {
            nick: "alice".to_string(),
            user_id: 42,
            paid_until: Some("2026-11-01T00:00:00+00:00".to_string()),
            lifetime_paid: true,
            lifetime_anon: false,
            ledger: vec![AdminLedgerRow {
                grant_id: 12,
                grant: LifetimeKind::Paid.wire().to_string(),
                source: "admin".to_string(),
                source_ref: "7:early supporter".to_string(),
                created_at: "2026-09-01T00:00:00+00:00".to_string(),
                revoked_at: None,
            }],
            quant_tier: None,
            quant_active_until: None,
            roles: vec!["moderator".to_string()],
        });
        let (texts, _result) = render(&st);
        for s in ["alice", "42", "2026-11-01", "moderator", "Lifetime grants", "lifetime paid", "admin", "2026-09-01"] {
            assert!(has(&texts, s), "{s}: {texts:?}");
        }
        assert!(has(&texts, "7:early supporter"));
    }

    #[test]
    fn an_error_notice_is_drawn_in_the_status_line() {
        let mut st = admin_state();
        st.admin.busy = true;
        st.admin.apply(AdminOutcome::Batches(Err(crate::ui::admin_tab::AdminFailure::Forbidden)));
        let (texts, _result) = render(&st);
        assert!(texts.iter().any(|t| t.contains("admin role")), "{texts:?}");
    }

    fn invite(id: &str, nick: &str, state: crate::ui::admin_tab::InviteState) -> crate::ui::admin_tab::AdminInviteRow {
        crate::ui::admin_tab::AdminInviteRow {
            id: id.to_string(),
            nick: nick.to_string(),
            paid_days: 365,
            note: "friend".to_string(),
            created_at: "2026-10-01T10:00:00+00:00".to_string(),
            state,
        }
    }

    #[test]
    fn the_strip_carries_the_invites_section() {
        let (texts, _result) = render(&admin_state());
        assert!(has(&texts, "Invites"), "section Invites: {texts:?}");
    }

    #[test]
    fn the_invites_section_draws_the_issue_form_with_its_three_boxes() {
        let mut st = admin_state();
        st.admin.set_section(AdminSection::Invites);
        let (texts, result) = render(&st);
        for s in ["Login challenge", "On", "Off", "Issue a registration invite", "Issue invite", "Nick", "Days", "Note"] {
            assert!(has(&texts, s), "{s}: {texts:?}");
        }
        for field in [AdminField::InviteNick, AdminField::InviteDays, AdminField::InviteNote] {
            assert!(
                result.content_items.iter().any(|(k, _)| *k == field.content_key()),
                "{field:?} publishes its geometry"
            );
        }
        assert!(!has(&texts, "Mint codes"), "the coupons form is not on this section");
    }

    #[test]
    fn the_invites_table_renders_rows_with_state_and_revoke_only_on_open_ones() {
        use crate::ui::admin_tab::InviteState;
        let mut st = admin_state();
        st.admin.set_section(AdminSection::Invites);
        st.admin.invites = Some(vec![invite("inv_1", "nix", InviteState::Open), invite("inv_2", "bob", InviteState::Redeemed)]);
        let mut ctx = Rec::default();
        let mut coord = uzor::input::InputCoordinator::new();
        render_user_settings_modal(
            &mut ctx,
            1280.0,
            800.0,
            &st,
            &CredentialsState::default(),
            &FrameTheme::default(),
            &ToolbarTheme::default(),
            0,
            &mut coord,
            false,
        );
        let texts = &ctx.texts;
        for s in ["State", "nix", "bob", "2026-10-01", "365", "Open", "Redeemed", "friend"] {
            assert!(has(texts, s), "{s}: {texts:?}");
        }
        assert_eq!(texts.iter().filter(|t| *t == "Revoke").count(), 2, "both rows draw the label");
        let target = |id: &str| coord.widget_rect(&uzor::WidgetId::from(AdminAction::RevokeInvite(id.into()).hit_id().as_str()));
        assert!(target("inv_1").is_some(), "an open invite stays clickable");
        assert!(target("inv_2").is_none(), "a redeemed invite has no click target");
    }

    #[test]
    fn an_empty_invites_table_says_so() {
        let mut st = admin_state();
        st.admin.set_section(AdminSection::Invites);
        st.admin.invites = Some(Vec::new());
        let (texts, _result) = render(&st);
        assert!(has(&texts, "No invites yet."), "{texts:?}");
    }

    #[test]
    fn revoking_an_invite_asks_first() {
        use crate::ui::admin_tab::InviteState;
        let mut st = admin_state();
        st.admin.set_section(AdminSection::Invites);
        st.admin.invites = Some(vec![invite("inv_1", "nix", InviteState::Open)]);
        st.admin.arm_revoke_invite("inv_1");
        let (texts, _result) = render(&st);
        assert!(texts.iter().any(|t| t.contains("Revoke the invite for nix")), "{texts:?}");
        assert!(has(&texts, "Confirm") && has(&texts, "Cancel"));
        assert!(st.admin.pending.is_none(), "nothing is sent before Confirm");
    }

    #[test]
    fn an_issued_invite_code_is_drawn_once_with_copy_and_the_shown_once_note() {
        let mut st = admin_state();
        st.admin.set_section(AdminSection::Invites);
        st.admin.issued_invite = Some(crate::ui::admin_tab::AdminIssuedInvite {
            nick: "nix".to_string(),
            paid_days: 365,
            code: "INV-7Q2M-9XPT-A4N8".to_string(),
        });
        let (texts, _result) = render(&st);
        assert!(has(&texts, "INV-7Q2M-9XPT-A4N8"), "{texts:?}");
        assert!(texts.iter().any(|t| t.contains("Shown once")), "{texts:?}");
        assert!(has(&texts, "Copy") && has(&texts, "Done"));

        st.close();
        st.open();
        st.admin.served = true;
        st.active_tab = UserSettingsTab::Admin;
        st.admin.set_section(AdminSection::Invites);
        let (after, _result) = render(&st);
        assert!(!has(&after, "INV-7Q2M-9XPT-A4N8"), "closing dropped the code");
    }
}
