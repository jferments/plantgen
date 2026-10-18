"""Read a crown profile (features.yaml `habit.crown_profile`) from a photo or a render.

Python standard library and Pillow only (pip install pillow).

    python3 photo-rules/tools/crown_profile.py grid PHOTO --out grid.png
        Draw a percentage grid on the photo, to read the ground point (where
        the trunk meets the ground) and the top, in per cent of the image's
        width and height from its top left corner.

    python3 photo-rules/tools/crown_profile.py bands PHOTO --ground 48,93 --top 51,6 --out bands.png
        Crop the plant and draw the ten bands (0.05 ... 0.95 of its height)
        with ticks every 0.02 of its height out from the trunk line, both
        sides, labelled every 0.1. Read where the foliage edge crosses each
        band on the left and on the right; the band's width is the two
        readings added.

    python3 photo-rules/tools/crown_profile.py glb PLANT-lod1.glb
        A render's profile from its geometry: a `plantc export` level-1
        file (no --compress). Foliage vertices only (every material but
        wood and bark), in ten slabs of a tenth of the height each; a slab's
        width is the spread of x from its 0.5th to its 99.5th percentile, so
        single strays are left out, over the height; a slab with fewer than
        20 vertices is 0. Prints {"height": metres, "widths": [ten shares]}.
"""
from __future__ import annotations

import argparse
import json
import math
import struct
import sys
from array import array

BANDS = [0.05 + 0.1 * k for k in range(10)]
TICK = 0.02   # of the height
REACH = 0.6   # ticks reach this far from the trunk line, each side
INK, HALO, BAND = (255, 0, 200, 255), (0, 0, 0, 200), (255, 230, 0, 45)


def point(text: str) -> tuple[float, float]:
    x, y = (float(v) for v in text.split(","))
    return x / 100, y / 100


def grid(args) -> None:
    from PIL import Image, ImageDraw
    im = Image.open(args.photo).convert("RGBA")
    over = Image.new("RGBA", im.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(over)
    w, h = im.size
    for p in range(0, 101, 5):
        major = p % 10 == 0
        colour = INK if major else (255, 0, 200, 110)
        x, y = round(w * p / 100), round(h * p / 100)
        d.line([(x, 0), (x, h)], fill=colour, width=2 if major else 1)
        d.line([(0, y), (w, y)], fill=colour, width=2 if major else 1)
        if major:
            d.text((x + 3, 3), str(p), fill=INK, stroke_width=2, stroke_fill=HALO)
            d.text((3, y + 3), str(p), fill=INK, stroke_width=2, stroke_fill=HALO)
    Image.alpha_composite(im, over).convert("RGB").save(args.out)


def bands(args) -> None:
    from PIL import Image, ImageDraw
    im = Image.open(args.photo).convert("RGBA")
    w, h = im.size
    (gx, gy), (tx, ty) = point(args.ground), point(args.top)
    gx, gy, tx, ty = gx * w, gy * h, tx * w, ty * h
    height = gy - ty
    if height <= 0:
        raise SystemExit("the top must be above the ground point")
    # The crop: the trunk line's reach each side, a little above the top and below the ground.
    left = round(min(gx, tx) - (REACH + 0.05) * height)
    right = round(max(gx, tx) + (REACH + 0.05) * height)
    top, bottom = round(ty - 0.06 * height), round(gy + 0.04 * height)
    crop = Image.new("RGBA", (right - left, bottom - top), (128, 128, 128, 255))
    crop.paste(im.crop((max(left, 0), max(top, 0), min(right, w), min(bottom, h))), (max(-left, 0), max(-top, 0)))
    scale = max(1.0, 900 / height)  # small trees are enlarged so the ticks stay legible
    if scale > 1:
        crop = crop.resize((round(crop.width * scale), round(crop.height * scale)), Image.Resampling.LANCZOS)
    over = Image.new("RGBA", crop.size, (0, 0, 0, 0))
    d = ImageDraw.Draw(over)

    def at(x: float, y: float) -> tuple[float, float]:
        return (x - left) * scale, (y - top) * scale

    def trunk_x(share: float) -> float:  # the trunk line from the ground point to the top
        return gx + (tx - gx) * share

    H = height * scale
    d.line([at(gx, gy), at(tx, ty)], fill=INK, width=1)
    d.line([at(gx - REACH * height, gy), at(gx + REACH * height, gy)], fill=INK, width=2)
    for k, share in enumerate(BANDS):
        y = gy - share * height
        x0 = trunk_x(share)
        lo, hi = at(x0 - REACH * height, y + 0.05 * height), at(x0 + REACH * height, y - 0.05 * height)
        if k % 2 == 0:
            d.rectangle([lo[0], hi[1], hi[0], lo[1]], fill=BAND)
        cx, cy = at(x0, y)
        d.line([at(x0 - REACH * height, y), at(x0 + REACH * height, y)], fill=INK, width=1)
        for i in range(1, round(REACH / TICK) + 1):
            major = i % 5 == 0
            length = (0.025 if major else 0.012) * H
            for side in (-1, 1):
                x = cx + side * i * TICK * H
                d.line([(x, cy - length), (x, cy + length)], fill=INK, width=2 if major else 1)
                if major:
                    d.text((x - 8, cy + length + 1), f"{i * TICK:.1f}", fill=INK, stroke_width=2, stroke_fill=HALO)
        d.text((3, cy - 7), f"{k}: {share:.2f} H", fill=INK, stroke_width=2, stroke_fill=HALO)
    Image.alpha_composite(crop, over).convert("RGB").save(args.out)
    print(json.dumps({"height_px": round(height), "bands": [round(b, 2) for b in BANDS],
                      "read": "each band's width = left reading + right reading, as a share of the height"}))


# glTF binary

def mat_mul(a: list[float], b: list[float]) -> list[float]:
    """4x4 matrices in glTF's column-major order."""
    return [sum(a[r + 4 * k] * b[k + 4 * c] for k in range(4)) for c in range(4) for r in range(4)]


def node_matrix(node: dict) -> list[float]:
    if "matrix" in node:
        return list(node["matrix"])
    x, y, z, w = node.get("rotation", [0, 0, 0, 1])
    sx, sy, sz = node.get("scale", [1, 1, 1])
    tx, ty, tz = node.get("translation", [0, 0, 0])
    r = [1 - 2 * (y * y + z * z), 2 * (x * y + z * w), 2 * (x * z - y * w),
         2 * (x * y - z * w), 1 - 2 * (x * x + z * z), 2 * (y * z + x * w),
         2 * (x * z + y * w), 2 * (y * z - x * w), 1 - 2 * (x * x + y * y)]
    return [r[0] * sx, r[1] * sx, r[2] * sx, 0, r[3] * sy, r[4] * sy, r[5] * sy, 0,
            r[6] * sz, r[7] * sz, r[8] * sz, 0, tx, ty, tz, 1]


def positions(gltf: dict, blob: bytes, index: int) -> array:
    acc = gltf["accessors"][index]
    if acc.get("componentType") != 5126 or acc.get("type") != "VEC3":
        raise SystemExit("positions are not plain floats: export without --compress")
    view = gltf["bufferViews"][acc["bufferView"]]
    start = view.get("byteOffset", 0) + acc.get("byteOffset", 0)
    stride, n = view.get("byteStride", 12), acc["count"]
    if stride == 12:
        out = array("f")
        out.frombytes(blob[start:start + 12 * n])
    else:
        out = array("f", (v for i in range(n) for v in struct.unpack_from("<3f", blob, start + i * stride)))
    if sys.byteorder == "big":
        out.byteswap()
    return out


def percentile(sorted_values: list[float], p: float) -> float:
    """Linear between the nearest ranks, as numpy's default."""
    pos = (len(sorted_values) - 1) * p / 100
    lo = math.floor(pos)
    hi = min(lo + 1, len(sorted_values) - 1)
    return sorted_values[lo] + (sorted_values[hi] - sorted_values[lo]) * (pos - lo)


def glb_profile(path: str, axis: str = "x") -> dict:
    data = open(path, "rb").read()
    if data[:4] != b"glTF":
        raise SystemExit(f"{path} is not a glTF binary")
    n = struct.unpack_from("<I", data, 12)[0]
    gltf = json.loads(data[20:20 + n])
    m = struct.unpack_from("<I", data, 20 + n)[0]
    blob = data[28 + n:28 + n + m]
    materials = [mat.get("name", "") for mat in gltf.get("materials", [])]
    foliage, every = [], []
    ident = [1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1, 0, 0, 0, 0, 1]
    scene = gltf.get("scenes", [{}])[gltf.get("scene", 0)]
    stack = [(i, ident) for i in scene.get("nodes", range(len(gltf.get("nodes", []))))]
    col = {"x": 0, "z": 2}[axis]
    while stack:
        i, parent = stack.pop()
        node = gltf["nodes"][i]
        world = mat_mul(parent, node_matrix(node))
        stack += [(c, world) for c in node.get("children", [])]
        if "mesh" not in node:
            continue
        for prim in gltf["meshes"][node["mesh"]]["primitives"]:
            name = materials[prim["material"]].lower() if "material" in prim else ""
            wood = any(k in name for k in ("bark", "wood", "stem"))
            p = positions(gltf, blob, prim["attributes"]["POSITION"])
            for j in range(0, len(p), 3):
                x, y, z = p[j], p[j + 1], p[j + 2]
                v = [world[r] * x + world[r + 4] * y + world[r + 8] * z + world[r + 12] for r in range(3)]
                every.append(v[1])
                if not wood:
                    foliage.append((v[1], v[col]))
    if not every:
        raise SystemExit(f"{path} has no vertices")
    y0 = min(every)
    height = max(every) - y0
    slabs = [[] for _ in range(10)]
    for y, across in foliage:
        k = int((y - y0) / height * 10)
        if 0 <= k < 10:
            slabs[k].append(across)
    widths = []
    for s in slabs:
        if len(s) < 20:
            widths.append(0.0)
            continue
        s.sort()
        widths.append(round((percentile(s, 99.5) - percentile(s, 0.5)) / height, 3))
    return {"height": round(height, 2), "widths": widths, "axis": axis}


def main(argv=None) -> None:
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    sub = ap.add_subparsers(dest="cmd", required=True)
    p = sub.add_parser("grid", help="draw a percentage grid on a photo")
    p.add_argument("photo")
    p.add_argument("--out", required=True)
    p = sub.add_parser("bands", help="crop the plant and draw the ten tick bands")
    p.add_argument("photo")
    p.add_argument("--ground", required=True, help="X,Y in per cent: where the trunk meets the ground")
    p.add_argument("--top", required=True, help="X,Y in per cent: the plant's highest point")
    p.add_argument("--out", required=True)
    p = sub.add_parser("glb", help="a render's profile from a plantc export level-1 file")
    p.add_argument("glb")
    p.add_argument("--axis", choices=["x", "z"], default="x", help="the width seen from the side: across x (default) or z")
    args = ap.parse_args(argv)
    if args.cmd == "glb":
        print(json.dumps(glb_profile(args.glb, args.axis)))
    else:
        {"grid": grid, "bands": bands}[args.cmd](args)


if __name__ == "__main__":
    main()
