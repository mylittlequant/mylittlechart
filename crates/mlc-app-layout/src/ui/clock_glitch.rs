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
const GLITCH_CHARS: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    // Half-width katakana — the half of the alphabet that actually reads as
    // "matrix" rather than "a digit went wrong". The first pass shipped 16
    // glyphs that were almost all digits, and the owner's verdict was
    // immediate (2026-09-02: "там слишком мало разных символов"). A swap
    // into another digit is nearly invisible in a clock; a swap into a
    // katakana is the effect.
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ',
    'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ', 'ﾀ', 'ﾁ', 'ﾂ', 'ﾃ', 'ﾄ',
    'ﾅ', 'ﾆ', 'ﾇ', 'ﾈ', 'ﾉ', 'ﾊ', 'ﾋ', 'ﾌ', 'ﾍ', 'ﾎ',
    // A few terminal-glitch symbols, from the engine's own `GlitchLetter`
    // set (`uzor-text/src/ascii/cell_shader.rs`).
    '#', '@', '%', '&', '$', '*', '/', '|', '=', '<', '>', '?', '+',
];

/// Which shader the clock runs. Both are real `CellShader`s stepped by the
/// same `AsciiGrid` — the switch exists because the owner asked to see the
/// engine and the gate4agent shimmer side by side (2026-09-02).
///
/// The comparison's own answer, worth writing down: once the shimmer's rules
/// are expressed as a `CellShader` they cost nothing extra and produce the
/// same picture. The engine was never the limiting factor — the first pass
/// looked weaker than the reference because of ITS parameters, a thin glyph
/// set and a 2.2-cell brush, not because of what it was stepped by.
#[derive(Clone, Copy, PartialEq)]
// `SHADER_KIND` is a const, so whichever arm is not selected reads as
// unconstructed. That is the point of a compile-time switch — both arms
// stay compiled and comparable rather than one rotting behind a flag.
#[allow(dead_code)]
pub(super) enum ClockShaderKind {
    /// Distance-falloff brush, wide glyph set, probability breathing over
    /// real time. Uses the engine's own `sd_circle` for the brush.
    Engine,
    /// gate4agent's `shimmer.rs` rules, transcribed: a hard-edged circular
    /// brush, no falloff and no pulse — inside the radius a cell always
    /// rolls, outside it never does.
    Shimmer,
}

/// The active one. Flip to compare; `Engine` is the default.
const SHADER_KIND: ClockShaderKind = ClockShaderKind::Engine;

/// Rendered frames per animation step.
///
/// The step still rides the render cadence rather than a clock of its own,
/// but one step per FRAME is 60 steps a second and reads as noise rather
/// than as glitching (owner, 2026-09-02: "слишком быструю", then "скорость
/// в 2 раза меньше сделай" once six frames was still quick). Twelve frames
/// is about five steps a second at 60fps — slow enough that the eye lands
/// on each state instead of averaging them into a blur. A divider keeps the
/// property that matters: an FPS-limited app slows the animation with
/// everything else instead of the two rates drifting apart.
const FRAMES_PER_STEP: u64 = 12;

/// Brush radius in grid cells for [`ClockShaderKind::Engine`].
///
/// Roughly a mouse cursor's own footprint, which is what the effect is
/// supposed to be tracking. 2.2 was too tight to notice and 9.0 was so wide
/// it covered the whole string at once — at which point nothing reads as
/// following the pointer, it just looks like the entire clock repainting
/// (owner, 2026-09-02: "оно не понимает примерные размеры мышки, потому там
/// сразу всё начинает перерисовываться"). Three cells is about twenty
/// pixels at this font size: a local smudge that moves with the cursor.
const BRUSH_RADIUS: f64 = 3.0;

/// Radius around the pointer that swaps with certainty, in grid cells.
///
/// Slightly over one cell, so whichever character the cursor is actually on
/// — and its immediate neighbour when the cursor sits on a boundary — never
/// shows itself while the mouse is there.
const BRUSH_CORE: f64 = 1.3;

/// gate4agent uses 2.25 CELLS in a terminal grid, where a cell is a whole
/// character box — kept verbatim for the comparison arm so the difference
/// being looked at is the rules, not a re-tuned radius.
const SHIMMER_BRUSH_RADIUS: f64 = 2.25;

/// The animation advances one step per RENDERED FRAME, so it runs at the
/// app's own FPS cadence.
///
/// It used to quantise wall-clock milliseconds into 72ms steps, borrowed
/// from the terminal reference. Wrong frame of reference here: a windowed
/// app already has a cadence, and pinning the effect to a second clock made
/// it advance at a rate unrelated to what the renderer was doing — the owner
/// put it exactly ("тик должен быть привязан к каденсу фпс"). One step per
/// frame also means the FPS limit is what throttles it, rather than two
/// independent rates beating against each other.

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
    // `ctx` carries the engine's frame/time/grid context; this shader is
    // driven entirely by the frame-derived tick on `self` and the cursor,
    // so it reads nothing from it.
    fn main(&self, coord: Coord, _ctx: &GridContext, cursor: &Cursor) -> Cell {
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
        // A CORE of full intensity around the pointer, then a falloff to the
        // brush edge. Falling off straight from the centre meant a cell half
        // a cell away from the cursor already had a real chance of showing
        // itself — and since the pointer is almost never dead-centre in a
        // cell, "the character under the mouse" was exactly the case that
        // kept flickering back to the original. Inside the core the swap is
        // certain.
        let dist = BRUSH_RADIUS + sd;
        let brush = if dist <= BRUSH_CORE {
            1.0
        } else {
            ((BRUSH_RADIUS - dist) / (BRUSH_RADIUS - BRUSH_CORE)).clamp(0.0, 1.0)
        };
        let intensity = (cursor.intensity * brush).clamp(0.0, 1.0);

        if intensity <= 0.001 {
            return Cell { ch, color: self.color, alpha: 1.0, scale: 1.0 };
        }

        // Whether a cell participates falls off with distance — that is what
        // gives the brush a soft edge. At the pointer itself `intensity` is
        // 1.0, so the cell under the mouse ALWAYS swaps: a character that
        // keeps reappearing under the cursor is the one thing the effect
        // must never do (owner, 2026-09-02: "там постоянно проглядывают
        // оригинальные символы пока на них мышка — что супер тупо, когда
        // мышка, должны быть какие угодно кроме оригинальных"). No pulse in
        // this decision either; it used to hold the centre at 0.8 and let
        // the original through a fifth of the time.
        let roll = cell_roll(coord.x, self.tick);
        // Top 32 bits decide participation, the low bits pick the glyph —
        // one hash, two independent-enough draws.
        let swap = (roll >> 32) as f64 / u32::MAX as f64;
        if swap >= intensity {
            return Cell { ch, color: self.color, alpha: 1.0, scale: 1.0 };
        }
        // ...and when it does swap, never back to itself. Picking from the
        // whole set and hoping is how a digit lands on the same digit and
        // the glitch silently skips a beat.
        let out_ch = pick_other(ch, roll);

        Cell { ch: out_ch, color: self.color, alpha: 1.0, scale: 1.0 }
    }
}

/// Pick a glyph from `GLITCH_CHARS` that is NOT `ch`.
///
/// Walks forward from the hashed index rather than re-rolling, so the answer
/// stays a pure function of (cell, tick) — a retry loop on a fresh roll
/// would reintroduce the per-frame instability the hash exists to remove.
fn pick_other(ch: char, roll: u64) -> char {
    let n = GLITCH_CHARS.len();
    let start = (roll as usize) % n;
    for k in 0..n {
        let c = GLITCH_CHARS[(start + k) % n];
        if c != ch {
            return c;
        }
    }
    ch
}

/// gate4agent's own 30-glyph set, verbatim from
/// `gate4agent-tui/src/shimmer.rs` — ten digits and twenty half-width
/// katakana, nothing else.
const SHIMMER_GLYPHS: &[char] = &[
    '0', '1', '2', '3', '4', '5', '6', '7', '8', '9',
    'ｱ', 'ｲ', 'ｳ', 'ｴ', 'ｵ', 'ｶ', 'ｷ', 'ｸ', 'ｹ', 'ｺ',
    'ｻ', 'ｼ', 'ｽ', 'ｾ', 'ｿ', 'ﾀ', 'ﾁ', 'ﾂ', 'ﾃ', 'ﾄ',
];

/// gate4agent's shimmer rules, transcribed into a `CellShader` so the two
/// approaches can be compared under the same `AsciiGrid`.
///
/// The differences from [`ClockGlitchShader`] are the whole point of the
/// comparison: a HARD-edged brush (in or out, no falloff), no time pulse
/// (the tick alone drives change), and every cell inside the radius swaps
/// every tick rather than rolling a probability. That is why it reads as a
/// denser, blockier effect than the engine arm's — it is not doing less
/// work, it is doing different work.
struct ShimmerShader<'a> {
    chars: &'a [char],
    color: [u8; 3],
    tick: u64,
}

impl CellShader for ShimmerShader<'_> {
    fn main(&self, coord: Coord, _ctx: &GridContext, cursor: &Cursor) -> Cell {
        let ch = self.chars.get(coord.x).copied().unwrap_or(' ');
        if !ch.is_ascii_alphanumeric() {
            return Cell { ch, color: self.color, alpha: 1.0, scale: 1.0 };
        }
        let dx = coord.x as f64 + 0.5 - cursor.x;
        let dy = 0.5 - cursor.y;
        if dx * dx + dy * dy > SHIMMER_BRUSH_RADIUS * SHIMMER_BRUSH_RADIUS {
            return Cell { ch, color: self.color, alpha: 1.0, scale: 1.0 };
        }
        let roll = cell_roll(coord.x, self.tick);
        let out = SHIMMER_GLYPHS[(roll as usize) % SHIMMER_GLYPHS.len()];
        Cell { ch: out, color: self.color, alpha: 1.0, scale: 1.0 }
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
    anim_frame: u64,
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
    // One step per rendered frame — see the note on `anim_frame`. `ctx.time`
    // inside the shader still gets real elapsed seconds for its pulse, which
    // only shifts probability rather than picking glyphs.
    let tick = anim_frame / FRAMES_PER_STEP;
    let mut grid = AsciiGrid::new(cols, 1);
    let gx = (pointer.0 - ox) / cell_w;
    let gy = (pointer.1 - oy) / cell_h;
    grid.set_cursor(Cursor { x: gx, y: gy, pressed: false, inside: true, intensity: 1.0 });

    // One grid, one step, one render — only the per-cell program differs.
    // That IS the comparison's finding: the engine hosts either rule set at
    // identical cost, so what separated the first pass from the reference
    // was its glyph set and brush size, not the machinery underneath.
    let aspect = cell_w / cell_h;
    match SHADER_KIND {
        ClockShaderKind::Engine => {
            grid.step(&ClockGlitchShader { chars: &chars, color, tick }, anim_time_s, aspect);
        }
        ClockShaderKind::Shimmer => {
            grid.step(&ShimmerShader { chars: &chars, color, tick }, anim_time_s, aspect);
        }
    }
    grid.render(ctx, ox, oy, cell_w, cell_h);
}
