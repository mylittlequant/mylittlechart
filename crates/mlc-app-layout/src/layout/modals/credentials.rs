//! Credentials Modal — exchange API keys + the alert webhook URL.
//!
//! The one place a user issues, reviews and removes exchange API keys, and
//! edits the webhook URL that used to be a decorative, unclickable field in
//! `alert_settings.rs`'s Notifications tab. Two pages, not tabs — `List` and
//! `AddKey` — because unlike Alert Settings' tabs (all reachable regardless
//! of state), `AddKey` is reachable only while unlocked; a tab that silently
//! refuses to switch would look broken, so it is a page transition instead,
//! same idiom as `profile_manager.rs`'s pages.
//!
//! # Locked state is the default, and it is honest
//!
//! While sealed, `state.exchange_keys` still carries `exchange_id`/`label`/
//! `api_key` for every entry — `UserProfileManager::seal_vault` redacts only
//! `api_secret`/`passphrase` in place, so the list can honestly show WHAT
//! exists without showing any VALUE. No edit surface is reachable: no
//! webhook input (a locked-vault notice replaces it, mirroring
//! `alert_settings.rs`'s own pattern for the same field), and "+ Add key"
//! routes to unlock instead of opening the form — see
//! `ChartApp::handle_credentials_click`'s module doc for the "return to the
//! add form after unlock" contract.
//!
//! # What is NOT here
//!
//! No countdown / `remaining()` — this modal drives off
//! `BackendCore::UserProfileManager::vault_key` (`Option<VaultKey>`, no TTL
//! concept), not `mlc-vault`'s `SecretStore` (which does have one). Swapping
//! the vault-key source for `SecretStore` is a separate, larger piece of
//! work (it would also mean `ProfileManager`/`BackendCore` agreeing on one
//! session-lifetime object instead of a plain `Option<VaultKey>`) and is
//! deliberately not folded into this modal. A fake countdown not tied to a
//! real auto-seal would be exactly the dishonest-UI failure mode this
//! module's doc above calls out, so there isn't one; sealing here is
//! manual-only (the "Lock" button).

use mlc_core::engine::render::draw_svg_icon;
use mlc_core::engine::render::RenderContext;
use crate::layout::render_chart::FrameTheme;
use mlc_core::render::{TextAlign, TextBaseline};
use crate::ui::modal_settings::CredentialsState;
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::ui::widgets::{
    begin_modal_shell, draw_input, draw_input_cursor, end_modal_shell, register_dimmer_zones,
    InputConfig, InputType, ModalFooter, ModalPlacement, ModalShellSpec, ModalTheme,
};
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::ui::Icon;
use uzor::input::Sense;
use uzor::types::Rect as WidgetRect;

const MODAL_WIDTH: f64 = 460.0;
const ROW_H: f64 = 32.0;
const ITEM_PADDING: f64 = 8.0;
const RADIUS: f64 = 6.0;
const LABEL_W: f64 = 90.0;

/// What the input handler needs back for click dispatch and text-cursor
/// placement — same shape as every other modal's result (`AlertSettingsResult`
/// is the closest sibling).
#[derive(Clone, Debug, Default)]
pub struct CredentialsResult {
    pub modal_rect: WidgetRect,
    pub header_rect: WidgetRect,
    pub close_btn_rect: WidgetRect,
    pub content_rect: WidgetRect,
    /// Content item rects for hit testing: (item_id, rect).
    pub content_items: Vec<(String, WidgetRect)>,
    /// Per-field character X positions for click-to-cursor in text inputs.
    pub input_char_positions: Vec<(String, Vec<f64>)>,
}

fn rows_height(rows: usize) -> f64 {
    rows as f64 * (ROW_H + ITEM_PADDING) - ITEM_PADDING
}

fn compute_list_height(state: &CredentialsState) -> f64 {
    let mut rows = 1usize; // vault status + lock/unlock row
    rows += state.exchange_keys.len().max(1); // key rows, or "no keys yet"
    rows += 1; // "+ Add key" row
    rows += 1; // "Webhook" section caption
    rows += 1; // webhook field or locked notice
    rows_height(rows)
}

fn compute_add_key_height() -> f64 {
    // back + gap, exchange, label, api_key, api_secret, passphrase, testnet
    24.0 + 10.0 + rows_height(6)
}

/// Render the credentials modal.
pub fn render_credentials_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &CredentialsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> CredentialsResult {
    use crate::ui::modal_settings::CredentialsView;

    let mut result = CredentialsResult::default();

    let content_h = match state.view {
        CredentialsView::List => compute_list_height(state),
        CredentialsView::AddKey => compute_add_key_height(),
    };
    let title = match state.view {
        CredentialsView::List => "Credentials",
        CredentialsView::AddKey => "Add Exchange Key",
    };
    let footer = match state.view {
        CredentialsView::List => ModalFooter::None,
        CredentialsView::AddKey => ModalFooter::Creator { label: "Add" },
    };

    let modal_theme = ModalTheme::from_toolbar(&frame_theme.modal_bg, &toolbar_theme.separator, toolbar_theme);
    let spec = ModalShellSpec {
        radius: RADIUS,
        dim_backdrop: true,
        ..ModalShellSpec::new("cred", title, MODAL_WIDTH, content_h)
            .placed(ModalPlacement::At(state.position))
            .with_footer(footer)
    };
    let (shell, layer_id) = begin_modal_shell(ctx, input_coordinator, &spec, &modal_theme, (screen_w, screen_h));

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();
    result.content_rect = shell.content;

    // A dim-backdrop modal registers nothing for the dimmed area itself
    // (see `register_dimmer_zones`'s own doc) — this is what lets a click
    // out there dismiss it at all.
    register_dimmer_zones(input_coordinator, &layer_id, "cred", shell.frame, (screen_w, screen_h));

    let (content_x, content_y, content_w) = (shell.content.x, shell.content.y, shell.content.width);

    match state.view {
        CredentialsView::List => render_list_view(
            ctx, state, toolbar_theme, frame_theme, input_coordinator, &layer_id, &mut result,
            content_x, content_y, content_w,
        ),
        CredentialsView::AddKey => render_add_key_view(
            ctx, state, toolbar_theme, frame_theme, input_coordinator, &layer_id, &mut result,
            content_x, content_y, content_w,
        ),
    }

    end_modal_shell(ctx, input_coordinator, &spec, &modal_theme, &shell, &layer_id, &[]);

    result
}

// =============================================================================
// List view
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_list_view(
    ctx: &mut dyn RenderContext,
    state: &CredentialsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut CredentialsResult,
    content_x: f64,
    content_y: f64,
    content_w: f64,
) {
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
        draw_svg_icon(ctx, icon.svg(), content_x, y + (ROW_H - 14.0) / 2.0, 14.0, 14.0, &toolbar_theme.item_text_muted);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(status_text, content_x + 20.0, y + ROW_H / 2.0);

        let btn_w = 64.0;
        let btn_x = content_x + content_w - btn_w;
        let is_hovered = state.hovered_item_id.as_deref() == Some(btn_id);
        let bg = if is_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(btn_x, y + 2.0, btn_w, ROW_H - 4.0, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(btn_x, y + 2.0, btn_w, ROW_H - 4.0, 4.0);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_text_align(TextAlign::Center);
        ctx.fill_text(btn_label, btn_x + btn_w / 2.0, y + ROW_H / 2.0);
        let r = WidgetRect::new(btn_x, y, btn_w, ROW_H);
        result.content_items.push((btn_id.to_string(), r));
        input_coordinator.register_on_layer(btn_id, r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
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
        ctx.fill_text(empty_label, content_x, y + ROW_H / 2.0);
        y += ROW_H + ITEM_PADDING;
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
            ctx.fill_text(&title, content_x, y + ROW_H / 2.0);

            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(&key.api_key, content_x + content_w - remove_w - 8.0, y + ROW_H / 2.0);
            ctx.set_text_align(TextAlign::Left);

            if state.secrets_available {
                let rm_x = content_x + content_w - remove_w;
                let rm_hovered = state.hovered_item_id.as_deref() == Some(remove_id.as_str());
                if rm_hovered {
                    ctx.set_fill_color(&toolbar_theme.danger);
                    ctx.fill_rounded_rect(rm_x, y + 4.0, remove_w, ROW_H - 8.0, 3.0);
                }
                draw_svg_icon(ctx, Icon::Close.svg(), rm_x + 4.0, y + (ROW_H - 16.0) / 2.0, 16.0, 16.0, &toolbar_theme.item_text_muted);
                let rm_r = WidgetRect::new(rm_x, y, remove_w, ROW_H);
                result.content_items.push((remove_id.clone(), rm_r));
                input_coordinator.register_on_layer(remove_id.as_str(), rm_r, Sense::CLICK, layer_id);
            }

            y += ROW_H + ITEM_PADDING;
        }
    }

    // --- "+ Add key" row — always reachable, even sealed (see module doc). ---
    {
        let is_hovered = state.hovered_item_id.as_deref() == Some("cred:add_key");
        let bg = if is_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(content_x, y, content_w, ROW_H, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(content_x, y, content_w, ROW_H, 4.0);
        draw_svg_icon(ctx, Icon::Plus.svg(), content_x + 8.0, y + (ROW_H - 14.0) / 2.0, 14.0, 14.0, &toolbar_theme.item_text);
        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Add key", content_x + 28.0, y + ROW_H / 2.0);
        let r = WidgetRect::new(content_x, y, content_w, ROW_H);
        result.content_items.push(("add_key".to_string(), r));
        input_coordinator.register_on_layer("cred:add_key", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
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
        ctx.fill_text(notice, content_x, y + ROW_H / 2.0);
    } else {
        let field_x = content_x + LABEL_W;
        let field_w = content_w - LABEL_W;
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("URL", content_x, y + ROW_H / 2.0);

        let render = &state.webhook_url_render;
        let (sel_start, sel_end) = if let Some((lo, hi)) = render.selection_range() {
            (Some(lo), Some(hi))
        } else {
            (None, None)
        };
        let field_rect = WidgetRect::new(field_x, y, field_w, ROW_H);
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

        result.content_items.push(("webhook_url".to_string(), field_rect));
        result.input_char_positions.push(("cred:webhook_url".to_string(), input_result.char_x_positions));
        input_coordinator.register_on_layer("cred:webhook_url", field_rect, Sense::CLICK, layer_id);
    }
}

// =============================================================================
// Add Key view
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_add_key_view(
    ctx: &mut dyn RenderContext,
    state: &CredentialsState,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut CredentialsResult,
    content_x: f64,
    content_y: f64,
    content_w: f64,
) {
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
        result.content_items.push(("add:back".to_string(), r));
        input_coordinator.register_on_layer("cred:add:back", r, Sense::CLICK, layer_id);
        y += 24.0 + 10.0;
    }

    let field_x = content_x + LABEL_W;
    let field_w = content_w - LABEL_W;

    let draw_field_row = |ctx: &mut dyn RenderContext,
                              input_coordinator: &mut uzor::input::InputCoordinator,
                              result: &mut CredentialsResult,
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
        ctx.fill_text(label, content_x, row_y + ROW_H / 2.0);

        let field_rect = WidgetRect::new(field_x, row_y, field_w, ROW_H);
        let (sel_start, sel_end) = if let Some((lo, hi)) = render.selection_range() {
            (Some(lo), Some(hi))
        } else {
            (None, None)
        };
        // API secret / passphrase draw a reveal toggle inside their own
        // right edge — widen the input's own padding so masked/typed text
        // never runs under the icon (same knob the search box uses for its
        // own leading icon, `search_overlay.rs`), and swap the effective
        // type to plain text while revealed.
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
        let local_id = widget_id.trim_start_matches("cred:").to_string();
        result.content_items.push((local_id, field_rect));
        result.input_char_positions.push((widget_id.to_string(), input_result.char_x_positions));
        input_coordinator.register_on_layer(widget_id, field_rect, Sense::CLICK, layer_id);

        if is_secret {
            const ICON_SIZE: f64 = 14.0;
            const RIGHT_INSET: f64 = 8.0;
            let icon_x = field_rect.x + field_rect.width - RIGHT_INSET - ICON_SIZE;
            let icon_y = field_rect.y + (field_rect.height - ICON_SIZE) / 2.0;
            let icon_hovered = state.hovered_item_id.as_deref() == Some("cred:add:reveal");
            let icon_color = if icon_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted };
            draw_svg_icon(
                ctx,
                if revealed { Icon::EyeOff.svg() } else { Icon::Eye.svg() },
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
            result.content_items.push(("add:reveal".to_string(), hit_rect));
            input_coordinator.register_on_layer("cred:add:reveal", hit_rect, Sense::CLICK | Sense::HOVER, layer_id);
        }
    };

    draw_field_row(ctx, input_coordinator, result, "Exchange", "e.g. binance", InputType::Text, &state.add_exchange_render, state.add_exchange_focused, "cred:add:exchange", y);
    y += ROW_H + ITEM_PADDING;
    draw_field_row(ctx, input_coordinator, result, "Label", "main account", InputType::Text, &state.add_label_render, state.add_label_focused, "cred:add:label", y);
    y += ROW_H + ITEM_PADDING;
    draw_field_row(ctx, input_coordinator, result, "API Key", "public key", InputType::Text, &state.add_api_key_render, state.add_api_key_focused, "cred:add:api_key", y);
    y += ROW_H + ITEM_PADDING;
    draw_field_row(ctx, input_coordinator, result, "API Secret", "secret", InputType::Password, &state.add_api_secret_render, state.add_api_secret_focused, "cred:add:api_secret", y);
    y += ROW_H + ITEM_PADDING;
    draw_field_row(ctx, input_coordinator, result, "Passphrase", "optional", InputType::Password, &state.add_passphrase_render, state.add_passphrase_focused, "cred:add:passphrase", y);
    y += ROW_H + ITEM_PADDING;

    // --- Testnet checkbox ---
    {
        let cb_size = 16.0;
        let cb_x = content_x + LABEL_W;
        let cb_y = y + (ROW_H - cb_size) / 2.0;
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
        ctx.fill_text("Testnet", content_x, y + ROW_H / 2.0);
        let r = WidgetRect::new(cb_x, y, content_w - LABEL_W, ROW_H);
        result.content_items.push(("add:testnet".to_string(), r));
        input_coordinator.register_on_layer("cred:add:testnet", r, Sense::CLICK, layer_id);
    }

    if let Some(ref msg) = state.error_message {
        y += ROW_H + ITEM_PADDING;
        ctx.set_fill_color(&toolbar_theme.danger);
        ctx.set_font("11px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(msg, content_x, y + ROW_H / 2.0);
    }
}
