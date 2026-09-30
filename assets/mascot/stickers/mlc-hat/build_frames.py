#!/usr/bin/env python3
"""Bake the six `mlc-hat` sticker animations into static per-frame SVGs, in
the SVG dialect uzor's hand-written `draw_svg_multicolor` parser actually
renders (`uzor/uzor/src/core/render/svg.rs`) — generalised from
`assets/mascot/spinner/build_frames.py`, which solved the ONE-animation
version of this problem for the loading-spinner turntable. Read that
script's own doc comment first for the parser's dialect limits; the short
version, re-derived directly from `draw_svg_multicolor` itself:

  - `<path>` with `fill`/`stroke`/`stroke-width`/`stroke-linecap`/
    `stroke-linejoin`/`opacity` — commands `M L H V C S Q T A Z` all work,
    but ONLY because the renderer walks the string itself; a `<g transform>`
    does NOT compose per element — `parse_g_transform` reads the FIRST
    `<g transform="translate()/scale()">` in the WHOLE document (no
    `rotate()`/`matrix()`, no nesting awareness) and applies it to
    EVERY path/gradient/circle uniformly. CSS `<style>`/`@keyframes`,
    `<use>`, and per-element `transform` attributes are never read at all.
  - `<linearGradient id=... x1 y1 x2 y2 gradientUnits="userSpaceOnUse">` is
    resolved by a plain textual `id="..."` search anywhere in the document
    and its `x1/y1/x2/y2` are read as raw numbers in the SAME coordinate
    space as the referencing path's `d` — i.e. gradients must live in
    exactly the same local space as the geometry they paint, or move with
    it if that geometry animates. `radialGradient` and `stop-opacity` are
    never read. Percentage/`objectBoundingBox` gradients still get read as
    literal 0..1 user-space numbers, which — for a 2-stop gradient — reads
    almost indistinguishably from a flat fill in the FIRST stop's colour
    (the "first stop wins" rule the spinner script's own doc documents).
  - No `<ellipse>` support anywhere in the renderer, and `<circle>` is
    ALWAYS painted solid black in `draw_svg_multicolor` (it never reads the
    circle's own `fill`) — every ellipse/circle in these six sources is
    rewritten here as an equivalent `<path>` (the same 4-Bezier kappa
    approximation `uzor::render::svg::draw_circle_bezier` already uses),
    which keeps its real `fill` colour.

Since none of that composes across a document, EVERY frame this script
writes is fully baked, self-contained absolute-coordinate geometry: no
`<g>` of any kind, one `<path>`/frame per originally-animated shape, with
`d` and any gradient it references transformed by the SAME matrix (a
plain 2D affine, composed from every CSS `transform` in effect at that
instant plus any static `translate()/scale()` wrapper the source SVG used
to place the hat).

Six animations, sampled at FRAME_COUNT=16 evenly-spaced instants per loop
(matching the two cel-based sources — `y_spin`, `turntable_quality` — which
already ARE 16 discrete drawn views; the four CSS-continuous sources get
resampled onto the same count for one consistent contract across the pack):

  bounce     `src/magic_hat_bounce.svg`           2400ms loop, 150ms/frame
  spin       `src/magic_hat_spin.svg`             2400ms loop, 150ms/frame
  wave       `src/magic_hat_tip_wave.svg`         2400ms loop, 150ms/frame
  twinkle    `src/magic_hat_twinkle.svg`          2800ms loop, 175ms/frame
  flip       `src/magic_hat_y_spin.svg`           3200ms loop, 200ms/frame
  turntable  `src/magic_hat_turntable_quality.svg` 2400ms loop, 150ms/frame

`bounce`/`spin`/`wave`/`twinkle` are CSS-keyframe-driven: this script parses
their `<style>` `@keyframes` blocks itself (percentage stops of `transform`/
`opacity`/`d: path(...)`), evaluates the declared easing (`cubic-bezier`,
`ease-out`, `ease-in-out`, `linear`) at each of the 16 sample instants, and
bakes the resulting per-instant 2D transform directly into every affected
path's (and its gradient's) coordinates. `flip`/`turntable` are already
16 discrete drawn views (`opacity` toggled by `steps(1,end)` keyframes, no
continuous interpolation needed) — this script just flattens whichever
view is visible at each of the 16 steps, identically in spirit to how
`assets/mascot/spinner/build_frames.py` already flattens `turntable`'s
twin source (`magic_hat_turntable_quality.svg` is byte-identical to that
script's own `boot-hat-source.svg`).

Run: `python3 build_frames.py` from any cwd — paths are relative to this
script's own directory. Regenerates every `<id>/frame_NN.svg` and
`pack.json` in-place; re-run after any edit under `src/`.
"""
from __future__ import annotations

import json
import math
import re
import xml.etree.ElementTree as ET
from pathlib import Path

SVG_NS = "http://www.w3.org/2000/svg"
ET.register_namespace("", SVG_NS)

SCRIPT_DIR = Path(__file__).resolve().parent
SRC_DIR = SCRIPT_DIR / "src"
FRAME_COUNT = 16
KAPPA = 0.5522847498  # 4/3 * (sqrt(2) - 1) — same constant as draw_circle_bezier.

# =============================================================================
# 2D affine matrices — 6-tuple (a, b, c, d, e, f): x' = a*x + c*y + e;
# y' = b*x + d*y + f (same convention `uzor-vectorize`/the spinner script use).
# =============================================================================

Matrix = tuple  # (float, float, float, float, float, float)
IDENTITY: Matrix = (1.0, 0.0, 0.0, 1.0, 0.0, 0.0)


def mat_translate(dx: float, dy: float) -> Matrix:
    return (1.0, 0.0, 0.0, 1.0, dx, dy)


def mat_scale(sx: float, sy: float) -> Matrix:
    return (sx, 0.0, 0.0, sy, 0.0, 0.0)


def mat_rotate(deg: float) -> Matrix:
    r = math.radians(deg)
    c, s = math.cos(r), math.sin(r)
    return (c, s, -s, c, 0.0, 0.0)


def mat_skew_x(deg: float) -> Matrix:
    return (1.0, 0.0, math.tan(math.radians(deg)), 1.0, 0.0, 0.0)


def compose(m1: Matrix, m2: Matrix) -> Matrix:
    """`m1` after `m2`: `result(p) = m1(m2(p))`."""
    a1, b1, c1, d1, e1, f1 = m1
    a2, b2, c2, d2, e2, f2 = m2
    return (
        a1 * a2 + c1 * b2, b1 * a2 + d1 * b2,
        a1 * c2 + c1 * d2, b1 * c2 + d1 * d2,
        a1 * e2 + c1 * f2 + e1, b1 * e2 + d1 * f2 + f1,
    )


def apply_point(m: Matrix, x: float, y: float) -> tuple[float, float]:
    a, b, c, d, e, f = m
    return a * x + c * y + e, b * x + d * y + f


def around_origin(m: Matrix, ox: float, oy: float) -> Matrix:
    """`m` applied around `(ox, oy)` instead of the coordinate origin — the
    standard CSS `transform-origin` construction: `translate(o) . m .
    translate(-o)`."""
    return compose(compose(mat_translate(ox, oy), m), mat_translate(-ox, -oy))


def matrix_scale_factor(m: Matrix) -> float:
    """Uniform-scale-equivalent of `m`'s linear part — `sqrt(|det|)`, the
    same area-factor approximation the spinner script uses to scale a
    baked path's `stroke-width` (uzor's stroke model has no anisotropic
    width, so a non-uniform scale/skew only gets this approximation)."""
    a, b, c, d, _e, _f = m
    return math.sqrt(abs(a * d - b * c))


def scale_stroke_width(width: float, m: Matrix) -> float:
    return width * matrix_scale_factor(m)


# =============================================================================
# SVG path-data tokenizing, baking, bbox, and keyframe `d` morph interpolation
# — every source in this pack uses only M/L/C/V/Q/Z (verified with `grep -oP
# '(?<= d=")[^"]+' src/*.svg | grep -oP '[A-Za-z]' | sort -u`; only
# `magic_hat_turntable_quality.svg` uses Q, in its flap-fold detail paths).
# =============================================================================

_PATH_TOKEN_RE = re.compile(r"[MLCVQZ]|-?\d*\.?\d+(?:[eE][-+]?\d+)?")


def tokenize_path(d: str) -> list[str]:
    return _PATH_TOKEN_RE.findall(d)


def transform_path_d(d: str, m: Matrix) -> str:
    """Bake affine matrix `m` into absolute M/L/C/Z path data (a baked `V`
    degenerates cleanly into `L` — once transformed by an arbitrary matrix
    a previously-vertical run is not necessarily vertical any more)."""
    tokens = tokenize_path(d)
    out: list[str] = []
    i = 0
    cx = cy = 0.0
    while i < len(tokens):
        cmd = tokens[i]
        if cmd == "M" or cmd == "L":
            x, y = float(tokens[i + 1]), float(tokens[i + 2])
            nx, ny = apply_point(m, x, y)
            out.append(f"{'M' if cmd == 'M' else 'L'} {nx:.3f} {ny:.3f}")
            cx, cy = x, y
            i += 3
        elif cmd == "V":
            y = float(tokens[i + 1])
            nx, ny = apply_point(m, cx, y)
            out.append(f"L {nx:.3f} {ny:.3f}")
            cy = y
            i += 2
        elif cmd == "C":
            nums = [float(v) for v in tokens[i + 1:i + 7]]
            pts = [apply_point(m, nums[j], nums[j + 1]) for j in (0, 2, 4)]
            out.append("C " + " ".join(f"{px:.3f} {py:.3f}" for px, py in pts))
            cx, cy = nums[4], nums[5]
            i += 7
        elif cmd == "Q":
            nums = [float(v) for v in tokens[i + 1:i + 5]]
            pts = [apply_point(m, nums[j], nums[j + 1]) for j in (0, 2)]
            out.append("Q " + " ".join(f"{px:.3f} {py:.3f}" for px, py in pts))
            cx, cy = nums[2], nums[3]
            i += 5
        elif cmd == "Z":
            out.append("Z")
            i += 1
        else:
            raise ValueError(f"unsupported path command {cmd!r} in {d!r}")
    return " ".join(out)


def path_bbox(d: str) -> tuple[float, float, float, float]:
    """Bounding box of a path's own control AND end points (a superset of
    the tight geometric bbox — conservative, fine for a transform-origin
    approximation on curvature this gentle)."""
    tokens = tokenize_path(d)
    xs: list[float] = []
    ys: list[float] = []
    i = 0
    cx = cy = 0.0
    while i < len(tokens):
        cmd = tokens[i]
        if cmd == "M" or cmd == "L":
            x, y = float(tokens[i + 1]), float(tokens[i + 2])
            xs.append(x)
            ys.append(y)
            cx, cy = x, y
            i += 3
        elif cmd == "V":
            y = float(tokens[i + 1])
            xs.append(cx)
            ys.append(y)
            cy = y
            i += 2
        elif cmd == "C":
            nums = [float(v) for v in tokens[i + 1:i + 7]]
            xs.extend([nums[0], nums[2], nums[4]])
            ys.extend([nums[1], nums[3], nums[5]])
            cx, cy = nums[4], nums[5]
            i += 7
        elif cmd == "Q":
            nums = [float(v) for v in tokens[i + 1:i + 5]]
            xs.extend([nums[0], nums[2]])
            ys.extend([nums[1], nums[3]])
            cx, cy = nums[2], nums[3]
            i += 5
        elif cmd == "Z":
            i += 1
        else:
            raise ValueError(f"unsupported path command {cmd!r} in {d!r}")
    return min(xs), min(ys), max(xs), max(ys)


def union_bbox(boxes: list[tuple[float, float, float, float]]) -> tuple[float, float, float, float]:
    return (
        min(b[0] for b in boxes), min(b[1] for b in boxes),
        max(b[2] for b in boxes), max(b[3] for b in boxes),
    )


def fill_box_origin(bbox: tuple[float, float, float, float], pct_x: float, pct_y: float) -> tuple[float, float]:
    x0, y0, x1, y1 = bbox
    return x0 + (x1 - x0) * pct_x, y0 + (y1 - y0) * pct_y


def lerp(a: float, b: float, t: float) -> float:
    return a + (b - a) * t


def interpolate_path_d(d0: str, d1: str, t: float) -> str:
    """Interpolate two same-structure path-data strings coordinate-by-
    coordinate — the ONLY way to bake a CSS `d: path(...)` keyframe morph
    (`magic-hat-cone-bend` in `wave`), since the parser has no notion of a
    "path" it could morph itself."""
    t0, t1 = tokenize_path(d0), tokenize_path(d1)
    if len(t0) != len(t1):
        raise ValueError("path morph keyframes have different token structure")
    out = []
    for a, b in zip(t0, t1):
        if a[:1].isalpha():
            if a != b:
                raise ValueError(f"path morph keyframes disagree on command: {a!r} vs {b!r}")
            out.append(a)
        else:
            out.append(f"{lerp(float(a), float(b), t):.6f}")
    return " ".join(out)


# =============================================================================
# CSS `transform` function lists, easing, and `@keyframes` parsing
# =============================================================================

_FUNC_RE = re.compile(r"(\w+)\(([^)]*)\)")
_NUM_RE = re.compile(r"-?\d*\.?\d+(?:[eE][-+]?\d+)?")


def parse_transform_list(value: str) -> list[tuple[str, list[float]]]:
    """`'translateY(3px) scale(1.02, 0.97) rotate(-0.6deg)'` ->
    `[('translateY', [3.0]), ('scale', [1.02, 0.97]), ('rotate', [-0.6])]`."""
    return [(name, [float(v) for v in _NUM_RE.findall(args)]) for name, args in _FUNC_RE.findall(value)]


def transform_list_to_matrix(funcs: list[tuple[str, list[float]]]) -> Matrix:
    """CSS composes left-to-right as nested function application — the
    FIRST listed function is applied LAST spatially: `M = F1 . F2 . ... .
    Fn`, built here from the right so each step matches that definition."""
    m = IDENTITY
    for name, args in reversed(funcs):
        if name == "translate":
            fm = mat_translate(args[0], args[1] if len(args) > 1 else 0.0)
        elif name == "translateX":
            fm = mat_translate(args[0], 0.0)
        elif name == "translateY":
            fm = mat_translate(0.0, args[0])
        elif name == "scale":
            fm = mat_scale(args[0], args[1] if len(args) > 1 else args[0])
        elif name == "rotate":
            fm = mat_rotate(args[0])
        elif name == "skewX":
            fm = mat_skew_x(args[0])
        else:
            raise ValueError(f"unsupported CSS transform function {name!r}")
        m = compose(fm, m)
    return m


def lerp_transform_lists(
    f0: list[tuple[str, list[float]]], f1: list[tuple[str, list[float]]], t: float,
) -> list[tuple[str, list[float]]]:
    if len(f0) != len(f1):
        raise ValueError("keyframe transform lists have different function counts")
    out = []
    for (n0, a0), (n1, a1) in zip(f0, f1):
        if n0 != n1 or len(a0) != len(a1):
            raise ValueError(f"keyframe transform function mismatch: {n0} vs {n1}")
        out.append((n0, [lerp(x, y, t) for x, y in zip(a0, a1)]))
    return out


def cubic_bezier(x1: float, y1: float, x2: float, y2: float):
    """Standard CSS `cubic-bezier(x1,y1,x2,y2)` easing: solve `x(t) = u` by
    bisection (30 iterations — comfortably past float precision for a
    0..1 input), then evaluate `y(t)`."""
    def bezier(t: float, p1: float, p2: float) -> float:
        mt = 1.0 - t
        return 3.0 * mt * mt * t * p1 + 3.0 * mt * t * t * p2 + t * t * t

    def solve(u: float) -> float:
        if u <= 0.0:
            return 0.0
        if u >= 1.0:
            return 1.0
        lo, hi = 0.0, 1.0
        for _ in range(30):
            mid = (lo + hi) / 2.0
            if bezier(mid, x1, x2) < u:
                lo = mid
            else:
                hi = mid
        return bezier((lo + hi) / 2.0, y1, y2)

    return solve


LINEAR = lambda u: u  # noqa: E731 — trivial, named for parse_easing's dispatch table.
EASE_OUT = cubic_bezier(0.0, 0.0, 0.58, 1.0)
EASE_IN_OUT = cubic_bezier(0.42, 0.0, 0.58, 1.0)


def parse_easing(spec: str):
    spec = spec.strip()
    if spec == "linear":
        return LINEAR
    if spec == "ease-out":
        return EASE_OUT
    if spec == "ease-in-out":
        return EASE_IN_OUT
    m = re.match(r"cubic-bezier\(([^)]*)\)", spec)
    if m:
        x1, y1, x2, y2 = (float(v) for v in m.group(1).split(","))
        return cubic_bezier(x1, y1, x2, y2)
    raise ValueError(f"unsupported easing {spec!r}")


def parse_keyframes(style_text: str) -> dict[str, list[tuple[float, dict[str, str]]]]:
    """`{'hat-bounce': [(0.0, {'transform': '...'}), (7.0, {...}), ...], ...}`
    — brace-matched (a keyframes block nests one `{...}` per percentage
    stop, so a flat regex over the whole block cannot find its own end)."""
    result: dict[str, list[tuple[float, dict[str, str]]]] = {}
    i = 0
    while True:
        m = re.search(r"@keyframes\s+([\w-]+)\s*\{", style_text[i:])
        if not m:
            break
        name = m.group(1)
        body_start = i + m.end()
        depth = 1
        j = body_start
        while depth > 0:
            if style_text[j] == "{":
                depth += 1
            elif style_text[j] == "}":
                depth -= 1
            j += 1
        body = style_text[body_start:j - 1]

        stops: list[tuple[float, dict[str, str]]] = []
        for stop_m in re.finditer(r"([^{}]+)\{([^{}]*)\}", body):
            selector, decls = stop_m.group(1), stop_m.group(2)
            props: dict[str, str] = {}
            for decl in decls.split(";"):
                decl = decl.strip()
                if not decl:
                    continue
                prop, _, val = decl.partition(":")
                props[prop.strip()] = val.strip()
            for token in selector.split(","):
                token = token.strip()
                if token == "from":
                    pct = 0.0
                elif token == "to":
                    pct = 100.0
                else:
                    pct = float(token.rstrip("%"))
                stops.append((pct, props))
        stops.sort(key=lambda s: s[0])
        result[name] = stops
        i = j
    return result


def parse_path_func(value: str) -> str:
    """`'path("M 1,2 Z")'` -> `'M 1,2 Z'` (the `d: path(...)` keyframe
    value CSS uses for morphing a path's own geometry)."""
    value = value.strip()
    if not (value.startswith('path("') and value.endswith('")')):
        raise ValueError(f"unsupported d keyframe value {value!r}")
    return value[len('path("'):-2]


class Track:
    """One CSS animation (a `@keyframes` block plus its own duration/delay/
    easing) sampled at an absolute elapsed time — the SAME loop-relative
    contract [`loading_hat.rs`]'s `hat_frame_index` documents for the
    baked frames this produces: only `elapsed_ms % duration_ms` (shifted by
    `delay_s`) ever matters."""

    def __init__(self, stops: list[tuple[float, dict[str, str]]], duration_ms: float, delay_s: float, easing):
        self.stops = stops
        self.duration_ms = duration_ms
        self.delay_s = delay_s
        self.easing = easing

    def _bounds(self, pct: float) -> tuple[dict[str, str], dict[str, str], float]:
        for k in range(len(self.stops) - 1):
            p0, props0 = self.stops[k]
            p1, props1 = self.stops[k + 1]
            if p0 <= pct <= p1:
                u = 0.0 if p1 == p0 else (pct - p0) / (p1 - p0)
                return props0, props1, u
        # Every keyframe block in this pack spans the full 0..100 range
        # (explicit 0%/100% stops), so this is unreachable in practice —
        # kept total (last stop, no interpolation) rather than panicking.
        return self.stops[-1][1], self.stops[-1][1], 0.0

    def sample(self, elapsed_ms: float) -> dict[str, object]:
        local_ms = elapsed_ms - self.delay_s * 1000.0
        pct = (local_ms / self.duration_ms % 1.0) * 100.0
        props0, props1, u = self._bounds(pct)
        eased = self.easing(u)
        out: dict[str, object] = {}
        if "opacity" in props0:
            out["opacity"] = lerp(float(props0["opacity"]), float(props1["opacity"]), eased)
        if "transform" in props0:
            f0 = parse_transform_list(props0["transform"])
            f1 = parse_transform_list(props1["transform"])
            out["transform"] = lerp_transform_lists(f0, f1, eased)
        if "d" in props0:
            out["d"] = interpolate_path_d(parse_path_func(props0["d"]), parse_path_func(props1["d"]), eased)
        return out


# =============================================================================
# XML helpers
# =============================================================================

def tag_local(elem: ET.Element) -> str:
    return elem.tag.rsplit("}", 1)[-1]


def by_id(root: ET.Element, elem_id: str) -> ET.Element:
    for e in root.iter():
        if e.get("id") == elem_id:
            return e
    raise KeyError(elem_id)


def by_aria_label(root: ET.Element, label: str) -> ET.Element:
    for e in root.iter():
        if e.get("aria-label") == label:
            return e
    raise KeyError(label)


def child_paths(elem: ET.Element) -> list[ET.Element]:
    """Every `<path>` in `elem`'s subtree, in document order."""
    return [e for e in elem.iter() if tag_local(e) == "path"]


def parse_static_transform(transform_attr: str | None) -> Matrix:
    """The only static-transform shape these sources use:
    `translate(tx ty) scale(s)` (order fixed, so `parse_transform_list` +
    `transform_list_to_matrix` handle it with no special-casing)."""
    if not transform_attr:
        return IDENTITY
    return transform_list_to_matrix(parse_transform_list(transform_attr))


def find_gradient(root: ET.Element, gid: str) -> ET.Element:
    for e in root.iter():
        if tag_local(e) == "linearGradient" and e.get("id") == gid:
            return e
    raise KeyError(gid)


def gradient_stops_svg(gradient: ET.Element) -> str:
    parts = []
    for stop in gradient:
        if tag_local(stop) != "stop":
            continue
        offset = stop.get("offset", "0")
        color = stop.get("stop-color", "#000000")
        parts.append(f'<stop offset="{offset}" stop-color="{color}"/>')
    return "".join(parts)


def baked_gradient_svg(gradient: ET.Element, m: Matrix) -> str:
    """Bake matrix `m` into a `userSpaceOnUse` gradient's endpoints — the
    SAME matrix baked into the path that references it, since
    `draw_svg_multicolor` reads gradient coordinates in that path's own
    `d` coordinate space (see this module's own top doc)."""
    x1, y1 = float(gradient.get("x1", "0")), float(gradient.get("y1", "0"))
    x2, y2 = float(gradient.get("x2", "0")), float(gradient.get("y2", "1"))
    nx1, ny1 = apply_point(m, x1, y1)
    nx2, ny2 = apply_point(m, x2, y2)
    gid = gradient.get("id")
    return (
        f'<linearGradient id="{gid}" gradientUnits="userSpaceOnUse" '
        f'x1="{nx1:.3f}" y1="{ny1:.3f}" x2="{nx2:.3f}" y2="{ny2:.3f}">'
        f"{gradient_stops_svg(gradient)}</linearGradient>"
    )


def first_stop_color(gradient: ET.Element) -> str:
    for stop in gradient:
        if tag_local(stop) == "stop":
            color = stop.get("stop-color")
            if color:
                return color
    raise ValueError(f"gradient {gradient.get('id')!r} has no stop-color")


# =============================================================================
# Frame assembly
# =============================================================================

def ellipse_to_path_d(cx: float, cy: float, rx: float, ry: float) -> str:
    """A circle/ellipse as 4 cubic Beziers — same kappa constant
    `uzor::render::svg::draw_circle_bezier` uses. Needed because
    `draw_svg_multicolor` supports neither `<ellipse>` nor a correctly-
    coloured `<circle>` (see this module's own top doc)."""
    kx, ky = KAPPA * rx, KAPPA * ry
    return (
        f"M {cx + rx:.3f} {cy:.3f} "
        f"C {cx + rx:.3f} {cy - ky:.3f} {cx + kx:.3f} {cy - ry:.3f} {cx:.3f} {cy - ry:.3f} "
        f"C {cx - kx:.3f} {cy - ry:.3f} {cx - rx:.3f} {cy - ky:.3f} {cx - rx:.3f} {cy:.3f} "
        f"C {cx - rx:.3f} {cy + ky:.3f} {cx - kx:.3f} {cy + ry:.3f} {cx:.3f} {cy + ry:.3f} "
        f"C {cx + kx:.3f} {cy + ry:.3f} {cx + rx:.3f} {cy + ky:.3f} {cx + rx:.3f} {cy:.3f} Z"
    )


def emit_path(
    d: str, fill: str | None = None, stroke: str | None = None, stroke_width: str | None = None,
    stroke_linecap: str | None = None, stroke_linejoin: str | None = None, opacity: float | None = None,
) -> str:
    attrs = [f'd="{d}"']
    if fill is not None:
        attrs.append(f'fill="{fill}"')
    if stroke is not None:
        attrs.append(f'stroke="{stroke}"')
    if stroke_width is not None:
        attrs.append(f'stroke-width="{stroke_width}"')
    if stroke_linecap is not None:
        attrs.append(f'stroke-linecap="{stroke_linecap}"')
    if stroke_linejoin is not None:
        attrs.append(f'stroke-linejoin="{stroke_linejoin}"')
    if opacity is not None:
        attrs.append(f'opacity="{opacity:.3f}"')
    return f'<path {" ".join(attrs)}/>'


def _float_or_none(v: str | None) -> float | None:
    return float(v) if v is not None else None


def bake_path_element(
    elem: ET.Element, matrix: Matrix, gradient_uses: dict[str, Matrix],
    d_override: str | None = None, opacity_override: float | None = None,
) -> str:
    """Bake one source `<path>` (or a `d`-morph override) through `matrix`,
    recording any gradient it references (by original id — every baked
    frame is its own standalone SVG file, so ids never collide across
    frames and never need renaming)."""
    d = transform_path_d(d_override if d_override is not None else elem.get("d", ""), matrix)
    fill = elem.get("fill")
    if fill and fill.startswith("url(#"):
        gradient_uses[fill[len("url(#"):-1]] = matrix
    sw = elem.get("stroke-width")
    sw_baked = f"{scale_stroke_width(float(sw), matrix):.3f}" if sw is not None else None
    opacity = opacity_override if opacity_override is not None else _float_or_none(elem.get("opacity"))
    return emit_path(
        d, fill=fill, stroke=elem.get("stroke"), stroke_width=sw_baked,
        stroke_linecap=elem.get("stroke-linecap"), stroke_linejoin=elem.get("stroke-linejoin"),
        opacity=opacity,
    )


def bake_round_element(elem: ET.Element, matrix: Matrix, opacity_override: float | None = None) -> str:
    """Bake one source `<circle>`/`<ellipse>` into an equivalent `<path>`
    (see this module's own top doc for why: neither renders correctly
    through `draw_svg_multicolor`)."""
    cx, cy = float(elem.get("cx")), float(elem.get("cy"))
    if tag_local(elem) == "circle":
        rx = ry = float(elem.get("r"))
    else:
        rx, ry = float(elem.get("rx")), float(elem.get("ry"))
    nx, ny = apply_point(matrix, cx, cy)
    scale = matrix_scale_factor(matrix)
    d = ellipse_to_path_d(nx, ny, rx * scale, ry * scale)
    opacity = opacity_override if opacity_override is not None else _float_or_none(elem.get("opacity"))
    return emit_path(d, fill=elem.get("fill"), opacity=opacity)


def wrap_svg(paths_svg: list[str], defs_svg: str, viewbox: str, w: int, h: int) -> str:
    return (
        f'<svg xmlns="{SVG_NS}" width="{w}" height="{h}" viewBox="{viewbox}" fill="none">\n'
        f"<defs>{defs_svg}</defs>\n"
        f"{chr(10).join(paths_svg)}\n"
        "</svg>\n"
    )


def assemble_frame(
    root: ET.Element, paths_svg: list[str], gradient_uses: dict[str, Matrix],
    viewbox: str, w: int, h: int,
) -> str:
    defs_svg = "".join(baked_gradient_svg(find_gradient(root, gid), m) for gid, m in gradient_uses.items())
    return wrap_svg(paths_svg, defs_svg, viewbox, w, h)


def write_frames(sticker_id: str, frame_svgs: list[str]) -> None:
    out_dir = SCRIPT_DIR / sticker_id
    out_dir.mkdir(parents=True, exist_ok=True)
    for i, svg in enumerate(frame_svgs):
        (out_dir / f"frame_{i:02d}.svg").write_text(svg, encoding="utf-8", newline="\n")
    print(f"wrote {sticker_id}/frame_00.svg .. frame_{len(frame_svgs) - 1:02d}.svg")


# =============================================================================
# bounce — the whole hat hops/squashes as one rigid group; a separate
# "landing dust" group (ellipse + 2 circles + 3 paths, absolute coordinates,
# no outer wrapper) bursts outward on landing, each particle its own track.
# =============================================================================

DUST_TRACK_NAMES = {
    "dust-ring": "dust-ring-burst",
    "dust-left-far": "dust-left-far",
    "dust-left-near": "dust-left-near",
    "dust-right-near": "dust-right-near",
    "dust-right-far": "dust-right-far",
    "dust-up": "dust-up",
}


def build_bounce_frames() -> list[str]:
    root = ET.parse(SRC_DIR / "magic_hat_bounce.svg").getroot()
    style_text = next(e for e in root if tag_local(e) == "style").text or ""
    keyframes = parse_keyframes(style_text)

    hat_track = Track(keyframes["hat-bounce"], 2400.0, 0.0, parse_easing("cubic-bezier(0.45, 0, 0.2, 1)"))
    dust_tracks = {
        variant: Track(keyframes[name], 2400.0, 0.0, EASE_OUT) for variant, name in DUST_TRACK_NAMES.items()
    }

    outer = parse_static_transform(by_id(root, "magic-hat").get("transform"))
    brim_path = child_paths(by_id(root, "hat-brim"))[0]
    cone_path = by_id(root, "hat-cone").find(f"{{{SVG_NS}}}path")
    symbol_paths = child_paths(by_id(root, "hat-symbols"))
    dust_group = by_aria_label(root, "landing dust")
    dust_elems = list(dust_group)

    frame_ms = 2400.0 / FRAME_COUNT
    frames = []
    for i in range(FRAME_COUNT):
        elapsed = i * frame_ms
        sample = hat_track.sample(elapsed)
        hat_matrix = around_origin(transform_list_to_matrix(sample["transform"]), 256.0, 400.0)
        total = compose(hat_matrix, outer)

        gradient_uses: dict[str, Matrix] = {}
        paths = [bake_path_element(brim_path, total, gradient_uses)]
        paths.append(bake_path_element(cone_path, total, gradient_uses))
        for p in symbol_paths:
            paths.append(bake_path_element(p, total, gradient_uses))

        for elem in dust_elems:
            classes = (elem.get("class") or "").split()
            variant = next(c for c in classes if c in DUST_TRACK_NAMES)
            track = dust_tracks[variant]
            dsample = track.sample(elapsed)
            if tag_local(elem) in ("circle", "ellipse"):
                cx, cy = float(elem.get("cx")), float(elem.get("cy"))
            else:
                cx, cy = fill_box_origin(path_bbox(elem.get("d")), 0.5, 0.5)
            dust_matrix = around_origin(transform_list_to_matrix(dsample["transform"]), cx, cy)
            opacity = float(dsample["opacity"])
            if tag_local(elem) in ("circle", "ellipse"):
                paths.append(bake_round_element(elem, dust_matrix, opacity_override=opacity))
            else:
                paths.append(bake_path_element(elem, dust_matrix, gradient_uses, opacity_override=opacity))

        frames.append(assemble_frame(root, paths, gradient_uses, "0 0 512 512", 512, 512))
    return frames


# =============================================================================
# spin — the whole hat rotates in place around the view-box centre.
# =============================================================================

def build_spin_frames() -> list[str]:
    root = ET.parse(SRC_DIR / "magic_hat_spin.svg").getroot()
    style_text = next(e for e in root if tag_local(e) == "style").text or ""
    keyframes = parse_keyframes(style_text)
    spin_track = Track(keyframes["magic-hat-spin"], 2400.0, 0.0, LINEAR)

    outer = parse_static_transform(by_id(root, "magic-hat").get("transform"))
    brim_path = child_paths(by_id(root, "hat-brim"))[0]
    cone_path = by_id(root, "hat-cone").find(f"{{{SVG_NS}}}path")
    symbol_paths = child_paths(by_id(root, "hat-symbols"))

    frame_ms = 2400.0 / FRAME_COUNT
    frames = []
    for i in range(FRAME_COUNT):
        sample = spin_track.sample(i * frame_ms)
        spin_matrix = around_origin(transform_list_to_matrix(sample["transform"]), 256.0, 256.0)
        total = compose(spin_matrix, outer)

        gradient_uses: dict[str, Matrix] = {}
        paths = [bake_path_element(brim_path, total, gradient_uses), bake_path_element(cone_path, total, gradient_uses)]
        paths.extend(bake_path_element(p, total, gradient_uses) for p in symbol_paths)
        frames.append(assemble_frame(root, paths, gradient_uses, "0 0 512 512", 512, 512))
    return frames


# =============================================================================
# wave (tip_wave) — the brim recoils, the cone flexes+bends, and the symbol
# clusters near the tip/mid follow with their own lagged sway.
# =============================================================================

def build_wave_frames() -> list[str]:
    root = ET.parse(SRC_DIR / "magic_hat_tip_wave.svg").getroot()
    style_text = next(e for e in root if tag_local(e) == "style").text or ""
    keyframes = parse_keyframes(style_text)
    easing = parse_easing("cubic-bezier(0.45, 0, 0.2, 1)")

    outer = parse_static_transform(by_id(root, "magic-hat").get("transform"))

    brim_group = by_id(root, "hat-brim")
    brim_path = child_paths(brim_group)[0]
    brim_origin = fill_box_origin(path_bbox(brim_path.get("d")), 0.52, 0.54)
    brim_track = Track(keyframes["magic-hat-brim-recoil"], 2400.0, 0.0, easing)

    flex_group = by_id(root, "hat-cone-flex")
    cone_shape = by_id(root, "hat-cone-shape")
    tip_group = by_id(root, "hat-symbols-tip")
    mid_group = by_id(root, "hat-symbols-mid")
    lower_group = by_id(root, "hat-symbols-lower")
    tip_paths = child_paths(tip_group)
    mid_paths = child_paths(mid_group)
    lower_paths = child_paths(lower_group)

    flex_bbox = union_bbox([path_bbox(cone_shape.get("d"))] + [path_bbox(p.get("d")) for p in tip_paths + mid_paths + lower_paths])
    flex_origin = fill_box_origin(flex_bbox, 0.55, 0.91)
    flex_track = Track(keyframes["magic-hat-cone-flex"], 2400.0, 0.0, easing)
    cone_bend_track = Track(keyframes["magic-hat-cone-bend"], 2400.0, 0.0, easing)

    tip_origin = fill_box_origin(union_bbox([path_bbox(p.get("d")) for p in tip_paths]), 0.64, 0.82)
    tip_track = Track(keyframes["magic-hat-tip-follow"], 2400.0, 0.0, easing)
    mid_origin = fill_box_origin(union_bbox([path_bbox(p.get("d")) for p in mid_paths]), 0.50, 0.82)
    mid_track = Track(keyframes["magic-hat-mid-follow"], 2400.0, 0.0, easing)

    frame_ms = 2400.0 / FRAME_COUNT
    frames = []
    for i in range(FRAME_COUNT):
        elapsed = i * frame_ms

        brim_matrix = around_origin(transform_list_to_matrix(brim_track.sample(elapsed)["transform"]), *brim_origin)
        flex_matrix = around_origin(transform_list_to_matrix(flex_track.sample(elapsed)["transform"]), *flex_origin)
        tip_matrix = around_origin(transform_list_to_matrix(tip_track.sample(elapsed)["transform"]), *tip_origin)
        mid_matrix = around_origin(transform_list_to_matrix(mid_track.sample(elapsed)["transform"]), *mid_origin)
        bent_d = cone_bend_track.sample(elapsed)["d"]

        total_brim = compose(outer, brim_matrix)
        total_flex = compose(outer, flex_matrix)
        total_tip = compose(total_flex, tip_matrix)
        total_mid = compose(total_flex, mid_matrix)

        gradient_uses: dict[str, Matrix] = {}
        paths = [bake_path_element(brim_path, total_brim, gradient_uses)]
        paths.append(bake_path_element(cone_shape, total_flex, gradient_uses, d_override=bent_d))
        paths.extend(bake_path_element(p, total_tip, gradient_uses) for p in tip_paths)
        paths.extend(bake_path_element(p, total_mid, gradient_uses) for p in mid_paths)
        paths.extend(bake_path_element(p, total_flex, gradient_uses) for p in lower_paths)

        frames.append(assemble_frame(root, paths, gradient_uses, "0 0 512 512", 512, 512))
    return frames


# =============================================================================
# twinkle — the hat itself is static; only the 6 star + 1 moon symbol paths
# pulse in place, plus 4 free-floating sparkles fade in/out around it.
# =============================================================================

SPARKLE_DELAYS = {"sparkle-1": -0.08, "sparkle-2": -0.91, "sparkle-3": -1.56, "sparkle-4": -2.29}
STAR_DELAYS = {
    "star-1": -0.16, "star-2": -0.72, "star-3": -1.34, "star-4": -2.08, "star-5": -0.98, "star-6": -1.82,
}
MOON_DELAY = -0.42


def build_twinkle_frames() -> list[str]:
    root = ET.parse(SRC_DIR / "magic_hat_twinkle.svg").getroot()
    style_text = next(e for e in root if tag_local(e) == "style").text or ""
    keyframes = parse_keyframes(style_text)

    outer = parse_static_transform(by_id(root, "magic-hat").get("transform"))
    brim_path = child_paths(by_id(root, "hat-brim"))[0]
    cone_path = by_id(root, "hat-cone").find(f"{{{SVG_NS}}}path")
    symbol_group = by_id(root, "hat-symbols")
    symbol_paths = child_paths(symbol_group)

    sparkle_group = by_aria_label(root, "subtle magical sparkles")
    sparkle_wrappers = list(sparkle_group)  # each a <g transform="translate(x y)"> holding one <path>

    frame_ms = 2800.0 / FRAME_COUNT
    frames = []
    for i in range(FRAME_COUNT):
        elapsed = i * frame_ms

        gradient_uses: dict[str, Matrix] = {}
        paths = [bake_path_element(brim_path, outer, gradient_uses), bake_path_element(cone_path, outer, gradient_uses)]

        for p in symbol_paths:
            classes = (p.get("class") or "").split()
            own_origin = fill_box_origin(path_bbox(p.get("d")), 0.5, 0.5)
            if "twinkle-star" in classes:
                variant = next(c for c in classes if c in STAR_DELAYS)
                track = Track(keyframes["star-twinkle"], 2800.0, STAR_DELAYS[variant], EASE_IN_OUT)
            else:
                track = Track(keyframes["moon-glow"], 2800.0, MOON_DELAY, EASE_IN_OUT)
            sample = track.sample(elapsed)
            own_matrix = around_origin(transform_list_to_matrix(sample["transform"]), *own_origin)
            total = compose(outer, own_matrix)
            paths.append(bake_path_element(p, total, gradient_uses, opacity_override=float(sample["opacity"])))

        for wrapper in sparkle_wrappers:
            static = parse_static_transform(wrapper.get("transform"))
            sparkle_path = child_paths(wrapper)[0]
            classes = (sparkle_path.get("class") or "").split()
            variant = next(c for c in classes if c in SPARKLE_DELAYS)
            track = Track(keyframes["sparkle-pop"], 2800.0, SPARKLE_DELAYS[variant], EASE_IN_OUT)
            sample = track.sample(elapsed)
            own_origin = fill_box_origin(path_bbox(sparkle_path.get("d")), 0.5, 0.5)
            own_matrix = around_origin(transform_list_to_matrix(sample["transform"]), *own_origin)
            total = compose(static, own_matrix)
            paths.append(bake_path_element(sparkle_path, total, gradient_uses, opacity_override=float(sample["opacity"])))

        frames.append(assemble_frame(root, paths, gradient_uses, "0 0 512 512", 512, 512))
    return frames


# =============================================================================
# flip (y_spin) and turntable (turntable_quality) — already 16 discrete
# drawn views (`steps(1,end)` opacity toggling, no interpolation needed);
# each frame just gets flattened, once, with no time-varying transform.
# =============================================================================

def bake_static_subtree(group: ET.Element, matrix: Matrix, gradient_uses: dict[str, Matrix]) -> list[str]:
    """Recursively bake a subtree whose only nesting is a plain
    `<g transform="translate() scale()">` wrapper (both `flip` and
    `turntable`'s per-frame source groups are this shape — no CSS
    animation left to evaluate, just static placement to flatten)."""
    out: list[str] = []
    for child in group:
        local = tag_local(child)
        if local == "g":
            child_matrix = compose(matrix, parse_static_transform(child.get("transform")))
            out.extend(bake_static_subtree(child, child_matrix, gradient_uses))
        elif local in ("ellipse", "circle"):
            out.append(bake_round_element(child, matrix))
        elif local == "path":
            out.append(bake_path_element(child, matrix, gradient_uses))
    return out


def build_flip_frames() -> list[str]:
    root = ET.parse(SRC_DIR / "magic_hat_y_spin.svg").getroot()
    frame_groups: dict[int, ET.Element] = {}
    for g in root:
        if tag_local(g) != "g":
            continue
        classes = (g.get("class") or "").split()
        if "hat-frame" not in classes:
            continue
        idx = int(next(c for c in classes if c.startswith("frame-")).split("-", 1)[1])
        frame_groups[idx] = g
    if len(frame_groups) != FRAME_COUNT:
        raise ValueError(f"expected {FRAME_COUNT} hat-frame groups, found {len(frame_groups)}")

    frames = []
    for i in range(FRAME_COUNT):
        gradient_uses: dict[str, Matrix] = {}
        paths = bake_static_subtree(frame_groups[i], IDENTITY, gradient_uses)
        frames.append(assemble_frame(root, paths, gradient_uses, "0 0 512 512", 512, 512))
    return frames


def build_turntable_frames() -> list[str]:
    """Port of `assets/mascot/spinner/build_frames.py`'s own `build_frame`
    (its source, `magic_hat_turntable_quality.svg`, is byte-identical to
    that script's `boot-hat-source.svg`) — `<use href="#star|#moon"
    transform="matrix(...)">` inlined into plain paths, `quality-light-NN`
    sheen overlays dropped (their whole point is a `stop-opacity` sheen the
    parser never reads — see that script's own doc §3), `brim-gradient`
    flattened to its first stop (objectBoundingBox — see §4), and every
    per-frame `quality-{crown,flap,fold}-NN` `userSpaceOnUse` gradient kept
    as-is (already in this frame's own absolute pixel space, so `matrix`
    stays `IDENTITY` throughout — no outer wrapper ever applies here)."""
    root = ET.parse(SRC_DIR / "magic_hat_turntable_quality.svg").getroot()
    defs = next(e for e in root if tag_local(e) == "defs")
    symbols: dict[str, ET.Element] = {}
    for elem in defs:
        if tag_local(elem) == "path" and elem.get("id") in ("star", "moon"):
            symbols[elem.get("id")] = elem
    if set(symbols) != {"star", "moon"}:
        raise ValueError(f"expected #star and #moon symbol defs, found: {sorted(symbols)}")

    brim_gradient = find_gradient(root, "brim-gradient")
    brim_flat_color = first_stop_color(brim_gradient)

    frame_groups = {e.get("id"): e for e in root if tag_local(e) == "g" and (e.get("id") or "").startswith("hat-frame-")}
    if len(frame_groups) != FRAME_COUNT:
        raise ValueError(f"expected {FRAME_COUNT} hat-frame-NN groups, found {len(frame_groups)}")

    frames = []
    for i in range(FRAME_COUNT):
        frame_group = frame_groups[f"hat-frame-{i:02d}"]
        gradient_uses: dict[str, Matrix] = {}
        paths: list[str] = []
        for elem in frame_group.iter():
            local = tag_local(elem)
            if local == "path":
                fill = elem.get("fill")
                if fill is not None and fill.startswith("url(#quality-light-"):
                    continue  # sheen overlay — see this function's own doc.
                if fill == "url(#brim-gradient)":
                    attrs = dict(elem.attrib)
                    attrs["fill"] = brim_flat_color
                    paths.append(emit_path(
                        transform_path_d(attrs["d"], IDENTITY), fill=attrs["fill"], stroke=attrs.get("stroke"),
                        stroke_width=attrs.get("stroke-width"), stroke_linecap=attrs.get("stroke-linecap"),
                        stroke_linejoin=attrs.get("stroke-linejoin"), opacity=_float_or_none(attrs.get("opacity")),
                    ))
                else:
                    paths.append(bake_path_element(elem, IDENTITY, gradient_uses))
            elif local == "use":
                href = elem.get("href")
                if not href or not href.startswith("#"):
                    raise ValueError(f"unsupported <use> href: {href!r}")
                symbol = symbols[href[1:]]
                # `matrix(a b c d e f)` is not one of parse_transform_list's
                # named functions (CSS `transform` never uses it in this
                # pack) — read it directly instead.
                m = elem.get("transform", "matrix(1 0 0 1 0 0)").strip()
                if not (m.startswith("matrix(") and m.endswith(")")):
                    raise ValueError(f"only matrix() transforms are supported on <use>, got: {m!r}")
                nums = [float(v) for v in m[len("matrix("):-1].replace(",", " ").split()]
                use_matrix = tuple(nums)  # already (a, b, c, d, e, f)
                paths.append(bake_path_element(symbol, use_matrix, gradient_uses))
        frames.append(assemble_frame(root, paths, gradient_uses, "0 0 240 180", 240, 180))
    return frames


# =============================================================================
# main
# =============================================================================

PACK_SPEC = [
    ("bounce", "Bounce", build_bounce_frames, 150),
    ("spin", "Spin", build_spin_frames, 150),
    ("wave", "Wave", build_wave_frames, 150),
    ("twinkle", "Twinkle", build_twinkle_frames, 175),
    ("flip", "Flip", build_flip_frames, 200),
    ("turntable", "Turntable", build_turntable_frames, 150),
]


def main() -> None:
    stickers = []
    for sticker_id, alt, builder, frame_ms in PACK_SPEC:
        frames = builder()
        if len(frames) != FRAME_COUNT:
            raise ValueError(f"{sticker_id}: expected {FRAME_COUNT} frames, built {len(frames)}")
        write_frames(sticker_id, frames)
        stickers.append({"id": sticker_id, "alt": alt, "frames": FRAME_COUNT, "frame_ms": frame_ms})

    pack = {"pack": "mlc-hat", "stickers": stickers}
    pack_path = SCRIPT_DIR / "pack.json"
    pack_path.write_text(json.dumps(pack, indent=2) + "\n", encoding="utf-8", newline="\n")
    print(f"wrote {pack_path.name}")


if __name__ == "__main__":
    main()
