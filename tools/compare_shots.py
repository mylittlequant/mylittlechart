#!/usr/bin/env python3
"""Side-by-side and diff of a port screenshot against the old app's reference.

    compare_shots.py <reference.png> <port.png> <out.png> [--region name:x,y,w,h ...]

Writes <out.png>: reference | port | diff (changed pixels in red over a dimmed
reference), and prints the share of pixels that differ (any channel by more than
--tol, default 8) for the whole image and for each region. Both images must have
the same size: render the port at the reference's size.
"""
import argparse
import sys

from PIL import Image, ImageChops


def mismatch(a, b, tol):
    d = ImageChops.difference(a, b).convert("L").point(lambda v: 255 if v > tol else 0)
    hist = d.histogram()
    return d, hist[255] / max(1, a.width * a.height)


def main():
    p = argparse.ArgumentParser()
    p.add_argument("ref")
    p.add_argument("port")
    p.add_argument("out")
    p.add_argument("--tol", type=int, default=8)
    p.add_argument("--region", action="append", default=[])
    a = p.parse_args()
    ref = Image.open(a.ref).convert("RGB")
    port = Image.open(a.port).convert("RGB")
    if ref.size != port.size:
        sys.exit(f"size differs: reference {ref.size}, port {port.size}")
    mask, share = mismatch(ref, port, a.tol)
    print(f"all: {share * 100:.2f}% of pixels differ")
    for spec in a.region:
        name, box = spec.split(":", 1)
        x, y, w, h = (int(v) for v in box.split(","))
        r = (x, y, x + w, y + h)
        _, s = mismatch(ref.crop(r), port.crop(r), a.tol)
        print(f"{name}: {s * 100:.2f}%")
    w, h = ref.size
    dim = Image.blend(ref, Image.new("RGB", ref.size), 0.6)
    red = Image.new("RGB", ref.size, (255, 40, 40))
    diff = Image.composite(red, dim, mask)
    out = Image.new("RGB", (w * 3 + 16, h), (40, 40, 40))
    out.paste(ref, (0, 0))
    out.paste(port, (w + 8, 0))
    out.paste(diff, (2 * w + 16, 0))
    out.save(a.out)


if __name__ == "__main__":
    main()
