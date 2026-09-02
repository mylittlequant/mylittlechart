//! Toolbar clock hover animation — a direct demo of `uzor-text`'s ASCII
//! cell-shader engine (`uzor_text::ascii`) wired onto one concrete element
//! (owner spec 2026-09-02: "используй асции движок для конкретного места",
//! explicitly rejecting a hand-rolled shimmer lookalike). Not hovered: the
//! clock still renders through the plain single `fill_text` call in
//! `toolbar_core.rs`'s `ToolbarItem::Clock` arm — this module only runs
//! while the pointer is actually over the clock and a pointer position is
//! known.
//!
//! `AsciiGrid` is rebuilt fresh every hovered frame — `draw_clock_glitch` is
//! the only entry point here, nothing stores a grid across frames. The
//! shader (`ClockGlitchShader::main`) is a pure function of
//! `(coord, ctx, cursor)`, so there is nothing that needs to persist between
//! frames (matches `uzor-text::ascii`'s own `ParagraphAsciiShader`
//! convention: a caller-stepped, caller-owned grid, no hidden retained
//! state).

use mlc_core::state::toolbar_geometry::WidgetRect;
use mlc_core::theme::ThemeManager;
use uzor::render::RenderContext;
use uzor_text::ascii::{sd_circle, AsciiGrid, Cell, CellShader, Coord, Cursor, GridContext};

/// Candidate glyphs a glitching clock cell may swap to — digits/colon (the
/// clock's own native alphabet) plus a handful of terminal-glitch symbols,
/// narrowed from `GlitchLetter`'s own `glitch_chars` set
/// (`uzor-text/src/ascii/cell_shader.rs`) to what reads as a clock digit
/// glitching rather than a text letter glitching.
const GLITCH_CHARS: &[char] =
    &['0', '1', '2', '3', '4', '5', '6', '7', '8', '9', ':', '#', '%', '&', '*', '/'];

/// Brush radius in grid cells — "a small brush, a couple of cells' radius"
/// per owner spec.
const BRUSH_RADIUS: f64 = 2.2;

/// How long one glitch frame holds, in milliseconds.
///
/// The swap used to be re-rolled on every rendered frame, which at 60Hz made
/// the glyphs boil rather than glitch and forced a full repaint every single
/// frame for as long as the pointer rested on the clock — the owner saw it
/// as "работает медленно" (2026-09-02). Quantising to a tick is what the
/// reference effect does too, and it buys both things at once: the animation
/// reads as discrete glitch steps, and every frame inside one tick paints
/// identical output.
const TICK_MS: f64 = 72.0;

/// Per-cell program: shows the clock's real character at rest. Inside the
/// pointer's brush radius it rolls a glitch swap whose probability rises
/// with closeness to the pointer AND breathes over real elapsed time
/// (`ctx.time`), so the effect reads as alive rather than a single
/// fixed-probability coin flip repeated every frame.
struct ClockGlitchShader<'a> {
    chars: &'a [char],
    color: [u8; 3],
    /// Which 72ms step we are in. Everything random about a cell is derived
    /// from this plus the cell's own index, so the shader is a pure function
    /// and two frames inside one tick are byte-identical.
    tick: u64,
}

/// Deterministic per-(cell, tick) roll — splitmix64's finaliser over the two
/// mixed together.
///
/// Deliberately not `fastrand`: an RNG would give a different answer on every
/// call, so a cell would flicker between frames of the SAME tick and the
/// quantisation above would buy nothing. A hash makes "which glyph is this
/// cell showing right now" a property of the tick, not of how many times we
/// happened to repaint during it.
fn cell_roll(index: usize, tick: u64) -> u64 {
    let mut z = (tick << 8) ^ (index as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15);
    z = z.wrapping_add(0x9E37_79B9_7F4A_7C15);
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

impl CellShader for ClockGlitchShader<'_> {
    fn main(&self, coord: Coord, ctx: &GridContext, cursor: &Cursor) -> Cell {
        let ch = self.chars.get(coord.x).copied().unwrap_or(' ');

        // Separators are the clock's skeleton, not its content: the brackets,
        // the colons and the spaces of `[UTC+0] 8:37:26 PM` are what makes it
        // readable as a clock at all. Glitching them turned the whole string
        // into noise (owner 2026-09-02). Only alphanumerics move — the same
        // line the reference effect draws.
        if !ch.is_ascii_alphanumeric() {
            return Cell { ch, color: self.color, alpha: 1.0, scale: 1.0 };
        }

        // Distance from this cell's center to the pointer, in grid cells —
        // `sd_circle` (the engine's own SDF helper) gives the signed
        // distance to the brush's edge; negative = inside the brush.
        let dx = coord.x as f64 + 0.5 - cursor.x;
        let dy = 0.5 - cursor.y;
        let sd = sd_circle(dx, dy, BRUSH_RADIUS);
        let brush = (-sd / BRUSH_RADIUS).clamp(0.0, 1.0);
        let intensity = (cursor.intensity * brush).clamp(0.0, 1.0);

        if intensity <= 0.001 {
            return Cell { ch, color: self.color, alpha: 1.0, scale: 1.0 };
        }

        // Real-time shimmer: a per-cell phase-shifted pulse riding on top of
        // the distance falloff, so the glitch visibly breathes instead of
        // sitting at one fixed swap probability every frame.
        let pulse = 0.8 + 0.2 * (ctx.time * 6.0 + coord.x as f64 * 0.9).sin();
        let swap_p = (0.65 * intensity * pulse).clamp(0.0, 1.0);
        let roll = cell_roll(coord.x, self.tick);
        // Top 32 bits decide whether this cell swaps at all this tick, the
        // low bits pick which glyph — one hash, two independent-enough draws.
        let swap = (roll >> 32) as f64 / u32::MAX as f64;
        let out_ch = if swap < swap_p {
            GLITCH_CHARS[(roll as usize) % GLITCH_CHARS.len()]
        } else {
            ch
        };

        Cell { ch: out_ch, color: self.color, alpha: 1.0, scale: 1.0 }
    }
}

/// Draw `time` through the ASCII cell-shader engine, glitching whichever
/// characters fall under the pointer's brush.
///
/// * `pointer` — window coordinates, the same space as `item_rect`.
/// * `color_hex` — the clock's own `theme.item_text`; no palette invented
///   here, the hovered glyphs paint in the exact same colour as the plain
///   path.
/// * `anim_time_s` — a real, monotonically increasing elapsed-time value
///   threaded in by the caller. This painter never calls
///   `SystemTime::now()` itself (see this module's own doc comment and the
///   owner's brief).
pub(super) fn draw_clock_glitch(
    ctx: &mut dyn RenderContext,
    time: &str,
    item_rect: WidgetRect,
    color_hex: &str,
    pointer: (f64, f64),
    anim_time_s: f64,
) {
    let chars: Vec<char> = time.chars().collect();
    let cols = chars.len();
    if cols == 0 {
        return;
    }

    ctx.set_font("13px monospace");
    let total_w = ctx.measure_text(time);
    let cell_w = (total_w / cols as f64).max(1.0);
    let cell_h = 13.0_f64;

    // Same anchor the plain path draws at: right-aligned, 8px inset from
    // the item's right edge, vertically centered — the animated text must
    // land exactly where the plain text was.
    let ox = item_rect.right() - 8.0 - total_w;
    let oy = item_rect.center_y() - cell_h / 2.0;

    let color = ThemeManager::parse_hex_color(color_hex)
        .map(|(r, g, b)| [r, g, b])
        .unwrap_or_else(|| Cell::default().color);
    // Quantised so the roll below is stable for 72ms at a time; `ctx.time`
    // inside the shader still gets the un-quantised value for its pulse,
    // which only shifts probability rather than picking glyphs.
    let tick = (anim_time_s * 1000.0 / TICK_MS).floor().max(0.0) as u64;
    let shader = ClockGlitchShader { chars: &chars, color, tick };

    let mut grid = AsciiGrid::new(cols, 1);
    let gx = (pointer.0 - ox) / cell_w;
    let gy = (pointer.1 - oy) / cell_h;
    grid.set_cursor(Cursor { x: gx, y: gy, pressed: false, inside: true, intensity: 1.0 });
    grid.step(&shader, anim_time_s, cell_w / cell_h);
    grid.render(ctx, ox, oy, cell_w, cell_h);
}
