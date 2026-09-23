//! ApplicationHandler::window_event — thin adapter layer.
//!
//! Translates winit WindowEvent → ShellEvent and calls
//! mlc_app_input_shell::process_event via VelloPlatform.
//! After process_event, copies DispatchOut fields back into PerWindowState
//! so the rest of main.rs (about_to_wait) continues to work unchanged.

use crate::{screenshot, App};
use winit::{
    event::{ElementState, MouseButton, WindowEvent},
    event_loop::ActiveEventLoop,
    window::WindowId,
};
use mlc_app_input_shell::{
    ShellEvent,
    NormalizedModifiers,
    MouseBtn,
    LogicalKey,
    PhysicalKey,
    UNITS_PER_NOTCH,
    pixels_to_notch_units,
};
use crate::window::vello_platform::VelloPlatform;

impl App<'_> {
    pub(crate) fn handle_window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        id: WindowId,
        event: WindowEvent,
    ) {
        // OS-level close → set the flag directly (no ShellEvent needed).
        if let WindowEvent::CloseRequested = event {
            if let Some(pw) = self.windows.get_mut(&id) {
                pw.close_requested = true;
            }
            return;
        }

        if let WindowEvent::Focused(true) = event {
            self.last_focused = Some(id);
        }

        if let WindowEvent::Resized(_) = event {
            self.wait_for_gpu_frame();
        }

        let Some(pw) = self.windows.get_mut(&id) else { return; };

        // ── GPU resize (must happen before process_event) ─────────────────
        if let WindowEvent::Resized(size) = event {
            if size.width > 0 && size.height > 0 {
                self.render_cx
                    .resize_surface(&mut pw.surface, size.width, size.height);
                let device = &self.render_cx.devices[pw.surface.dev_id].device;
                screenshot::add_copy_src_to_target_texture(&mut pw.surface, device);
                // Telemetry's screen_width/screen_height — last-resized window
                // wins, same "one deterministic value across N windows" choice
                // already made for other single-scalar snapshots (see App::telemetry).
                self.telemetry.set_screen_size(size.width, size.height);
            }
        }

        // ── Translate winit → ShellEvent ──────────────────────────────────
        let shell_event = match event {
            WindowEvent::CloseRequested => unreachable!(),

            WindowEvent::Focused(true) => ShellEvent::Focused,
            WindowEvent::Focused(false) => return,

            WindowEvent::Resized(size) => ShellEvent::Resized {
                w: size.width,
                h: size.height,
            },

            WindowEvent::Moved(_) => ShellEvent::Moved,

            WindowEvent::CursorMoved { position, .. } => ShellEvent::CursorMoved {
                x: position.x,
                y: position.y,
            },

            WindowEvent::CursorLeft { .. } => ShellEvent::CursorLeft,

            WindowEvent::MouseInput { state, button, .. } => {
                let btn = match button {
                    MouseButton::Left   => MouseBtn::Left,
                    MouseButton::Right  => MouseBtn::Right,
                    MouseButton::Middle => MouseBtn::Middle,
                    _                   => MouseBtn::Other,
                };
                let pressed = state == ElementState::Pressed;
                let (x, y) = pw.last_mouse_pos;
                ShellEvent::MouseInput { button: btn, pressed, x, y }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                let (dx, dy) = match delta {
                    winit::event::MouseScrollDelta::LineDelta(x, y) => {
                        (x as f64 * UNITS_PER_NOTCH, y as f64 * UNITS_PER_NOTCH)
                    }
                    // Trackpad: raw pixels, same contract conversion the web
                    // shell applies to its own CSS-pixel deltas (recon: item
                    // 1, `docs/mlc/research/owner-review-2026-09-23/
                    // input-hotkeys-and-modal-wheel.md`) — previously passed
                    // through unnormalized.
                    winit::event::MouseScrollDelta::PixelDelta(pos) => {
                        (pixels_to_notch_units(pos.x), pixels_to_notch_units(pos.y))
                    }
                };
                ShellEvent::Scroll { dx, dy }
            }

            WindowEvent::ModifiersChanged(new_modifiers) => {
                // Keep the winit ModifiersState in pw.modifiers for any code that
                // still reads it (about_to_wait GetCursorPos block etc.).
                pw.modifiers = new_modifiers.state();
                ShellEvent::ModifiersChanged(NormalizedModifiers {
                    ctrl:  new_modifiers.state().control_key(),
                    shift: new_modifiers.state().shift_key(),
                    alt:   new_modifiers.state().alt_key(),
                })
            }

            WindowEvent::KeyboardInput { event: kev, .. } => {
                if kev.state != ElementState::Pressed {
                    return;
                }
                use winit::keyboard::{Key, KeyCode, NamedKey, PhysicalKey as WinitPhysical};
                let logical = match &kev.logical_key {
                    Key::Named(NamedKey::Escape)    => LogicalKey::Escape,
                    Key::Named(NamedKey::Backspace)  => LogicalKey::Backspace,
                    Key::Named(NamedKey::Enter)      => LogicalKey::Enter,
                    Key::Named(NamedKey::Space)      => LogicalKey::Space,
                    Key::Named(NamedKey::Tab)        => LogicalKey::Tab,
                    Key::Named(NamedKey::Delete)     => LogicalKey::Delete,
                    Key::Named(NamedKey::ArrowLeft)  => LogicalKey::ArrowLeft,
                    Key::Named(NamedKey::ArrowRight) => LogicalKey::ArrowRight,
                    Key::Named(NamedKey::ArrowUp)    => LogicalKey::ArrowUp,
                    Key::Named(NamedKey::ArrowDown)  => LogicalKey::ArrowDown,
                    Key::Named(NamedKey::Home)       => LogicalKey::Home,
                    Key::Named(NamedKey::End)        => LogicalKey::End,
                    Key::Named(NamedKey::PageUp)     => LogicalKey::PageUp,
                    Key::Named(NamedKey::PageDown)   => LogicalKey::PageDown,
                    Key::Character(text)             => LogicalKey::Character(text.to_string()),
                    _                                => LogicalKey::Other,
                };
                let physical = match kev.physical_key {
                    WinitPhysical::Code(KeyCode::KeyA) => PhysicalKey::KeyA,
                    WinitPhysical::Code(KeyCode::KeyB) => PhysicalKey::KeyB,
                    WinitPhysical::Code(KeyCode::KeyC) => PhysicalKey::KeyC,
                    WinitPhysical::Code(KeyCode::KeyS) => PhysicalKey::KeyS,
                    WinitPhysical::Code(KeyCode::KeyV) => PhysicalKey::KeyV,
                    WinitPhysical::Code(KeyCode::KeyX) => PhysicalKey::KeyX,
                    WinitPhysical::Code(KeyCode::KeyY) => PhysicalKey::KeyY,
                    WinitPhysical::Code(KeyCode::KeyZ) => PhysicalKey::KeyZ,
                    _                                  => PhysicalKey::Other,
                };
                ShellEvent::KeyPressed { logical, physical }
            }

            WindowEvent::Ime(winit::event::Ime::Commit(text)) => {
                ShellEvent::ImeCommit(text)
            }

            _ => return,
        };

        // ── Call process_event via VelloPlatform ──────────────────────────
        // NOTE: dispatch reads `chrome_st.tabs.get(idx)` to translate
        // `ChromeHit::Tab(idx)` into a `chart.load_preset(&tab_id)` call.
        // Tabs are synced from `chart.panel_app.open_tabs` in
        // `render/scene_builder.rs:93` directly into `pw.chrome_state.tabs`
        // (the legacy field). The carve-out commit (f5517a58) introduced a
        // parallel `pw.shell_chrome` and pointed dispatch at it without
        // updating scene_builder to write into the new field — so
        // `shell_chrome.tabs` stayed empty forever, every tab click became
        // a no-op (and same for `+`, `x`, context menu — they all index
        // into the same tabs Vec or share the same hovered/context_menu
        // state). Point dispatch at `pw.chrome_state`, the field render
        // and update_tooltip already use. `pw.shell_chrome` is now a dead
        // duplicate; left in place to keep this fix minimal.
        let platform = VelloPlatform { window: &pw.window };
        mlc_app_input_shell::process_event(
            &mut pw.shell_input,
            &mut pw.chrome_state,
            &mut pw.shell_dirty,
            &mut pw.chart,
            &platform,
            shell_event,
            &mut pw.toolbar_tooltip,
            &mut pw.screenshot_pending,
            &mut pw.dispatch_out,
        );

        // ── Sync DispatchOut → PerWindowState fields ──────────────────────
        // DispatchOut flags are consumed here; reset after reading.
        if pw.dispatch_out.close_requested {
            pw.close_requested = true;
            pw.dispatch_out.close_requested = false;
        }
        if pw.dispatch_out.close_window_requested {
            pw.close_window_requested = true;
            pw.dispatch_out.close_window_requested = false;
        }
        if pw.dispatch_out.delete_window_requested {
            pw.delete_window_requested = true;
            pw.dispatch_out.delete_window_requested = false;
        }
        if pw.dispatch_out.spawn_new_window {
            pw.spawn_new_window = true;
            pw.dispatch_out.spawn_new_window = false;
        }
        if pw.dispatch_out.restart_requested {
            pw.dispatch_out.restart_requested = false;
            // The one place this application restarts itself, and only ever
            // because someone pressed the control that said "Restart". The
            // updater installs in the background and then waits — see
            // `ChromeUpdate` — so nothing here is a surprise to the user.
            pw.restart_requested = true;
        }

        // ── Sync shell_dirty → PerWindowState dirty flags ─────────────────
        // Additive: dirty flags from shell_dirty propagate to pw, never clear.
        if pw.shell_dirty.chart_dirty         { pw.chart_dirty         = true; pw.shell_dirty.chart_dirty         = false; }
        if pw.shell_dirty.toolbar_dirty       { pw.toolbar_dirty       = true; pw.shell_dirty.toolbar_dirty       = false; }
        if pw.shell_dirty.sidebar_dirty_scene { pw.sidebar_dirty_scene = true; pw.shell_dirty.sidebar_dirty_scene = false; }
        if pw.shell_dirty.overlay_dirty       { pw.overlay_dirty       = true; pw.shell_dirty.overlay_dirty       = false; }

        // ── Sync shell_input → PerWindowState fields ──────────────────────
        pw.last_mouse_pos         = pw.shell_input.last_mouse_pos;
        pw.mouse_pressed          = pw.shell_input.mouse_pressed;
        pw.last_drag_pos          = pw.shell_input.last_drag_pos;
        pw.drag_start_pos         = pw.shell_input.drag_start_pos;
        pw.last_click             = pw.shell_input.last_click;
        pw.drawing_capture        = pw.shell_input.drawing_capture;
        pw.last_sidebar_hover_row = pw.shell_input.last_sidebar_hover_row;
        pw.was_minimized          = pw.shell_input.was_minimized;
        pw.skeleton               = pw.shell_input.skeleton;
        // NO `shell_chrome -> chrome_state` mirror here. `process_event` now
        // writes directly into `pw.chrome_state` (see fix above), so copying
        // the stale `pw.shell_chrome.*` over the fresh `pw.chrome_state.*`
        // would clobber the dispatch's just-set hover / context-menu / tooltip
        // — that's why every chrome hover stopped repainting the moment tabs
        // were re-pointed. The legacy `pw.shell_chrome` field is left in
        // place but is now write-once-by-Default; dispatch reads / writes
        // `chrome_state` end-to-end.
    }
}
