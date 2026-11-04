#!/usr/bin/env python3
"""Draw a site field (`plantc field`) as a site of its own, for
`plantc site` to render: each ground cell a tile at its height coloured by
one of its values, each surface patch a square on its face coloured by
its host's kind (or by its sky, or kept-clear and mossy patches picked
out).

    python3 docs/sites/field_view.py FIELD.json OUT_FOLDER [--ground sky|wetness|heat|rooting|material]
           [--patches kind|sky|soil]

writes OUT_FOLDER/view.obj and view.json; then
    plantc site OUT_FOLDER/view.json --out view.png --view oblique
"""

import argparse
import json
import math
from pathlib import Path

KINDS = {
    "wall": (0.80, 0.55, 0.30),
    "roof": (0.75, 0.20, 0.15),
    "overhang": (0.55, 0.20, 0.55),
    "pole": (0.95, 0.85, 0.20),
    "fence": (0.95, 0.60, 0.10),
    "thin": (0.10, 0.10, 0.10),
    "boulder": (0.55, 0.55, 0.60),
    "log": (0.45, 0.25, 0.10),
    "other": (1.0, 1.0, 1.0),
}
RAMP = [(0.05, 0.05, 0.35), (0.10, 0.35, 0.70), (0.15, 0.65, 0.55), (0.60, 0.80, 0.25), (0.95, 0.85, 0.20)]


def ramp(t):
    t = min(1.0, max(0.0, t)) * (len(RAMP) - 1)
    i = min(int(t), len(RAMP) - 2)
    f = t - i
    return tuple(a + (b - a) * f for a, b in zip(RAMP[i], RAMP[i + 1]))


def lin(c):
    return [round(v ** 2.2, 4) for v in c]


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("field")
    ap.add_argument("out")
    ap.add_argument("--ground", default="sky")
    ap.add_argument("--patches", default="kind")
    ap.add_argument("--plant", default="poa-annua")
    a = ap.parse_args()
    field = json.loads(Path(a.field).read_text())
    out = Path(a.out)
    out.mkdir(parents=True, exist_ok=True)
    g = field["ground"]
    cell = field["settings"]["cell_m"]
    ox, oz = field["origin"]
    cols = field["columns"]
    verts, groups, materials = [], {}, {}

    def quad(name, colour, corners, thin=None):
        if name not in materials:
            entry = {"as": "soil" if thin is None else "built", "colour": lin(colour)}
            if thin is not None:
                entry["thin_m"] = thin
            materials[name] = entry
        base = len(verts)
        verts.extend(corners)
        groups.setdefault(name, []).append((base + 1, base + 2, base + 3, base + 4))

    def ground_value(i):
        if a.ground == "sky":
            return g["sky"][i]
        if a.ground == "wetness":
            return (g["wetness"][i] - 2.0) / 10.0
        if a.ground == "heat":
            return (g["heat_load"][i] - 0.6) / 0.5
        if a.ground == "rooting":
            r = g["rooting_m"][i]
            return 1.0 if r is None else min(1.0, r / 1.0)
        return 0.0

    for i, h in enumerate(g["height"]):
        if h is None:
            continue
        c, r = i % cols, i // cols
        x0, z0 = ox + c * cell, oz + r * cell
        if a.ground == "material":
            colour = {1: (0.30, 0.45, 0.15), 2: (0.5, 0.5, 0.5), 5: (0.35, 0.35, 0.38)}.get(g["material"][i], (1, 0, 1))
            if g["under_roof"][i]:
                colour = (0.6, 0.2, 0.6)
            elif g["cracks"][i]:
                colour = (0.85, 0.55, 0.15)
            band = f"g-{g['material'][i]}-{int(g['under_roof'][i])}-{int(bool(g['cracks'][i]))}"
        else:
            v = ground_value(i)
            band = f"g{int(min(0.999, max(0.0, v)) * 20)}"
            colour = ramp(int(min(0.999, max(0.0, v)) * 20) / 19)
        y = h + 0.01
        # Counter-clockwise seen from above, +Y up.
        quad(band, colour, [(x0, y, z0), (x0, y, z0 + cell), (x0 + cell, y, z0 + cell), (x0 + cell, y, z0)])

    size = field["settings"]["patch_m"] * 0.9
    hosts = field["hosts"]
    for p in field["patches"]:
        kind = hosts[p["host"]]["kind"]
        if a.patches == "sky":
            band = f"p{int(min(0.999, p['sky']) * 10)}"
            colour = ramp(int(min(0.999, p["sky"]) * 10) / 9)
        elif a.patches == "soil":
            if p["clear"]:
                band, colour = "p-clear", (0.2, 0.6, 1.0)
            elif p["soil_m"] > 0:
                band, colour = "p-soil", (0.25, 0.75, 0.15)
            elif p["climbable"]:
                band, colour = "p-climb", (0.85, 0.75, 0.55)
            else:
                band, colour = "p-no", (0.3, 0.1, 0.1)
        else:
            band, colour = f"p-{kind}", KINDS.get(kind, (1, 1, 1))
            if p["clear"]:
                band, colour = "p-clear", (0.2, 0.6, 1.0)
        n = p["normal"]
        c = [p["centre"][k] + n[k] * 0.04 for k in range(3)]
        helper = (0.0, 1.0, 0.0) if abs(n[1]) < 0.9 else (1.0, 0.0, 0.0)
        t = (helper[1] * n[2] - helper[2] * n[1], helper[2] * n[0] - helper[0] * n[2], helper[0] * n[1] - helper[1] * n[0])
        tl = math.sqrt(sum(v * v for v in t)) or 1.0
        t = tuple(v / tl for v in t)
        b = (n[1] * t[2] - n[2] * t[1], n[2] * t[0] - n[0] * t[2], n[0] * t[1] - n[1] * t[0])
        s = 0.5 * min(size, math.sqrt(max(p["area"], 1e-4)))
        corners = [tuple(c[k] + (su * t[k] + sv * b[k]) * s for k in range(3))
                   for su, sv in ((-1, -1), (1, -1), (1, 1), (-1, 1))]
        quad(band, colour, corners, thin=0.01)

    lines = ["# docs/sites/field_view.py"] + [f"v {x:.4f} {y:.4f} {z:.4f}" for x, y, z in verts]
    for name, faces in groups.items():
        lines.append(f"usemtl {name}")
        lines += [f"f {a_} {b_} {c_} {d_}" for a_, b_, c_, d_ in faces]
    (out / "view.obj").write_text("\n".join(lines) + "\n")
    # Small plants at the field's four corners: `plantc site` needs one,
    # and draws the site round them.
    rows = field["rows"]
    plants = []
    for cc, rr in ((0, 0), (cols - 1, 0), (0, rows - 1), (cols - 1, rows - 1)):
        best = min((i for i, h in enumerate(g["height"]) if h is not None and g["material"][i] == 1),
                   key=lambda i: (i % cols - cc) ** 2 + (i // cols - rr) ** 2)
        plants.append({"species": a.plant, "at": [ox + (best % cols + 0.5) * cell, oz + (best // cols + 0.5) * cell],
                       "seed": len(plants) + 1, "years": 1})
    site = {"mesh": "view.obj", "materials": materials, "plants": plants}
    (out / "view.json").write_text(json.dumps(site, indent=1) + "\n")
    print(f"wrote {out}/view.obj ({len(verts)} corners) and view.json")


if __name__ == "__main__":
    main()
