//! Alert Settings Modal — create/edit alerts on chart objects.
//!
//! Three tabs:
//!   - Settings: condition, price, trigger mode, message
//!   - Notifications: popup/sound/webhook transport toggles
//!   - AlertsList: scrollable list of all alerts with filter buttons

use crate::engine::render::draw_svg_icon;
use crate::engine::render::RenderContext;
use crate::layout::render_chart::FrameTheme;
use crate::layout::render_frame::AlertSettingsResult;
use crate::render::{TextAlign, TextBaseline};
use crate::ui::modal_settings::{
    AlertCondition, AlertListFilter, AlertSettingsState, AlertSettingsTab, AlertStatus,
    AlertTriggerMode,
};
use crate::ui::scroll_widget::{draw_scrollbar, ScrollbarConfig, ScrollbarState as SbState};
use crate::ui::toolbar_render::ToolbarTheme;
use crate::ui::widgets::{render_modal_frame_only, ModalTheme, WidgetTheme};
use crate::ui::z_order::ZLayer;
use crate::ui::Icon;
use uzor::input::sense::Sense;
use uzor::types::Rect as WidgetRect;

// =============================================================================
// Layout constants
// =============================================================================

const MODAL_WIDTH: f64 = 480.0;
const HEADER_H: f64 = 36.0;
const TAB_BAR_H: f64 = 32.0;
const ROW_H: f64 = 32.0;
const PADDING: f64 = 16.0;
const ITEM_PADDING: f64 = 8.0;
const RADIUS: f64 = 6.0;
const BTN_H: f64 = 32.0;
const BTN_W: f64 = 100.0;
const BTN_GAP: f64 = 8.0;
const LABEL_W: f64 = 110.0;

// =============================================================================
// Helpers
// =============================================================================

/// Draw a simple labeled row with a readonly text value.
#[allow(clippy::too_many_arguments)]
fn draw_readonly_row(
    ctx: &mut dyn RenderContext,
    label: &str,
    value: &str,
    content_x: f64,
    y: f64,
    row_h: f64,
    label_w: f64,
    toolbar_theme: &ToolbarTheme,
) {
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, content_x, y + row_h / 2.0);

    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.fill_text(value, content_x + label_w, y + row_h / 2.0);
}

/// Draw an editable / dropdown-style field (filled rounded rect + text).
#[allow(clippy::too_many_arguments)]
fn draw_field(
    ctx: &mut dyn RenderContext,
    label: &str,
    value: &str,
    is_hovered: bool,
    content_x: f64,
    y: f64,
    row_h: f64,
    label_w: f64,
    field_w: f64,
    toolbar_theme: &ToolbarTheme,
) {
    // Label
    ctx.set_fill_color(&toolbar_theme.item_text_muted);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(label, content_x, y + row_h / 2.0);

    // Field background
    let field_x = content_x + label_w;
    let bg = if is_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
    ctx.set_fill_color(bg);
    ctx.fill_rounded_rect(field_x, y, field_w, row_h, 4.0);
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.stroke_rounded_rect(field_x, y, field_w, row_h, 4.0);

    // Value text
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("12px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(value, field_x + 8.0, y + row_h / 2.0);
}

/// Draw a dropdown field (with caret arrow).
#[allow(clippy::too_many_arguments)]
fn draw_dropdown_field(
    ctx: &mut dyn RenderContext,
    label: &str,
    value: &str,
    is_hovered: bool,
    content_x: f64,
    y: f64,
    row_h: f64,
    label_w: f64,
    field_w: f64,
    toolbar_theme: &ToolbarTheme,
) {
    draw_field(ctx, label, value, is_hovered, content_x, y, row_h, label_w, field_w, toolbar_theme);
    let field_x = content_x + label_w;
    let arrow_size = 12.0;
    let arrow_x = field_x + field_w - arrow_size - 6.0;
    let arrow_y = y + (row_h - arrow_size) / 2.0;
    draw_svg_icon(ctx, Icon::ChevronDown.svg(), arrow_x, arrow_y, arrow_size, arrow_size, &toolbar_theme.item_text_muted);
}

/// Draw Cancel + Create/Save buttons at the bottom of the modal.
#[allow(clippy::too_many_arguments)]
fn draw_buttons(
    ctx: &mut dyn RenderContext,
    result: &mut AlertSettingsResult,
    content_x: f64,
    content_w: f64,
    y: f64,
    editing: bool,
    state: &AlertSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
) {
    let buttons_x = content_x + content_w - BTN_W * 2.0 - BTN_GAP;

    // Cancel
    {
        let cancel_hovered = state.hovered_item_id.as_deref() == Some("alert_set:cancel");
        let cancel_bg = if cancel_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
        ctx.set_fill_color(cancel_bg);
        ctx.fill_rounded_rect(buttons_x, y, BTN_W, BTN_H, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(buttons_x, y, BTN_W, BTN_H, 4.0);

        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Cancel", buttons_x + BTN_W / 2.0, y + BTN_H / 2.0);

        let r = WidgetRect::new(buttons_x, y, BTN_W, BTN_H);
        result.content_items.push(("alert_set:cancel".to_string(), r));
        input_coordinator.register_on_layer("alert_set:cancel", r, Sense::CLICK, layer_id);
    }

    // Create / Save
    {
        let save_x = buttons_x + BTN_W + BTN_GAP;
        let save_hovered = state.hovered_item_id.as_deref() == Some("alert_set:save");
        let save_bg = if save_hovered { "#1e88e5" } else { "#2962ff" };
        ctx.set_fill_color(save_bg);
        ctx.fill_rounded_rect(save_x, y, BTN_W, BTN_H, 4.0);

        ctx.set_fill_color("#ffffff");
        ctx.set_font("bold 12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        let label = if editing { "Save" } else { "Create" };
        ctx.fill_text(label, save_x + BTN_W / 2.0, y + BTN_H / 2.0);

        let r = WidgetRect::new(save_x, y, BTN_W, BTN_H);
        result.content_items.push(("alert_set:save".to_string(), r));
        input_coordinator.register_on_layer("alert_set:save", r, Sense::CLICK, layer_id);
    }
}

// =============================================================================
// Main render entry point
// =============================================================================

/// Render the Alert Settings modal.
///
/// Returns hit-zone information used by the input handler for click dispatch,
/// drag, and scroll handling.
pub fn render_alert_settings_modal(
    ctx: &mut dyn RenderContext,
    screen_w: f64,
    screen_h: f64,
    state: &AlertSettingsState,
    frame_theme: &FrameTheme,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
) -> AlertSettingsResult {
    let mut result = AlertSettingsResult::default();

    // -------------------------------------------------------------------------
    // Compute modal height based on active tab
    // -------------------------------------------------------------------------
    let content_h = match state.active_tab {
        AlertSettingsTab::Settings => compute_settings_tab_height(state),
        AlertSettingsTab::Notifications => compute_notifications_tab_height(state),
        AlertSettingsTab::AlertsList => compute_alerts_list_tab_height(state),
    };
    let modal_height = HEADER_H + TAB_BAR_H + content_h;

    let (modal_x, modal_y) = state.position.unwrap_or_else(|| {
        ((screen_w - MODAL_WIDTH) / 2.0, (screen_h - modal_height) / 2.0)
    });
    let modal_x = modal_x.clamp(0.0, (screen_w - MODAL_WIDTH).max(0.0));
    let modal_y = modal_y.clamp(0.0, (screen_h - modal_height).max(0.0));

    let modal_rect = WidgetRect::new(modal_x, modal_y, MODAL_WIDTH, modal_height);
    result.modal_rect = modal_rect;

    // Modal frame
    let modal_theme = ModalTheme::from_frame_theme(
        &frame_theme.toolbar_bg,
        &toolbar_theme.separator,
        &toolbar_theme.item_text,
        &toolbar_theme.item_text_hover,
        &toolbar_theme.separator,
    );
    render_modal_frame_only(ctx, modal_rect, &modal_theme, RADIUS);

    // Push modal Z-layer
    let layer_id = ZLayer::Modal.push_named(input_coordinator, "alert_settings");

    // Catch-all background
    input_coordinator.register_on_layer(
        "alert_set:modal_bg",
        modal_rect,
        Sense::CLICK,
        &layer_id,
    );

    // -------------------------------------------------------------------------
    // Header
    // -------------------------------------------------------------------------
    let header_rect = WidgetRect::new(modal_x, modal_y, MODAL_WIDTH, HEADER_H);
    result.header_rect = header_rect;

    input_coordinator.register_on_layer(
        "alert_set:header",
        header_rect,
        Sense::CLICK,
        &layer_id,
    );

    let title = if state.editing_alert_id.is_some() { "Edit Alert" } else { "Create Alert" };
    ctx.set_fill_color(&toolbar_theme.item_text);
    ctx.set_font("bold 14px sans-serif");
    ctx.set_text_align(TextAlign::Left);
    ctx.set_text_baseline(TextBaseline::Middle);
    ctx.fill_text(title, modal_x + PADDING, modal_y + HEADER_H / 2.0);

    // Close button
    let close_size = 20.0;
    let close_x = modal_x + MODAL_WIDTH - PADDING - close_size;
    let close_y = modal_y + (HEADER_H - close_size) / 2.0;
    let close_rect = WidgetRect::new(close_x, close_y, close_size, close_size);
    result.close_btn_rect = close_rect;
    input_coordinator.register_on_layer("alert_set:close", close_rect, Sense::CLICK, &layer_id);

    draw_svg_icon(ctx, Icon::Close.svg(), close_x, close_y, close_size, close_size, &toolbar_theme.item_text_muted);

    // Header separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, modal_y + HEADER_H);
    ctx.line_to(modal_x + MODAL_WIDTH, modal_y + HEADER_H);
    ctx.stroke();

    // -------------------------------------------------------------------------
    // Tab bar
    // -------------------------------------------------------------------------
    let tab_bar_y = modal_y + HEADER_H;
    let alerts_count = state.all_alerts.len();
    let tab_labels: [(&str, &str); 3] = [
        ("Settings", "alert_set:tab:settings"),
        ("Notifications", "alert_set:tab:notifications"),
        (&"", "alert_set:tab:list"), // rendered specially below
    ];
    let tab_w = MODAL_WIDTH / 3.0;

    for (i, (base_label, widget_id)) in tab_labels.iter().enumerate() {
        let tab_x = modal_x + i as f64 * tab_w;
        let tab_rect = WidgetRect::new(tab_x, tab_bar_y, tab_w, TAB_BAR_H);

        let this_tab = match i {
            0 => AlertSettingsTab::Settings,
            1 => AlertSettingsTab::Notifications,
            _ => AlertSettingsTab::AlertsList,
        };
        let is_active = state.active_tab == this_tab;

        // Tab background
        if is_active {
            ctx.set_fill_color(&toolbar_theme.item_bg_active);
            ctx.fill_rect(tab_x, tab_bar_y, tab_w, TAB_BAR_H);
        }

        // Tab text
        let text_color = if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted };
        ctx.set_fill_color(text_color);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);

        let label = if i == 2 {
            format!("Alerts ({})", alerts_count)
        } else {
            base_label.to_string()
        };
        ctx.fill_text(&label, tab_x + tab_w / 2.0, tab_bar_y + TAB_BAR_H / 2.0);

        result.content_items.push((widget_id.to_string(), tab_rect));
        input_coordinator.register_on_layer(*widget_id, tab_rect, Sense::CLICK, &layer_id);
    }

    // Tab bar bottom separator
    ctx.set_stroke_color(&toolbar_theme.separator);
    ctx.set_stroke_width(1.0);
    ctx.begin_path();
    ctx.move_to(modal_x, tab_bar_y + TAB_BAR_H);
    ctx.line_to(modal_x + MODAL_WIDTH, tab_bar_y + TAB_BAR_H);
    ctx.stroke();

    // -------------------------------------------------------------------------
    // Content area — dispatch to tab-specific renderer
    // -------------------------------------------------------------------------
    let content_x = modal_x + PADDING;
    let content_y = modal_y + HEADER_H + TAB_BAR_H;
    let content_w = MODAL_WIDTH - PADDING * 2.0;
    result.content_rect = WidgetRect::new(content_x, content_y, content_w, content_h);

    match state.active_tab {
        AlertSettingsTab::Settings => render_settings_tab(
            ctx, state, toolbar_theme, input_coordinator, &layer_id, &mut result,
            content_x, content_y, content_w,
        ),
        AlertSettingsTab::Notifications => render_notifications_tab(
            ctx, state, toolbar_theme, input_coordinator, &layer_id, &mut result,
            content_x, content_y, content_w,
        ),
        AlertSettingsTab::AlertsList => render_alerts_list_tab(
            ctx, state, toolbar_theme, input_coordinator, &layer_id, &mut result,
            content_x, content_y, content_w,
        ),
    }

    result
}

// =============================================================================
// Height computation helpers
// =============================================================================

fn compute_settings_tab_height(state: &AlertSettingsState) -> f64 {
    let mut rows = 4usize; // source, condition, price, name
    if state.condition.requires_second_price() {
        rows += 1;
    }
    if state.condition.requires_percentage() {
        rows += 1;
    }
    // trigger_mode row + optional count row
    rows += 1;
    if matches!(state.trigger_mode, AlertTriggerMode::TimesN(_)) {
        rows += 1;
    }
    // kind_filter dropdown row (only for Signal alerts with available kinds)
    if !state.available_signal_kinds.is_empty() {
        rows += 1;
    }
    rows as f64 * (ROW_H + ITEM_PADDING) + PADDING * 2.0 + BTN_H + ITEM_PADDING
}

fn compute_notifications_tab_height(state: &AlertSettingsState) -> f64 {
    // Rows: toast, sound, telegram-header, [token, subscribers..., detect, detected..., screenshot, test+status], webhook-header, [url]
    let mut rows = 3usize; // toast, sound, telegram-toggle
    if state.notification_settings.telegram.enabled {
        let sub_count = state.notification_settings.telegram.subscribers.len();
        let det_count = state.tg_detected_users.len();
        rows += 1; // bot token
        rows += sub_count.max(1); // subscriber rows (at least 1 for "none" label or label row)
        rows += 1; // detect button
        rows += det_count; // detected user rows
        rows += 1; // send screenshots
        rows += 1; // test button + status
    }
    rows += 1; // webhook toggle
    if state.notification_settings.webhook.enabled {
        rows += 1; // webhook url field
    }
    rows as f64 * (ROW_H + ITEM_PADDING) + PADDING * 2.0 + BTN_H + ITEM_PADDING
}

fn compute_alerts_list_tab_height(state: &AlertSettingsState) -> f64 {
    let filter_h = ROW_H + ITEM_PADDING; // filter row
    let visible_alerts = filtered_alert_count(state);
    let list_h = if visible_alerts == 0 {
        ROW_H // "No alerts" message
    } else {
        (visible_alerts as f64 * (ROW_H + ITEM_PADDING)).min(240.0)
    };
    filter_h + list_h + PADDING * 2.0
}

fn filtered_alert_count(state: &AlertSettingsState) -> usize {
    state
        .all_alerts
        .iter()
        .filter(|a| match state.list_filter {
            AlertListFilter::All => true,
            AlertListFilter::Active => a.status == AlertStatus::Active,
            AlertListFilter::Triggered => a.status == AlertStatus::Triggered,
        })
        .count()
}

// =============================================================================
// Settings tab
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_settings_tab(
    ctx: &mut dyn RenderContext,
    state: &AlertSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut AlertSettingsResult,
    content_x: f64,
    content_y: f64,
    content_w: f64,
) {
    let field_x = content_x + LABEL_W;
    let field_w = content_w - LABEL_W;
    let mut y = content_y + PADDING;

    // --- 1. Source (readonly) ---
    draw_readonly_row(ctx, "Source", &state.source_name, content_x, y, ROW_H, LABEL_W, toolbar_theme);
    y += ROW_H + ITEM_PADDING;

    // --- 1b. Signal Kind filter (only for Signal alerts with available kinds) ---
    if !state.available_signal_kinds.is_empty() {
        let kind_display = state.kind_filter.as_deref().unwrap_or("Any");
        let is_hovered = state.hovered_item_id.as_deref() == Some("alert_set:item:kind_filter");
        draw_dropdown_field(
            ctx,
            "Signal Kind",
            kind_display,
            is_hovered,
            content_x,
            y,
            ROW_H,
            LABEL_W,
            field_w,
            toolbar_theme,
        );
        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:kind_filter".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:kind_filter", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // --- 2. Condition dropdown ---
    {
        let is_hovered = state.hovered_item_id.as_deref() == Some("alert_set:item:condition");
        draw_dropdown_field(
            ctx,
            "Condition",
            state.condition.display_name(),
            is_hovered,
            content_x,
            y,
            ROW_H,
            LABEL_W,
            field_w,
            toolbar_theme,
        );
        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:condition".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:condition", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // --- 3. Price ---
    {
        let is_hovered = state.hovered_item_id.as_deref() == Some("alert_set:item:price");
        draw_field(
            ctx,
            "Price",
            &format!("{:.2}", state.price),
            is_hovered,
            content_x,
            y,
            ROW_H,
            LABEL_W,
            field_w,
            toolbar_theme,
        );
        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:price".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:price", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // --- 4. Price2 (conditional — range conditions) ---
    if state.condition.requires_second_price() {
        let is_hovered = state.hovered_item_id.as_deref() == Some("alert_set:item:price2");
        draw_field(
            ctx,
            "Price 2",
            &format!("{:.2}", state.price2),
            is_hovered,
            content_x,
            y,
            ROW_H,
            LABEL_W,
            field_w,
            toolbar_theme,
        );
        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:price2".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:price2", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // --- 5. Percentage (conditional — MovingUp/Down conditions) ---
    if state.condition.requires_percentage() {
        let is_hovered = state.hovered_item_id.as_deref() == Some("alert_set:item:percentage");
        let pct_text = format!("{:.1}%", state.percentage);
        draw_field(
            ctx,
            "Percentage",
            &pct_text,
            is_hovered,
            content_x,
            y,
            ROW_H,
            LABEL_W,
            field_w,
            toolbar_theme,
        );
        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:percentage".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:percentage", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // --- 6. Trigger Mode dropdown ---
    {
        let is_hovered = state.hovered_item_id.as_deref() == Some("alert_set:item:trigger_mode");
        draw_dropdown_field(
            ctx,
            "Trigger Mode",
            state.trigger_mode.display_name(),
            is_hovered,
            content_x,
            y,
            ROW_H,
            LABEL_W,
            field_w,
            toolbar_theme,
        );
        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:trigger_mode".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:trigger_mode", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // --- 6b. Count field (only when TimesN is selected) ---
    if matches!(state.trigger_mode, AlertTriggerMode::TimesN(_)) {
        let is_hovered = state.hovered_item_id.as_deref() == Some("alert_set:item:times_n");
        draw_field(
            ctx,
            "Count",
            &state.times_n.to_string(),
            is_hovered,
            content_x,
            y,
            ROW_H,
            LABEL_W,
            field_w,
            toolbar_theme,
        );
        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:times_n".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:times_n", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // --- 7. Message/Name ---
    {
        let is_hovered = state.hovered_item_id.as_deref() == Some("alert_set:item:name");
        let (display_name, text_color) = if state.name.is_empty() {
            ("Alert name...", toolbar_theme.item_text_muted.as_str())
        } else {
            (state.name.as_str(), toolbar_theme.item_text.as_str())
        };

        // Label
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("Message", content_x, y + ROW_H / 2.0);

        // Field background
        let name_bg = if is_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
        ctx.set_fill_color(name_bg);
        ctx.fill_rounded_rect(field_x, y, field_w, ROW_H, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(field_x, y, field_w, ROW_H, 4.0);

        ctx.set_fill_color(text_color);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(display_name, field_x + 8.0, y + ROW_H / 2.0);

        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:name".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:name", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // --- Buttons (rendered BEFORE dropdown overlays so dropdowns draw on top) ---
    let btn_y = y + PADDING / 2.0;
    draw_buttons(
        ctx, result, content_x, content_w, btn_y,
        state.editing_alert_id.is_some(), state, toolbar_theme,
        input_coordinator, layer_id,
    );

    // --- Condition dropdown overlay (LAST — renders and registers on top) ---
    if state.condition_dropdown_open {
        let conditions = AlertCondition::all();
        let dd_item_h = 28.0;
        let dd_height = conditions.len() as f64 * dd_item_h;
        // Position below the condition row
        let dd_y = content_y + PADDING + (ROW_H + ITEM_PADDING) + ROW_H;
        let dd_rect = WidgetRect::new(field_x, dd_y, field_w, dd_height);
        result.content_items.push(("alert_set:condition_dropdown".to_string(), dd_rect));

        ctx.set_fill_color(&toolbar_theme.dropdown_bg);
        ctx.fill_rounded_rect(field_x, dd_y, field_w, dd_height, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(field_x, dd_y, field_w, dd_height, 4.0);

        for (i, cond) in conditions.iter().enumerate() {
            let item_y = dd_y + i as f64 * dd_item_h;
            let dd_item_id = format!("alert_set:cond:{}", i);
            let dd_hovered = state.hovered_item_id.as_deref() == Some(&dd_item_id);
            let is_selected = *cond == state.condition;

            if dd_hovered {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rect(field_x + 1.0, item_y, field_w - 2.0, dd_item_h);
            }
            if is_selected {
                ctx.set_fill_color(&toolbar_theme.item_bg_active);
                ctx.fill_rect(field_x + 1.0, item_y, field_w - 2.0, dd_item_h);
            }

            let text_color = if is_selected { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text };
            ctx.set_fill_color(text_color);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(cond.display_name(), field_x + 8.0, item_y + dd_item_h / 2.0);

            let r = WidgetRect::new(field_x, item_y, field_w, dd_item_h);
            result.content_items.push((dd_item_id.clone(), r));
            input_coordinator.register_on_layer(dd_item_id.as_str(), r, Sense::CLICK, layer_id);
        }
    }

    // --- Trigger Mode dropdown overlay (LAST — renders and registers on top) ---
    if state.trigger_mode_dropdown_open {
        let modes = [
            AlertTriggerMode::OneShot,
            AlertTriggerMode::EveryTime,
            AlertTriggerMode::OncePerBar,
            AlertTriggerMode::TimesN(state.times_n),
        ];
        let dd_item_h = 28.0;
        let dd_height = modes.len() as f64 * dd_item_h;
        // Position below trigger mode row — compute row index (0-based, after source+condition+price)
        let mut tmode_row_index = 3usize; // source + condition + price
        if state.condition.requires_second_price() {
            tmode_row_index += 1;
        }
        if state.condition.requires_percentage() {
            tmode_row_index += 1;
        }
        let dd_y = content_y + PADDING + tmode_row_index as f64 * (ROW_H + ITEM_PADDING) + ROW_H;

        let dd_rect = WidgetRect::new(field_x, dd_y, field_w, dd_height);
        result.content_items.push(("alert_set:tmode_dropdown".to_string(), dd_rect));

        ctx.set_fill_color(&toolbar_theme.dropdown_bg);
        ctx.fill_rounded_rect(field_x, dd_y, field_w, dd_height, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(field_x, dd_y, field_w, dd_height, 4.0);

        for (i, mode) in modes.iter().enumerate() {
            let item_y = dd_y + i as f64 * dd_item_h;
            let dd_item_id = format!("alert_set:tmode:{}", i);
            let dd_hovered = state.hovered_item_id.as_deref() == Some(&dd_item_id);
            let is_selected = std::mem::discriminant(mode) == std::mem::discriminant(&state.trigger_mode);

            if dd_hovered {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rect(field_x + 1.0, item_y, field_w - 2.0, dd_item_h);
            }
            if is_selected {
                ctx.set_fill_color(&toolbar_theme.item_bg_active);
                ctx.fill_rect(field_x + 1.0, item_y, field_w - 2.0, dd_item_h);
            }

            let text_color = if is_selected { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text };
            ctx.set_fill_color(text_color);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(mode.display_name(), field_x + 8.0, item_y + dd_item_h / 2.0);

            let r = WidgetRect::new(field_x, item_y, field_w, dd_item_h);
            result.content_items.push((dd_item_id.clone(), r));
            input_coordinator.register_on_layer(dd_item_id.as_str(), r, Sense::CLICK, layer_id);
        }
    }
}

// =============================================================================
// Notifications tab
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_notifications_tab(
    ctx: &mut dyn RenderContext,
    state: &AlertSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut AlertSettingsResult,
    content_x: f64,
    content_y: f64,
    content_w: f64,
) {
    let mut y = content_y + PADDING;
    let ns = &state.notification_settings;

    // ------------------------------------------------------------------
    // Local helper: draw a toggle checkbox row and register its hit zone.
    // ------------------------------------------------------------------
    let draw_toggle = |ctx: &mut dyn RenderContext,
                       result: &mut AlertSettingsResult,
                       input_coordinator: &mut uzor::input::InputCoordinator,
                       label: &str,
                       widget_id: &str,
                       enabled: bool,
                       row_y: f64| {
        let cb_size = 16.0;
        let cb_x = content_x;
        let cb_y = row_y + (ROW_H - cb_size) / 2.0;

        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(cb_x, cb_y, cb_size, cb_size, 2.0);

        if enabled {
            ctx.set_fill_color(&toolbar_theme.item_text_active);
            ctx.fill_rounded_rect(cb_x + 3.0, cb_y + 3.0, cb_size - 6.0, cb_size - 6.0, 1.0);
        }

        ctx.set_fill_color(&toolbar_theme.item_text);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, cb_x + cb_size + 10.0, row_y + ROW_H / 2.0);

        let r = WidgetRect::new(cb_x, row_y, content_w, ROW_H);
        result.content_items.push((widget_id.to_string(), r));
        input_coordinator.register_on_layer(widget_id, r, Sense::CLICK, layer_id);
    };

    // ------------------------------------------------------------------
    // Local helper: draw a labeled text input field (with focus highlight).
    // ------------------------------------------------------------------
    let draw_text_field = |ctx: &mut dyn RenderContext,
                           result: &mut AlertSettingsResult,
                           input_coordinator: &mut uzor::input::InputCoordinator,
                           label: &str,
                           widget_id: &str,
                           value: &str,
                           placeholder: &str,
                           focused: bool,
                           row_y: f64| {
        let label_w = LABEL_W;
        let field_x = content_x + label_w;
        let field_w = content_w - label_w;

        // Label
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, content_x, row_y + ROW_H / 2.0);

        // Field background
        let bg = if focused { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(field_x, row_y, field_w, ROW_H, 4.0);

        // Focus border highlight
        let border_color = if focused { &toolbar_theme.item_text_active } else { &toolbar_theme.separator };
        ctx.set_stroke_color(border_color);
        ctx.set_stroke_width(if focused { 1.5 } else { 1.0 });
        ctx.stroke_rounded_rect(field_x, row_y, field_w, ROW_H, 4.0);

        // Value text (mask with asterisks for token field if non-empty and not focused)
        let (display_text, text_color) = if value.is_empty() {
            (placeholder.to_string(), toolbar_theme.item_text_muted.clone())
        } else {
            (value.to_string(), toolbar_theme.item_text.clone())
        };
        ctx.set_fill_color(&text_color);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(&display_text, field_x + 8.0, row_y + ROW_H / 2.0);

        let r = WidgetRect::new(field_x, row_y, field_w, ROW_H);
        result.content_items.push((widget_id.to_string(), r));
        input_coordinator.register_on_layer(widget_id, r, Sense::CLICK, layer_id);
    };

    // === 1. Toast Notifications toggle ===
    draw_toggle(
        ctx, result, input_coordinator,
        "Toast notifications", "alert_set:notif:toast",
        ns.toast_enabled, y,
    );
    y += ROW_H + ITEM_PADDING;

    // === 2. Sound toggle ===
    draw_toggle(
        ctx, result, input_coordinator,
        "Sound", "alert_set:notif:sound",
        ns.sound_enabled, y,
    );
    y += ROW_H + ITEM_PADDING;

    // === 3. Telegram Bot section ===
    draw_toggle(
        ctx, result, input_coordinator,
        "Telegram Bot", "alert_set:notif:telegram",
        ns.telegram.enabled, y,
    );
    y += ROW_H + ITEM_PADDING;

    if ns.telegram.enabled {
        // -- Bot Token field --
        // Show masked value: if non-empty show "****...last4", else placeholder
        let token_display = if state.tg_bot_token_input.is_empty() {
            String::new()
        } else if state.tg_token_focused {
            state.tg_bot_token_input.clone()
        } else {
            // Show last 4 chars masked
            let chars: Vec<char> = state.tg_bot_token_input.chars().collect();
            let visible = chars.len().min(4);
            let masked_count = chars.len().saturating_sub(visible);
            let masked: String = "*".repeat(masked_count.min(8));
            let tail: String = chars[chars.len() - visible..].iter().collect();
            format!("{}{}", masked, tail)
        };
        draw_text_field(
            ctx, result, input_coordinator,
            "Bot Token", "alert_set:notif:tg_token",
            &token_display,
            "Paste bot token...",
            state.tg_token_focused,
            y,
        );
        y += ROW_H + ITEM_PADDING;

        // -- Subscribers section header --
        {
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Subscribers", content_x + LABEL_W, y + ROW_H / 2.0);
            y += ROW_H + ITEM_PADDING;
        }

        // -- Subscriber rows --
        {
            let subscribers = ns.telegram.subscribers.clone();
            if subscribers.is_empty() {
                ctx.set_fill_color(&toolbar_theme.separator);
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(
                    "No subscribers. Send /start to your bot, then Detect.",
                    content_x + LABEL_W,
                    y + ROW_H / 2.0,
                );
                y += ROW_H + ITEM_PADDING;
            } else {
                for (idx, sub) in subscribers.iter().enumerate() {
                    let toggle_id = format!("alert_set:notif:tg_sub_toggle:{idx}");
                    let remove_id = format!("alert_set:notif:tg_sub_remove:{idx}");

                    // Checkbox (active toggle)
                    let chk_size = 14.0;
                    let chk_x = content_x;
                    let chk_y = y + (ROW_H - chk_size) / 2.0;
                    let chk_hovered = state.hovered_item_id.as_deref() == Some(&toggle_id);
                    let chk_bg = if sub.active { "#2563eb" } else if chk_hovered { "#374151" } else { "#1f2937" };
                    ctx.set_fill_color(chk_bg);
                    ctx.fill_rounded_rect(chk_x, chk_y, chk_size, chk_size, 3.0);
                    if sub.active {
                        ctx.set_fill_color("#ffffff");
                        ctx.set_font("10px sans-serif");
                        ctx.set_text_align(TextAlign::Center);
                        ctx.set_text_baseline(TextBaseline::Middle);
                        ctx.fill_text("✓", chk_x + chk_size / 2.0, chk_y + chk_size / 2.0);
                    }
                    let chk_r = WidgetRect::new(chk_x, y, chk_size + 4.0, ROW_H);
                    result.content_items.push((toggle_id.clone(), chk_r));
                    input_coordinator.register_on_layer(toggle_id.as_str(), chk_r, Sense::CLICK, layer_id);

                    // Subscriber label: display_name @username (chat_id)
                    let label = if sub.username.is_empty() {
                        format!("{} ({})", sub.display_name, sub.chat_id)
                    } else {
                        format!("{} {} ({})", sub.display_name, sub.username, sub.chat_id)
                    };
                    ctx.set_fill_color(&toolbar_theme.item_text);
                    ctx.set_font("11px sans-serif");
                    ctx.set_text_align(TextAlign::Left);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text(&label, content_x + chk_size + 8.0, y + ROW_H / 2.0);

                    // Remove [X] button on the right
                    let rm_btn_w = 24.0;
                    let rm_x = content_x + content_w - rm_btn_w;
                    let rm_hovered = state.hovered_item_id.as_deref() == Some(&remove_id);
                    let rm_bg = if rm_hovered { "#dc2626" } else { "#374151" };
                    ctx.set_fill_color(rm_bg);
                    ctx.fill_rounded_rect(rm_x, y + (ROW_H - 20.0) / 2.0, rm_btn_w, 20.0, 3.0);
                    ctx.set_fill_color("#ffffff");
                    ctx.set_font("11px sans-serif");
                    ctx.set_text_align(TextAlign::Center);
                    ctx.set_text_baseline(TextBaseline::Middle);
                    ctx.fill_text("X", rm_x + rm_btn_w / 2.0, y + ROW_H / 2.0);
                    let rm_r = WidgetRect::new(rm_x, y, rm_btn_w, ROW_H);
                    result.content_items.push((remove_id.clone(), rm_r));
                    input_coordinator.register_on_layer(remove_id.as_str(), rm_r, Sense::CLICK, layer_id);

                    y += ROW_H + ITEM_PADDING;
                }
            }
        }

        // -- Detect Users button --
        {
            let detect_btn_w = 100.0;
            let detect_hovered = state.hovered_item_id.as_deref() == Some("alert_set:notif:tg_detect");
            let detect_bg = if detect_hovered { "#2563eb" } else { "#1e40af" };
            ctx.set_fill_color(detect_bg);
            ctx.fill_rounded_rect(content_x + LABEL_W, y, detect_btn_w, ROW_H, 4.0);
            ctx.set_fill_color("#ffffff");
            ctx.set_font("11px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Detect Users", content_x + LABEL_W + detect_btn_w / 2.0, y + ROW_H / 2.0);
            let detect_r = WidgetRect::new(content_x + LABEL_W, y, detect_btn_w, ROW_H);
            result.content_items.push(("alert_set:notif:tg_detect".to_string(), detect_r));
            input_coordinator.register_on_layer("alert_set:notif:tg_detect", detect_r, Sense::CLICK, layer_id);
            y += ROW_H + ITEM_PADDING;
        }

        // -- Detected users (not yet subscribed) --
        {
            let existing_ids: std::collections::HashSet<&str> = ns
                .telegram
                .subscribers
                .iter()
                .map(|s| s.chat_id.as_str())
                .collect();
            let new_detected: Vec<_> = state
                .tg_detected_users
                .iter()
                .enumerate()
                .filter(|(_, (cid, _, _))| !existing_ids.contains(cid.as_str()))
                .collect();

            for (idx, (cid, name, uname)) in &new_detected {
                let add_id = format!("alert_set:notif:tg_add_detected:{idx}");
                let label = if uname.is_empty() {
                    format!("{} ({})", name, cid)
                } else {
                    format!("{} {} ({})", name, uname, cid)
                };
                ctx.set_fill_color(&toolbar_theme.item_text);
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&label, content_x + LABEL_W, y + ROW_H / 2.0);

                let add_btn_w = 40.0;
                let add_x = content_x + content_w - add_btn_w;
                let add_hovered = state.hovered_item_id.as_deref() == Some(&add_id);
                let add_bg = if add_hovered { "#059669" } else { "#065f46" };
                ctx.set_fill_color(add_bg);
                ctx.fill_rounded_rect(add_x, y + (ROW_H - 20.0) / 2.0, add_btn_w, 20.0, 3.0);
                ctx.set_fill_color("#ffffff");
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Center);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text("Add", add_x + add_btn_w / 2.0, y + ROW_H / 2.0);
                let add_r = WidgetRect::new(add_x, y, add_btn_w, ROW_H);
                result.content_items.push((add_id.clone(), add_r));
                input_coordinator.register_on_layer(add_id.as_str(), add_r, Sense::CLICK, layer_id);

                y += ROW_H + ITEM_PADDING;
            }
        }

        // -- Send Screenshots toggle --
        draw_toggle(
            ctx, result, input_coordinator,
            "Send screenshots", "alert_set:notif:tg_screenshot",
            ns.telegram.send_screenshots, y,
        );
        y += ROW_H + ITEM_PADDING;

        // -- Test Connection button + status message --
        {
            let test_btn_w = 120.0;
            let test_hovered = state.hovered_item_id.as_deref() == Some("alert_set:notif:tg_test");
            let test_bg = if test_hovered { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
            ctx.set_fill_color(test_bg);
            ctx.fill_rounded_rect(content_x, y, test_btn_w, ROW_H, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(content_x, y, test_btn_w, ROW_H, 4.0);

            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text("Test Connection", content_x + test_btn_w / 2.0, y + ROW_H / 2.0);

            let test_r = WidgetRect::new(content_x, y, test_btn_w, ROW_H);
            result.content_items.push(("alert_set:notif:tg_test".to_string(), test_r));
            input_coordinator.register_on_layer("alert_set:notif:tg_test", test_r, Sense::CLICK, layer_id);

            // Status message (green for success, red for error)
            if !state.tg_status_message.is_empty() {
                let status_color = if state.tg_status_message.starts_with("Connected")
                    || state.tg_status_message.starts_with("Sent")
                {
                    "#4caf50"
                } else {
                    "#f44336"
                };
                ctx.set_fill_color(status_color);
                ctx.set_font("11px sans-serif");
                ctx.set_text_align(TextAlign::Left);
                ctx.set_text_baseline(TextBaseline::Middle);
                ctx.fill_text(&state.tg_status_message, content_x + test_btn_w + 10.0, y + ROW_H / 2.0);
            }

            y += ROW_H + ITEM_PADDING;
        }
    }

    // === 4. Webhook section ===
    draw_toggle(
        ctx, result, input_coordinator,
        "Webhook", "alert_set:notif:webhook",
        ns.webhook.enabled, y,
    );
    y += ROW_H + ITEM_PADDING;

    // Webhook URL field (shown only when webhook is enabled)
    if ns.webhook.enabled {
        let field_x = content_x + LABEL_W;
        let field_w = content_w - LABEL_W;
        let is_focused = state.hovered_item_id.as_deref() == Some("alert_set:item:webhook_url");
        let (url_display, text_color) = if ns.webhook.url.is_empty() {
            ("https://...", toolbar_theme.item_text_muted.as_str())
        } else {
            (ns.webhook.url.as_str(), toolbar_theme.item_text.as_str())
        };

        // Label
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("URL", content_x, y + ROW_H / 2.0);

        // Field background
        let bg = if is_focused { &toolbar_theme.item_bg_hover } else { &toolbar_theme.dropdown_bg };
        ctx.set_fill_color(bg);
        ctx.fill_rounded_rect(field_x, y, field_w, ROW_H, 4.0);
        ctx.set_stroke_color(&toolbar_theme.separator);
        ctx.set_stroke_width(1.0);
        ctx.stroke_rounded_rect(field_x, y, field_w, ROW_H, 4.0);

        ctx.set_fill_color(text_color);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Left);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(url_display, field_x + 8.0, y + ROW_H / 2.0);

        let r = WidgetRect::new(field_x, y, field_w, ROW_H);
        result.content_items.push(("alert_set:item:webhook_url".to_string(), r));
        input_coordinator.register_on_layer("alert_set:item:webhook_url", r, Sense::CLICK, layer_id);
        y += ROW_H + ITEM_PADDING;
    }

    // Buttons
    let btn_y = y + PADDING / 2.0;
    draw_buttons(
        ctx, result, content_x, content_w, btn_y,
        state.editing_alert_id.is_some(), state, toolbar_theme,
        input_coordinator, layer_id,
    );
}

// =============================================================================
// Alerts List tab
// =============================================================================

#[allow(clippy::too_many_arguments)]
fn render_alerts_list_tab(
    ctx: &mut dyn RenderContext,
    state: &AlertSettingsState,
    toolbar_theme: &ToolbarTheme,
    input_coordinator: &mut uzor::input::InputCoordinator,
    layer_id: &uzor::input::LayerId,
    result: &mut AlertSettingsResult,
    content_x: f64,
    content_y: f64,
    content_w: f64,
) {
    let mut y = content_y + PADDING;

    // ---- Filter row ----
    let filter_items = [
        ("All", AlertListFilter::All, "alert_set:filter:all"),
        ("Active", AlertListFilter::Active, "alert_set:filter:active"),
        ("Triggered", AlertListFilter::Triggered, "alert_set:filter:triggered"),
    ];
    let filter_btn_w = 80.0;
    let filter_btn_h = ROW_H;
    let filter_btn_gap = 6.0;

    for (i, (label, filter, widget_id)) in filter_items.iter().enumerate() {
        let btn_x = content_x + i as f64 * (filter_btn_w + filter_btn_gap);
        let is_active = state.list_filter == *filter;

        if is_active {
            ctx.set_fill_color(&toolbar_theme.item_bg_active);
            ctx.fill_rounded_rect(btn_x, y, filter_btn_w, filter_btn_h, 4.0);
        } else {
            ctx.set_fill_color(&toolbar_theme.dropdown_bg);
            ctx.fill_rounded_rect(btn_x, y, filter_btn_w, filter_btn_h, 4.0);
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.stroke_rounded_rect(btn_x, y, filter_btn_w, filter_btn_h, 4.0);
        }

        let text_color = if is_active { &toolbar_theme.item_text_active } else { &toolbar_theme.item_text_muted };
        ctx.set_fill_color(text_color);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text(label, btn_x + filter_btn_w / 2.0, y + filter_btn_h / 2.0);

        let r = WidgetRect::new(btn_x, y, filter_btn_w, filter_btn_h);
        result.content_items.push((widget_id.to_string(), r));
        input_coordinator.register_on_layer(*widget_id, r, Sense::CLICK, layer_id);
    }
    y += ROW_H + ITEM_PADDING;

    // ---- Alert list ----
    let filtered: Vec<_> = state
        .all_alerts
        .iter()
        .filter(|a| match state.list_filter {
            AlertListFilter::All => true,
            AlertListFilter::Active => a.status == AlertStatus::Active,
            AlertListFilter::Triggered => a.status == AlertStatus::Triggered,
        })
        .collect();

    // The list viewport starts after the filter row and fills the remaining content area.
    // content_h is computed by compute_alerts_list_tab_height() which caps at 240px list area.
    // We derive list_h from the same formula so it matches exactly.
    let list_top = y;
    let list_h = if filtered.is_empty() {
        ROW_H
    } else {
        (filtered.len() as f64 * (ROW_H + ITEM_PADDING)).min(240.0)
    };

    result.list_viewport_rect = Some(WidgetRect::new(content_x, list_top, content_w, list_h));

    if filtered.is_empty() {
        ctx.set_fill_color(&toolbar_theme.item_text_muted);
        ctx.set_font("12px sans-serif");
        ctx.set_text_align(TextAlign::Center);
        ctx.set_text_baseline(TextBaseline::Middle);
        ctx.fill_text("No alerts", content_x + content_w / 2.0, list_top + ROW_H / 2.0);
        result.list_total_content_height = ROW_H;
    } else {
        let list_item_h = ROW_H;
        let action_btn_w = 24.0;
        let action_btn_gap = 4.0;
        let scrollbar_w = 6.0;

        let total_content_h = filtered.len() as f64 * (list_item_h + ITEM_PADDING);
        result.list_total_content_height = total_content_h;

        // Clip list content to viewport.
        ctx.save();
        ctx.clip_rect(content_x, list_top, content_w, list_h);

        let mut item_y = list_top - state.list_scroll.offset;

        for alert in filtered.iter() {
            let row_id = format!("alert_set:list_item:{}", alert.id);
            let delete_id = format!("alert_set:list_delete:{}", alert.id);
            let pause_id = format!("alert_set:list_pause:{}", alert.id);

            let is_hovered = state.hovered_item_id.as_deref() == Some(row_id.as_str());
            if is_hovered {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(content_x, item_y, content_w, list_item_h, 3.0);
            }

            // Status dot
            let dot_r = 4.0;
            let dot_x = content_x + dot_r + 2.0;
            let dot_y = item_y + list_item_h / 2.0;
            let dot_color = match alert.status {
                AlertStatus::Active => "#4caf50",
                AlertStatus::Triggered => "#ff9800",
                AlertStatus::Paused => "#9e9e9e",
                AlertStatus::Expired => "#616161",
            };
            ctx.set_fill_color(dot_color);
            ctx.begin_path();
            ctx.arc(dot_x, dot_y, dot_r, 0.0, std::f64::consts::TAU);
            ctx.fill();

            // Source label
            let source_text = alert.source_display();
            ctx.set_fill_color(&toolbar_theme.item_text);
            ctx.set_font("12px sans-serif");
            ctx.set_text_align(TextAlign::Left);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(&source_text, content_x + dot_r * 2.0 + 8.0, item_y + list_item_h / 2.0);

            // Condition + price (right-aligned before action buttons)
            let cond_text = format!("{} {:.2}", alert.condition.display_name(), alert.price);
            let actions_total_w = (action_btn_w + action_btn_gap) * 2.0;
            let cond_x = content_x + content_w - actions_total_w - 8.0;
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.set_text_align(TextAlign::Right);
            ctx.fill_text(&cond_text, cond_x, item_y + list_item_h / 2.0);

            // Delete button
            let delete_x = content_x + content_w - actions_total_w;
            let delete_btn_hovered = state.hovered_item_id.as_deref() == Some(delete_id.as_str());
            if delete_btn_hovered {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(delete_x, item_y + 4.0, action_btn_w, list_item_h - 8.0, 3.0);
            }
            let icon_s = action_btn_w - 6.0;
            let ix = delete_x + (action_btn_w - icon_s) / 2.0;
            let iy = item_y + (list_item_h - icon_s) / 2.0;
            draw_svg_icon(ctx, Icon::Close.svg(), ix, iy, icon_s, icon_s, &toolbar_theme.item_text_muted);

            let delete_r = WidgetRect::new(delete_x, item_y, action_btn_w, list_item_h);
            result.content_items.push((delete_id.clone(), delete_r));
            input_coordinator.register_on_layer(delete_id.as_str(), delete_r, Sense::CLICK, layer_id);

            // Pause/Resume button
            let pause_x = delete_x + action_btn_w + action_btn_gap;
            let pause_btn_hovered = state.hovered_item_id.as_deref() == Some(pause_id.as_str());
            if pause_btn_hovered {
                ctx.set_fill_color(&toolbar_theme.item_bg_hover);
                ctx.fill_rounded_rect(pause_x, item_y + 4.0, action_btn_w, list_item_h - 8.0, 3.0);
            }
            let pause_label = if alert.status == AlertStatus::Paused { ">" } else { "||" };
            ctx.set_fill_color(&toolbar_theme.item_text_muted);
            ctx.set_font("10px sans-serif");
            ctx.set_text_align(TextAlign::Center);
            ctx.set_text_baseline(TextBaseline::Middle);
            ctx.fill_text(pause_label, pause_x + action_btn_w / 2.0, item_y + list_item_h / 2.0);

            let pause_r = WidgetRect::new(pause_x, item_y, action_btn_w, list_item_h);
            result.content_items.push((pause_id.clone(), pause_r));
            input_coordinator.register_on_layer(pause_id.as_str(), pause_r, Sense::CLICK, layer_id);

            // Row hit zone for edit
            let row_r = WidgetRect::new(content_x, item_y, content_w - actions_total_w - 8.0, list_item_h);
            result.content_items.push((row_id.clone(), row_r));
            input_coordinator.register_on_layer(row_id.as_str(), row_r, Sense::CLICK, layer_id);

            // Row separator
            ctx.set_stroke_color(&toolbar_theme.separator);
            ctx.set_stroke_width(1.0);
            ctx.begin_path();
            ctx.move_to(content_x, item_y + list_item_h);
            ctx.line_to(content_x + content_w, item_y + list_item_h);
            ctx.stroke();

            item_y += list_item_h + ITEM_PADDING;
        }

        ctx.restore();

        // Draw scrollbar if content overflows viewport.
        if total_content_h > list_h {
            let sb_x = content_x + content_w - scrollbar_w - 2.0;
            let sb_rect = WidgetRect::new(sb_x, list_top, scrollbar_w, list_h);
            let sb_config = ScrollbarConfig::new(total_content_h, list_h, state.list_scroll.offset);
            let sb_state = if state.list_scroll.is_dragging {
                SbState::Dragging
            } else {
                SbState::Active
            };
            let widget_theme = WidgetTheme::default();
            let sb_result = draw_scrollbar(ctx, &sb_config, sb_state, sb_rect, &widget_theme, None);
            result.scrollbar_handle_rect = Some(sb_result.handle_rect);
            result.scrollbar_track_rect = Some(sb_result.track_rect);
        }
    }
}
