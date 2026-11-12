#!/usr/bin/env python3
"""Find how far each crop of a port shot is off from the reference.

shift_shots.py <ref.png> <port.png> name=x0,y0,x1,y1 [name=...] [--tol 8] [--range 4]

For each named crop (window pixels, x1/y1 exclusive) it counts the pixels whose
grey per-channel difference is above --tol (the same rule as compare_shots.py),
first at no shift, then for every (dx, dy) in -range..range, and prints the best.
"Shift port by dx, dy" means the port's content at (x+dx, y+dy) matches the
reference at (x, y): positive dy = the port draws it lower. A crop that drops to 0
at some shift is misplaced by exactly that; one that stays high differs in shape.
"""
import argparse
from PIL import Image, ImageChops


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("ref")
    ap.add_argument("port")
    ap.add_argument("crops", nargs="+")
    ap.add_argument("--tol", type=int, default=8)
    ap.add_argument("--range", type=int, default=4)
    a = ap.parse_args()
    r = Image.open(a.ref).convert("RGB")
    f = Image.open(a.port).convert("RGB")

    def score(box, dx, dy):
        x0, y0, x1, y1 = box
        d = ImageChops.difference(r.crop(box), f.crop((x0 + dx, y0 + dy, x1 + dx, y1 + dy))).convert("L")
        return sum(1 for v in d.point(lambda v: 255 if v > a.tol else 0).tobytes() if v)

    for spec in a.crops:
        name, nums = spec.split("=")
        box = tuple(int(v) for v in nums.split(","))
        n = (box[2] - box[0]) * (box[3] - box[1])
        s0 = score(box, 0, 0)
        rg = range(-a.range, a.range + 1)
        best = min((score(box, dx, dy), dx, dy) for dx in rg for dy in rg)
        print(f"{name:12s} at 0: {s0:6d} ({100 * s0 / n:5.1f}%)   best {best[0]:6d} at dx={best[1]} dy={best[2]}")


if __name__ == "__main__":
    main()
