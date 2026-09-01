//! Profile Manager modal — an ordinary dismissible modal over a live chart.
//!
//! Owner's ruling 2026-08-31: full-screen was the old scheme, where the
//! profile was an entry gate and the opaque fill hid the fact that data had
//! not loaded yet. Data now loads independently of account and vault, so
//! there is nothing left to hide — this is a standard [`begin_modal_shell`]
//! dialog, dimmed backdrop included, same shell every other modal uses
//! (`crate::layout::modals::alert_settings` is the tabbed reference,
//! `crate::layout::modals::cloud_login` the closer one: no tabs, dim
//! backdrop, per-phase body, dismissible).
//!
//! Dismissible by ×, dimmer click and Esc — EXCEPT while
//! `UserSettingsState::profile_manager_mandatory` is set. That flag is only
//! true for the migration path: an existing plaintext profile with no vault,
//! on a window that genuinely has no chart yet. Every other page (the
//! ordinary profile list, unlock, create, recovery) closes normally.
//!
//! Pages:
//!   ProfileList       — List all profiles; select one to load or create a new one.
//!   UnlockPassphrase  — Enter passphrase to unlock a profile with vault.enc.
//!   CreatePassphrase  — Set a new passphrase for a profile that has no vault.
//!   CreateNew         — Enter name for a brand-new profile (sync toggled in settings).
//!   ShowRecoveryKey   — Display the recovery key once after vault creation.
//!   UseRecoveryKey    — Enter a recovery key to restore access.
//!   SetNewPassphrase  — Set a new passphrase after a recovery-key unlock.

use mlc_core::engine::render::{RenderContext, draw_svg_icon};
use mlc_core::i18n::{current_language, TextKey, ProfileKey};

use crate::ui::Icon;
use uzor::render::{TextAlign, TextBaseline};
use uzor::types::Rect as WidgetRect;
use uzor::input::Sense;
use crate::ui::modal_settings::{UserSettingsState, ProfileManagerPage};
use crate::ui::toolbar_theme::ToolbarTheme;
use crate::layout::render_frame::UserSettingsResult;
use crate::ui::z_order::ZLayer;
use crate::ui::widgets::{
    begin_modal_shell, end_modal_shell, register_dimmer_zones, draw_input, draw_input_cursor,
    InputConfig, InputType, ModalFooter, ModalPlacement, ModalShellSpec, ModalTheme,
};
use crate::ui::widgets::types::WidgetState;
use crate::layout::render_ui::toolbar_to_widget_theme;
use crate::layout::render_chart::FrameTheme;

// =============================================================================
// Layout constants
// =============================================================================

const MODAL_W: f64 = 520.0;

const PROFILE_HEADER_H: f64 = 36.0;
const PROFILE_ZT_ROW_H: f64 = 18.0;
const PROFILE_ROW_H: f64 = 52.0;
const PROFILE_ROW_GAP: f64 = 6.0;
/// Ceiling on the profile list's own viewport — beyond this it scrolls,
/// exactly like `alert_settings`' AlertsList tab.
const PROFILE_LIST_MAX_H: f64 = 280.0;

/// Content height per non-list page. Each constant is the exact bottom edge
/// (title dropped — the shell header carries it now) traced by hand through
/// its render function's `cy` deltas below — not an estimate summed from the
/// page's original vertical increments (that was the prior pass; it ran 28
/// to 36px over what the render function actually draws, on every page here,
/// which is what left visible dead space below each page's last button).
/// Three pages (`UnlockPassphrase`, `UseRecoveryKey`, `SetNewPassphrase`)
/// carry an optional error/hint line reserved unconditionally in the render
/// function on two symmetric branches (always +N regardless of the
/// condition) — those already fold into the trace below with no special
/// case. The exception is each page's OWN optional trailing error line
/// (`vault_unlock_error` / `set_passphrase_error`), which the render
/// function draws only when `Some`/non-empty with no `cy` bump either way;
/// the constant below reserves room for that worst case so the frame never
/// clips when the error is present, at the cost of a small (~18-22px) gap
/// when it is not.
fn page_content_height(state: &UserSettingsState) -> f64 {
    match &state.profile_manager_page {
        ProfileManagerPage::ProfileList => profile_list_content_height(state),
        // back(24) + gap(10) + subtitle(22) + passphrase_input(18+48) +
        // button+gap(32+10) + error(18, worst case) + link(16) = 198.
        ProfileManagerPage::UnlockPassphrase => 198.0,
        // back(24) + gap(10) + subtitle(22) + hint(16) + passphrase_input
        // (18+48) + confirm_label(18) + confirm_input+gap(32+4) +
        // mismatch_line(18) + button(32) = 242.
        ProfileManagerPage::CreatePassphrase => 242.0,
        // back(24) + gap(12) + label(18) + input+gap(32+16) + button(32) = 134.
        ProfileManagerPage::CreateNew => 134.0,
        // warning_line1(18) + warning_line2(26) + key_box+gap(40+10) +
        // copy_btn+gap(28+12) + confirm_btn(34) = 168.
        ProfileManagerPage::ShowRecoveryKey => 168.0,
        // back(24) + gap(10) + subtitle(22) + input+gap(32+14) +
        // button+gap(32+10) + error(18, worst case) = 162.
        ProfileManagerPage::UseRecoveryKey => 162.0,
        // line1(16) + line2(26) + new_label(18) + new_input+gap(32+14) +
        // confirm_label(18) + confirm_input+gap(32+16) + error(22, worst
        // case) + button(32) = 226.
        ProfileManagerPage::SetNewPassphrase => 226.0,
    }
}

fn profile_list_content_height(state: &UserSettingsState) -> f64 {
    let header = PROFILE_HEADER_H + 8.0 + PROFILE_ZT_ROW_H + 8.0;
    let rows = state.profiles_with_vault_status.len().max(1) as f64;
    let list_h =
        (rows * (PROFILE_ROW_H + PROFILE_ROW_GAP) - PROFILE_ROW_GAP).min(PROFILE_LIST_MAX_H);
    header + list_h
}

/// Title shown in the shell header for the active page.
fn page_title(state: &UserSettingsState) -> String {
    let lang = current_language();
    match &state.profile_manager_page {
        ProfileManagerPage::ProfileList => ProfileKey::Profiles.get(lang).to_string(),
        ProfileManagerPage::UnlockPassphrase => {
            if state.profile_manager_target_name.is_empty() {
                "Unlock Profile".to_string()
            } else {
                format!("Unlock {}", state.profile_manager_target_name)
            }
        }
        ProfileManagerPage::CreatePassphrase => {
            if state.profile_manager_target_name.is_empty() {
                "Set Up Encryption".to_string()
            } else {
                format!("Set Up Encryption for {}", state.profile_manager_target_name)
            }
        }
        ProfileManagerPage::CreateNew => ProfileKey::NewProfile.get(lang).to_string(),
        ProfileManagerPage::ShowRecoveryKey => ProfileKey::RecoveryKeyTitle.get(lang).to_string(),
        ProfileManagerPage::UseRecoveryKey => ProfileKey::RecoverWithKey.get(lang).to_string(),
        ProfileManagerPage::SetNewPassphrase => ProfileKey::SetNewPassphrase.get(lang).to_string(),
    }
}

// =============================================================================
// Main render entry point
// =============================================================================

/// Render the Profile Manager modal.
///
/// `screen_w/screen_h` are the full window dimensions — the shell centers
/// and clamps the frame within them, same as `alert_settings`/`cloud_login`.
#[allow(clippy::too_many_arguments)]
pub fn render_profile_manager(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let mandatory = state.profile_manager_mandatory;
    let title = page_title(state);
    let content_h = page_content_height(state);

    let theme = ModalTheme::from_toolbar(&frame_theme.modal_bg, &toolbar_theme.separator, toolbar_theme);
    let spec = ModalShellSpec {
        // Sign-in-shaped flow: answered or dismissed before the chart takes
        // input again. The migration step additionally has no close button —
        // set below — so the dimmer is its only other "exit", and that exit
        // is refused in the click handler while `mandatory` holds.
        dim_backdrop: true,
        footer: ModalFooter::None,
        show_close: !mandatory,
        // No remembered position to drag to (none of the seven pages ever
        // needed one) — centered like every non-draggable modal.
        draggable: false,
        placement: ModalPlacement::Center,
        layer: ZLayer::ModalOverlay,
        ..ModalShellSpec::new("profile_manager", &title, MODAL_W, content_h)
    };
    let (shell, layer_id) =
        begin_modal_shell(ctx, input_coordinator, &spec, &theme, (screen_w, screen_h));
    register_dimmer_zones(input_coordinator, &layer_id, "profile_manager", shell.frame, (screen_w, screen_h));

    result.modal_rect = shell.frame;
    result.header_rect = shell.header;
    result.close_btn_rect = shell.close_btn.unwrap_or_default();

    let (x, y, w) = (shell.content.x, shell.content.y, shell.content.width);

    match &state.profile_manager_page {
        ProfileManagerPage::ProfileList => render_page_profile_list(
            ctx, x, y, w, state, text_color, toolbar_theme, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::UnlockPassphrase => render_page_unlock(
            ctx, x, y, w, state, text_color, toolbar_theme,
            frame_theme, current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::CreatePassphrase => render_page_create_passphrase(
            ctx, x, y, w, state, text_color, toolbar_theme,
            frame_theme, current_time_ms, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::CreateNew => render_page_create_new(
            ctx, x, y, w, state, text_color, toolbar_theme,
            frame_theme, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::ShowRecoveryKey => render_page_show_recovery_key(
            ctx, x, y, w, state, text_color, toolbar_theme,
            frame_theme, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::UseRecoveryKey => render_page_use_recovery_key(
            ctx, x, y, w, state, text_color, toolbar_theme,
            frame_theme, &layer_id, input_coordinator, result,
        ),
        ProfileManagerPage::SetNewPassphrase => render_page_set_new_passphrase(
            ctx, x, y, w, state, text_color, toolbar_theme,
            frame_theme, &layer_id, input_coordinator, result,
        ),
    }

    end_modal_shell(ctx, input_coordinator, &spec, &theme, &shell, &layer_id, &[]);
}

// =============================================================================
// Page: ProfileList
// =============================================================================

/// Single-column list: the old two-column layout (240px account rail, mascot,
/// zero-trust badge) assumed a full-bleed content area. At modal width there
/// is no room for a second column, so this collapses to one — the picker
/// itself (rows, create, delete, unlock) is untouched, only the account
/// rail's decoration (mascot) is dropped and the zero-trust/version line
/// moves under the header.
#[allow(clippy::too_many_arguments)]
fn render_page_profile_list(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let mut cy = y;

    // ── Header row: "Profiles" title (left) + "+ Create New Profile" (right) ──
    let create_w: f64 = 180.0;
    ctx.set_font("bold 18px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(ProfileKey::Profiles.get(current_language()), x, cy + PROFILE_HEADER_H / 2.0);

    let create_id = "profile_mgr:create_new";
    let is_create_hovered = hovered == Some(create_id);
    let create_bg = if is_create_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let create_x = x + w - create_w;
    ctx.set_fill_color(create_bg);
    ctx.fill_rounded_rect(create_x, cy, create_w, PROFILE_HEADER_H, 4.0);
    ctx.set_font("bold 13px sans-serif");
    ctx.set_fill_color(if is_create_hovered { toolbar_theme.item_text.as_str() } else { toolbar_theme.item_text_muted.as_str() });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(ProfileKey::CreateNewProfile.get(current_language()), create_x + create_w / 2.0, cy + PROFILE_HEADER_H / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let create_rect = WidgetRect::new(create_x, cy, create_w, PROFILE_HEADER_H);
    result.content_items.push((create_id.to_string(), create_rect));
    input_coordinator.register_on_layer(
        format!("user_settings:{}", create_id).as_str(),
        create_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    cy += PROFILE_HEADER_H + 8.0;

    // ── Zero-trust badge + version (single row) ──────────────────────────────
    {
        let icon_size = 14.0;
        let mid_y = cy + PROFILE_ZT_ROW_H / 2.0;
        draw_svg_icon(ctx, Icon::ShieldCheck.svg(), x, mid_y - icon_size / 2.0, icon_size, icon_size, toolbar_theme.item_text_muted.as_str());
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(ProfileKey::ZeroTrust.get(current_language()), x + icon_size + 4.0, mid_y);
        ctx.set_text_align(TextAlign::Right);
        ctx.fill_text(&format!("v{}", env!("CARGO_PKG_VERSION")), x + w, mid_y);
        ctx.set_text_align(TextAlign::Left);
        cy += PROFILE_ZT_ROW_H + 8.0;
    }

    // ── Scrollable viewport for profile rows ──────────────────────────────────
    let scroll_viewport_y = cy;
    let total_content_h = (state.profiles_with_vault_status.len() as f64) * (PROFILE_ROW_H + PROFILE_ROW_GAP);
    let scroll_viewport_h = total_content_h.max(PROFILE_ROW_H).min(PROFILE_LIST_MAX_H);

    let max_scroll = (total_content_h - scroll_viewport_h).max(0.0);
    let scroll_offset = state.profile_list_scroll.offset.min(max_scroll);

    result.profile_list_viewport_rect = WidgetRect::new(x, scroll_viewport_y, w, scroll_viewport_h);
    result.profile_list_total_content_h = total_content_h;

    input_coordinator.register_on_layer(
        "user_settings:profile_list_viewport",
        WidgetRect::new(x, scroll_viewport_y, w, scroll_viewport_h),
        Sense::SCROLL,
        layer_id,
    );

    ctx.save();
    ctx.clip_rect(x, scroll_viewport_y, w, scroll_viewport_h);

    cy = scroll_viewport_y - scroll_offset;

    for (id, display_name, _avatar, has_vault) in &state.profiles_with_vault_status {
        let widget_id = format!("profile_mgr:select:{}", id);
        let is_row_hovered = hovered == Some(widget_id.as_str());
        let is_active = *id == state.runtime_profile_id;

        let row_bg = if is_row_hovered {
            toolbar_theme.button_bg_hover.as_str()
        } else {
            toolbar_theme.button_bg.as_str()
        };
        ctx.set_fill_color(row_bg);
        ctx.fill_rounded_rect(x, cy, w, PROFILE_ROW_H, 4.0);

        if is_active {
            ctx.set_fill_color(&toolbar_theme.accent);
            ctx.fill_rounded_rect(x, cy, 3.0, PROFILE_ROW_H, 2.0);
        }

        let row_mid_y = cy + PROFILE_ROW_H / 2.0;

        let name_x = if is_active { x + 10.0 } else { x + 8.0 };
        ctx.set_font("bold 14px sans-serif");
        ctx.set_fill_color(text_color);
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(display_name.as_str(), name_x, row_mid_y);

        if *has_vault {
            let row_rect = WidgetRect::new(x, cy, w, PROFILE_ROW_H);
            result.content_items.push((widget_id.clone(), row_rect));
            let hit_id = format!("user_settings:{}", widget_id);
            input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);
        } else {
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Right);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.set_fill_color(toolbar_theme.danger.as_str());
            ctx.fill_text(ProfileKey::Unprotected.get(current_language()), x + w - 70.0, row_mid_y);

            let del_w = 54.0;
            let del_h = 22.0;
            let del_x = x + w - del_w - 6.0;
            let del_y = row_mid_y - del_h / 2.0;
            let del_id = format!("profile_delete:{}", id);
            let is_del_hovered = hovered == Some(del_id.as_str());
            let del_bg = mlc_core::apply_opacity(&toolbar_theme.danger, if is_del_hovered { 0.6 } else { 0.3 });
            ctx.set_fill_color(&del_bg);
            ctx.fill_rounded_rect(del_x, del_y, del_w, del_h, 3.0);
            ctx.set_font("bold 10px sans-serif");
            ctx.set_fill_color(toolbar_theme.item_text.as_str());
            ctx.set_text_align(TextAlign::Center);
            ctx.fill_text(TextKey::Delete.get(current_language()), del_x + del_w / 2.0, row_mid_y);
            ctx.set_text_align(TextAlign::Left);

            let row_rect = WidgetRect::new(x, cy, w, PROFILE_ROW_H);
            result.content_items.push((widget_id.clone(), row_rect));
            let hit_id = format!("user_settings:{}", widget_id);
            input_coordinator.register_on_layer(hit_id.as_str(), row_rect, Sense::CLICK | Sense::HOVER, layer_id);

            let del_rect = WidgetRect::new(del_x, del_y, del_w, del_h);
            result.content_items.push((del_id.clone(), del_rect));
            input_coordinator.register_on_layer(
                format!("user_settings:{}", del_id).as_str(),
                del_rect,
                Sense::CLICK | Sense::HOVER,
                layer_id,
            );
        }

        cy += PROFILE_ROW_H + PROFILE_ROW_GAP;
    }

    ctx.restore();

    if total_content_h > scroll_viewport_h {
        let scrollbar_w = 4.0;
        let track_x = x + w + 2.0;
        let ratio = (scroll_viewport_h / total_content_h).min(1.0);
        let thumb_h = (scroll_viewport_h * ratio).max(24.0);
        let scroll_ratio = if max_scroll > 0.0 { scroll_offset / max_scroll } else { 0.0 };
        let thumb_y = scroll_viewport_y + scroll_ratio * (scroll_viewport_h - thumb_h);
        ctx.set_fill_color(toolbar_theme.separator.as_str());
        ctx.fill_rounded_rect(track_x, thumb_y, scrollbar_w, thumb_h, 2.0);

        let track_rect = WidgetRect::new(track_x, scroll_viewport_y, scrollbar_w, scroll_viewport_h);
        let handle_rect = WidgetRect::new(track_x, thumb_y, scrollbar_w, thumb_h);
        result.profile_list_handle_rect = Some(handle_rect);
        result.profile_list_track_rect = Some(track_rect);

        let inflated = uzor::types::Rect::new(
            handle_rect.x - 10.0,
            handle_rect.y - 2.0,
            handle_rect.width + 20.0,
            handle_rect.height + 4.0,
        );
        input_coordinator.register_on_layer(
            "user_settings:profile_list:scrollbar_handle",
            inflated,
            uzor::input::Sense::DRAG,
            layer_id,
        );
        input_coordinator.register_on_layer(
            "user_settings:profile_list:scrollbar_track",
            uzor::types::Rect::new(track_rect.x, track_rect.y, track_rect.width, track_rect.height),
            uzor::input::Sense::CLICK,
            layer_id,
        );
    }
}

// =============================================================================
// Page: UnlockPassphrase
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_unlock(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let mut cy = y;

    render_back_button(ctx, x, &mut cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    cy += 10.0;

    // Subtitle (the title now lives in the shell header).
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(ProfileKey::EnterPassphraseToDecrypt.get(current_language()), x + w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 22.0;

    render_passphrase_input(
        ctx, x, w, &mut cy, state, text_color, toolbar_theme,
        frame_theme, current_time_ms, layer_id, input_coordinator, result,
    );

    let unlock_disabled = state.new_passphrase_render.text.is_empty();
    let is_unlock_hovered = !unlock_disabled && hovered == Some("profile_mgr:unlock");
    let btn_bg = if is_unlock_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let btn_text_col = if unlock_disabled { toolbar_theme.item_text_muted.as_str() } else { toolbar_theme.item_text_active.as_str() };
    let btn_h = 32.0;
    let btn_w = w.min(180.0);
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(btn_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(ProfileKey::BtnUnlock.get(current_language()), x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !unlock_disabled {
        let btn_rect = WidgetRect::new(x, cy, btn_w, btn_h);
        result.content_items.push(("profile_mgr:unlock".to_string(), btn_rect));
        input_coordinator.register_on_layer("user_settings:profile_mgr:unlock", btn_rect, Sense::CLICK | Sense::HOVER, layer_id);
    }
    cy += btn_h + 10.0;

    if let Some(ref err_msg) = state.vault_unlock_error {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(toolbar_theme.danger.as_str());
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(err_msg.as_str(), x + w / 2.0, cy);
        ctx.set_text_align(TextAlign::Left);
        cy += 18.0;
    }

    let link_text = ProfileKey::LinkUseRecoveryKey.get(current_language());
    let is_link_hovered = hovered == Some("profile_mgr:use_recovery_key");
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(if is_link_hovered {
        toolbar_theme.accent.as_str()
    } else {
        toolbar_theme.item_text_muted.as_str()
    });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(link_text, x + w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    let link_w = 140.0;
    let link_h = 16.0;
    let link_x = x + (w - link_w) / 2.0;
    let link_rect = WidgetRect::new(link_x, cy, link_w, link_h);
    result.content_items.push(("profile_mgr:use_recovery_key".to_string(), link_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:use_recovery_key", link_rect, Sense::CLICK | Sense::HOVER, layer_id,
    );
}

// =============================================================================
// Page: CreatePassphrase
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_create_passphrase(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let mut cy = y;

    render_back_button(ctx, x, &mut cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    cy += 10.0;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(ProfileKey::CreatePassphraseForKeys.get(current_language()), x + w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 22.0;

    if state.new_passphrase_render.text.len() < mlc_core::MIN_PASSPHRASE_LENGTH {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(
            &format!("Minimum {} characters", mlc_core::MIN_PASSPHRASE_LENGTH),
            x,
            cy,
        );
        cy += 16.0;
    } else {
        cy += 16.0;
    }

    render_passphrase_input(
        ctx, x, w, &mut cy, state, text_color, toolbar_theme,
        frame_theme, current_time_ms, layer_id, input_coordinator, result,
    );

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.set_text_align(TextAlign::Left);
    ctx.fill_text(ProfileKey::ConfirmPassphrase.get(current_language()), x, cy);
    cy += 18.0;

    let confirm_h = 32.0;
    let confirm_rect = WidgetRect::new(x, cy, w, confirm_h);
    let conf_render = &state.confirm_passphrase_render;
    let (conf_sel_start, conf_sel_end) = if let Some((lo, hi)) = conf_render.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let confirm_revealed = state.passphrase_revealed;
    let confirm_config = InputConfig::new(&conf_render.text)
        .with_focused(state.confirm_passphrase_focused)
        .with_cursor(conf_render.cursor)
        .with_placeholder("Confirm passphrase\u{2026}")
        .with_type(if confirm_revealed { InputType::Text } else { InputType::Password })
        .with_padding_right(28.0)
        .with_selection(conf_sel_start, conf_sel_end);

    let confirm_result = draw_input(ctx, &confirm_config, WidgetState::Normal, confirm_rect, &widget_theme);

    result.content_items.push(("profile_mgr:create_confirm_passphrase_input".to_string(), confirm_rect));
    result.input_char_positions.push(("profile_mgr:create_confirm_passphrase_input".to_string(), confirm_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:create_confirm_passphrase_input",
        confirm_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.confirm_passphrase_focused && conf_render.cursor_visible {
        draw_input_cursor(
            ctx,
            confirm_result.cursor_x,
            confirm_result.cursor_y,
            confirm_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    draw_passphrase_reveal_toggle(ctx, confirm_rect, confirm_revealed, state, toolbar_theme, layer_id, input_coordinator, result);

    cy += confirm_h + 4.0;

    let passphrase_text = &state.new_passphrase_render.text;
    let confirm_text = &state.confirm_passphrase_render.text;
    if !confirm_text.is_empty() && !passphrase_text.is_empty() && confirm_text != passphrase_text {
        ctx.set_font("11px sans-serif");
        ctx.set_fill_color(toolbar_theme.danger.as_str());
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(ProfileKey::PassphrasesMismatch.get(current_language()), x, cy);
        cy += 18.0;
    } else {
        cy += 18.0;
    }

    let confirm_matches_create = state.confirm_passphrase_render.text == state.new_passphrase_render.text;
    let encrypt_disabled = state.new_passphrase_render.text.len() < mlc_core::MIN_PASSPHRASE_LENGTH
        || !confirm_matches_create;
    let is_encrypt_hovered = !encrypt_disabled && hovered == Some("profile_mgr:create_passphrase");
    let btn_bg = if is_encrypt_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let btn_text_col = if encrypt_disabled { toolbar_theme.item_text_muted.as_str() } else { toolbar_theme.item_text_active.as_str() };
    let btn_h = 32.0;
    let btn_w = w.min(180.0);
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(btn_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(ProfileKey::BtnEncrypt.get(current_language()), x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !encrypt_disabled {
        let btn_rect = WidgetRect::new(x, cy, btn_w, btn_h);
        result.content_items.push(("profile_mgr:create_passphrase".to_string(), btn_rect));
        input_coordinator.register_on_layer(
            "user_settings:profile_mgr:create_passphrase",
            btn_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
}

// =============================================================================
// Page: ShowRecoveryKey
// =============================================================================

/// Shown once after a successful vault creation. The user must click
/// "Я записал" ("I have written it down") to proceed — that step, not the
/// modal's own dismissibility, is what makes this page a one-way door; the ×
/// and Esc still work like any other page here (the recovery key itself is
/// never destroyed by closing early, only the forward flow is).
#[allow(clippy::too_many_arguments)]
fn render_page_show_recovery_key(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    state: &UserSettingsState,
    _text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let mut cy = y;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(toolbar_theme.accent.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(
        ProfileKey::RecoveryKeyWriteDownWarning.get(current_language()),
        x + w / 2.0,
        cy,
    );
    cy += 18.0;
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.fill_text(
        ProfileKey::RecoveryKeyPasswordLossNote.get(current_language()),
        x + w / 2.0,
        cy,
    );
    ctx.set_text_align(TextAlign::Left);
    cy += 26.0;

    let key_text = state
        .recovery_key_display
        .as_deref()
        .unwrap_or("(key not available)");

    let key_box_h = 40.0;
    let key_display_rect = WidgetRect::new(x, cy, w, key_box_h);

    let key_widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);

    let rk_render = &state.recovery_key_display_render;
    let (sel_start, sel_end) = if let Some((lo, hi)) = rk_render.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let display_focused = state.recovery_key_display_focused;
    let key_display_config = InputConfig::new(key_text)
        .with_focused(display_focused)
        .with_cursor(rk_render.cursor)
        .with_type(InputType::Text)
        .with_font_size(13.0)
        .with_padding(10.0)
        .with_radius(4.0)
        .with_selection(sel_start, sel_end);

    let key_display_result = draw_input(ctx, &key_display_config, crate::ui::widgets::types::WidgetState::Normal, key_display_rect, &key_widget_theme);

    result.content_items.push(("profile_mgr:recovery_key_display".to_string(), key_display_rect));
    result.input_char_positions.push(("profile_mgr:recovery_key_display".to_string(), key_display_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:recovery_key_display",
        key_display_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );

    if display_focused && rk_render.cursor_visible {
        draw_input_cursor(
            ctx,
            key_display_result.cursor_x,
            key_display_result.cursor_y,
            key_display_result.cursor_height,
            toolbar_theme.accent.as_str(),
        );
    }

    cy += key_box_h + 10.0;

    let copy_btn_label = ProfileKey::BtnCopyKey.get(current_language());
    let is_copy_hovered = hovered == Some("profile_mgr:recovery_key_copy");
    let copy_btn_bg = if is_copy_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let copy_btn_h = 28.0;
    let copy_btn_w = w.min(200.0);
    let copy_btn_x = x + (w - copy_btn_w) / 2.0;
    ctx.set_fill_color(copy_btn_bg);
    ctx.fill_rounded_rect(copy_btn_x, cy, copy_btn_w, copy_btn_h, 4.0);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(if is_copy_hovered { toolbar_theme.item_text_hover.as_str() } else { toolbar_theme.item_text.as_str() });
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(copy_btn_label, copy_btn_x + copy_btn_w / 2.0, cy + copy_btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let copy_btn_rect = WidgetRect::new(copy_btn_x, cy, copy_btn_w, copy_btn_h);
    result.content_items.push(("profile_mgr:recovery_key_copy".to_string(), copy_btn_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:recovery_key_copy",
        copy_btn_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );

    cy += copy_btn_h + 12.0;

    let btn_label = ProfileKey::BtnRecoveryConfirmed.get(current_language());
    let is_btn_hovered = hovered == Some("profile_mgr:recovery_key_confirm");
    let btn_bg = if is_btn_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let btn_text_col = toolbar_theme.item_text_active.as_str();
    let btn_h = 34.0;
    let btn_w = w.min(260.0);
    let btn_x = x + (w - btn_w) / 2.0;
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(btn_x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(btn_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(btn_label, btn_x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    let btn_rect = WidgetRect::new(btn_x, cy, btn_w, btn_h);
    result.content_items.push(("profile_mgr:recovery_key_confirm".to_string(), btn_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:recovery_key_confirm",
        btn_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
}

// =============================================================================
// Page: CreateNew
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_create_new(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let mut cy = y;

    render_back_button(ctx, x, &mut cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    cy += 12.0;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(ProfileKey::ProfileName.get(current_language()), x, cy);
    cy += 18.0;

    let input_h = 32.0;
    let name_rect = WidgetRect::new(x, cy, w, input_h);
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let npn_render = &state.new_profile_name_render;
    let (sel_start, sel_end) = if let Some((lo, hi)) = npn_render.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let name_config = InputConfig::new(&npn_render.text)
        .with_focused(state.new_profile_name_focused)
        .with_cursor(npn_render.cursor)
        .with_placeholder("Profile name\u{2026}")
        .with_type(InputType::Text)
        .with_selection(sel_start, sel_end);
    let name_result = draw_input(ctx, &name_config, WidgetState::Normal, name_rect, &widget_theme);

    result.content_items.push(("profile_mgr:name_input".to_string(), name_rect));
    result.input_char_positions.push(("profile_mgr:name_input".to_string(), name_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:name_input",
        name_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.new_profile_name_focused && npn_render.cursor_visible {
        draw_input_cursor(
            ctx,
            name_result.cursor_x,
            name_result.cursor_y,
            name_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }
    cy += input_h + 16.0;

    let name_is_empty = state.new_profile_name_render.text.trim().is_empty();
    let create_disabled = name_is_empty;
    let is_create_hovered = !create_disabled && hovered == Some("profile_mgr:create_confirm");
    let create_bg = if is_create_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let create_text_col = if create_disabled { toolbar_theme.item_text_muted.as_str() } else { toolbar_theme.item_text_active.as_str() };
    let create_btn_h = 32.0;
    let create_btn_w = w.min(180.0);
    ctx.set_fill_color(create_bg);
    ctx.fill_rounded_rect(x, cy, create_btn_w, create_btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(create_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(ProfileKey::BtnCreate.get(current_language()), x + create_btn_w / 2.0, cy + create_btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !create_disabled {
        let create_rect = WidgetRect::new(x, cy, create_btn_w, create_btn_h);
        result.content_items.push(("profile_mgr:create_confirm".to_string(), create_rect));
        input_coordinator.register_on_layer(
            "user_settings:profile_mgr:create_confirm",
            create_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
}

// =============================================================================
// Page: SetNewPassphrase
// =============================================================================

/// Shown after a successful recovery key unlock. No back button — the same
/// reasoning as `ShowRecoveryKey`: this step being one-way is a property of
/// the recovery flow, not of the modal, so the × and Esc still dismiss it.
#[allow(clippy::too_many_arguments)]
fn render_page_set_new_passphrase(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let has_error = !state.set_passphrase_error.is_empty();
    let mut cy = y;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(
        ProfileKey::VaultUnlockedWithKey.get(current_language()),
        x + w / 2.0,
        cy,
    );
    cy += 16.0;
    ctx.fill_text(
        ProfileKey::SetPassphraseToContinue.get(current_language()),
        x + w / 2.0,
        cy,
    );
    ctx.set_text_align(TextAlign::Left);
    cy += 26.0;

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let input_h = 32.0;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(ProfileKey::NewPassphrase.get(current_language()), x, cy);
    cy += 18.0;

    let new_pass_rect = WidgetRect::new(x, cy, w, input_h);
    let np_render = &state.new_passphrase_render;
    let (sel_start, sel_end) = if let Some((lo, hi)) = np_render.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let new_pass_revealed = state.passphrase_revealed;
    let new_pass_config = InputConfig::new(&np_render.text)
        .with_focused(state.new_passphrase_focused)
        .with_cursor(np_render.cursor)
        .with_placeholder("New passphrase\u{2026}")
        .with_type(if new_pass_revealed { InputType::Text } else { InputType::Password })
        .with_padding_right(28.0)
        .with_selection(sel_start, sel_end);
    let new_pass_result = draw_input(ctx, &new_pass_config, WidgetState::Normal, new_pass_rect, &widget_theme);

    result.content_items.push(("profile_mgr:new_passphrase_input".to_string(), new_pass_rect));
    result.input_char_positions.push(("profile_mgr:new_passphrase_input".to_string(), new_pass_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:new_passphrase_input",
        new_pass_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.new_passphrase_focused && np_render.cursor_visible {
        draw_input_cursor(
            ctx,
            new_pass_result.cursor_x,
            new_pass_result.cursor_y,
            new_pass_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }
    draw_passphrase_reveal_toggle(ctx, new_pass_rect, new_pass_revealed, state, toolbar_theme, layer_id, input_coordinator, result);
    cy += input_h + 14.0;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(ProfileKey::ConfirmPassphrase.get(current_language()), x, cy);
    cy += 18.0;

    let confirm_pass_rect = WidgetRect::new(x, cy, w, input_h);
    let cp_render = &state.confirm_passphrase_render;
    let (sel_start2, sel_end2) = if let Some((lo, hi)) = cp_render.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let confirm_pass_revealed = state.passphrase_revealed;
    let confirm_pass_config = InputConfig::new(&cp_render.text)
        .with_focused(state.confirm_passphrase_focused)
        .with_cursor(cp_render.cursor)
        .with_placeholder("Confirm passphrase\u{2026}")
        .with_type(if confirm_pass_revealed { InputType::Text } else { InputType::Password })
        .with_padding_right(28.0)
        .with_selection(sel_start2, sel_end2);
    let confirm_pass_result = draw_input(ctx, &confirm_pass_config, WidgetState::Normal, confirm_pass_rect, &widget_theme);

    result.content_items.push(("profile_mgr:confirm_passphrase_input".to_string(), confirm_pass_rect));
    result.input_char_positions.push(("profile_mgr:confirm_passphrase_input".to_string(), confirm_pass_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:confirm_passphrase_input",
        confirm_pass_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.confirm_passphrase_focused && cp_render.cursor_visible {
        draw_input_cursor(
            ctx,
            confirm_pass_result.cursor_x,
            confirm_pass_result.cursor_y,
            confirm_pass_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }
    draw_passphrase_reveal_toggle(ctx, confirm_pass_rect, confirm_pass_revealed, state, toolbar_theme, layer_id, input_coordinator, result);
    cy += input_h + 16.0;

    if has_error {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(toolbar_theme.danger.as_str());
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(state.set_passphrase_error.as_str(), x + w / 2.0, cy);
        ctx.set_text_align(TextAlign::Left);
        cy += 22.0;
    }

    let new_text = &state.new_passphrase_render.text;
    let confirm_text = &state.confirm_passphrase_render.text;
    let save_disabled = new_text.len() < mlc_core::MIN_PASSPHRASE_LENGTH
        || confirm_text.len() < mlc_core::MIN_PASSPHRASE_LENGTH
        || new_text != confirm_text;

    let is_save_hovered = !save_disabled && hovered == Some("profile_mgr:save_new_passphrase");
    let save_bg = if is_save_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let save_text_col = if save_disabled { toolbar_theme.item_text_muted.as_str() } else { toolbar_theme.item_text_active.as_str() };
    let btn_h = 32.0;
    let btn_w = w.min(180.0);
    ctx.set_fill_color(save_bg);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(save_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(TextKey::Save.get(current_language()), x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !save_disabled {
        let btn_rect = WidgetRect::new(x, cy, btn_w, btn_h);
        result.content_items.push(("profile_mgr:save_new_passphrase".to_string(), btn_rect));
        input_coordinator.register_on_layer(
            "user_settings:profile_mgr:save_new_passphrase",
            btn_rect,
            Sense::CLICK | Sense::HOVER,
            layer_id,
        );
    }
}

// =============================================================================
// Page: UseRecoveryKey
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_page_use_recovery_key(
    ctx: &mut dyn RenderContext,
    x: f64,
    y: f64,
    w: f64,
    state: &UserSettingsState,
    _text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    let hovered = state.hovered_item_id.as_deref();
    let mut cy = y;

    render_back_button(ctx, x, &mut cy, toolbar_theme, layer_id, input_coordinator, result, hovered);
    cy += 10.0;

    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(toolbar_theme.item_text_muted.as_str());
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(ProfileKey::EnterRecoveryKeyShown.get(current_language()), x + w / 2.0, cy);
    ctx.set_text_align(TextAlign::Left);
    cy += 22.0;

    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let input_h = 32.0;
    let input_rect = WidgetRect::new(x, cy, w, input_h);
    let rk_input_render = &state.recovery_key_display_render;
    let (sel_start, sel_end) = if let Some((lo, hi)) = rk_input_render.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    let input_config = InputConfig::new(&rk_input_render.text)
        .with_focused(state.recovery_key_display_focused)
        .with_cursor(rk_input_render.cursor)
        .with_placeholder("xxxx-xxxx-xxxx-xxxx-xxxx-xxxx-xxxx-xxxx-\u{2026}")
        .with_type(InputType::Text)
        .with_selection(sel_start, sel_end);

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);

    result.content_items.push(("profile_mgr:recovery_key_input".to_string(), input_rect));
    result.input_char_positions.push(("profile_mgr:recovery_key_input".to_string(), input_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:recovery_key_input",
        input_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.recovery_key_display_focused && rk_input_render.cursor_visible {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }
    cy += input_h + 14.0;

    let recover_disabled = state.recovery_key_display_render.text.len() < 40;
    let is_recover_hovered = !recover_disabled && hovered == Some("profile_mgr:recovery_unlock");
    let btn_bg = if is_recover_hovered {
        toolbar_theme.button_bg_hover.as_str()
    } else {
        toolbar_theme.button_bg.as_str()
    };
    let btn_text_col = if recover_disabled { toolbar_theme.item_text_muted.as_str() } else { toolbar_theme.item_text_active.as_str() };
    let btn_h = 32.0;
    let btn_w = w.min(180.0);
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(x, cy, btn_w, btn_h, 4.0);
    ctx.set_font("bold 12px sans-serif");
    ctx.set_fill_color(btn_text_col);
    ctx.set_text_align(TextAlign::Center);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(ProfileKey::BtnRecover.get(current_language()), x + btn_w / 2.0, cy + btn_h / 2.0);
    ctx.set_text_align(TextAlign::Left);

    if !recover_disabled {
        let btn_rect = WidgetRect::new(x, cy, btn_w, btn_h);
        result.content_items.push(("profile_mgr:recovery_unlock".to_string(), btn_rect));
        input_coordinator.register_on_layer(
            "user_settings:profile_mgr:recovery_unlock", btn_rect, Sense::CLICK | Sense::HOVER, layer_id,
        );
    }
    cy += btn_h + 10.0;

    if let Some(ref err_msg) = state.vault_unlock_error {
        ctx.set_font("12px sans-serif");
        ctx.set_fill_color(toolbar_theme.danger.as_str());
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Top);
        ctx.fill_text(err_msg.as_str(), x + w / 2.0, cy);
        ctx.set_text_align(TextAlign::Left);
    }
}

// =============================================================================
// Shared helpers
// =============================================================================

/// Render a "← Back to profiles" button. Advances `cy` by button height.
fn render_back_button(
    ctx: &mut dyn RenderContext,
    x: f64,
    cy: &mut f64,
    toolbar_theme: &ToolbarTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
    hovered_item_id: Option<&str>,
) {
    let btn_w = 120.0;
    let btn_h = 24.0;
    let is_hovered = hovered_item_id == Some("profile_mgr:back");
    let btn_bg = if is_hovered { toolbar_theme.button_bg_hover.as_str() } else { toolbar_theme.button_bg.as_str() };
    ctx.set_fill_color(btn_bg);
    ctx.fill_rounded_rect(x, *cy, btn_w, btn_h, 4.0);
    let stroke_color = if is_hovered { toolbar_theme.item_text_muted.as_str() } else { toolbar_theme.separator.as_str() };
    ctx.set_stroke_color(stroke_color);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(x, *cy, btn_w, btn_h, 4.0);
    draw_svg_icon(ctx, Icon::ChevronLeft.svg(), x + 4.0, *cy + (btn_h - 14.0) / 2.0, 14.0, 14.0, &toolbar_theme.item_text);
    ctx.set_font("11px sans-serif");
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(ProfileKey::BackToProfiles.get(current_language()), x + 22.0, *cy + btn_h / 2.0);

    let back_rect = WidgetRect::new(x, *cy, btn_w, btn_h);
    result.content_items.push(("profile_mgr:back".to_string(), back_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:back",
        back_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
    *cy += btn_h;
}

/// Render the passphrase input box. Advances `cy` past the input field.
#[allow(clippy::too_many_arguments)]
fn render_passphrase_input(
    ctx: &mut dyn RenderContext,
    x: f64,
    w: f64,
    cy: &mut f64,
    state: &UserSettingsState,
    text_color: &str,
    toolbar_theme: &ToolbarTheme,
    frame_theme: &FrameTheme,
    _current_time_ms: u64,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    ctx.set_font("12px sans-serif");
    ctx.set_fill_color(text_color);
    ctx.set_text_baseline(TextBaseline::Top);
    ctx.fill_text(ProfileKey::Passphrase.get(current_language()), x, *cy);
    *cy += 18.0;

    let input_h = 32.0;
    let input_rect = WidgetRect::new(x, *cy, w, input_h);
    let widget_theme = toolbar_to_widget_theme(toolbar_theme, frame_theme);
    let pw_render = &state.new_passphrase_render;
    let (sel_start, sel_end) = if let Some((lo, hi)) = pw_render.selection_range() {
        (Some(lo), Some(hi))
    } else {
        (None, None)
    };
    // Widened right padding leaves room for the reveal toggle drawn below —
    // same knob the search box uses for its own leading icon
    // (`search_overlay.rs`), just on the trailing side here.
    let revealed = state.passphrase_revealed;
    let input_config = InputConfig::new(&pw_render.text)
        .with_focused(state.new_passphrase_focused)
        .with_cursor(pw_render.cursor)
        .with_placeholder("Click to type passphrase\u{2026}")
        .with_type(if revealed { InputType::Text } else { InputType::Password })
        .with_padding_right(28.0)
        .with_selection(sel_start, sel_end);

    let input_result = draw_input(ctx, &input_config, WidgetState::Normal, input_rect, &widget_theme);

    result.content_items.push(("e2e_passphrase_input".to_string(), input_rect));
    result.input_char_positions.push(("e2e_passphrase_input".to_string(), input_result.char_x_positions.clone()));
    input_coordinator.register_on_layer(
        "user_settings:e2e_passphrase_input",
        input_rect,
        Sense::CLICK,
        layer_id,
    );

    if state.new_passphrase_focused && pw_render.cursor_visible {
        draw_input_cursor(
            ctx,
            input_result.cursor_x,
            input_result.cursor_y,
            input_result.cursor_height,
            &toolbar_theme.item_text,
        );
    }

    draw_passphrase_reveal_toggle(ctx, input_rect, revealed, state, toolbar_theme, layer_id, input_coordinator, result);

    *cy += input_h + 16.0;
}

/// Draw the eye/eye-off reveal toggle inside the right edge of a passphrase
/// input, and register its (shared, per-surface) hit rect. One widget ID
/// covers every passphrase field on the profile-manager pages — see
/// `UserSettingsState::passphrase_revealed`'s own doc for why a pair of
/// fields (passphrase + confirm) reveals together.
#[allow(clippy::too_many_arguments)]
fn draw_passphrase_reveal_toggle(
    ctx: &mut dyn RenderContext,
    input_rect: WidgetRect,
    revealed: bool,
    state: &UserSettingsState,
    toolbar_theme: &ToolbarTheme,
    layer_id: &uzor::input::LayerId,
    input_coordinator: &mut uzor::input::InputCoordinator,
    result: &mut UserSettingsResult,
) {
    const ICON_SIZE: f64 = 14.0;
    const RIGHT_INSET: f64 = 8.0;
    let icon_x = input_rect.x + input_rect.width - RIGHT_INSET - ICON_SIZE;
    let icon_y = input_rect.y + (input_rect.height - ICON_SIZE) / 2.0;
    let is_hovered = state.hovered_item_id.as_deref() == Some("profile_mgr:reveal_passphrase");
    let icon_color = if is_hovered { &toolbar_theme.item_text } else { &toolbar_theme.item_text_muted };
    // The icon names the STATE, not the action: a closed/crossed eye means
    // the text is dots, an open eye means the passphrase is plaintext.
    draw_svg_icon(
        ctx,
        if revealed { Icon::Eye.svg() } else { Icon::EyeOff.svg() },
        icon_x, icon_y, ICON_SIZE, ICON_SIZE, icon_color,
    );

    // The hit target is the whole right-hand gutter, not the 14px glyph. A
    // 14×14 target is a miss waiting to happen, and the gutter belongs to
    // nothing else.
    let hit_rect = WidgetRect::new(
        icon_x - RIGHT_INSET,
        input_rect.y,
        ICON_SIZE + RIGHT_INSET * 2.0,
        input_rect.height,
    );
    result.content_items.push(("profile_mgr:reveal_passphrase".to_string(), hit_rect));
    input_coordinator.register_on_layer(
        "user_settings:profile_mgr:reveal_passphrase",
        hit_rect,
        Sense::CLICK | Sense::HOVER,
        layer_id,
    );
}
