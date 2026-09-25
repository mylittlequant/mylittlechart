#!/usr/bin/env python3
"""Generate 16 static per-frame SVGs from the magic-hat turntable source, in
the SVG dialect uzor's hand-written `draw_svg_multicolor` parser
(`uzor/uzor/src/core/render/svg.rs`) actually renders correctly.

Source: `boot-hat-source.svg` in this directory — an exact copy of
`mylittlechart/crates/mlc-app-shell-web/boot-hat.svg` (the web boot-splash
hat). It holds 16 frame groups `<g id="hat-frame-NN">` (240x180 viewBox,
2.4s loop, 150ms/frame via CSS `@keyframes` — none of which the parser
reads). Each frame's star/moon accents are instances of two shared symbol
paths (`<path id="star">`, `<path id="moon">`) placed via
`<use href="#star|#moon" transform="matrix(a,b,c,d,e,f)">`. Per-frame body
fills use `<linearGradient gradientUnits="userSpaceOnUse">` in absolute
pixel coordinates.

What the parser supports (read from svg.rs — see
`nemo/docs/mlc/audits/` for the walk, or just read the source):
  - `<path>` with `fill`, `stroke`, `stroke-width`, `stroke-linecap/join`,
    `opacity`/`fill-opacity`/`stroke-opacity`, and `fill="url(#id)"`.
  - `<linearGradient>` resolved by a plain textual `id="..."` search
    ANYWHERE in the document (no defs/nesting awareness), reading
    `x1/y1/x2/y2` as raw numbers in the SAME coordinate space as path
    geometry. Correct ONLY when the gradient declares
    `gradientUnits="userSpaceOnUse"` with absolute coordinates — the
    parser has no notion of `objectBoundingBox` (the implicit default
    when `gradientUnits` is omitted).
  - `<rect>`, `<circle>` (unused by this asset).
It does NOT support: `<use>`, any `transform` on an individual element
(only a single top-level `<g transform="translate()/scale()">` — no
`matrix()` — is read, and only once, globally), `<radialGradient>`,
`objectBoundingBox` gradients, `stop-opacity` (only `stop-color` is read,
so a translucency gradient renders fully OPAQUE), CSS `<style>`/
`@keyframes`, or opacity on a `<g>`.

So, per frame, this script:
  1. Inlines every `<use href="#star|#moon" transform="matrix(...)">` into
     a plain `<path>`: bakes the matrix into the symbol's `d` coordinates
     and scales its `stroke-width` by sqrt(|det(matrix)|) (the uniform-
     scale approximation of the matrix's area factor — uzor's stroke
     model has no anisotropic width).
  2. Copies every already-plain `<path>` in the frame through as-is,
     in the same document order (paint order in the parser is purely
     document-order of `<path` tags — group nesting is not consulted).
  3. Drops every `quality-light-NN` overlay path (`fill="url(#quality-
     light-NN)"`): that gradient's entire purpose is a low-opacity sheen
     via `stop-opacity`, which the parser ignores — rendering it would
     paint a solid, wrongly-saturated patch over the correct base fill
     instead of a subtle highlight. Omitting it (the flat-colour limit of
     "transparent") reproduces exactly how the already-shipped toolbar
     hat icon (`assets/mascot/magic_hat.svg`, no such overlay) already
     renders through this same parser.
  4. Flattens the shared `brim-gradient` (objectBoundingBox, 2 stops) to
     its first stop-color — same "first stop wins" rule the codebase
     already documents for unsupported gradients
     (`uzor::core::render::svg::resolve_gradient_color`).
  5. Drops the unused `crown-gradient` and the 16 unused
     `quality-tip-cap-NN` radialGradients: neither is ever referenced by
     any path's `fill` (`grep -c 'fill="url(#quality-tip-cap'` = 0,
     `grep -c 'url(#crown-gradient)'` = 0), and radialGradient isn't in
     the parser's dialect regardless.
  6. Keeps every per-frame `quality-{crown,flap,fold}-NN` linearGradient
     actually referenced in that frame — already `userSpaceOnUse` with
     absolute coordinates, the dialect the parser resolves correctly —
     copied into a per-frame `<defs>` with only the ids that frame uses.

Run: `python3 build_frames.py` from any cwd — paths are relative to this
script's own directory. Regenerates `hat_turntable_00.svg` … `_15.svg`
in-place; re-run after any edit to `boot-hat-source.svg`.
"""
from __future__ import annotations

import math
import re
import xml.etree.ElementTree as ET
from pathlib import Path

SVG_NS = "http://www.w3.org/2000/svg"
ET.register_namespace("", SVG_NS)

SCRIPT_DIR = Path(__file__).resolve().parent
SOURCE_SVG = SCRIPT_DIR / "boot-hat-source.svg"
FRAME_COUNT = 16
VIEWBOX = "0 0 240 180"

# Path-data tokens: single-letter commands or numbers (int/decimal/exponent).
_TOKEN_RE = re.compile(r"[MLCZ]|-?\d*\.?\d+(?:[eE][-+]?\d+)?")

# Attributes copied verbatim from a source <path> onto its output <path>,
# in a fixed, readable order. Cosmetic-only attrs (id/class/aria-label) are
# dropped — the parser ignores them and they add no rendering meaning.
_PATH_ATTRS = (
    "d", "fill", "stroke", "stroke-width",
    "stroke-linecap", "stroke-linejoin",
    "opacity", "fill-opacity", "stroke-opacity",
)


def tag_local(elem: ET.Element) -> str:
    """Strip the `{namespace}` prefix ElementTree puts on every tag name."""
    return elem.tag.rsplit("}", 1)[-1]


def parse_matrix(transform: str) -> tuple[float, float, float, float, float, float]:
    inner = transform.strip()
    assert inner.startswith("matrix(") and inner.endswith(")"), (
        f"only matrix() transforms are supported on <use>, got: {transform!r}"
    )
    nums = [float(v) for v in inner[len("matrix("):-1].replace(",", " ").split()]
    assert len(nums) == 6, f"matrix() needs 6 numbers, got {len(nums)}: {transform!r}"
    a, b, c, d, e, f = nums
    return a, b, c, d, e, f


def apply_matrix(m: tuple[float, float, float, float, float, float], x: float, y: float) -> tuple[float, float]:
    a, b, c, d, e, f = m
    return a * x + c * y + e, b * x + d * y + f


def transform_path_d(d: str, m: tuple[float, float, float, float, float, float]) -> str:
    """Bake an affine matrix into absolute M/L/C/Z path data.

    Only the commands the two symbol defs (#star, #moon) actually use —
    absolute M/L/C/Z — are supported; anything else raises, so a future
    edit to the source symbols fails loudly here instead of silently
    mis-transforming.
    """
    tokens = _TOKEN_RE.findall(d)
    out: list[str] = []
    i = 0
    while i < len(tokens):
        cmd = tokens[i]
        if cmd == "M":
            x, y = float(tokens[i + 1]), float(tokens[i + 2])
            nx, ny = apply_matrix(m, x, y)
            out.append(f"M {nx:.3f} {ny:.3f}")
            i += 3
        elif cmd == "L":
            x, y = float(tokens[i + 1]), float(tokens[i + 2])
            nx, ny = apply_matrix(m, x, y)
            out.append(f"L {nx:.3f} {ny:.3f}")
            i += 3
        elif cmd == "C":
            nums = [float(v) for v in tokens[i + 1:i + 7]]
            pts = [apply_matrix(m, nums[j], nums[j + 1]) for j in (0, 2, 4)]
            out.append("C " + " ".join(f"{px:.3f} {py:.3f}" for px, py in pts))
            i += 7
        elif cmd == "Z":
            out.append("Z")
            i += 1
        else:
            raise ValueError(f"unsupported path command {cmd!r} in symbol data: {d!r}")
    return " ".join(out)


def scale_stroke_width(width: str, m: tuple[float, float, float, float, float, float]) -> str:
    a, b, c, d, _e, _f = m
    scale = math.sqrt(abs(a * d - b * c))
    return f"{float(width) * scale:.3f}"


def first_stop_color(gradient: ET.Element) -> str:
    for stop in gradient:
        if tag_local(stop) == "stop":
            color = stop.get("stop-color")
            if color:
                return color
    raise ValueError(f"gradient {gradient.get('id')!r} has no stop-color")


def gradient_to_defs_svg(gradient: ET.Element) -> str:
    """Serialize a <linearGradient> exactly as the parser expects: an
    `id`, absolute `x1/y1/x2/y2`, and `<stop offset stop-color>` children
    (stop-opacity is dropped — the parser never reads it, see module doc).
    """
    gid = gradient.get("id")
    x1, y1 = gradient.get("x1", "0"), gradient.get("y1", "0")
    x2, y2 = gradient.get("x2", "0"), gradient.get("y2", "1")
    stops = []
    for stop in gradient:
        if tag_local(stop) != "stop":
            continue
        offset = stop.get("offset", "0")
        color = stop.get("stop-color", "#000000")
        stops.append(f'<stop offset="{offset}" stop-color="{color}"/>')
    stops_svg = "".join(stops)
    return (
        f'<linearGradient id="{gid}" gradientUnits="userSpaceOnUse" '
        f'x1="{x1}" y1="{y1}" x2="{x2}" y2="{y2}">{stops_svg}</linearGradient>'
    )


def render_path_attrs(attrs: dict[str, str]) -> str:
    parts = []
    for key in _PATH_ATTRS:
        value = attrs.get(key)
        if value is not None:
            parts.append(f'{key}="{value}"')
    return " ".join(parts)


def build_frame(frame_group: ET.Element, symbols: dict[str, ET.Element],
                 linear_gradients: dict[str, ET.Element], brim_flat_color: str) -> str:
    output_paths: list[str] = []
    used_gradient_ids: list[str] = []

    def note_gradient_ref(fill: str | None) -> str | None:
        if fill is None or not fill.startswith("url(#"):
            return fill
        gid = fill[len("url(#"):-1]
        if gid == "brim-gradient":
            return brim_flat_color
        if gid not in linear_gradients:
            # crown-gradient / quality-tip-cap-* — unused or unsupported
            # (radialGradient); neither should ever be hit (see module
            # doc §5), so surface a loud failure instead of silently
            # emitting a broken url() reference.
            raise ValueError(f"path references unresolvable gradient {gid!r}")
        if gid not in used_gradient_ids:
            used_gradient_ids.append(gid)
        return fill

    for elem in frame_group.iter():
        local = tag_local(elem)
        if local == "path":
            attrs = dict(elem.attrib)
            raw_fill = attrs.get("fill")
            if raw_fill is not None and raw_fill.startswith("url(#quality-light-"):
                continue  # sheen overlay — see module doc §3 (never resolved as a gradient ref)
            fill = note_gradient_ref(raw_fill)
            if fill is not None:
                attrs["fill"] = fill
            output_paths.append(render_path_attrs(attrs))
        elif local == "use":
            href = elem.get("href")
            assert href and href.startswith("#"), f"unsupported <use> href: {href!r}"
            symbol = symbols[href[1:]]
            m = parse_matrix(elem.get("transform", "matrix(1 0 0 1 0 0)"))
            attrs = {
                "d": transform_path_d(symbol.get("d", ""), m),
                "fill": symbol.get("fill"),
                "stroke": symbol.get("stroke"),
                "stroke-width": scale_stroke_width(symbol.get("stroke-width", "1"), m),
                "stroke-linecap": symbol.get("stroke-linecap"),
                "stroke-linejoin": symbol.get("stroke-linejoin"),
            }
            output_paths.append(render_path_attrs(attrs))

    defs_svg = "".join(gradient_to_defs_svg(linear_gradients[gid]) for gid in used_gradient_ids)
    paths_svg = "\n".join(f"<path {p}/>" for p in output_paths)
    return (
        f'<svg xmlns="{SVG_NS}" width="240" height="180" viewBox="{VIEWBOX}" fill="none">\n'
        f"<defs>{defs_svg}</defs>\n"
        f"{paths_svg}\n"
        "</svg>\n"
    )


def main() -> None:
    tree = ET.parse(SOURCE_SVG)
    root = tree.getroot()

    defs = next(e for e in root if tag_local(e) == "defs")
    symbols: dict[str, ET.Element] = {}
    linear_gradients: dict[str, ET.Element] = {}
    for elem in defs:
        local = tag_local(elem)
        elem_id = elem.get("id")
        if local == "path" and elem_id in ("star", "moon"):
            symbols[elem_id] = elem
        elif local == "linearGradient" and elem_id not in ("brim-gradient", "crown-gradient"):
            linear_gradients[elem_id] = elem
    assert set(symbols) == {"star", "moon"}, f"expected #star and #moon symbol defs, found: {sorted(symbols)}"

    brim_gradient = next(e for e in defs if tag_local(e) == "linearGradient" and e.get("id") == "brim-gradient")
    brim_flat_color = first_stop_color(brim_gradient)

    frame_groups = {
        e.get("id"): e for e in root if tag_local(e) == "g" and (e.get("id") or "").startswith("hat-frame-")
    }
    assert len(frame_groups) == FRAME_COUNT, f"expected {FRAME_COUNT} hat-frame-NN groups, found {len(frame_groups)}"

    for i in range(FRAME_COUNT):
        frame_id = f"hat-frame-{i:02d}"
        svg_text = build_frame(frame_groups[frame_id], symbols, linear_gradients, brim_flat_color)
        out_path = SCRIPT_DIR / f"hat_turntable_{i:02d}.svg"
        out_path.write_text(svg_text, encoding="utf-8", newline="\n")
        print(f"wrote {out_path.name}")


if __name__ == "__main__":
    main()
