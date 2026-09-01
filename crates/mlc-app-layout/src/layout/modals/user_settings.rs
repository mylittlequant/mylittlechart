//! User settings modal renderer — tabbed settings dialog.
//!
//! Tabs:
//!   - General    : placeholder for future general settings
//!   - Performance: RecalcMode radio group selector

use mlc_core::engine::render::{draw_svg_icon, RenderContext};
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::Sense;
use mlc_core::i18n::{current_language, Language, TextKey, UserSettingsKey};
use mlc_core::drawing::primitives_v2::{ControlPointShape, ControlPointSize};
use crate::ui::modal_settings::{CredentialsState, CredentialsView, UserSettingsState, UserSettingsTab};
use crate::ui::scroll_state::ScrollState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{ModalTheme, WidgetTheme, RadioOption, draw_radio_group, auto_btn_width, fit_text_to_width};
use crate::ui::widgets::{
    begin_modal_shell, end_modal_shell, ModalPlacement, ModalShellSpec, ModalTab,
    ModalTabs, FOOTER_H, HEADER_H, TAB_RAIL_W,
};
use crate::ui::widgets::{draw_input, draw_input_cursor, InputConfig, InputType};
use crate::ui::widgets::{render_single_slider, SliderConfig, SliderTrackInfo};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::Icon;
use crate::ui::scroll_widget::ScrollableContainer;
use crate::layout::modals::profile_manager::{
    render_page_create_passphrase, render_page_set_new_passphrase, render_page_show_recovery_key,
    render_page_unlock, render_page_use_recovery_key, VaultPageHost,
};

/// Row height for the Vault tab's own rows (status/key/webhook/add-key
/// fields) — carried over unchanged from the standalone credentials modal
/// this tab replaced.
const CRED_ROW_H: f64 = 32.0;
/// Vertical gap between Vault tab rows.
const CRED_ITEM_PADDING: f64 = 8.0;
/// Label column width in the Vault tab's add-key form / webhook row.
const CRED_LABEL_W: f64 = 90.0;

/// Render the User Settings modal dialog.
pub fn render_user_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &UserSettingsState,
    credentials: &CredentialsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> UserSettingsResult {
    let mut result = UserSettingsResult::default();

    let modal_w = 540.0;
    let modal_h = 500.0;
    let padding = 20.0;
    // A fixed-height modal needs no height field: hand the shell the body
    // height that leaves the frame exactly where it was.
    let content_height = modal_h - HEADER_H - FOOTER_H;

    let tab_items: Vec<ModalTab<'_>> = UserSettingsTab::all()
        .iter()
        .map(|tab| {
            let icon = match tab {
                UserSettingsTab::General => Icon::Settings,
                UserSettingsTab::Sync => Icon::Lock,
                UserSettingsTab::Vault => Icon::Lock,
                UserSettingsTab::Persistence => Icon::Pin,
                UserSettingsTab::Performance => Icon::Grid,
                UserSettingsTab::Server => Icon::Layers,
            };
            ModalTab::with_icon(tab.id(), tab.label(), icon)
        })
        .collect();
    let active_tab_idx = UserSettingsTab::all()
        .iter()
        .position(|t| *t == state.active_tab)
        .unwrap_or(0);

    let modal_theme = ModalTheme::from_toolbar(
        &frame_theme.modal_bg,
        &toolbar_theme.separator,
        toolbar_theme,
    );
    let spec = ModalShellSpec {
        // The body insets itself by `padding`, so the shell adds none.
        padding: 0.0,
        placement: ModalPlacement::At(state.position),
        ..ModalShellSpec::new(
            "user_settings",
            UserSettingsKey::Title.get(current_language()),
            modal_w,
            content_height,
        )
        .with_tabs(ModalTabs::Vertical {
            width: TAB_RAIL_W,
            items: &tab_items,
            active: active_tab_idx,
        })
        .with_max_content_height(content_height)
    };
    let (shell, layer_id) =
        begin_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h));

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result.tab_rects = shell.tab_rects.clone();

    let text_color = &toolbar_theme.item_text;

    // =========================================================================
    // Content area
    // =========================================================================
    // The rail and the body are flush in the shell; this is the gap the tab
    // content has always had between them.
    let sidebar_gap = 6.0;
    let content_x = shell.content.x + sidebar_gap;
    let content_y = shell.content.y;
    let content_w = shell.content.width - sidebar_gap;

    // Tab title
    ctx.set_font("bold 14px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(state.active_tab.label(), content_x + padding, content_y + padding);

    let settings_y = content_y + padding + 30.0;
    let scroll_viewport_h = shell.content.height - padding - 30.0;

    // WidgetTheme used for scrollbar rendering in General / Sync tabs
    let scroll_widget_theme = WidgetTheme {
        bg_normal:      toolbar_theme.item_bg_hover.clone(),
        bg_hover:       toolbar_theme.item_bg_hover.clone(),
        bg_pressed:     toolbar_theme.item_bg_active.clone(),
        bg_disabled:    toolbar_theme.item_bg_hover.clone(),
        text_normal:    toolbar_theme.item_text.clone(),
        text_hover:     toolbar_theme.item_text_active.clone(),
        text_disabled:  toolbar_theme.item_text_muted.clone(),
        border_normal:  toolbar_theme.separator.clone(),
        border_hover:   toolbar_theme.separator.clone(),
        border_focused: toolbar_theme.item_bg_active.clone(),
        accent:         toolbar_theme.accent.clone(),
        accent_hover:   toolbar_theme.accent.clone(),
        success:        toolbar_theme.success.clone(),
        warning:        toolbar_theme.warning.clone(),
        danger:         toolbar_theme.danger.clone(),
    };

    match state.active_tab {
        UserSettingsTab::General => {
            let viewport_rect = WidgetRect::new(
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                scroll_viewport_h,
            );
            render_general_tab(
                ctx,
                viewport_rect,
                state,
                text_color,
                toolbar_theme,
                frame_theme,
                &scroll_widget_theme,
                current_time_ms,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Sync => {}
        UserSettingsTab::Vault => {
            let viewport_rect = WidgetRect::new(
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                scroll_viewport_h,
            );
            render_vault_tab(
                ctx,
                viewport_rect,
                state,
                credentials,
                toolbar_theme,
                frame_theme,
                &scroll_widget_theme,
                current_time_ms,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Persistence => {
            let viewport_rect = WidgetRect::new(
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                scroll_viewport_h,
            );
            render_persistence_tab(
                ctx,
                viewport_rect,
                state,
                toolbar_theme,
                &scroll_widget_theme,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Performance => {
            let viewport_rect = WidgetRect::new(
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                scroll_viewport_h,
            );
            render_performance_tab(
                ctx,
                viewport_rect,
                state,
                toolbar_theme,
                &scroll_widget_theme,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
        UserSettingsTab::Server => {
            let viewport_rect = WidgetRect::new(
                content_x + padding,
                settings_y,
                content_w - padding * 2.0,
                scroll_viewport_h,
            );
            render_server_tab(
                ctx,
                viewport_rect,
                state,
                toolbar_theme,
                frame_theme,
                &scroll_widget_theme,
                input_coordinator,
                &layer_id,
                &mut result,
            );
        }
    }

    end_modal_shell(ctx, input_coordinator, &spec, &modal_theme, &shell, &layer_id, &[]);

    result
}

// =============================================================================
// Tab content renderers
// =============================================================================

/// Word-wrap `text` to `max_w` pixels using the current font. Char-count
/// wrap is wrong here: 11px CJK vs Latin would overflow the same way the
/// unwrapped `fill_text` did.
fn wrap_to_width(ctx: &dyn RenderContext, text: &str, max_w: f64) -> Vec<String> {
    let mut out = Vec::new();
    for para in text.split('\n') {
        let mut rest = para.trim();
        if rest.is_empty() {
            out.push(String::new());
            continue;
        }
        while !rest.is_empty() {
            if ctx.measure_text(rest) <= max_w {
                out.push(rest.to_string());
                break;
            }
            let mut last_fit_end = 0usize;
            let mut last_fit_space = 0usize;
            let mut acc = String::new();
            for (i, ch) in rest.char_indices() {
                acc.push(ch);
                if ctx.measure_text(&acc) > max_w {
                    break;
                }
                last_fit_end = i + ch.len_utf8();
                if ch == ' ' {
                    last_fit_space = last_fit_end;
                }
            }
            if last_fit_end == 0 {
                let ch = rest.chars().next().unwrap();
                out.push(ch.to_string());
                rest = &rest[ch.len_utf8()..];
                continue;
            }
            let cut = if last_fit_space > 0 { last_fit_space } else { last_fit_end };
            out.push(rest[..cut].trim_end().to_string());
            rest = rest[cut..].trim_start();
        }
    }
    out
}

fn persist_checkbox_row(
    ctx: &mut dyn RenderContext,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
    x: f64,
    y: f64,
    available_w: f64,
    checked: bool,
    hit_id: &str,
    label: &str,
    hint: &str,
) -> f64 {
    let row_h = 24.0;
    let cb_size = 16.0;
    let cb_y = y + (row_h - cb_size) / 2.0;
    let text_x = x + cb_size + 10.0;
    let text_w = (available_w - cb_size - 10.0).max(0.0);

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, cb_y, cb_size, cb_size, 2.0);
    if checked {
        ctx.set_fill_color(&toolbar_theme.item_text_active);
        ctx.fill_rounded_rect(x + 3.0, cb_y + 3.0, cb_size - 6.0, cb_size - 6.0, 1.0);
    }

    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&fit_text_to_width(ctx, label, text_w), text_x, y + row_h / 2.0);

    let desc_y = y + row_h + 2.0;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    let lines = wrap_to_width(ctx, hint, text_w);
    let line_h = 14.0;
    for (i, line) in lines.iter().enumerate() {
        ctx.fill_text(line, text_x, desc_y + i as f64 * line_h);
    }
    let hint_h = (lines.len().max(1) as f64) * line_h;
    let hit_h = row_h + 2.0 + hint_h;

    result.content_items.push((hit_id.to_string(), WidgetRect::new(x, y, available_w, hit_h)));
    input_coordinator.register_on_layer(
        format!("user_settings:{hit_id}"),
        uzor::types::Rect::new(x, y, available_w, hit_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    desc_y + hint_h + 12.0
}

// =============================================================================
// Vault tab (exchange API keys + the alert webhook URL)
// =============================================================================
//
// Moved in from the standalone credentials modal (`crate::layout::modals::
// credentials`, retired) — opening it used to CLOSE this settings modal
// (a modal stacked on a modal has no honest back button), so the vault
// surface is a tab here instead. `render_list_view` / `render_add_key_view`
// are the two pages `CredentialsState::view` switches between; unchanged
// from the modal version except: (1) they paint into the tab's own
// `ScrollableContainer` viewport instead of a fixed-size modal body, and
// (2) their hit rects land in `UserSettingsResult` instead of the retired
// `CredentialsResult`. Every widget id stays byte-identical to what the
// modal used (`cred:lock`, `cred:add:exchange`, `cred:create`, ...) —
// `handle_credentials_click` (mlc-app) dispatches on the `cred:` prefix
// regardless of which surface drew the widget, and hover comparisons
// against `hovered_item_id` test the WHOLE id the same way `alert_set:`
// does. Nothing here should ever "tidy" one of these ids.

/// Render the Vault tab body — dispatches on `credentials.view`.
///
/// Five of the seven variants (everything but `List`/`AddKey`) are the
/// vault pages shared with the standalone profile-manager modal — see
/// `layout::modals::profile_manager`'s module doc for why the SAME render
/// functions are called from both places (`VaultPageHost::Tab` here,
/// `VaultPageHost::Modal` there), each given this tab's viewport
/// coordinates instead of the modal's `shell.content`.
#[allow(clippy::too_many_arguments)]
fn render_vault_tab(
    ctx: &mut dyn RenderContext,
    viewport_rect: WidgetRect,
    state: &UserSettingsState,
    credentials: &CredentialsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    scroll_widget_theme: &WidgetTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let container = ScrollableContainer::new(viewport_rect, &state.vault_tab_scroll, None);
    container.begin(ctx);
    let x = viewport_rect.x;
    let available_w = container.content_width();
    let cy = container.content_y();
    let text_color = &toolbar_theme.item_text;

    let final_cy = match credentials.view {
        CredentialsView::List => render_list_view(
            ctx, credentials, toolbar_theme, frame_theme, input_coordinator, layer_id, result,
            x, cy, available_w,
        ),
        CredentialsView::AddKey => render_add_key_view(
            ctx, credentials, toolbar_theme, frame_theme, input_coordinator, layer_id, result,
            x, cy, available_w,
        ),
        CredentialsView::UnlockPassphrase => render_page_unlock(
            ctx, x, cy, available_w, state, text_color, toolbar_theme, frame_theme,
            current_time_ms, layer_id, input_coordinator, result, VaultPageHost::Tab,
        ),
        CredentialsView::CreatePassphrase => render_page_create_passphrase(
            ctx, x, cy, available_w, state, text_color, toolbar_theme, frame_theme,
            current_time_ms, layer_id, input_coordinator, result, VaultPageHost::Tab,
        ),
        CredentialsView::ShowRecoveryKey => render_page_show_recovery_key(
            ctx, x, cy, available_w, state, text_color, toolbar_theme, frame_theme,
            layer_id, input_coordinator, result,
        ),
        CredentialsView::UseRecoveryKey => render_page_use_recovery_key(
            ctx, x, cy, available_w, state, text_color, toolbar_theme, frame_theme,
            layer_id, input_coordinator, result, VaultPageHost::Tab,
        ),
        CredentialsView::SetNewPassphrase => render_page_set_new_passphrase(
            ctx, x, cy, available_w, state, text_color, toolbar_theme, frame_theme,
            layer_id, input_coordinator, result,
        ),
    };

    let total_content_h = final_cy - container.content_y();
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            layer_id,
        );
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        input_coordinator.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            uzor::input::Sense::SCROLL,
            layer_id,
        );
    }
}

/// The vault status row (Lock/Unlock/Set up), the exchange-key list (or its
/// empty notice), the "+ Add key" row, and the webhook section. Returns the
/// `y` past the last row so the caller can size the scroll content.
#[allow(clippy::too_many_arguments)]
fn render_list_view(
    ctx: &mut dyn RenderContext,
    state: &CredentialsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
    content_x: f64,
    content_y: f64,
    content_w: f64,
) -> f64 {
    let mut y = content_y;

    // --- Vault status row: "Vault: Unlocked/Sealed" + Lock/Unlock button ---
    {
        // Three states, not two: a profile with no vault is not a sealed
        // one, and telling it to "Unlock" is what sent the user to a
        // single-field prompt that created nothing.
        let (status_text, icon, btn_id, btn_label) = if state.secrets_available {
            ("Vault: Unlocked", Icon::Unlock, "cred:lock", "Lock")
        } else if state.vault_exists {
            ("Vault: Sealed", Icon::Lock, "cred:unlock", "Unlock")
        } else {
            ("No vault yet", Icon::Lock, "cred:unlock", "Set up")
        };
        draw_svg_icon(ctx, icon.svg(), content_x, y + (CRED_ROW_H - 14.0) / 2.0, 14.0, 14.0, &toolbar_theme.item_text_muted);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(status_text, content_x + 20.0, y + CRED_ROW_H / 2.0);

        let btn_w = 64.0;
        let btn_x = content_x + content_w - btn_w;
        let is_hovered = state.hovered_item_id.as_deref() == Some(btn_id);
        let bg = if is_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(btn_x, y + 2.0, btn_w, CRED_ROW_H - 4.0, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(btn_x, y + 2.0, btn_w, CRED_ROW_H - 4.0, 4.0);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Center);
        ctx.fill_text(btn_label, btn_x + btn_w / 2.0, y + CRED_ROW_H / 2.0);
        let r = WidgetRect::new(btn_x, y, btn_w, CRED_ROW_H);
        result.content_items.push((btn_id.to_string(), r));
        input_coordinator.register_on_layer(btn_id, r, Sense::CLICK, layer_id);
        y += CRED_ROW_H + CRED_ITEM_PADDING;
    }

    // --- Exchange key rows ---
    if state.exchange_keys.is_empty() {
        // Empty is not the same fact while sealed. `state.exchange_keys`
        // mirrors whatever is currently in RAM: for an unlocked (or never-
        // encrypted) profile that IS the true count, but for a vault that
        // exists on disk and was never unlocked this session, RAM holds
        // nothing regardless of what `vault.enc` actually contains — saying
        // "No exchange keys yet." there would assert a count nobody has
        // measured. Say the honest thing instead; unlocking is what turns
        // this into a real answer.
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        let empty_label = if state.secrets_available || !state.vault_exists {
            "No exchange keys yet."
        } else {
            "Sealed — contents unknown until unlocked."
        };
        ctx.fill_text(empty_label, content_x, y + CRED_ROW_H / 2.0);
        y += CRED_ROW_H + CRED_ITEM_PADDING;
    } else {
        for (idx, key) in state.exchange_keys.iter().enumerate() {
            let remove_id = format!("cred:remove:{idx}");
            // Remove is a mutation that needs the vault key to persist
            // correctly (see the module doc on why sealed removal would be
            // silently undone by the next unlock's vault.enc merge) — while
            // sealed it is simply not offered, same principle as "no
            // editable fields while locked".
            let remove_w = if state.secrets_available { 24.0 } else { 0.0 };

            let title = if key.label.is_empty() {
                key.exchange_id.clone()
            } else {
                format!("{} · {}", key.exchange_id, key.label)
            };
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&title, content_x, y + CRED_ROW_H / 2.0);

            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(&key.api_key, content_x + content_w - remove_w - 8.0, y + CRED_ROW_H / 2.0);
            ctx.set_text_align(TextAlign::Left);

            if state.secrets_available {
                let rm_x = content_x + content_w - remove_w;
                let rm_hovered = state.hovered_item_id.as_deref() == Some(remove_id.as_str());
                if rm_hovered {
                    ctx.set_fill_color(&toolbar_theme.danger);
                    ctx.fill_rounded_rect(rm_x, y + 4.0, remove_w, CRED_ROW_H - 8.0, 3.0);
                }
                draw_svg_icon(ctx, Icon::Close.svg(), rm_x + 4.0, y + (CRED_ROW_H - 16.0) / 2.0, 16.0, 16.0, &toolbar_theme.item_text_muted);
                let rm_r = WidgetRect::new(rm_x, y, remove_w, CRED_ROW_H);
                result.content_items.push((remove_id.clone(), rm_r));
                input_coordinator.register_on_layer(remove_id.as_str(), rm_r, Sense::CLICK, layer_id);
            }

            y += CRED_ROW_H + CRED_ITEM_PADDING;
        }
    }

    // --- "+ Add key" row — always reachable, even sealed (see module doc). ---
    {
        let is_hovered = state.hovered_item_id.as_deref() == Some("cred:add_key");
        let bg = if is_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(content_x, y, content_w, CRED_ROW_H, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(content_x, y, content_w, CRED_ROW_H, 4.0);
        draw_svg_icon(ctx, Icon::Plus.svg(), content_x + 8.0, y + (CRED_ROW_H - 14.0) / 2.0, 14.0, 14.0, &toolbar_theme.item_text);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Add key", content_x + 28.0, y + CRED_ROW_H / 2.0);
        let r = WidgetRect::new(content_x, y, content_w, CRED_ROW_H);
        result.content_items.push(("cred:add_key".to_string(), r));
        input_coordinator.register_on_layer("cred:add_key", r, Sense::CLICK, layer_id);
        y += CRED_ROW_H + CRED_ITEM_PADDING;
    }

    // --- Webhook section ---
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.accent, 0.85));
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text("Webhook", content_x, y);
    y += 20.0;

    if !state.secrets_available {
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        // Same three-state honesty as the status row above: with no vault
        // there is nothing to unlock, and telling the user to unlock one
        // points at a door that does not exist.
        let notice = if state.vault_exists {
            "Unlock the vault to view the webhook URL."
        } else {
            "Set up the vault to store a webhook URL."
        };
        ctx.fill_text(notice, content_x, y + CRED_ROW_H / 2.0);
        y += CRED_ROW_H;
    } else {
        let field_x = content_x + CRED_LABEL_W;
        let field_w = content_w - CRED_LABEL_W;
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("URL", content_x, y + CRED_ROW_H / 2.0);

        let render = &state.webhook_url_render;
        let (sel_start, sel_end) = if let Some((lo, hi)) = render.selection_range() {
            (Some(lo), Some(hi))
        } else {
            (None, None)
        };
        let field_rect = WidgetRect::new(field_x, y, field_w, CRED_ROW_H);
        let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
        let config = InputConfig::new(&render.text)
            .with_placeholder("https://...")
            .with_focused(state.webhook_url_focused)
            .with_cursor(render.cursor)
            .with_selection(sel_start, sel_end)
            .with_font_size(12.0);
        let input_result = draw_input(
            ctx, &config, crate::ui::widgets::types::WidgetState::Normal, field_rect, &widget_theme,
        );
        if state.webhook_url_focused && render.cursor_visible {
            draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &toolbar_theme.item_text);
        }

        result.content_items.push(("cred:webhook_url".to_string(), field_rect));
        result.input_char_positions.push(("cred:webhook_url".to_string(), input_result.char_x_positions));
        input_coordinator.register_on_layer("cred:webhook_url", field_rect, Sense::CLICK, layer_id);
        y += CRED_ROW_H;
    }

    y
}

/// The "Add Exchange Key" form: exchange / label / API key / API secret /
/// passphrase / testnet, plus the "Add" submit row. Returns the `y` past
/// the last row so the caller can size the scroll content.
#[allow(clippy::too_many_arguments)]
fn render_add_key_view(
    ctx: &mut dyn RenderContext,
    state: &CredentialsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
    content_x: f64,
    content_y: f64,
    content_w: f64,
) -> f64 {
    let mut y = content_y;
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

    // --- Back to the list ---
    {
        let is_hovered = state.hovered_item_id.as_deref() == Some("cred:add:back");
        ctx.set_fill_color(if is_hovered { &toolbar_theme.accent } else { &toolbar_theme.item_text_muted });
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("< Back", content_x, y + 12.0);
        let r = WidgetRect::new(content_x, y, 60.0, 24.0);
        result.content_items.push(("cred:add:back".to_string(), r));
        input_coordinator.register_on_layer("cred:add:back", r, Sense::CLICK, layer_id);
        y += 24.0 + 10.0;
    }

    let field_x = content_x + CRED_LABEL_W;
    let field_w = content_w - CRED_LABEL_W;

    let draw_field_row = |ctx: &mut dyn RenderContext,
                              input_coordinator: &mut uzor::input::InputCoordinator,
                              result: &mut UserSettingsResult,
                              label: &str,
                              placeholder: &str,
                              input_type: InputType,
                              render: &crate::ui::modal_settings::ActiveFieldRender,
                              focused: bool,
                              widget_id: &str,
                              row_y: f64| {
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, content_x, row_y + CRED_ROW_H / 2.0);

        let field_rect = WidgetRect::new(field_x, row_y, field_w, CRED_ROW_H);
        let (sel_start, sel_end) = if let Some((lo, hi)) = render.selection_range() {
            (Some(lo), Some(hi))
        } else {
            (None, None)
        };
        // API secret / passphrase draw a reveal toggle inside their own
        // right edge — widen the input's own padding so masked/typed text
        // never runs under the icon (same knob the search box uses for its
        // own leading icon), and swap the effective type to plain text
        // while revealed.
        let is_secret = input_type == InputType::Password;
        let revealed = is_secret && state.secret_revealed;
        let effective_type = if revealed { InputType::Text } else { input_type };
        let mut config = InputConfig::new(&render.text)
            .with_placeholder(placeholder)
            .with_focused(focused)
            .with_cursor(render.cursor)
            .with_selection(sel_start, sel_end)
            .with_type(effective_type)
            .with_font_size(12.0);
        if is_secret {
            config = config.with_padding_right(28.0);
        }
        let input_result = draw_input(
            ctx, &config, crate::ui::widgets::types::WidgetState::Normal, field_rect, &widget_theme,
        );
        if focused && render.cursor_visible {
            draw_input_cursor(ctx, input_result.cursor_x, input_result.cursor_y, input_result.cursor_height, &toolbar_theme.item_text);
        }
        result.content_items.push((widget_id.to_string(), field_rect));
        result.input_char_positions.push((widget_id.to_string(), input_result.char_x_positions));
        input_coordinator.register_on_layer(widget_id, field_rect, Sense::CLICK, layer_id);

        if is_secret {
            const ICON_SIZE: f64 = 14.0;
            const RIGHT_INSET: f64 = 8.0;
            let icon_x = field_rect.x + field_rect.width - RIGHT_INSET - ICON_SIZE;
            let icon_y = field_rect.y + (field_rect.height - ICON_SIZE) / 2.0;
            let icon_hovered = state.hovered_item_id.as_deref() == Some("cred:add:reveal");
            let icon_color = if icon_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted };
            // The icon names the STATE, not the action: a closed/crossed eye
            // means the text is dots, an open eye means it is plaintext —
            // same rule `profile_manager.rs`'s own reveal toggle follows.
            draw_svg_icon(
                ctx,
                if revealed { Icon::Eye.svg() } else { Icon::EyeOff.svg() },
                icon_x, icon_y, ICON_SIZE, ICON_SIZE, icon_color,
            );
            // Hit the gutter, not the glyph — a 14px target inside a field
            // that itself takes clicks is a coin toss.
            let hit_rect = WidgetRect::new(
                icon_x - RIGHT_INSET,
                field_rect.y,
                ICON_SIZE + RIGHT_INSET * 2.0,
                field_rect.height,
            );
            result.content_items.push(("cred:add:reveal".to_string(), hit_rect));
            input_coordinator.register_on_layer("cred:add:reveal", hit_rect, Sense::CLICK | Sense::HOVER, layer_id);
        }
    };

    draw_field_row(ctx, input_coordinator, result, "Exchange", "e.g. binance", InputType::Text, &state.add_exchange_render, state.add_exchange_focused, "cred:add:exchange", y);
    y += CRED_ROW_H + CRED_ITEM_PADDING;
    draw_field_row(ctx, input_coordinator, result, "Label", "main account", InputType::Text, &state.add_label_render, state.add_label_focused, "cred:add:label", y);
    y += CRED_ROW_H + CRED_ITEM_PADDING;
    draw_field_row(ctx, input_coordinator, result, "API Key", "public key", InputType::Text, &state.add_api_key_render, state.add_api_key_focused, "cred:add:api_key", y);
    y += CRED_ROW_H + CRED_ITEM_PADDING;
    draw_field_row(ctx, input_coordinator, result, "API Secret", "secret", InputType::Password, &state.add_api_secret_render, state.add_api_secret_focused, "cred:add:api_secret", y);
    y += CRED_ROW_H + CRED_ITEM_PADDING;
    draw_field_row(ctx, input_coordinator, result, "Passphrase", "optional", InputType::Password, &state.add_passphrase_render, state.add_passphrase_focused, "cred:add:passphrase", y);
    y += CRED_ROW_H + CRED_ITEM_PADDING;

    // --- Testnet checkbox ---
    {
        let cb_size = 16.0;
        let cb_x = content_x + CRED_LABEL_W;
        let cb_y = y + (CRED_ROW_H - cb_size) / 2.0;
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(cb_x, cb_y, cb_size, cb_size, 2.0);
        if state.add_testnet {
            ctx.set_fill_color(&toolbar_theme.item_text_active);
            ctx.fill_rounded_rect(cb_x + 3.0, cb_y + 3.0, cb_size - 6.0, cb_size - 6.0, 1.0);
        }
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Testnet", content_x, y + CRED_ROW_H / 2.0);
        let r = WidgetRect::new(cb_x, y, content_w - CRED_LABEL_W, CRED_ROW_H);
        result.content_items.push(("cred:add:testnet".to_string(), r));
        input_coordinator.register_on_layer("cred:add:testnet", r, Sense::CLICK, layer_id);
        y += CRED_ROW_H;
    }

    if let Some(ref msg) = state.error_message {
        y += CRED_ITEM_PADDING;
        ctx.set_fill_color(&toolbar_theme.danger);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(msg, content_x, y + CRED_ROW_H / 2.0);
        y += CRED_ROW_H;
    }

    // --- "Add" — this used to be the standalone modal shell's own Creator
    // footer button (id `cred:create`, drawn by `end_modal_shell`); the tab
    // has no shell footer of its own, so the same click id is drawn as an
    // ordinary accent row here instead. Keeping the literal id is what lets
    // `handle_credentials_click`'s `"create"` arm stay unaware this moved.
    y += 6.0;
    {
        let btn_h = 32.0;
        let is_hovered = state.hovered_item_id.as_deref() == Some("cred:create");
        let bg = if is_hovered { &toolbar_theme.accent_hover } else { &toolbar_theme.accent };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(content_x, y, content_w, btn_h, 4.0);
        ctx.set_fill_color(&toolbar_theme.item_text_active);
        ctx.set_font("13px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Add", content_x + content_w / 2.0, y + btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);
        let r = WidgetRect::new(content_x, y, content_w, btn_h);
        result.content_items.push(("cred:create".to_string(), r));
        input_coordinator.register_on_layer("cred:create", r, Sense::CLICK | Sense::HOVER, layer_id);
        y += btn_h;
    }

    y
}

fn render_persistence_tab(
    ctx: &mut dyn RenderContext,
    viewport_rect: WidgetRect,
    state: &UserSettingsState,
    toolbar_theme: &ToolbarTheme,
    scroll_widget_theme: &WidgetTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let lang = current_language();
    let container = ScrollableContainer::new(viewport_rect, &state.persistence_tab_scroll, None);
    container.begin(ctx);
    let x = viewport_rect.x;
    let available_w = container.content_width();
    let mut cy = container.content_y();

    cy = persist_checkbox_row(
        ctx, toolbar_theme, input_coordinator, layer_id, result,
        x, cy, available_w,
        state.persist_toolbar_last_tools,
        "persist_toolbar_last_tools",
        UserSettingsKey::PersistToolbarLastTools.get(lang),
        UserSettingsKey::PersistToolbarLastToolsHint.get(lang),
    );
    cy = persist_checkbox_row(
        ctx, toolbar_theme, input_coordinator, layer_id, result,
        x, cy, available_w,
        state.persist_toolbar_toggles,
        "persist_toolbar_toggles",
        UserSettingsKey::PersistToolbarToggles.get(lang),
        UserSettingsKey::PersistToolbarTogglesHint.get(lang),
    );

    let total_content_h = cy - container.content_y();
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;
}

#[allow(clippy::too_many_arguments)]
fn render_general_tab(
    ctx: &mut dyn RenderContext,
    viewport_rect: WidgetRect,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    scroll_widget_theme: &WidgetTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let container = ScrollableContainer::new(
        viewport_rect,
        &state.general_tab_scroll,
        None,
    );
    container.begin(ctx);
    let x = viewport_rect.x;
    let available_w = container.content_width();
    let mut cy = container.content_y();

    // ── Section: PROFILE ─────────────────────────────────────────────────────
    // Owner's identity ruling 2026-09-02: a profile's identity is its UUID;
    // an anonymous, single-profile user needs no user-facing name at all —
    // not even a default one. Below the paid tier (`multi_profile`) and
    // signed out (`account_signed_in`), there is nothing to show here at
    // all: no heading, no radio dot, no name. Signed-in-free and
    // multi-profile keep this section exactly as before.
    if state.account_signed_in || state.multi_profile {
        ctx.set_font("600 11px sans-serif");
        // A section heading is an accented label, not a warning. The gold it
        // used to be hardcoded to is the mascot theme's `warning` token, which
        // is how one theme's palette ended up painted into every other one.
        ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.accent, 0.85));
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(UserSettingsKey::SectionProfile.get(current_language()), x, cy);
        cy += 20.0;

        cy = render_profile_section(
            ctx, x, cy, available_w, state, text_color, toolbar_theme, frame_theme,
            current_time_ms, input_coordinator, layer_id, result,
        );
        cy += 16.0;
    }

    // The "Credentials / Manage Exchange Keys & Webhook" row that used to
    // live here was a pointer to a modal stacked on top of this one — the
    // navigation dead end this whole tab move exists to close. The vault
    // surface is now the `Vault` tab, one click away in the same window;
    // there is nothing left to point at from in here.

    // ── Section: LANGUAGE ────────────────────────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    // A section heading is an accented label, not a warning. The gold it
    // used to be hardcoded to is the mascot theme's `warning` token, which
    // is how one theme's palette ended up painted into every other one.
    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.accent, 0.85));
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionLanguage.get(current_language()), x, cy);
    cy += 20.0;

    {
        let lang_options: Vec<RadioOption<'_>> = Language::all().iter().map(|l| RadioOption {
            key: l.code(),
            label: l.native_name(),
            description: "",
        }).collect();

        let lang_selected = Language::all().iter()
            .position(|l| l.code() == state.language.as_str())
            .unwrap_or(0);

        let lang_radio_result = draw_radio_group(
            ctx,
            &lang_options,
            lang_selected,
            state.hovered_item_id.as_deref(),
            x,
            cy,
            available_w,
            scroll_widget_theme,
            true, // compact — language options are single-line (no description)
        );

        for (i, (rx, ry, rw, rh)) in lang_radio_result.option_rects.iter().enumerate() {
            let hit_id = format!("user_settings:language:{}", lang_options[i].key);
            result.content_items.push((
                format!("language:{}", lang_options[i].key),
                WidgetRect::new(*rx, *ry, *rw, *rh),
            ));
            input_coordinator.register_on_layer(
                hit_id.as_str(),
                uzor::types::Rect::new(*rx, *ry, *rw, *rh),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        // Advance past the rendered rows using their actual bottom edge.
        if let Some((_, ry, _, rh)) = lang_radio_result.option_rects.last() {
            cy = ry + rh + 16.0;
        }
    }

    // ── Section: CONTROL POINTS ──────────────────────────────────────────────
    // Two compact radio groups (Shape, Size) — same idiom as the LANGUAGE
    // section above (`draw_radio_group`, compact single-line rows), each
    // preceded by a small muted caption identifying which knob it is since,
    // unlike LANGUAGE, this section hosts two closed-choice groups instead
    // of one (control-point-style-ui arc, 2026-08-08).
    ctx.set_font("600 11px sans-serif");
    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.accent, 0.85));
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionControlPoints.get(current_language()), x, cy);
    cy += 20.0;

    {
        let shape_options = [
            RadioOption {
                key: "Circle",
                label: UserSettingsKey::ControlPointShapeCircle.get(current_language()),
                description: "",
            },
            RadioOption {
                key: "RoundedSquare",
                label: UserSettingsKey::ControlPointShapeRoundedSquare.get(current_language()),
                description: "",
            },
            RadioOption {
                key: "Square",
                label: UserSettingsKey::ControlPointShapeSquare.get(current_language()),
                description: "",
            },
        ];
        let shape_selected = match state.control_point_shape {
            ControlPointShape::Circle => 0,
            ControlPointShape::RoundedSquare => 1,
            ControlPointShape::Square => 2,
        };

        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(UserSettingsKey::ControlPointShapeLabel.get(current_language()), x, cy);
        cy += 16.0;

        let shape_radio_result = draw_radio_group(
            ctx,
            &shape_options,
            shape_selected,
            state.hovered_item_id.as_deref(),
            x,
            cy,
            available_w,
            scroll_widget_theme,
            true, // compact — shape options are single-line (no description)
        );

        for (i, (rx, ry, rw, rh)) in shape_radio_result.option_rects.iter().enumerate() {
            let hit_id = format!("user_settings:control_point_shape:{}", shape_options[i].key);
            result.content_items.push((
                format!("control_point_shape:{}", shape_options[i].key),
                WidgetRect::new(*rx, *ry, *rw, *rh),
            ));
            input_coordinator.register_on_layer(
                hit_id.as_str(),
                uzor::types::Rect::new(*rx, *ry, *rw, *rh),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        if let Some((_, ry, _, rh)) = shape_radio_result.option_rects.last() {
            cy = ry + rh + 12.0;
        }
    }

    {
        let size_options = [
            RadioOption {
                key: "Small",
                label: UserSettingsKey::ControlPointSizeSmall.get(current_language()),
                description: "",
            },
            RadioOption {
                key: "Medium",
                label: UserSettingsKey::ControlPointSizeMedium.get(current_language()),
                description: "",
            },
            RadioOption {
                key: "Large",
                label: UserSettingsKey::ControlPointSizeLarge.get(current_language()),
                description: "",
            },
        ];
        let size_selected = match state.control_point_size {
            ControlPointSize::Small => 0,
            ControlPointSize::Medium => 1,
            ControlPointSize::Large => 2,
        };

        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(UserSettingsKey::ControlPointSizeLabel.get(current_language()), x, cy);
        cy += 16.0;

        let size_radio_result = draw_radio_group(
            ctx,
            &size_options,
            size_selected,
            state.hovered_item_id.as_deref(),
            x,
            cy,
            available_w,
            scroll_widget_theme,
            true, // compact — size options are single-line (no description)
        );

        for (i, (rx, ry, rw, rh)) in size_radio_result.option_rects.iter().enumerate() {
            let hit_id = format!("user_settings:control_point_size:{}", size_options[i].key);
            result.content_items.push((
                format!("control_point_size:{}", size_options[i].key),
                WidgetRect::new(*rx, *ry, *rw, *rh),
            ));
            input_coordinator.register_on_layer(
                hit_id.as_str(),
                uzor::types::Rect::new(*rx, *ry, *rw, *rh),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        if let Some((_, ry, _, rh)) = size_radio_result.option_rects.last() {
            cy = ry + rh + 16.0;
        }
    }

    // ── Trading tab topbar takeover (workspace-panel arc, Stage 4) ──────────
    // No section banner of its own — mirrors the "Show Welcome Wizard" button
    // below (a bare row, no header) rather than adding another localized
    // SECTION key for a single niche toggle.
    {
        let row_h = 24.0;
        let cb_size = 16.0;
        let cb_x = x;
        let cb_y = cy + (row_h - cb_size) / 2.0;

        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(cb_x, cb_y, cb_size, cb_size, 2.0);

        if state.trading_panel_topbar_takeover {
            ctx.set_fill_color(&toolbar_theme.item_text_active);
            ctx.fill_rounded_rect(cb_x + 3.0, cb_y + 3.0, cb_size - 6.0, cb_size - 6.0, 1.0);
        }

        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(
            UserSettingsKey::TradingPanelTopbarTakeover.get(current_language()),
            cb_x + cb_size + 10.0,
            cy + row_h / 2.0,
        );

        let desc_y = cy + row_h + 2.0;
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_text_baseline(TextBaseline::Top);
        let hint_w = (available_w - cb_size - 10.0).max(0.0);
        let hint_lines = wrap_to_width(
            ctx,
            "Symbol/exchange controls move to the topbar while the Trading tab is focused.",
            hint_w,
        );
        let line_h = 14.0;
        for (i, line) in hint_lines.iter().enumerate() {
            ctx.fill_text(line, cb_x + cb_size + 10.0, desc_y + i as f64 * line_h);
        }

        let hit_h = row_h + 2.0 + (hint_lines.len().max(1) as f64) * line_h;
        let toggle_rect = WidgetRect::new(cb_x, cy, available_w, hit_h);
        result.content_items.push(("trading_panel_topbar_takeover".to_string(), toggle_rect));
        input_coordinator.register_on_layer(
            "user_settings:trading_panel_topbar_takeover",
            uzor::types::Rect::new(cb_x, cy, available_w, hit_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );

        cy = desc_y + (hint_lines.len().max(1) as f64) * line_h + 16.0;
    }

    // ── Version info (always shown at bottom) ─────────────────────────────────
    ctx.set_font("600 11px sans-serif");
    // A section heading is an accented label, not a warning. The gold it
    // used to be hardcoded to is the mascot theme's `warning` token, which
    // is how one theme's palette ended up painted into every other one.
    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.accent, 0.85));
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionVersion.get(current_language()), x, cy);
    cy += 18.0;

    ctx.set_font("700 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.fill_text(&format!("v{}", env!("CARGO_PKG_VERSION")), x, cy);
    cy += 30.0;

    // "Show Welcome Wizard" debug button
    let btn_h = 24.0;
    let btn_w = available_w.min(180.0);
    let is_wizard_hovered = state.hovered_item_id.as_deref() == Some("show_wizard");
    let wizard_bg = if is_wizard_hovered {
        mlc_core::apply_opacity(&toolbar_theme.item_text, 0.10)
    } else {
        toolbar_theme.item_bg_hover.clone()
    };
    ctx.set_fill_color(&wizard_bg);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, if is_wizard_hovered { 0.80 } else { 0.55 }));
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(UserSettingsKey::ShowWelcomeWizard.get(current_language()), x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    result.content_items.push(("show_wizard".to_string(), WidgetRect::new(x, cy, btn_w, btn_h)));
    input_coordinator.register_on_layer(
        "user_settings:show_wizard",
        uzor::types::Rect::new(x, cy, btn_w, btn_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    cy += btn_h + 8.0;

    let total_content_h = cy - container.content_y();
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            layer_id,
        );
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        input_coordinator.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            uzor::input::Sense::SCROLL,
            layer_id,
        );
    }
}

// =============================================================================
// Profile section renderer
// =============================================================================

/// Maps an avatar key string to a short display tag used as a colored label.
fn avatar_tag(key: &str) -> &'static str {
    match key {
        "chart"  => "CH",
        "rocket" => "RK",
        "shield" => "SH",
        "fire"   => "FR",
        "star"   => "ST",
        "moon"   => "MN",
        "sun"    => "SN",
        "ghost"  => "GH",
        _        => "??",
    }
}

/// Returns a CSS-style hex color string for an avatar key.
fn avatar_color(key: &str) -> &'static str {
    match key {
        "chart"  => "#4a8fe7",
        "rocket" => "#f07030",
        "shield" => "#4caf50",
        "fire"   => "#e53935",
        "star"   => "#fdd835",
        "moon"   => "#9c6dd8",
        "sun"    => "#ffb300",
        "ghost"  => "#90a4ae",
        _        => "#888888",
    }
}

/// Renders a small colored dot (circle) for a profile avatar.
fn draw_avatar_dot(ctx: &mut dyn RenderContext, cx: f64, cy: f64, r: f64, color: &str) {
    ctx.set_fill_color(color);
    ctx.begin_path();
    ctx.arc(cx, cy, r, 0.0, std::f64::consts::TAU);
    ctx.fill();
}

/// PROFILE row for an identity that cannot have more than one profile
/// (`UserSettingsState::multi_profile == false`) — draws exactly the active
/// profile's avatar dot and name, and nothing else: no radio dot (nothing to
/// switch to), no Avatar-picker button, no Delete button, no "+ New Profile"
/// row. Per the owner's ruling, a tier that cannot use a control does not
/// get a disabled copy of it either — the widget id is simply never
/// registered, so a stale or forged click for it cannot reach a handler
/// (`crate::layout::modals::user_settings`'s own module is the render side
/// of that; `mlc-app/src/input/panel_click.rs`'s matching handler guards are
/// the other half).
///
/// A Rename button is the one exception, and only when
/// `UserSettingsState::account_signed_in` — an anonymous identity has a
/// fixed name (nothing to rename), a signed-in free identity IS its profile
/// with an editable nickname at most (`UserSettingsKey::BtnNickname`, not
/// `BtnRename` — "Rename" reads as "one of several", which does not exist
/// here).
#[allow(clippy::too_many_arguments)]
fn render_single_profile_row(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) -> f64 {
    let profile_row_h = 32.0;
    let btn_h = 22.0;
    let btn_gap = 6.0;
    let btn_padding_x = 8.0;
    let btn_min_w = 40.0;
    let row_cy = y + profile_row_h / 2.0;
    let id = state.profile_id.as_str();

    // ── Avatar dot only — no radio dot, there is nothing to switch to. ──
    let dot_r = 5.0;
    let dot_cx = x + dot_r;
    draw_avatar_dot(ctx, dot_cx, row_cy, dot_r, avatar_color(&state.profile_avatar));

    let name_x = dot_cx + dot_r + 8.0;
    let right_edge = x + available_w;
    let is_renaming = state.account_signed_in
        && state.profile_rename_mode
        && state.profile_rename_target_id.as_deref() == Some(id);

    if is_renaming {
        // Inline rename input + Save/Cancel — same idiom as the multi-profile
        // row's own rename mode, minus the Avatar/Delete columns that never
        // exist at this tier.
        let save_label   = TextKey::Save.get(current_language());
        let cancel_label = TextKey::Cancel.get(current_language());

        ctx.set_font("11px sans-serif");
        let save_w   = auto_btn_width(ctx, save_label,   "11px sans-serif", btn_padding_x, btn_min_w);
        let cancel_w = auto_btn_width(ctx, cancel_label, "11px sans-serif", btn_padding_x, btn_min_w);
        let avail_btns = (right_edge - name_x).max(0.0);
        let (save_w, cancel_w) = {
            let total = save_w + cancel_w + btn_gap;
            if total > avail_btns && total > 0.0 {
                let scale = avail_btns / total;
                (save_w * scale, cancel_w * scale)
            } else {
                (save_w, cancel_w)
            }
        };
        let cancel_btn_x = right_edge - cancel_w;
        let confirm_btn_x = cancel_btn_x - save_w - btn_gap;

        let input_w = (confirm_btn_x - name_x - btn_gap).max(0.0);
        let input_h = 22.0;
        let input_y = y + (profile_row_h - input_h) / 2.0;
        let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

        let pr_render = &state.profile_rename_render;
        let (sel_start, sel_end) = if let Some((lo, hi)) = pr_render.selection_range() {
            (Some(lo), Some(hi))
        } else {
            (None, None)
        };
        let rename_input_config = InputConfig::new(&pr_render.text)
            .with_focused(state.profile_rename_focused)
            .with_cursor(pr_render.cursor)
            .with_placeholder("Profile name...")
            .with_selection(sel_start, sel_end);
        let rename_input_result = draw_input(
            ctx, &rename_input_config, WidgetState::Normal,
            WidgetRect::new(name_x, input_y, input_w, input_h), &widget_theme,
        );

        result.content_items.push((
            "user_settings:profile_rename_input".to_string(),
            WidgetRect::new(name_x, input_y, input_w, input_h),
        ));
        result.input_char_positions.push((
            "user_settings:profile_rename_input".to_string(),
            rename_input_result.char_x_positions.clone(),
        ));
        input_coordinator.register_on_layer(
            "user_settings:profile_rename_input",
            uzor::types::Rect::new(name_x, input_y, input_w, input_h),
            Sense::CLICK,
            layer_id,
        );

        if state.profile_rename_focused && pr_render.cursor_visible {
            draw_input_cursor(
                ctx,
                rename_input_result.cursor_x,
                rename_input_result.cursor_y,
                rename_input_result.cursor_height,
                text_color,
            );
        }

        let btn_y = y + (profile_row_h - btn_h) / 2.0;

        // Save button (green tint)
        ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.success, 0.2));
        ctx.fill_rounded_rect(confirm_btn_x, btn_y, save_w, btn_h, 3.0);
        ctx.set_stroke_color(&mlc_core::apply_opacity(&toolbar_theme.success, 0.6));
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(confirm_btn_x, btn_y, save_w, btn_h, 3.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.success);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(fit_text_to_width(ctx, save_label, save_w - btn_padding_x).as_str(), confirm_btn_x + save_w / 2.0, btn_y + btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        result.content_items.push(("profile_rename_confirm".to_string(), WidgetRect::new(confirm_btn_x, btn_y, save_w, btn_h)));
        input_coordinator.register_on_layer(
            "user_settings:profile_rename_confirm",
            uzor::types::Rect::new(confirm_btn_x, btn_y, save_w, btn_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );

        // Cancel button
        ctx.set_fill_color(&toolbar_theme.item_bg_hover);
        ctx.fill_rounded_rect(cancel_btn_x, btn_y, cancel_w, btn_h, 3.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(cancel_btn_x, btn_y, cancel_w, btn_h, 3.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, 0.7));
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(fit_text_to_width(ctx, cancel_label, cancel_w - btn_padding_x).as_str(), cancel_btn_x + cancel_w / 2.0, btn_y + btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        result.content_items.push(("profile_rename_cancel".to_string(), WidgetRect::new(cancel_btn_x, btn_y, cancel_w, btn_h)));
        input_coordinator.register_on_layer(
            "user_settings:profile_rename_cancel",
            uzor::types::Rect::new(cancel_btn_x, btn_y, cancel_w, btn_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    } else {
        // Normal display: name, plus the two identity controls a real
        // signed-in account gets — a nickname and an avatar. An anonymous
        // identity has neither: its name is fixed and there is nothing to
        // personalise, so no buttons, no hit ids, nothing for a stray click
        // to reach.
        //
        // Avatar sits here rather than with the profile-management controls
        // because it is identity, not management: it says who this is, not
        // how many of them there may be.
        let avail_btns = (right_edge - name_x).max(0.0);
        let rename = state.account_signed_in.then(|| {
            let label = UserSettingsKey::BtnNickname.get(current_language());
            ctx.set_font("11px sans-serif");
            let w = auto_btn_width(ctx, label, "11px sans-serif", btn_padding_x, btn_min_w).min(avail_btns.max(btn_min_w));
            (label, w)
        });
        let avatar = state.account_signed_in.then(|| {
            let label = UserSettingsKey::BtnAvatar.get(current_language());
            ctx.set_font("11px sans-serif");
            let w = auto_btn_width(ctx, label, "11px sans-serif", btn_padding_x, btn_min_w).min(avail_btns.max(btn_min_w));
            (label, w)
        });

        ctx.set_font("600 13px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&state.profile_display_name, name_x, row_cy);

        // Right-aligned, avatar outermost: [ nickname ][ avatar ]
        let avatar_w = avatar.map(|(_, w)| w).unwrap_or(0.0);
        let avatar_btn_x = right_edge - avatar_w;
        let btn_y = y + (profile_row_h - btn_h) / 2.0;
        if let Some((rename_label, rename_w)) = rename {
            let rename_btn_x = if avatar.is_some() {
                (avatar_btn_x - btn_gap - rename_w).max(name_x)
            } else {
                right_edge - rename_w
            };

            let rename_hover_id = format!("profile_rename:{}", id);
            let is_rename_hovered = state.hovered_item_id.as_deref() == Some(rename_hover_id.as_str());
            let rename_bg = if is_rename_hovered {
                mlc_core::apply_opacity(&toolbar_theme.item_text, 0.12)
            } else {
                toolbar_theme.item_bg_hover.clone()
            };
            ctx.set_fill_color(&rename_bg);
            ctx.fill_rounded_rect(rename_btn_x, btn_y, rename_w, btn_h, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(rename_btn_x, btn_y, rename_w, btn_h, 3.0);
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, if is_rename_hovered { 0.95 } else { 0.7 }));
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(fit_text_to_width(ctx, rename_label, rename_w - btn_padding_x).as_str(), rename_btn_x + rename_w / 2.0, btn_y + btn_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            let rename_hit_id = format!("user_settings:profile_rename:{}", id);
            result.content_items.push((format!("profile_rename:{}", id), WidgetRect::new(rename_btn_x, btn_y, rename_w, btn_h)));
            input_coordinator.register_on_layer(
                rename_hit_id.as_str(),
                uzor::types::Rect::new(rename_btn_x, btn_y, rename_w, btn_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        if let Some((avatar_label, avatar_w)) = avatar {
            let toggle_hover_id = format!("profile_avatar_toggle:{}", id);
            let is_hovered = state.hovered_item_id.as_deref() == Some(toggle_hover_id.as_str());
            let bg = if is_hovered {
                mlc_core::apply_opacity(&toolbar_theme.item_text, 0.12)
            } else {
                toolbar_theme.item_bg_hover.clone()
            };
            ctx.set_fill_color(&bg);
            ctx.fill_rounded_rect(avatar_btn_x, btn_y, avatar_w, btn_h, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(avatar_btn_x, btn_y, avatar_w, btn_h, 3.0);
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, if is_hovered { 0.95 } else { 0.7 }));
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(fit_text_to_width(ctx, avatar_label, avatar_w - btn_padding_x).as_str(), avatar_btn_x + avatar_w / 2.0, btn_y + btn_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push((toggle_hover_id.clone(), WidgetRect::new(avatar_btn_x, btn_y, avatar_w, btn_h)));
            input_coordinator.register_on_layer(
                format!("user_settings:{}", toggle_hover_id).as_str(),
                uzor::types::Rect::new(avatar_btn_x, btn_y, avatar_w, btn_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }
    }

    // The picker drops below the row when the toggle above opened it. Same
    // eight avatars and the same `profile_avatar:{name}` ids the multi-profile
    // list uses — one avatar vocabulary, not two.
    let picker_open = state.account_signed_in
        && state.show_avatar_picker
        && state.profile_avatar_target_id.as_deref().map(|t| t == id).unwrap_or(true);
    if !picker_open {
        return y + profile_row_h;
    }

    let avatars = [
        "chart", "rocket", "shield", "fire",
        "star",  "moon",   "sun",    "ghost",
    ];
    let cell_size = 28.0;
    let picker_w = cell_size * avatars.len() as f64;
    let picker_h = cell_size + 8.0;
    let picker_y = y + profile_row_h;

    ctx.set_fill_color(&toolbar_theme.item_bg_hover);
    ctx.fill_rounded_rect(x, picker_y, picker_w, picker_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, picker_y, picker_w, picker_h, 4.0);

    for (i, av) in avatars.iter().enumerate() {
        let cell_x = x + i as f64 * cell_size;
        if *av == state.profile_avatar.as_str() {
            ctx.set_fill_color(&toolbar_theme.item_bg_active);
            ctx.fill_rounded_rect(cell_x + 2.0, picker_y + 2.0, cell_size - 4.0, picker_h - 4.0, 3.0);
        }
        draw_avatar_dot(ctx, cell_x + cell_size / 2.0, picker_y + picker_h / 2.0, 7.0, avatar_color(av));
        result.content_items.push((format!("profile_avatar:{}", av), WidgetRect::new(cell_x, picker_y, cell_size, picker_h)));
        input_coordinator.register_on_layer(
            format!("user_settings:profile_avatar:{}", av).as_str(),
            uzor::types::Rect::new(cell_x, picker_y, cell_size, picker_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }

    y + profile_row_h + picker_h + 4.0
}

#[allow(clippy::too_many_arguments)]
fn render_profile_section(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    available_w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    _current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) -> f64 {
    // Below the paid tier there is no profile LIST at all — just the one
    // active identity (owner's ruling, `UserSettingsState::multi_profile`'s
    // own doc). `render_single_profile_row` is the entire section in that
    // case; the loop below (unabridged multi-profile surface) only ever
    // runs when the feature is actually entitled.
    if !state.multi_profile {
        return render_single_profile_row(
            ctx, x, y, available_w, state, text_color, toolbar_theme, frame_theme,
            input_coordinator, layer_id, result,
        );
    }

    let mut cy = y;

    // ── Unified profile list ───────────────────────────────────────────────────
    // Each row: [radio dot] [avatar dot] Name (mode) [Rename] [Avatar] -or- [Delete]
    // Active row shows Rename + Avatar buttons; inactive rows show Delete + are clickable.
    let profile_row_h = 32.0;
    let btn_h = 22.0;
    let btn_gap = 6.0;
    let btn_padding_x = 8.0;
    let btn_min_w = 40.0;

    for (id, name, avatar) in &state.available_profiles {
        // Use runtime_profile_id (the ACTUALLY loaded profile) so that buttons
        // remain on the correct row even after a pending profile_switch.
        let is_active = *id == state.runtime_profile_id;
        let is_renaming = state.profile_rename_mode
            && state.profile_rename_target_id.as_deref() == Some(id.as_str());
        let is_avatar_open = state.show_avatar_picker
            && state.profile_avatar_target_id.as_deref() == Some(id.as_str());

        let row_cy = cy + profile_row_h / 2.0;

        // ── Row hover highlight for inactive rows ──
        if !is_active {
            let row_hover_id = format!("profile_switch:{}", id);
            if state.hovered_item_id.as_deref() == Some(row_hover_id.as_str()) {
                ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, 0.04));
                ctx.fill_rounded_rect(x, cy, available_w, profile_row_h, 3.0);
            }
        }

        // ── Radio dot (filled = active, empty ring = inactive) ──
        let radio_r = 5.0;
        let radio_cx = x + radio_r + 2.0;
        let radio_cy = row_cy;
        if is_active {
            // Outer ring in accent color
            ctx.set_stroke_color(&toolbar_theme.accent);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.arc(radio_cx, radio_cy, radio_r, 0.0, std::f64::consts::TAU);
            ctx.stroke();
            // Inner fill dot
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.begin_path();
            ctx.arc(radio_cx, radio_cy, radio_r - 2.5, 0.0, std::f64::consts::TAU);
            ctx.fill();
        } else {
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.5);
            ctx.begin_path();
            ctx.arc(radio_cx, radio_cy, radio_r, 0.0, std::f64::consts::TAU);
            ctx.stroke();
        }

        // ── Avatar dot ──
        let dot_r = 5.0;
        let dot_cx = radio_cx + radio_r + 8.0 + dot_r;
        let dot_cy = row_cy;
        draw_avatar_dot(ctx, dot_cx, dot_cy, dot_r, avatar_color(avatar));

        // ── Name + mode label ──
        let name_x = dot_cx + dot_r + 8.0;
        let name_alpha = if is_active { 1.0 } else { 0.65 };
        let row_name_color = mlc_core::apply_opacity(&toolbar_theme.item_text, name_alpha);

        // Right-side buttons layout (computed from right edge, i18n-aware auto-width)
        // All rows: [Rename] [Avatar] [Delete]
        // Active row: Delete is hidden (can't delete the running profile).
        // Inactive row: all three buttons visible + whole-row click area for switch.
        let right_edge = x + available_w;

        // Measure per-row button widths from actual translated labels.
        let rename_label = UserSettingsKey::BtnRename.get(current_language());
        let avatar_label = UserSettingsKey::BtnAvatar.get(current_language());
        let delete_label = TextKey::Delete.get(current_language());
        let save_label   = TextKey::Save.get(current_language());
        let cancel_label = TextKey::Cancel.get(current_language());

        ctx.set_font("11px sans-serif");
        let rename_w = auto_btn_width(ctx, rename_label, "11px sans-serif", btn_padding_x, btn_min_w);
        let avatar_w = auto_btn_width(ctx, avatar_label, "11px sans-serif", btn_padding_x, btn_min_w);
        let delete_w = auto_btn_width(ctx, delete_label, "11px sans-serif", btn_padding_x, btn_min_w);
        let save_w   = auto_btn_width(ctx, save_label,   "11px sans-serif", btn_padding_x, btn_min_w);
        let cancel_w = auto_btn_width(ctx, cancel_label, "11px sans-serif", btn_padding_x, btn_min_w);

        // Available horizontal space for buttons (from name_x to right_edge).
        let avail_btns = (right_edge - name_x).max(0.0);

        // For inactive rows: clamp each width if total would overflow.
        let (rename_w, avatar_w, delete_w) = {
            let total = rename_w + avatar_w + delete_w + btn_gap * 2.0;
            if total > avail_btns && total > 0.0 {
                let scale = avail_btns / total;
                (rename_w * scale, avatar_w * scale, delete_w * scale)
            } else {
                (rename_w, avatar_w, delete_w)
            }
        };
        // For active rows (no Delete): clamp rename+avatar.
        let (rename_w_active, avatar_w_active) = {
            let total = rename_w + avatar_w + btn_gap;
            if total > avail_btns && total > 0.0 {
                let scale = avail_btns / total;
                (rename_w * scale, avatar_w * scale)
            } else {
                (rename_w, avatar_w)
            }
        };
        // For rename mode: clamp save+cancel.
        let (save_w, cancel_w) = {
            let total = save_w + cancel_w + btn_gap;
            if total > avail_btns && total > 0.0 {
                let scale = avail_btns / total;
                (save_w * scale, cancel_w * scale)
            } else {
                (save_w, cancel_w)
            }
        };

        // Positions right-to-left for each mode:
        // inactive: delete | avatar | rename (leftmost)
        let delete_btn_x = right_edge - delete_w;
        let avatar_btn_x_inactive = delete_btn_x - avatar_w - btn_gap;
        let rename_btn_x_inactive = avatar_btn_x_inactive - rename_w - btn_gap;
        // active (no delete): avatar | rename (leftmost)
        let avatar_btn_x_active = right_edge - avatar_w_active;
        let rename_btn_x_active  = avatar_btn_x_active - rename_w_active - btn_gap;
        // rename mode: cancel | save (leftmost)
        let cancel_btn_x = right_edge - cancel_w;
        let confirm_btn_x = cancel_btn_x - save_w - btn_gap;

        // Per-row convenience aliases resolved after is_active check below.
        let (rename_btn_x, avatar_btn_x, cur_rename_w, cur_avatar_w) = if is_active {
            (rename_btn_x_active, avatar_btn_x_active, rename_w_active, avatar_w_active)
        } else {
            (rename_btn_x_inactive, avatar_btn_x_inactive, rename_w, avatar_w)
        };
        if is_renaming {
            // Inline rename input replaces name text
            let input_w = confirm_btn_x - name_x - btn_gap;
            let input_h = 22.0;
            let input_y = cy + (profile_row_h - input_h) / 2.0;

            let input_rect = WidgetRect::new(name_x, input_y, input_w, input_h);
            let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

            let pr_render = &state.profile_rename_render;
            let (sel_start, sel_end) = if let Some((lo, hi)) = pr_render.selection_range() {
                (Some(lo), Some(hi))
            } else {
                (None, None)
            };
            let rename_input_config = InputConfig::new(&pr_render.text)
                .with_focused(state.profile_rename_focused)
                .with_cursor(pr_render.cursor)
                .with_placeholder("Profile name...")
                .with_selection(sel_start, sel_end);

            let rename_input_result = draw_input(ctx, &rename_input_config, WidgetState::Normal, input_rect, &widget_theme);

            // Surface outer rect + char positions to the embedder so it can feed
            // them to TextFieldStore.update_field every frame (required for
            // click-to-cursor and drag-select to land on the right glyph).
            result.content_items.push((
                "user_settings:profile_rename_input".to_string(),
                WidgetRect::new(name_x, input_y, input_w, input_h),
            ));
            result.input_char_positions.push((
                "user_settings:profile_rename_input".to_string(),
                rename_input_result.char_x_positions.clone(),
            ));

            // Register click target so mouse clicks focus this input
            input_coordinator.register_on_layer(
                "user_settings:profile_rename_input",
                uzor::types::Rect::new(name_x, input_y, input_w, input_h),
                Sense::CLICK,
                layer_id,
            );

            // Draw blinking cursor when focused
            if state.profile_rename_focused && pr_render.cursor_visible {
                draw_input_cursor(
                    ctx,
                    rename_input_result.cursor_x,
                    rename_input_result.cursor_y,
                    rename_input_result.cursor_height,
                    text_color,
                );
            }

            let btn_y = cy + (profile_row_h - btn_h) / 2.0;

            // Save button (green tint)
            ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.success, 0.2));
            ctx.fill_rounded_rect(confirm_btn_x, btn_y, save_w, btn_h, 3.0);
            ctx.set_stroke_color(&mlc_core::apply_opacity(&toolbar_theme.success, 0.6));
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(confirm_btn_x, btn_y, save_w, btn_h, 3.0);
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&toolbar_theme.success);
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(fit_text_to_width(ctx, save_label, save_w - btn_padding_x).as_str(), confirm_btn_x + save_w / 2.0, btn_y + btn_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push(("profile_rename_confirm".to_string(), WidgetRect::new(confirm_btn_x, btn_y, save_w, btn_h)));
            input_coordinator.register_on_layer(
                "user_settings:profile_rename_confirm",
                uzor::types::Rect::new(confirm_btn_x, btn_y, save_w, btn_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );

            // Cancel button
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(cancel_btn_x, btn_y, cancel_w, btn_h, 3.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(cancel_btn_x, btn_y, cancel_w, btn_h, 3.0);
            ctx.set_font("11px sans-serif");
            ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, 0.7));
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(fit_text_to_width(ctx, cancel_label, cancel_w - btn_padding_x).as_str(), cancel_btn_x + cancel_w / 2.0, btn_y + btn_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push(("profile_rename_cancel".to_string(), WidgetRect::new(cancel_btn_x, btn_y, cancel_w, btn_h)));
            input_coordinator.register_on_layer(
                "user_settings:profile_rename_cancel",
                uzor::types::Rect::new(cancel_btn_x, btn_y, cancel_w, btn_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        } else {
            // Normal display: name + mode label
            ctx.set_font(if is_active { "600 13px sans-serif" } else { "13px sans-serif" });
            ctx.set_fill_color(row_name_color.as_str());
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(name.as_str(), name_x, row_cy);

            let btn_y = cy + (profile_row_h - btn_h) / 2.0;

            // ── Rename button (shown on every row) ──
            {
                let rename_hover_id = format!("profile_rename:{}", id);
                let is_rename_hovered = state.hovered_item_id.as_deref() == Some(rename_hover_id.as_str());
                let rename_bg = if is_rename_hovered {
                    mlc_core::apply_opacity(&toolbar_theme.item_text, 0.12)
                } else {
                    toolbar_theme.item_bg_hover.clone()
                };
                ctx.set_fill_color(&rename_bg);
                ctx.fill_rounded_rect(rename_btn_x, btn_y, cur_rename_w, btn_h, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(rename_btn_x, btn_y, cur_rename_w, btn_h, 3.0);
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, if is_rename_hovered { 0.95 } else { 0.7 }));
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(fit_text_to_width(ctx, rename_label, cur_rename_w - btn_padding_x).as_str(), rename_btn_x + cur_rename_w / 2.0, btn_y + btn_h / 2.0);
                ctx.set_text_align(TextAlign::Left);

                let rename_hit_id = format!("user_settings:profile_rename:{}", id);
                result.content_items.push((format!("profile_rename:{}", id), WidgetRect::new(rename_btn_x, btn_y, cur_rename_w, btn_h)));
                input_coordinator.register_on_layer(
                    rename_hit_id.as_str(),
                    uzor::types::Rect::new(rename_btn_x, btn_y, cur_rename_w, btn_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }

            // ── Avatar button (shown on every row) ──
            {
                let avatar_toggle_hover_id = format!("profile_avatar_toggle:{}", id);
                let is_avatar_hovered = state.hovered_item_id.as_deref() == Some(avatar_toggle_hover_id.as_str());
                let avatar_bg = if is_avatar_hovered {
                    mlc_core::apply_opacity(&toolbar_theme.item_text, 0.12)
                } else {
                    toolbar_theme.item_bg_hover.clone()
                };
                ctx.set_fill_color(&avatar_bg);
                ctx.fill_rounded_rect(avatar_btn_x, btn_y, cur_avatar_w, btn_h, 3.0);
                ctx.set_stroke_color(&toolbar_theme.separator);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(avatar_btn_x, btn_y, cur_avatar_w, btn_h, 3.0);
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, if is_avatar_hovered { 0.95 } else { 0.7 }));
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(fit_text_to_width(ctx, avatar_label, cur_avatar_w - btn_padding_x).as_str(), avatar_btn_x + cur_avatar_w / 2.0, btn_y + btn_h / 2.0);
                ctx.set_text_align(TextAlign::Left);

                let avatar_hit_id = format!("user_settings:profile_avatar_toggle:{}", id);
                result.content_items.push((format!("profile_avatar_toggle:{}", id), WidgetRect::new(avatar_btn_x, btn_y, cur_avatar_w, btn_h)));
                input_coordinator.register_on_layer(
                    avatar_hit_id.as_str(),
                    uzor::types::Rect::new(avatar_btn_x, btn_y, cur_avatar_w, btn_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }

            if !is_active {
                // ── Delete button (only on inactive rows — can't delete the running profile) ──
                let delete_hover_id = format!("profile_delete:{}", id);
                let is_delete_hovered = state.hovered_item_id.as_deref() == Some(delete_hover_id.as_str());
                let delete_bg = if is_delete_hovered {
                    mlc_core::apply_opacity(&toolbar_theme.danger, 0.2)
                } else {
                    toolbar_theme.item_bg_hover.clone()
                };
                ctx.set_fill_color(&delete_bg);
                ctx.fill_rounded_rect(delete_btn_x, btn_y, delete_w, btn_h, 3.0);
                let delete_border = if is_delete_hovered {
                    mlc_core::apply_opacity(&toolbar_theme.danger, 0.5)
                } else {
                    toolbar_theme.separator.clone()
                };
                ctx.set_stroke_color(&delete_border);
                ctx.set_stroke_width(1.0);
                ctx.stroke_rounded_rect(delete_btn_x, btn_y, delete_w, btn_h, 3.0);
                ctx.set_font("11px sans-serif");
                ctx.set_fill_color(&if is_delete_hovered {
                    mlc_core::apply_opacity(&toolbar_theme.danger, 0.95)
                } else {
                    mlc_core::apply_opacity(&toolbar_theme.item_text, 0.5)
                });
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(fit_text_to_width(ctx, delete_label, delete_w - btn_padding_x).as_str(), delete_btn_x + delete_w / 2.0, btn_y + btn_h / 2.0);
                ctx.set_text_align(TextAlign::Left);

                let delete_hit_id = format!("user_settings:profile_delete:{}", id);
                result.content_items.push((format!("profile_delete:{}", id), WidgetRect::new(delete_btn_x, btn_y, delete_w, btn_h)));
                input_coordinator.register_on_layer(
                    delete_hit_id.as_str(),
                    uzor::types::Rect::new(delete_btn_x, btn_y, delete_w, btn_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );

                // Whole-row click area for switching (excludes button area on the right)
                let row_click_w = rename_btn_x - x - btn_gap;
                let switch_hit_id = format!("user_settings:profile_switch:{}", id);
                result.content_items.push((format!("profile_switch:{}", id), WidgetRect::new(x, cy, row_click_w, profile_row_h)));
                input_coordinator.register_on_layer(
                    switch_hit_id.as_str(),
                    uzor::types::Rect::new(x, cy, row_click_w, profile_row_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }
        }

        // ── Avatar picker popover (inline, below current row) ──
        if is_avatar_open {
            let avatars = [
                "chart", "rocket", "shield", "fire",
                "star",  "moon",   "sun",    "ghost",
            ];
            let cell_size = 28.0;
            let picker_cols = 8usize;
            let picker_w = cell_size * picker_cols as f64;
            let picker_h = cell_size + 8.0;
            let picker_x = x;
            let picker_y = cy + profile_row_h;

            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(picker_x, picker_y, picker_w, picker_h, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(picker_x, picker_y, picker_w, picker_h, 4.0);

            // Determine current avatar for the target profile
            let current_avatar = state.profile_avatar_target_id.as_deref()
                .and_then(|tid| state.available_profiles.iter().find(|(pid, _, _)| pid == tid))
                .map(|(_, _, av)| av.as_str())
                .unwrap_or(state.profile_avatar.as_str());

            for (i, av) in avatars.iter().enumerate() {
                let cell_x = picker_x + i as f64 * cell_size;
                let cell_cx = cell_x + cell_size / 2.0;
                let cell_cy = picker_y + picker_h / 2.0;
                let is_selected = *av == current_avatar;

                if is_selected {
                    ctx.set_fill_color(&toolbar_theme.item_bg_active);
                    ctx.fill_rounded_rect(cell_x + 2.0, picker_y + 2.0, cell_size - 4.0, picker_h - 4.0, 3.0);
                }

                draw_avatar_dot(ctx, cell_cx, cell_cy, 7.0, avatar_color(av));

                let hit_id = format!("user_settings:profile_avatar:{}", av);
                result.content_items.push((format!("profile_avatar:{}", av), WidgetRect::new(cell_x, picker_y, cell_size, picker_h)));
                input_coordinator.register_on_layer(
                    hit_id.as_str(),
                    uzor::types::Rect::new(cell_x, picker_y, cell_size, picker_h),
                    Sense::CLICK | Sense::HOVER,
                    layer_id,
                );
            }
            cy += picker_h + 4.0;
        }

        cy += profile_row_h;
    }

    // ── Separator ─────────────────────────────────────────────────────────────
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(x, cy);
    ctx.line_to(x + available_w, cy);
    ctx.stroke();
    cy += 8.0;

    // ── "New Profile" dialog / button ─────────────────────────────────────────
    if state.show_new_profile_dialog {
        let input_h = 24.0;
        let input_w = available_w - 60.0 - 6.0;

        let new_name_input_rect = WidgetRect::new(x, cy, input_w, input_h);
        let widget_theme_new = toolbar_to_widget_theme(toolbar_theme, frame_theme);

        let npn_us_render = &state.new_profile_name_render;
        let (new_sel_start, new_sel_end) = if let Some((lo, hi)) = npn_us_render.selection_range() {
            (Some(lo), Some(hi))
        } else {
            (None, None)
        };
        let new_name_input_config = InputConfig::new(&npn_us_render.text)
            .with_focused(state.new_profile_name_focused)
            .with_cursor(npn_us_render.cursor)
            .with_placeholder("Profile name...")
            .with_selection(new_sel_start, new_sel_end);

        let new_name_input_result = draw_input(ctx, &new_name_input_config, WidgetState::Normal, new_name_input_rect, &widget_theme_new);

        // Register click target so mouse clicks focus this input
        input_coordinator.register_on_layer(
            "user_settings:new_profile_name_input",
            uzor::types::Rect::new(x, cy, input_w, input_h),
            Sense::CLICK,
            layer_id,
        );

        // Draw blinking cursor when focused
        if state.new_profile_name_focused && npn_us_render.cursor_visible {
            draw_input_cursor(
                ctx,
                new_name_input_result.cursor_x,
                new_name_input_result.cursor_y,
                new_name_input_result.cursor_height,
                text_color,
            );
        }

        // Cancel (X) button — on same row as name input
        let cancel_x = x + input_w + 6.0;
        if cancel_x + 24.0 <= x + available_w {
            ctx.set_fill_color(&toolbar_theme.item_bg_hover);
            ctx.fill_rounded_rect(cancel_x, cy, 24.0, input_h, 3.0);
            ctx.set_font("12px sans-serif");
            ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, 0.5));
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("✕", cancel_x + 12.0, cy + input_h / 2.0);
            ctx.set_text_align(TextAlign::Left);

            result.content_items.push(("profile_new_cancel".to_string(), WidgetRect::new(cancel_x, cy, 24.0, input_h)));
            input_coordinator.register_on_layer(
                "user_settings:profile_new_cancel",
                uzor::types::Rect::new(cancel_x, cy, 24.0, input_h),
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        cy += input_h + 4.0;

        // Create button — full row
        ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.success, 0.2));
        ctx.fill_rounded_rect(x, cy, available_w, input_h, 3.0);
        ctx.set_stroke_color(&mlc_core::apply_opacity(&toolbar_theme.success, 0.6));
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, cy, available_w, input_h, 3.0);
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(&toolbar_theme.success);
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(UserSettingsKey::BtnCreate.get(current_language()), x + available_w / 2.0, cy + input_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        result.content_items.push(("profile_new_confirm".to_string(), WidgetRect::new(x, cy, available_w, input_h)));
        input_coordinator.register_on_layer(
            "user_settings:profile_new_confirm",
            uzor::types::Rect::new(x, cy, available_w, input_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );

        cy += input_h + 6.0;
    } else {
        // "+ New Profile" button
        let new_btn_h = 24.0;
        let new_btn_w = available_w.min(140.0);
        let is_new_profile_hovered = state.hovered_item_id.as_deref() == Some("profile_new");
        let new_profile_bg = if is_new_profile_hovered {
            mlc_core::apply_opacity(&toolbar_theme.item_text, 0.12)
        } else {
            toolbar_theme.item_bg_hover.clone()
        };
        ctx.set_fill_color(&new_profile_bg);
        ctx.fill_rounded_rect(x, cy, new_btn_w, new_btn_h, 3.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(x, cy, new_btn_w, new_btn_h, 3.0);
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(&mlc_core::apply_opacity(&toolbar_theme.item_text, if is_new_profile_hovered { 0.95 } else { 0.7 }));
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(UserSettingsKey::BtnNewProfile.get(current_language()), x + new_btn_w / 2.0, cy + new_btn_h / 2.0);
        ctx.set_text_align(TextAlign::Left);

        result.content_items.push(("profile_new".to_string(), WidgetRect::new(x, cy, new_btn_w, new_btn_h)));
        input_coordinator.register_on_layer(
            "user_settings:profile_new",
            uzor::types::Rect::new(x, cy, new_btn_w, new_btn_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );

        cy += new_btn_h;
    }

    // Suppress unused variable warning for avatar_tag helper (used for future text-only rendering).
    let _ = avatar_tag;

    cy
}

// =============================================================================
// Performance tab renderer
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_performance_tab(
    ctx: &mut dyn RenderContext,
    viewport_rect: WidgetRect,
    state: &UserSettingsState,
    toolbar_theme: &ToolbarTheme,
    scroll_widget_theme: &WidgetTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    let container = ScrollableContainer::new(
        viewport_rect,
        &state.performance_tab_scroll,
        None,
    );
    container.begin(ctx);
    let x = viewport_rect.x;
    let available_w = container.content_width();
    let mut cy = container.content_y();

    // Build WidgetTheme from toolbar colours
    let widget_theme = WidgetTheme {
        bg_normal:     toolbar_theme.item_bg_hover.clone(),
        bg_hover:      toolbar_theme.item_bg_hover.clone(),
        bg_pressed:    toolbar_theme.item_bg_active.clone(),
        bg_disabled:   toolbar_theme.item_bg_hover.clone(),
        text_normal:   toolbar_theme.item_text.clone(),
        text_hover:    toolbar_theme.item_text_active.clone(),
        text_disabled: toolbar_theme.item_text_muted.clone(),
        border_normal: toolbar_theme.separator.clone(),
        border_hover:  toolbar_theme.separator.clone(),
        border_focused: toolbar_theme.item_bg_active.clone(),
        accent:        toolbar_theme.accent.clone(),
        accent_hover:  toolbar_theme.accent.clone(),
        success:       toolbar_theme.success.clone(),
        warning:       toolbar_theme.warning.clone(),
        danger:        toolbar_theme.danger.clone(),
    };

    // Section label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionIndicatorRecalc.get(current_language()), x, cy);
    cy += 22.0;

    let options = [
        RadioOption {
            key: "PerFrame",
            label: "Per Frame",
            description: "Recalculate once per rendered frame. Best balance of accuracy and performance.",
        },
        RadioOption {
            key: "PerTick",
            label: "Per Tick",
            description: "Recalculate on every incoming trade. Most accurate, higher CPU. Best for agents.",
        },
        RadioOption {
            key: "PerBar",
            label: "Per Bar",
            description: "Recalculate only when a new bar closes. Minimal CPU, suitable for long timeframes.",
        },
    ];

    let selected_index = match state.recalc_mode_label.as_str() {
        "Per Tick" => 1,
        "Per Bar"  => 2,
        _          => 0, // Per Frame default
    };

    let radio_result = draw_radio_group(
        ctx,
        &options,
        selected_index,
        state.hovered_item_id.as_deref(),
        x,
        cy,
        available_w,
        &widget_theme,
        false, // full rows — these options have descriptions
    );

    // Register a hit zone for each radio option row
    for (i, (rx, ry, rw, rh)) in radio_result.option_rects.iter().enumerate() {
        let hit_id = format!("user_settings:recalc_mode:{}", options[i].key);
        result.content_items.push((
            format!("recalc_mode:{}", options[i].key),
            WidgetRect::new(*rx, *ry, *rw, *rh),
        ));
        input_coordinator.register_on_layer(
            hit_id.as_str(),
            uzor::types::Rect::new(*rx, *ry, *rw, *rh),
            uzor::input::Sense::CLICK | uzor::input::Sense::HOVER,
            layer_id,
        );
    }

    // Advance cy past the radio group
    cy += options.len() as f64 * (52.0 + 8.0) - 8.0 + 24.0;

    // ── Diagnostics toggle ────────────────────────────────────────────────────
    // Section label
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionDiagnostics.get(current_language()), x, cy);

    let row_h = 24.0;
    let cb_y_offset = cy + 18.0;

    // Checkbox (16 × 16)
    let cb_size = 16.0;
    let cb_x = x;
    let cb_y = cb_y_offset + (row_h - cb_size) / 2.0;

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cb_x, cb_y, cb_size, cb_size, 2.0);

    if state.diagnostics_enabled {
        ctx.set_fill_color(&toolbar_theme.item_text_active);
        ctx.fill_rounded_rect(cb_x + 3.0, cb_y + 3.0, cb_size - 6.0, cb_size - 6.0, 1.0);
    }

    // Label + description
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        UserSettingsKey::EnableDiagnosticLogging.get(current_language()),
        cb_x + cb_size + 10.0,
        cb_y_offset + row_h / 2.0,
    );

    let desc_y = cb_y_offset + row_h + 2.0;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(
        "Logs trade and recalculation counts every 5 seconds.",
        cb_x + cb_size + 10.0,
        desc_y,
    );

    // Hit zone covers the label row
    let toggle_rect = WidgetRect::new(cb_x, cb_y_offset, available_w, row_h);
    result.content_items.push(("diagnostics_toggle".to_string(), toggle_rect));
    input_coordinator.register_on_layer(
        "user_settings:diagnostics_toggle",
        uzor::types::Rect::new(cb_x, cb_y_offset, available_w, row_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );

    cy = desc_y + 15.0;

    // ── DATA & CACHE ──────────────────────────────────────────────────────────
    cy += 24.0;
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionDataCache.get(current_language()), x, cy);
    cy += 22.0;

    let content_w = available_w;
    let slider_h = 28.0;
    let desc_gap = 14.0;

    // Helper: display value is the committed value, unless a drag preview is active.
    let floating = state.data_slider_floating();

    // ── Slider 1: Background bars (300–10000) ──────────────────────────────
    let bg_bars_val = if let Some(("data_bg_bars", v)) = floating { v } else { state.data_bg_bars as f64 };
    let bg_bars_config = SliderConfig::new(300.0, 10000.0).with_step(100.0);
    let bg_bars_rect = WidgetRect::new(x, cy, content_w, slider_h);
    let bg_bars_result = render_single_slider(
        ctx,
        &bg_bars_config,
        bg_bars_val,
        bg_bars_rect,
        UserSettingsKey::SliderBgBars.get(current_language()),
        &widget_theme,
        false,
        None,
    );
    if let Some(track_info) = bg_bars_result.track_info {
        result.slider_tracks.push(SliderTrackInfo::new(
            "data_bg_bars",
            track_info.track_x,
            track_info.track_width,
            track_info.min_val,
            track_info.max_val,
        ));
        result.content_items.push(("data_bg_bars".to_string(), bg_bars_rect));
        input_coordinator.register_on_layer(
            "user_settings:data_bg_bars",
            uzor::types::Rect::new(x, cy, content_w, slider_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    cy += slider_h;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::DescBgBars.get(current_language()), x + 4.0, cy);
    cy += desc_gap;

    // ── Slider 2: Max bars in memory (0–50000) ────────────────────────────
    cy += 8.0;
    let max_bars_val = if let Some(("data_max_bars", v)) = floating { v } else { state.data_max_bars as f64 };
    let max_bars_config = SliderConfig::new(0.0, 50000.0).with_step(500.0);
    let max_bars_rect = WidgetRect::new(x, cy, content_w, slider_h);
    let max_bars_result = render_single_slider(
        ctx,
        &max_bars_config,
        max_bars_val,
        max_bars_rect,
        UserSettingsKey::SliderMaxBars.get(current_language()),
        &widget_theme,
        false,
        None,
    );
    if let Some(track_info) = max_bars_result.track_info {
        result.slider_tracks.push(SliderTrackInfo::new(
            "data_max_bars",
            track_info.track_x,
            track_info.track_width,
            track_info.min_val,
            track_info.max_val,
        ));
        result.content_items.push(("data_max_bars".to_string(), max_bars_rect));
        input_coordinator.register_on_layer(
            "user_settings:data_max_bars",
            uzor::types::Rect::new(x, cy, content_w, slider_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    cy += slider_h;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::DescMaxBars.get(current_language()), x + 4.0, cy);
    cy += desc_gap;

    // ── Slider 3: Cache size limit MB (50–5000) ───────────────────────────
    cy += 8.0;
    let store_mb_val = if let Some(("data_store_size_mb", v)) = floating { v } else { state.data_store_size_mb as f64 };
    let store_mb_config = SliderConfig::new(50.0, 5000.0).with_step(50.0);
    let store_mb_rect = WidgetRect::new(x, cy, content_w, slider_h);
    let store_mb_result = render_single_slider(
        ctx,
        &store_mb_config,
        store_mb_val,
        store_mb_rect,
        UserSettingsKey::SliderCacheSizeMb.get(current_language()),
        &widget_theme,
        false,
        None,
    );
    if let Some(track_info) = store_mb_result.track_info {
        result.slider_tracks.push(SliderTrackInfo::new(
            "data_store_size_mb",
            track_info.track_x,
            track_info.track_width,
            track_info.min_val,
            track_info.max_val,
        ));
        result.content_items.push(("data_store_size_mb".to_string(), store_mb_rect));
        input_coordinator.register_on_layer(
            "user_settings:data_store_size_mb",
            uzor::types::Rect::new(x, cy, content_w, slider_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    cy += slider_h;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::DescCacheSize.get(current_language()), x + 4.0, cy);
    cy += desc_gap;

    // ── Slider 4: Auto-cleanup days (1–365) ───────────────────────────────
    cy += 8.0;
    let cleanup_days_val = if let Some(("data_cleanup_days", v)) = floating { v } else { state.data_cleanup_days as f64 };
    let cleanup_days_config = SliderConfig::new(1.0, 365.0).with_step(1.0);
    let cleanup_days_rect = WidgetRect::new(x, cy, content_w, slider_h);
    let cleanup_days_result = render_single_slider(
        ctx,
        &cleanup_days_config,
        cleanup_days_val,
        cleanup_days_rect,
        UserSettingsKey::SliderAutoCleanupDays.get(current_language()),
        &widget_theme,
        false,
        None,
    );
    if let Some(track_info) = cleanup_days_result.track_info {
        result.slider_tracks.push(SliderTrackInfo::new(
            "data_cleanup_days",
            track_info.track_x,
            track_info.track_width,
            track_info.min_val,
            track_info.max_val,
        ));
        result.content_items.push(("data_cleanup_days".to_string(), cleanup_days_rect));
        input_coordinator.register_on_layer(
            "user_settings:data_cleanup_days",
            uzor::types::Rect::new(x, cy, content_w, slider_h),
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
    cy += slider_h;
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::DescAutoCleanup.get(current_language()), x + 4.0, cy);
    cy += desc_gap;

    cy += 8.0;

    let total_content_h = cy - container.content_y();
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            layer_id,
        );
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        input_coordinator.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            uzor::input::Sense::SCROLL,
            layer_id,
        );
    }
}

#[allow(clippy::too_many_arguments)]
fn render_server_tab(
    ctx: &mut dyn RenderContext,
    viewport_rect: WidgetRect,
    state: &UserSettingsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    scroll_widget_theme: &WidgetTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut UserSettingsResult,
) {
    // Server tab content is short (status row + endpoint info), no scroll needed.
    let no_scroll = ScrollState::default();
    let container = ScrollableContainer::new(
        viewport_rect,
        &no_scroll,
        None,
    );
    container.begin(ctx);
    let x = viewport_rect.x;
    let available_w = container.content_width();
    let mut cy = container.content_y();

    let row_h = 24.0;
    let section_gap = 18.0;

    // ── Section: SERVER ───────────────────────────────────────────────────────
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(UserSettingsKey::SectionServer.get(current_language()), x, cy);

    // Enable toggle checkbox row
    cy += section_gap;
    let cb_size = 16.0;
    let cb_x = x;
    let cb_y = cy + (row_h - cb_size) / 2.0;

    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(cb_x, cb_y, cb_size, cb_size, 2.0);

    if state.server_enabled {
        ctx.set_fill_color(&toolbar_theme.item_text_active);
        ctx.fill_rounded_rect(cb_x + 3.0, cb_y + 3.0, cb_size - 6.0, cb_size - 6.0, 1.0);
    }

    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(
        UserSettingsKey::EnableAgentApiServer.get(current_language()),
        cb_x + cb_size + 10.0,
        cy + row_h / 2.0,
    );

    let toggle_rect = WidgetRect::new(cb_x, cy, available_w, row_h);
    result.content_items.push(("server_toggle".to_string(), toggle_rect));
    input_coordinator.register_on_layer(
        "user_settings:server_toggle",
        uzor::types::Rect::new(cb_x, cy, available_w, row_h),
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    cy += row_h + 8.0;

    // Status indicator row
    let dot_r = 4.0;
    let dot_cx = x + dot_r;
    let dot_cy = cy + row_h / 2.0;

    let is_running = state.server_enabled && state.server_status == "running";
    let dot_color = if is_running { &toolbar_theme.success } else { &toolbar_theme.danger };
    ctx.set_fill_color(dot_color);
    ctx.begin_path();
    ctx.arc(dot_cx, dot_cy, dot_r, 0.0, std::f64::consts::TAU);
    ctx.fill();

    let status_text = if is_running {
        format!("Running on :{}", state.server_port)
    } else {
        UserSettingsKey::ServerStopped.get(current_language()).to_string()
    };
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(&status_text, dot_cx + dot_r + 6.0, dot_cy);
    cy += row_h + 16.0;

    // ── Access info (replaces the old API KEYS section) ──────────────────────
    // The OSS build serves the Agent API openly on 127.0.0.1, so there are no
    // user-managed keys. Tell the user where the API is reachable instead.
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(
        UserSettingsKey::ServerOpenAccess.get(current_language()),
        x,
        cy,
    );
    cy += 18.0;
    ctx.fill_text(
        &format!("Endpoint: http://127.0.0.1:{}/", state.server_port),
        x,
        cy,
    );
    let bottom = cy + 18.0;
    // Silence the now-unused parameter warning.
    let _ = frame_theme;
    let _ = available_w;

    let total_content_h = bottom - container.content_y();
    let scroll_result = container.end(ctx, total_content_h, scroll_widget_theme);
    result.scroll_viewport_rect = Some(viewport_rect);
    result.scroll_content_height = scroll_result.content_height;
    result.scrollbar_handle_rect = scroll_result.handle_rect;
    result.scrollbar_track_rect = scroll_result.track_rect;
    result.scroll_viewport_height = scroll_result.viewport_height;

    if let Some(ref hr) = result.scrollbar_handle_rect {
        let inflated = uzor::types::Rect::new(hr.x - 5.0, hr.y, hr.width + 10.0, hr.height);
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            layer_id,
        );
    }
    if let Some(ref tr) = result.scrollbar_track_rect {
        input_coordinator.register_on_layer(
            "user_settings:scrollbar_track",
            uzor::types::Rect::new(tr.x, tr.y, tr.width, tr.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
    if let Some(ref vp) = result.scroll_viewport_rect {
        input_coordinator.register_on_layer(
            "user_settings:scroll_viewport",
            uzor::types::Rect::new(vp.x, vp.y, vp.width, vp.height),
            uzor::input::Sense::SCROLL,
            layer_id,
        );
    }
}
